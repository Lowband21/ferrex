//! Optional libmpv control-plane foundation for Ferrex desktop playback.
//!
//! The default build has no native libmpv dependency. Enable the `linked`
//! feature only in environments that provide the selected shared library. The
//! safe owner is built over a small function table so lifecycle and failure
//! behavior remain testable without a display or libmpv installation.

#![forbid(unsafe_op_in_unsafe_fn)]

/// Raw function-table compatibility checks and libmpv handle ownership.
pub mod ffi;
/// AppKit main-thread and poll-driven native teardown boundary.
pub mod macos;
mod node;
mod owner;
mod raw;
mod session;

pub use ffi::{
    BINDINGS_CLIENT_API, MINIMUM_CLIENT_API, MpvClientApiVersion,
    MpvCompatibilityReport, MpvFfiError, MpvFunctionTable, MpvHandle,
};
pub use node::{MpvFormat, MpvNode, MpvNodeError, MpvNodeLimits};
pub use owner::{
    MpvShutdownReport, MpvWorker, MpvWorkerConfig, MpvWorkerError,
};
pub use session::{
    MpvAsyncReply, MpvConfigPolicy, MpvEndFile, MpvEndFileReason, MpvError,
    MpvEvent, MpvHook, MpvHookId, MpvHookRegistrationId, MpvLogLevel,
    MpvLogMessage, MpvMessageLevel, MpvObservationId, MpvOption,
    MpvPropertyChange, MpvRequestId, MpvRequestKind, MpvSession,
    MpvSessionConfig, MpvSessionError, MpvWakeupSignal,
};
