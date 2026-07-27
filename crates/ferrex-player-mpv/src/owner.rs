//! Dedicated serialized owner thread for [`crate::MpvSession`].

use std::{
    sync::mpsc::{
        self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError,
    },
    thread::{self, JoinHandle, Thread},
    time::{Duration, Instant},
};

use crate::{
    MpvEndFileReason, MpvEvent, MpvFormat, MpvFunctionTable, MpvHookId,
    MpvHookRegistrationId, MpvLogLevel, MpvNode, MpvObservationId,
    MpvRequestId, MpvSession, MpvSessionConfig, MpvSessionError,
};

/// Timing bounds for the serialized owner thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpvWorkerConfig {
    /// Maximum time to initialize the native core.
    pub startup_timeout: Duration,
    /// Maximum time a caller waits for native queue acceptance.
    pub request_timeout: Duration,
    /// Maximum time to drain stop/final events before native termination.
    pub shutdown_timeout: Duration,
    /// Recovery wake interval in case a platform loses an external signal.
    pub recovery_wake_interval: Duration,
}

impl Default for MpvWorkerConfig {
    fn default() -> Self {
        Self {
            startup_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(2),
            shutdown_timeout: Duration::from_secs(3),
            recovery_wake_interval: Duration::from_secs(1),
        }
    }
}

/// Result of ordered stop/event drain before the native core is destroyed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MpvShutdownReport {
    /// Stop request submitted during shutdown, if the core was still running.
    pub stop_request: Option<MpvRequestId>,
    /// Whether the matching async command reply was copied.
    pub stop_reply_received: bool,
    /// Whether an end-file event was copied during shutdown.
    pub end_file_received: bool,
    /// Whether the bounded drain reached its deadline.
    pub timed_out: bool,
    /// Number of copied native events forwarded during shutdown.
    pub forwarded_events: usize,
    /// Queueing failure, if stop could not be submitted.
    pub stop_error: Option<MpvSessionError>,
}

/// Failure to start or communicate with the serialized owner.
#[derive(Debug, thiserror::Error)]
pub enum MpvWorkerError {
    /// Native owner thread could not be spawned.
    #[error("could not spawn libmpv owner thread: {0}")]
    Spawn(#[source] std::io::Error),
    /// Session initialization failed on the owner thread.
    #[error("libmpv owner startup failed: {0}")]
    Startup(MpvSessionError),
    /// Owner did not complete startup within the configured bound.
    #[error("timed out waiting for libmpv owner startup")]
    StartupTimeout,
    /// Owner command channel is closed.
    #[error("libmpv owner command channel is closed")]
    CommandChannelClosed,
    /// Owner did not acknowledge a request within the configured bound.
    #[error("timed out waiting for libmpv owner request acknowledgement")]
    RequestTimeout,
    /// A serialized native operation failed.
    #[error(transparent)]
    Session(#[from] MpvSessionError),
    /// Owner thread panicked.
    #[error("libmpv owner thread panicked")]
    OwnerPanicked,
    /// An AppKit-local operation was attempted outside its main-thread token.
    #[error("the libmpv macOS operation requires the AppKit main thread")]
    AppKitMainThreadRequired,
}

type Response<T> = SyncSender<Result<T, MpvSessionError>>;

enum OwnerCommand {
    Command(Vec<String>, Response<MpvRequestId>),
    NodeCommand(MpvNode, Response<MpvRequestId>),
    Cancel(MpvRequestId, Response<()>),
    SetProperty(String, MpvNode, Response<MpvRequestId>),
    GetProperty(String, MpvFormat, Response<MpvRequestId>),
    Observe(String, MpvFormat, Response<MpvObservationId>),
    Unobserve(MpvObservationId, Response<()>),
    AddHook(String, i32, Response<MpvHookRegistrationId>),
    ContinueHook(MpvHookId, Response<()>),
    SetEvent(u32, bool, Response<()>),
    SetLogLevel(MpvLogLevel, Response<()>),
    Shutdown(SyncSender<MpvShutdownReport>),
}

/// Thread-safe command side of one serialized libmpv owner.
///
/// Native handles never cross this boundary. Every received [`MpvEvent`] owns
/// its complete payload and can be moved into an application reducer safely.
pub struct MpvWorker {
    command_tx: Sender<OwnerCommand>,
    event_rx: Receiver<MpvEvent>,
    owner_thread: Thread,
    join: Option<JoinHandle<()>>,
    config: MpvWorkerConfig,
    shutdown_rx: Option<Receiver<MpvShutdownReport>>,
    shutdown_report: Option<MpvShutdownReport>,
}

impl std::fmt::Debug for MpvWorker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MpvWorker")
            .field("owner_thread", &self.owner_thread.id())
            .field("config", &self.config)
            .field("shutdown_report", &self.shutdown_report)
            .finish_non_exhaustive()
    }
}

impl MpvWorker {
    /// Spawn a thread, then create and initialize libmpv on that thread.
    pub fn spawn(
        functions: MpvFunctionTable,
        session_config: MpvSessionConfig,
        worker_config: MpvWorkerConfig,
    ) -> Result<Self, MpvWorkerError> {
        Self::spawn_inner(functions, session_config, worker_config, None)
    }

    /// Spawn an owner and emit a coalescible signal after copied events are
    /// available. Notification delivery is best-effort and never blocks the
    /// native owner; callers should use a bounded channel and drain all events
    /// after each signal.
    pub fn spawn_with_event_notifier(
        functions: MpvFunctionTable,
        session_config: MpvSessionConfig,
        worker_config: MpvWorkerConfig,
        event_notifier: SyncSender<()>,
    ) -> Result<Self, MpvWorkerError> {
        Self::spawn_inner(
            functions,
            session_config,
            worker_config,
            Some(event_notifier),
        )
    }

    fn spawn_inner(
        functions: MpvFunctionTable,
        session_config: MpvSessionConfig,
        worker_config: MpvWorkerConfig,
        event_notifier: Option<SyncSender<()>>,
    ) -> Result<Self, MpvWorkerError> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);

        let join = thread::Builder::new()
            .name("ferrex-libmpv-owner".into())
            .spawn(move || {
                let session = MpvSession::create(functions, session_config);
                match session {
                    Ok(session) => {
                        let _ = startup_tx.send(Ok(thread::current()));
                        run_owner(
                            session,
                            command_rx,
                            event_tx,
                            event_notifier,
                            worker_config,
                        );
                    }
                    Err(error) => {
                        let _ = startup_tx.send(Err(error));
                    }
                }
            })
            .map_err(MpvWorkerError::Spawn)?;

        let owner_thread =
            match startup_rx.recv_timeout(worker_config.startup_timeout) {
                Ok(Ok(owner_thread)) => owner_thread,
                Ok(Err(error)) => {
                    let _ = join.join();
                    return Err(MpvWorkerError::Startup(error));
                }
                Err(RecvTimeoutError::Timeout) => {
                    // The thread may still be in native initialization. It owns all
                    // resources and will tear them down when command senders drop.
                    drop(command_tx);
                    return Err(MpvWorkerError::StartupTimeout);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let panicked = join.join().is_err();
                    return Err(if panicked {
                        MpvWorkerError::OwnerPanicked
                    } else {
                        MpvWorkerError::CommandChannelClosed
                    });
                }
            };

        Ok(Self {
            command_tx,
            event_rx,
            owner_thread,
            join: Some(join),
            config: worker_config,
            shutdown_rx: None,
            shutdown_report: None,
        })
    }

    /// Submit a pre-split arbitrary command.
    pub fn command_async<I, S>(
        &self,
        arguments: I,
    ) -> Result<MpvRequestId, MpvWorkerError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let arguments = arguments.into_iter().map(Into::into).collect();
        self.request(|response| OwnerCommand::Command(arguments, response))
    }

    /// Submit an arbitrary node-valued command.
    pub fn command_node_async(
        &self,
        command: MpvNode,
    ) -> Result<MpvRequestId, MpvWorkerError> {
        self.request(|response| OwnerCommand::NodeCommand(command, response))
    }

    /// Request cancellation of an in-flight command.
    pub fn cancel_request(
        &self,
        id: MpvRequestId,
    ) -> Result<(), MpvWorkerError> {
        self.request(|response| OwnerCommand::Cancel(id, response))
    }

    /// Set any typed or node-valued property.
    pub fn set_property_async(
        &self,
        name: impl Into<String>,
        value: MpvNode,
    ) -> Result<MpvRequestId, MpvWorkerError> {
        self.request(|response| {
            OwnerCommand::SetProperty(name.into(), value, response)
        })
    }

    /// Read any typed or node-valued property.
    pub fn get_property_async(
        &self,
        name: impl Into<String>,
        format: MpvFormat,
    ) -> Result<MpvRequestId, MpvWorkerError> {
        self.request(|response| {
            OwnerCommand::GetProperty(name.into(), format, response)
        })
    }

    /// Observe any property.
    pub fn observe_property(
        &self,
        name: impl Into<String>,
        format: MpvFormat,
    ) -> Result<MpvObservationId, MpvWorkerError> {
        self.request(|response| {
            OwnerCommand::Observe(name.into(), format, response)
        })
    }

    /// Remove one property observation.
    pub fn unobserve_property(
        &self,
        id: MpvObservationId,
    ) -> Result<(), MpvWorkerError> {
        self.request(|response| OwnerCommand::Unobserve(id, response))
    }

    /// Register an arbitrary mpv hook.
    pub fn add_hook(
        &self,
        name: impl Into<String>,
        priority: i32,
    ) -> Result<MpvHookRegistrationId, MpvWorkerError> {
        self.request(|response| {
            OwnerCommand::AddHook(name.into(), priority, response)
        })
    }

    /// Continue one received hook.
    pub fn continue_hook(&self, id: MpvHookId) -> Result<(), MpvWorkerError> {
        self.request(|response| OwnerCommand::ContinueHook(id, response))
    }

    /// Enable or disable a native event ID.
    pub fn set_event_enabled(
        &self,
        event_id: u32,
        enabled: bool,
    ) -> Result<(), MpvWorkerError> {
        self.request(|response| {
            OwnerCommand::SetEvent(event_id, enabled, response)
        })
    }

    /// Change native log filtering.
    pub fn set_log_level(
        &self,
        level: MpvLogLevel,
    ) -> Result<(), MpvWorkerError> {
        self.request(|response| OwnerCommand::SetLogLevel(level, response))
    }

    /// Drain all currently forwarded native events without blocking.
    pub fn drain_events(&self) -> Vec<MpvEvent> {
        self.event_rx.try_iter().collect()
    }

    /// Receive one forwarded native event with a deadline.
    pub fn recv_event_timeout(
        &self,
        timeout: Duration,
    ) -> Result<MpvEvent, RecvTimeoutError> {
        self.event_rx.recv_timeout(timeout)
    }

    /// Begin ordered shutdown without waiting for native teardown.
    ///
    /// This is the required first half of AppKit-safe shutdown. mpv's macOS VO
    /// dispatches teardown work synchronously to the main queue, so an AppKit
    /// event-loop callback must start shutdown, return to the run loop, and
    /// poll [`Self::try_finish_shutdown`] later instead of blocking in
    /// [`Self::shutdown`]. Calling this method repeatedly is idempotent.
    pub fn begin_shutdown(&mut self) -> Result<(), MpvWorkerError> {
        if self.shutdown_report.is_some() || self.shutdown_rx.is_some() {
            return Ok(());
        }
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        if self
            .command_tx
            .send(OwnerCommand::Shutdown(response_tx))
            .is_err()
        {
            return Err(MpvWorkerError::CommandChannelClosed);
        }
        self.shutdown_rx = Some(response_rx);
        self.owner_thread.unpark();
        Ok(())
    }

    /// Poll an ordered shutdown without blocking the caller.
    ///
    /// `Ok(None)` means native teardown is still pending and the caller must
    /// yield back to its platform event loop. A completed report is not made
    /// observable until the owner has dropped `MpvSession`, including removal
    /// of the wakeup callback and `mpv_terminate_destroy`.
    pub fn try_finish_shutdown(
        &mut self,
    ) -> Result<Option<MpvShutdownReport>, MpvWorkerError> {
        if let Some(report) = self.shutdown_report.clone() {
            return Ok(Some(report));
        }
        let Some(response_rx) = self.shutdown_rx.as_ref() else {
            return Ok(None);
        };
        match response_rx.try_recv() {
            Ok(report) => {
                self.shutdown_rx = None;
                self.join_owner()?;
                self.shutdown_report = Some(report.clone());
                Ok(Some(report))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.shutdown_rx = None;
                self.join_owner()?;
                Err(MpvWorkerError::CommandChannelClosed)
            }
        }
    }

    /// Whether ordered native teardown has started but has not completed.
    pub const fn shutdown_pending(&self) -> bool {
        self.shutdown_rx.is_some() && self.shutdown_report.is_none()
    }

    /// Stop, drain final events, destroy libmpv on its owner thread, and join.
    /// Calling this more than once returns the first report.
    ///
    /// Do not call this blocking convenience from the AppKit main thread once
    /// a macOS VO has been created. Use [`Self::begin_shutdown`] and
    /// [`Self::try_finish_shutdown`] so the main run loop remains serviceable.
    pub fn shutdown(&mut self) -> Result<MpvShutdownReport, MpvWorkerError> {
        if let Some(report) = self.shutdown_report.clone() {
            return Ok(report);
        }
        self.begin_shutdown()?;

        let response_timeout = self
            .config
            .shutdown_timeout
            .saturating_add(self.config.request_timeout);
        let response_rx = self
            .shutdown_rx
            .as_ref()
            .expect("begin_shutdown installs a completion receiver");
        let report = match response_rx.recv_timeout(response_timeout) {
            Ok(report) => report,
            Err(RecvTimeoutError::Timeout) => {
                return Err(MpvWorkerError::RequestTimeout);
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.shutdown_rx = None;
                self.join_owner()?;
                return Err(MpvWorkerError::CommandChannelClosed);
            }
        };
        self.shutdown_rx = None;
        self.join_owner()?;
        self.shutdown_report = Some(report.clone());
        Ok(report)
    }

    fn request<T>(
        &self,
        make_command: impl FnOnce(Response<T>) -> OwnerCommand,
    ) -> Result<T, MpvWorkerError> {
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        self.command_tx
            .send(make_command(response_tx))
            .map_err(|_| MpvWorkerError::CommandChannelClosed)?;
        self.owner_thread.unpark();
        match response_rx.recv_timeout(self.config.request_timeout) {
            Ok(result) => result.map_err(MpvWorkerError::Session),
            Err(RecvTimeoutError::Timeout) => {
                Err(MpvWorkerError::RequestTimeout)
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(MpvWorkerError::CommandChannelClosed)
            }
        }
    }

    fn join_owner(&mut self) -> Result<(), MpvWorkerError> {
        if let Some(join) = self.join.take() {
            join.join().map_err(|_| MpvWorkerError::OwnerPanicked)?;
        }
        Ok(())
    }
}

impl Drop for MpvWorker {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn run_owner(
    mut session: MpvSession,
    command_rx: Receiver<OwnerCommand>,
    event_tx: Sender<MpvEvent>,
    event_notifier: Option<SyncSender<()>>,
    config: MpvWorkerConfig,
) {
    let shutdown_completion = 'owner: loop {
        loop {
            match command_rx.try_recv() {
                Ok(OwnerCommand::Shutdown(response)) => {
                    let report = ordered_shutdown(
                        &mut session,
                        &event_tx,
                        event_notifier.as_ref(),
                        config.shutdown_timeout,
                    );
                    break 'owner Some((response, report));
                }
                Ok(command) => process_command(&mut session, command),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = ordered_shutdown(
                        &mut session,
                        &event_tx,
                        event_notifier.as_ref(),
                        config.shutdown_timeout,
                    );
                    break 'owner None;
                }
            }
        }

        match session.drain_events() {
            Ok(events) => {
                for event in events {
                    if !forward_event(&event_tx, event_notifier.as_ref(), event)
                    {
                        let _ = ordered_shutdown(
                            &mut session,
                            &event_tx,
                            event_notifier.as_ref(),
                            config.shutdown_timeout,
                        );
                        break 'owner None;
                    }
                }
            }
            Err(_) => break 'owner None,
        }

        session.wait_for_wakeup(config.recovery_wake_interval);
    };

    // mpv's macOS VO may synchronously dispatch AppKit teardown from this
    // drop. Publish completion only afterwards so poll-driven callers can
    // safely release native hosts and the worker once they observe the report.
    drop(session);
    if let Some((response, report)) = shutdown_completion {
        let _ = response.send(report);
    }
}

fn process_command(session: &mut MpvSession, command: OwnerCommand) {
    match command {
        OwnerCommand::Command(arguments, response) => {
            let _ = response.send(session.command_async(arguments));
        }
        OwnerCommand::NodeCommand(command, response) => {
            let _ = response.send(session.command_node_async(&command));
        }
        OwnerCommand::Cancel(id, response) => {
            let _ = response.send(session.cancel_request(id));
        }
        OwnerCommand::SetProperty(name, value, response) => {
            let _ = response.send(session.set_property_async(&name, &value));
        }
        OwnerCommand::GetProperty(name, format, response) => {
            let _ = response.send(session.get_property_async(&name, format));
        }
        OwnerCommand::Observe(name, format, response) => {
            let _ = response.send(session.observe_property(&name, format));
        }
        OwnerCommand::Unobserve(id, response) => {
            let _ = response.send(session.unobserve_property(id));
        }
        OwnerCommand::AddHook(name, priority, response) => {
            let _ = response.send(session.add_hook(&name, priority));
        }
        OwnerCommand::ContinueHook(id, response) => {
            let _ = response.send(session.continue_hook(id));
        }
        OwnerCommand::SetEvent(event_id, enabled, response) => {
            let _ = response.send(session.set_event_enabled(event_id, enabled));
        }
        OwnerCommand::SetLogLevel(level, response) => {
            let _ = response.send(session.set_log_level(level));
        }
        OwnerCommand::Shutdown(_) => {
            unreachable!("shutdown handled by owner loop")
        }
    }
}

fn forward_event(
    event_tx: &Sender<MpvEvent>,
    event_notifier: Option<&SyncSender<()>>,
    event: MpvEvent,
) -> bool {
    if event_tx.send(event).is_err() {
        return false;
    }
    if let Some(notifier) = event_notifier {
        // A full channel already represents pending work. A disconnected
        // notifier does not affect event ownership or ordered shutdown.
        let _ = notifier.try_send(());
    }
    true
}

fn ordered_shutdown(
    session: &mut MpvSession,
    event_tx: &Sender<MpvEvent>,
    event_notifier: Option<&SyncSender<()>>,
    timeout: Duration,
) -> MpvShutdownReport {
    let mut report = MpvShutdownReport::default();
    if session.is_shutting_down() {
        return report;
    }

    match session.command_async(["stop"]) {
        Ok(id) => report.stop_request = Some(id),
        Err(error) => {
            report.stop_error = Some(error);
            return report;
        }
    }

    let deadline = Instant::now() + timeout;
    let mut quiet_since = None;
    loop {
        let events = match session.drain_events() {
            Ok(events) => events,
            Err(error) => {
                report.stop_error = Some(error);
                return report;
            }
        };
        if events.is_empty() {
            if report.stop_reply_received {
                let quiet = quiet_since.get_or_insert_with(Instant::now);
                if quiet.elapsed() >= Duration::from_millis(25) {
                    return report;
                }
            }
        } else {
            quiet_since = None;
        }

        for event in events {
            if matches!(
                &event,
                MpvEvent::AsyncReply(reply)
                    if Some(reply.id) == report.stop_request
            ) {
                report.stop_reply_received = true;
            }
            if matches!(
                &event,
                MpvEvent::EndFile(end)
                    if matches!(
                        end.reason,
                        MpvEndFileReason::Stop
                            | MpvEndFileReason::Eof
                            | MpvEndFileReason::Quit
                    )
            ) {
                report.end_file_received = true;
            }
            report.forwarded_events = report.forwarded_events.saturating_add(1);
            let _ = forward_event(event_tx, event_notifier, event);
        }

        let now = Instant::now();
        if now >= deadline {
            report.timed_out = true;
            return report;
        }
        session.wait_for_wakeup(
            deadline
                .saturating_duration_since(now)
                .min(Duration::from_millis(25)),
        );
    }
}

// The macOS VO/AppKit ownership model remains an explicit P3 validation gate;
// do not claim a background-thread owner there from a headless unit test.
#[cfg(all(test, feature = "linked", not(target_os = "macos")))]
mod linked_tests {
    use super::*;

    #[test]
    fn worker_serializes_real_property_and_ordered_shutdown() {
        let (notifier_tx, notifier_rx) = mpsc::sync_channel(1);
        let mut worker = MpvWorker::spawn_with_event_notifier(
            MpvFunctionTable::linked(),
            MpvSessionConfig::default(),
            MpvWorkerConfig::default(),
            notifier_tx,
        )
        .unwrap();
        let request = worker
            .get_property_async("mpv-version", MpvFormat::String)
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            notifier_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("copied events notify the consumer");
            let events = worker.drain_events();
            if events.iter().any(|event| {
                matches!(
                    event,
                    MpvEvent::AsyncReply(reply) if reply.id == request
                )
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "property reply timed out");
        }

        let report = worker.shutdown().unwrap();
        assert!(report.stop_request.is_some());
        assert!(report.stop_reply_received, "{report:?}");
        assert!(!report.timed_out, "{report:?}");
        assert_eq!(worker.shutdown().unwrap(), report);
    }
}

#[cfg(all(test, feature = "linked"))]
mod linked_headless_shutdown_tests {
    use super::*;

    #[test]
    fn worker_supports_poll_driven_headless_shutdown() {
        let mut worker = MpvWorker::spawn(
            MpvFunctionTable::linked(),
            MpvSessionConfig::default(),
            MpvWorkerConfig::default(),
        )
        .unwrap();

        worker.begin_shutdown().unwrap();
        assert!(worker.shutdown_pending());
        let deadline = Instant::now() + Duration::from_secs(5);
        let report = loop {
            if let Some(report) = worker.try_finish_shutdown().unwrap() {
                break report;
            }
            assert!(
                Instant::now() < deadline,
                "poll-driven headless shutdown timed out"
            );
            thread::sleep(Duration::from_millis(1));
        };

        assert!(!worker.shutdown_pending());
        assert!(report.stop_request.is_some());
        assert!(report.stop_reply_received, "{report:?}");
        assert!(!report.timed_out, "{report:?}");
        assert_eq!(worker.try_finish_shutdown().unwrap(), Some(report));
    }
}
