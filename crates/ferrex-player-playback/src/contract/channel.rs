use std::{
    hash::{Hash, Hasher},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
};

use super::{
    EventSequence, PlaybackCommand, PlaybackEvent, PlaybackEventEnvelope,
    PlaybackSnapshot, Reduction, SessionGeneration, reduce_event,
};

/// Coalesced backend-event readiness source for an application subscription.
///
/// The signal carries no backend payload. Consumers wake, then drain owned
/// Ferrex events/snapshots through the normal session boundary.
#[derive(Clone)]
pub struct PlaybackEventSignal {
    generation: SessionGeneration,
    receiver: Arc<Mutex<Receiver<()>>>,
}

impl PlaybackEventSignal {
    #[cfg(any(feature = "mpv", test))]
    pub(crate) fn new(
        generation: SessionGeneration,
        receiver: Receiver<()>,
    ) -> Self {
        Self {
            generation,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    pub const fn generation(&self) -> SessionGeneration {
        self.generation
    }

    pub(crate) fn wait_blocking(&self) -> bool {
        self.receiver
            .lock()
            .is_ok_and(|receiver| receiver.recv().is_ok())
    }
}

impl std::fmt::Debug for PlaybackEventSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlaybackEventSignal")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl PartialEq for PlaybackEventSignal {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation
            && Arc::ptr_eq(&self.receiver, &other.receiver)
    }
}

impl Eq for PlaybackEventSignal {}

impl Hash for PlaybackEventSignal {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.generation.hash(state);
        Arc::as_ptr(&self.receiver).hash(state);
    }
}

/// Application-side command/event channel owner for one playback generation.
pub struct PlaybackController {
    generation: SessionGeneration,
    command_tx: Sender<PlaybackCommand>,
    event_rx: Receiver<PlaybackEventEnvelope>,
    shutdown_sent: bool,
}

impl std::fmt::Debug for PlaybackController {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlaybackController")
            .field("generation", &self.generation)
            .field("shutdown_sent", &self.shutdown_sent)
            .finish_non_exhaustive()
    }
}

/// Backend-side channel endpoint. Exactly one serialized backend owner should
/// hold this value and assign event sequence numbers through [`Self::emit`].
pub struct PlaybackBackendEndpoint {
    generation: SessionGeneration,
    next_sequence: EventSequence,
    command_rx: Receiver<PlaybackCommand>,
    event_tx: Sender<PlaybackEventEnvelope>,
}

impl std::fmt::Debug for PlaybackBackendEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlaybackBackendEndpoint")
            .field("generation", &self.generation)
            .field("next_sequence", &self.next_sequence)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlaybackControllerError {
    #[error("playback backend command channel is closed")]
    BackendClosed,
    #[error("playback controller is shutting down")]
    ShuttingDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BackendChannelError {
    #[error("playback application event channel is closed")]
    ApplicationClosed,
    #[error("playback event sequence is exhausted")]
    SequenceExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrainReport {
    pub applied: usize,
    pub ignored_stale_generation: usize,
    pub ignored_duplicate_or_out_of_order: usize,
    pub disconnected: bool,
}

/// Create the two owned endpoints for one playback session.
///
/// Commands and events are unbounded, non-blocking sends. The backend owner is
/// responsible for draining commands serially; the application drains copied
/// events into its snapshot. Dropping the controller best-effort sends one
/// [`PlaybackCommand::Shutdown`].
pub fn playback_channel(
    generation: SessionGeneration,
) -> (PlaybackController, PlaybackBackendEndpoint) {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    (
        PlaybackController {
            generation,
            command_tx,
            event_rx,
            shutdown_sent: false,
        },
        PlaybackBackendEndpoint {
            generation,
            next_sequence: EventSequence::FIRST,
            command_rx,
            event_tx,
        },
    )
}

impl PlaybackController {
    pub const fn generation(&self) -> SessionGeneration {
        self.generation
    }

    pub fn send(
        &self,
        command: PlaybackCommand,
    ) -> Result<(), PlaybackControllerError> {
        if self.shutdown_sent {
            return Err(PlaybackControllerError::ShuttingDown);
        }

        self.command_tx
            .send(command)
            .map_err(|_| PlaybackControllerError::BackendClosed)
    }

    /// Request ordered backend shutdown. Calling this more than once is safe.
    pub fn shutdown(&mut self) -> Result<(), PlaybackControllerError> {
        if self.shutdown_sent {
            return Ok(());
        }
        self.command_tx
            .send(PlaybackCommand::Shutdown)
            .map_err(|_| PlaybackControllerError::BackendClosed)?;
        self.shutdown_sent = true;
        Ok(())
    }

    /// Drain every currently queued event into `snapshot` without blocking.
    pub fn drain_into(
        &mut self,
        snapshot: &mut PlaybackSnapshot,
    ) -> DrainReport {
        let mut report = DrainReport::default();

        loop {
            match self.event_rx.try_recv() {
                Ok(envelope) => match reduce_event(snapshot, envelope) {
                    Reduction::Applied => report.applied += 1,
                    Reduction::IgnoredStaleGeneration => {
                        report.ignored_stale_generation += 1;
                    }
                    Reduction::IgnoredDuplicateOrOutOfOrder => {
                        report.ignored_duplicate_or_out_of_order += 1;
                    }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    report.disconnected = true;
                    break;
                }
            }
        }

        report
    }
}

impl Drop for PlaybackController {
    fn drop(&mut self) {
        if !self.shutdown_sent {
            let _ = self.command_tx.send(PlaybackCommand::Shutdown);
            self.shutdown_sent = true;
        }
    }
}

impl PlaybackBackendEndpoint {
    pub const fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// Receive the next queued command without blocking.
    pub fn try_recv(&self) -> Result<Option<PlaybackCommand>, TryRecvError> {
        match self.command_rx.try_recv() {
            Ok(command) => Ok(Some(command)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(error @ TryRecvError::Disconnected) => Err(error),
        }
    }

    /// Copy and sequence one backend event for application-side reduction.
    pub fn emit(
        &mut self,
        event: PlaybackEvent,
    ) -> Result<EventSequence, BackendChannelError> {
        let sequence = self.next_sequence;
        self.next_sequence = sequence
            .next()
            .ok_or(BackendChannelError::SequenceExhausted)?;

        self.event_tx
            .send(PlaybackEventEnvelope {
                generation: self.generation,
                sequence,
                event,
            })
            .map_err(|_| BackendChannelError::ApplicationClosed)?;

        Ok(sequence)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::contract::{
        PlaybackCapabilities, PlaybackState, PlaybackTarget,
    };

    fn snapshot(generation: SessionGeneration) -> PlaybackSnapshot {
        PlaybackSnapshot::new(
            generation,
            PlaybackTarget::GSTREAMER_EMBEDDED,
            PlaybackCapabilities::default(),
        )
    }

    #[test]
    fn event_signal_is_cloneable_and_ends_when_backend_disconnects() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let signal =
            PlaybackEventSignal::new(SessionGeneration::new(9), receiver);
        assert_eq!(signal, signal.clone());
        assert_eq!(signal.generation(), SessionGeneration::new(9));

        sender.try_send(()).unwrap();
        assert!(signal.wait_blocking());
        drop(sender);
        assert!(!signal.wait_blocking());
    }

    #[test]
    fn fake_backend_receives_commands_in_order() {
        let generation = SessionGeneration::INITIAL;
        let (controller, backend) = playback_channel(generation);

        controller.send(PlaybackCommand::SetPaused(true)).unwrap();
        controller
            .send(PlaybackCommand::SeekAbsolute(Duration::from_secs(42)))
            .unwrap();

        assert_eq!(
            backend.try_recv().unwrap(),
            Some(PlaybackCommand::SetPaused(true))
        );
        assert_eq!(
            backend.try_recv().unwrap(),
            Some(PlaybackCommand::SeekAbsolute(Duration::from_secs(42)))
        );
        assert_eq!(backend.try_recv().unwrap(), None);
    }

    #[test]
    fn fake_backend_events_reduce_into_one_snapshot() {
        let generation = SessionGeneration::INITIAL;
        let (mut controller, mut backend) = playback_channel(generation);
        let mut snapshot = snapshot(generation);

        backend
            .emit(PlaybackEvent::StateChanged(PlaybackState::Playing))
            .unwrap();
        backend
            .emit(PlaybackEvent::PositionChanged(Duration::from_secs(12)))
            .unwrap();

        let report = controller.drain_into(&mut snapshot);

        assert_eq!(report.applied, 2);
        assert!(!report.disconnected);
        assert_eq!(snapshot.state, PlaybackState::Playing);
        assert_eq!(snapshot.position, Duration::from_secs(12));
        assert_eq!(snapshot.last_sequence, Some(EventSequence::new(2)));
    }

    #[test]
    fn explicit_shutdown_is_idempotent() {
        let (mut controller, backend) =
            playback_channel(SessionGeneration::INITIAL);

        controller.shutdown().unwrap();
        controller.shutdown().unwrap();

        assert_eq!(
            backend.try_recv().unwrap(),
            Some(PlaybackCommand::Shutdown)
        );
        assert_eq!(backend.try_recv().unwrap(), None);
        drop(controller);
    }

    #[test]
    fn failed_shutdown_is_not_reported_as_complete() {
        let (mut controller, backend) =
            playback_channel(SessionGeneration::INITIAL);
        drop(backend);

        assert_eq!(
            controller.shutdown(),
            Err(PlaybackControllerError::BackendClosed)
        );
        assert_eq!(
            controller.shutdown(),
            Err(PlaybackControllerError::BackendClosed)
        );
    }

    #[test]
    fn drop_requests_shutdown() {
        let (controller, backend) =
            playback_channel(SessionGeneration::INITIAL);

        drop(controller);

        assert_eq!(
            backend.try_recv().unwrap(),
            Some(PlaybackCommand::Shutdown)
        );
    }
}
