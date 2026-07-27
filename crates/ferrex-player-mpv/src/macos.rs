//! AppKit main-thread and non-blocking shutdown boundary for macOS.
//!
//! mpv's modern macOS VO performs synchronous dispatches onto the AppKit main
//! queue while configuring and tearing down its native window. The libmpv
//! owner may remain on its serialized worker thread, but a caller on AppKit's
//! main thread must never wait for that worker to terminate. This module makes
//! the required start/yield/poll sequence explicit without moving native
//! objects across threads.

use std::{marker::PhantomData, rc::Rc, thread::ThreadId};

use crate::{MpvShutdownReport, MpvWorker, MpvWorkerError};

/// Failure to enter an AppKit-only operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AppKitMainThreadError {
    /// The operation is meaningful only in a macOS process.
    #[error("AppKit main-thread access is available only on macOS")]
    UnsupportedPlatform,
    /// The current callback is not executing on AppKit's process main thread.
    #[error(
        "the native presenter operation must run on the AppKit main thread"
    )]
    NotMainThread,
    /// A token was used from a different thread than the one that acquired it.
    #[error("the AppKit main-thread token was used from another thread")]
    WrongThread,
}

/// Proof that the current callback is executing on the AppKit main thread.
///
/// The token is deliberately neither `Send` nor `Sync`. Platform presenter
/// code should acquire it inside Iced's event-loop-local window callback and
/// require a borrow for every AppKit object access.
pub struct AppKitMainThreadToken {
    thread: ThreadId,
    _event_loop_local: PhantomData<Rc<()>>,
}

impl std::fmt::Debug for AppKitMainThreadToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppKitMainThreadToken")
            .finish_non_exhaustive()
    }
}

impl AppKitMainThreadToken {
    /// Acquire a non-transferable token on the process's AppKit main thread.
    pub fn acquire() -> Result<Self, AppKitMainThreadError> {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: `pthread_main_np` takes no arguments and has no lifetime
            // requirements. It returns non-zero only on the process main
            // thread, which is AppKit's required UI thread.
            if unsafe { pthread_main_np() } == 0 {
                return Err(AppKitMainThreadError::NotMainThread);
            }
            Ok(Self::for_current_thread())
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(AppKitMainThreadError::UnsupportedPlatform)
        }
    }

    /// Verify that this token is still used by its acquiring callback thread.
    pub fn verify(&self) -> Result<(), AppKitMainThreadError> {
        if self.thread == std::thread::current().id() {
            Ok(())
        } else {
            Err(AppKitMainThreadError::WrongThread)
        }
    }

    #[cfg(any(target_os = "macos", test))]
    fn for_current_thread() -> Self {
        Self {
            thread: std::thread::current().id(),
            _event_loop_local: PhantomData,
        }
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn pthread_main_np() -> std::os::raw::c_int;
}

/// State of the AppKit-safe owner shutdown handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKitShutdownState {
    /// No shutdown request has been sent.
    Ready,
    /// The owner is draining and/or waiting for AppKit teardown work.
    WaitingForOwner,
    /// Native teardown and the owner join both completed.
    Complete,
}

/// Result of one non-blocking shutdown poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppKitShutdownPoll {
    /// Return to the AppKit run loop before polling again.
    YieldToRunLoop,
    /// The native core is fully destroyed.
    Complete(MpvShutdownReport),
}

/// Main-thread-local driver for libmpv teardown that must service AppKit.
#[derive(Debug)]
pub struct AppKitShutdownDriver {
    state: AppKitShutdownState,
    report: Option<MpvShutdownReport>,
    _event_loop_local: PhantomData<Rc<()>>,
}

impl Default for AppKitShutdownDriver {
    fn default() -> Self {
        Self {
            state: AppKitShutdownState::Ready,
            report: None,
            _event_loop_local: PhantomData,
        }
    }
}

impl AppKitShutdownDriver {
    /// Current handshake state.
    pub const fn state(&self) -> AppKitShutdownState {
        self.state
    }

    /// Begin ordered owner shutdown and immediately return to the caller.
    pub fn begin(
        &mut self,
        main_thread: &AppKitMainThreadToken,
        worker: &mut MpvWorker,
    ) -> Result<(), MpvWorkerError> {
        main_thread
            .verify()
            .map_err(|_| MpvWorkerError::AppKitMainThreadRequired)?;
        self.begin_with(|| worker.begin_shutdown())
    }

    /// Poll once; pending work must yield back to AppKit rather than spin.
    pub fn poll(
        &mut self,
        main_thread: &AppKitMainThreadToken,
        worker: &mut MpvWorker,
    ) -> Result<AppKitShutdownPoll, MpvWorkerError> {
        main_thread
            .verify()
            .map_err(|_| MpvWorkerError::AppKitMainThreadRequired)?;
        self.poll_with(|| worker.try_finish_shutdown())
    }

    fn begin_with(
        &mut self,
        begin: impl FnOnce() -> Result<(), MpvWorkerError>,
    ) -> Result<(), MpvWorkerError> {
        match self.state {
            AppKitShutdownState::Ready => {
                begin()?;
                self.state = AppKitShutdownState::WaitingForOwner;
            }
            AppKitShutdownState::WaitingForOwner
            | AppKitShutdownState::Complete => {}
        }
        Ok(())
    }

    fn poll_with(
        &mut self,
        poll: impl FnOnce() -> Result<Option<MpvShutdownReport>, MpvWorkerError>,
    ) -> Result<AppKitShutdownPoll, MpvWorkerError> {
        match self.state {
            AppKitShutdownState::Ready => {
                Ok(AppKitShutdownPoll::YieldToRunLoop)
            }
            AppKitShutdownState::WaitingForOwner => match poll()? {
                Some(report) => {
                    self.state = AppKitShutdownState::Complete;
                    self.report = Some(report.clone());
                    Ok(AppKitShutdownPoll::Complete(report))
                }
                None => Ok(AppKitShutdownPoll::YieldToRunLoop),
            },
            AppKitShutdownState::Complete => Ok(AppKitShutdownPoll::Complete(
                self.report
                    .clone()
                    .expect("complete AppKit shutdown retains its report"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_requires_a_yield_between_begin_and_completion() {
        let mut driver = AppKitShutdownDriver::default();
        let mut began = false;
        driver
            .begin_with(|| {
                began = true;
                Ok(())
            })
            .unwrap();

        assert!(began);
        assert_eq!(driver.state(), AppKitShutdownState::WaitingForOwner);
        assert_eq!(
            driver.poll_with(|| Ok(None)).unwrap(),
            AppKitShutdownPoll::YieldToRunLoop
        );

        let report = MpvShutdownReport {
            stop_reply_received: true,
            ..MpvShutdownReport::default()
        };
        assert_eq!(
            driver.poll_with(|| Ok(Some(report.clone()))).unwrap(),
            AppKitShutdownPoll::Complete(report.clone())
        );
        assert_eq!(driver.state(), AppKitShutdownState::Complete);
        assert_eq!(
            driver
                .poll_with(|| panic!("completed driver must not poll again"))
                .unwrap(),
            AppKitShutdownPoll::Complete(report)
        );
    }

    #[test]
    fn duplicate_begin_does_not_send_another_shutdown() {
        let mut driver = AppKitShutdownDriver::default();
        let mut begins = 0;
        driver
            .begin_with(|| {
                begins += 1;
                Ok(())
            })
            .unwrap();
        driver
            .begin_with(|| {
                begins += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(begins, 1);
    }

    #[test]
    fn token_rejects_cross_thread_use() {
        let token = AppKitMainThreadToken::for_current_thread();
        assert_eq!(token.verify(), Ok(()));

        let owner = token.thread;
        let other =
            std::thread::spawn(move || owner == std::thread::current().id())
                .join()
                .unwrap();
        assert!(!other);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn appkit_token_is_unavailable_off_macos() {
        assert!(matches!(
            AppKitMainThreadToken::acquire(),
            Err(AppKitMainThreadError::UnsupportedPlatform)
        ));
    }
}
