//! Serialized libmpv control plane and owned event delivery.

use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr::{self, NonNull},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, Thread},
    time::Duration,
};

use crate::{
    MpvFfiError, MpvFunctionTable, MpvHandle,
    node::{
        MpvFormat, MpvNode, MpvNodeError, MpvNodeLimits, RawNodeArena,
        copy_raw_event_value, copy_raw_node,
    },
    raw::{
        END_FILE_REASON_EOF, END_FILE_REASON_ERROR, END_FILE_REASON_QUIT,
        END_FILE_REASON_REDIRECT, END_FILE_REASON_STOP, EVENT_AUDIO_RECONFIG,
        EVENT_CLIENT_MESSAGE, EVENT_COMMAND_REPLY, EVENT_END_FILE,
        EVENT_FILE_LOADED, EVENT_GET_PROPERTY_REPLY, EVENT_HOOK, EVENT_IDLE,
        EVENT_LOG_MESSAGE, EVENT_NONE, EVENT_PLAYBACK_RESTART,
        EVENT_PROPERTY_CHANGE, EVENT_QUEUE_OVERFLOW, EVENT_SEEK,
        EVENT_SET_PROPERTY_REPLY, EVENT_SHUTDOWN, EVENT_START_FILE, EVENT_TICK,
        EVENT_VIDEO_RECONFIG, FORMAT_DOUBLE, FORMAT_FLAG, FORMAT_INT64,
        FORMAT_NODE, FORMAT_STRING, MpvControlApi, RawMpvEvent,
        RawMpvEventClientMessage, RawMpvEventCommand, RawMpvEventEndFile,
        RawMpvEventHook, RawMpvEventLogMessage, RawMpvEventProperty,
        RawMpvEventStartFile, RawMpvHandle,
    },
};

/// Correlation identity for an asynchronous command or property operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MpvRequestId(u64);

impl MpvRequestId {
    /// Construct an ID for diagnostics or a fake backend.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Native `reply_userdata` value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identity for a property observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MpvObservationId(u64);

impl MpvObservationId {
    /// Construct an ID for diagnostics or a fake backend.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Native `reply_userdata` value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identity for a registered mpv hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MpvHookRegistrationId(u64);

impl MpvHookRegistrationId {
    /// Native `reply_userdata` value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Identity that must be continued exactly once after a hook event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MpvHookId(u64);

impl MpvHookId {
    /// Native hook identity.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Kind of asynchronous operation represented by a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpvRequestKind {
    /// String-vector command.
    Command,
    /// Node-valued command.
    NodeCommand,
    /// Property read.
    GetProperty,
    /// Property write.
    SetProperty,
}

/// One native mpv error code with a non-sensitive static description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("libmpv error {code}: {description}")]
pub struct MpvError {
    /// Native negative `mpv_error` value.
    pub code: i32,
    /// Static description derived from the public error enum.
    pub description: &'static str,
}

impl MpvError {
    fn from_code(code: i32) -> Self {
        Self {
            code,
            description: mpv_error_description(code),
        }
    }
}

const fn mpv_error_description(code: i32) -> &'static str {
    match code {
        0.. => "success",
        -1 => "event queue full",
        -2 => "out of memory",
        -3 => "uninitialized",
        -4 => "invalid parameter",
        -5 => "option not found",
        -6 => "option format unsupported",
        -7 => "option error",
        -8 => "property not found",
        -9 => "property format unsupported",
        -10 => "property unavailable",
        -11 => "property error",
        -12 => "command error",
        -13 => "loading failed",
        -14 => "audio output initialization failed",
        -15 => "video output initialization failed",
        -16 => "nothing to play",
        -17 => "unknown media format",
        -18 => "unsupported",
        -19 => "not implemented",
        -20 => "generic error",
        _ => "unknown error",
    }
}

/// Control-plane setup or submission failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MpvSessionError {
    /// Handle/version lifecycle failed.
    #[error(transparent)]
    Ffi(#[from] MpvFfiError),
    /// The function table only contains P2 lifecycle symbols.
    #[error("libmpv function table does not provide control-plane symbols")]
    MissingControlApi,
    /// A name/value could not cross the C string boundary.
    #[error("{category} contains an interior NUL byte")]
    InteriorNul {
        /// Non-sensitive value category.
        category: &'static str,
    },
    /// An asynchronous command did not contain a command name.
    #[error("mpv command must contain at least one argument")]
    EmptyCommand,
    /// A node command was not an array or map.
    #[error("mpv node command root must be an array or map")]
    InvalidNodeCommand,
    /// A native API call rejected a request before it could be queued.
    #[error("{operation} failed: {error}")]
    NativeCall {
        /// Static operation name.
        operation: &'static str,
        /// Native error.
        error: MpvError,
    },
    /// Request/observation/hook identities cannot be allocated anymore.
    #[error("libmpv userdata identity space is exhausted")]
    UserdataExhausted,
    /// Node conversion failed.
    #[error(transparent)]
    Node(#[from] MpvNodeError),
    /// The request is not currently pending.
    #[error("mpv request {0} is not pending")]
    UnknownRequest(u64),
    /// The observation is not currently registered.
    #[error("mpv observation {0} is not registered")]
    UnknownObservation(u64),
    /// The hook event has already been continued or was never received.
    #[error("mpv hook {0} is not awaiting continuation")]
    UnknownHook(u64),
    /// mpv has entered shutdown and rejects new work.
    #[error("libmpv session is shutting down")]
    ShuttingDown,
    /// `mpv_wait_event` violated its documented non-null contract.
    #[error("libmpv returned a null event pointer")]
    NullEvent,
}

/// Log filtering requested from libmpv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpvLogLevel {
    /// Disable native log events.
    None,
    /// Fatal only.
    Fatal,
    /// Errors and above.
    Error,
    /// Warnings and above.
    Warn,
    /// Informational messages and above.
    Info,
    /// Verbose messages and above.
    Verbose,
    /// Debug messages and above.
    Debug,
    /// All trace messages.
    Trace,
}

impl MpvLogLevel {
    const fn as_str(self) -> &'static str {
        match self {
            Self::None => "no",
            Self::Fatal => "fatal",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Verbose => "v",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

/// One deterministic pre-initialization option.
#[derive(Clone, PartialEq, Eq)]
pub struct MpvOption {
    name: String,
    value: String,
}

impl MpvOption {
    /// Create an option. Validation happens at session construction.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    /// Option name without leading dashes.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl std::fmt::Debug for MpvOption {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MpvOption")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// Policy controlling whether mpv may load configuration from the user's
/// standard mpv directories.
///
/// The trusted mode is deliberately coarse-grained: mpv configuration can
/// reference scripts and other native resources, so enabling it is equivalent
/// to allowing trusted code to execute inside the Ferrex process.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MpvConfigPolicy {
    /// Use only Ferrex-owned options and do not discover user config or scripts.
    #[default]
    Deterministic,
    /// Load the user's standard mpv config, input bindings, and scripts.
    TrustedUser,
}

impl MpvConfigPolicy {
    /// Stable diagnostic/configuration label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::TrustedUser => "trusted-user",
        }
    }

    /// Whether standard user config discovery is enabled.
    pub const fn user_config_enabled(self) -> bool {
        matches!(self, Self::TrustedUser)
    }

    /// Whether standard user script discovery is enabled.
    pub const fn user_scripts_enabled(self) -> bool {
        matches!(self, Self::TrustedUser)
    }
}

/// Deterministic initialization policy for one mpv core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpvSessionConfig {
    options: Vec<MpvOption>,
    log_level: MpvLogLevel,
    node_limits: MpvNodeLimits,
    config_policy: MpvConfigPolicy,
}

impl Default for MpvSessionConfig {
    fn default() -> Self {
        Self::native_window()
    }
}

impl MpvSessionConfig {
    /// Built-in profile for mpv's ordinary native VO/window.
    ///
    /// No render context, user config, scripts, external URL resolver, OSC, or
    /// default input bindings are enabled. A later native-window selector may
    /// opt into controlled OSC/input behavior explicitly.
    pub fn native_window() -> Self {
        Self::native_window_with_config_policy(MpvConfigPolicy::Deterministic)
    }

    /// Built-in native-window profile with an explicit user-config trust
    /// policy.
    ///
    /// [`MpvConfigPolicy::TrustedUser`] enables mpv's standard config and
    /// script discovery. Callers must expose that as an explicit trusted-code
    /// opt-in; it must never be inferred from the presence of config files.
    pub fn native_window_with_config_policy(
        config_policy: MpvConfigPolicy,
    ) -> Self {
        let discovered_config = if config_policy.user_config_enabled() {
            "yes"
        } else {
            "no"
        };
        let discovered_scripts = if config_policy.user_scripts_enabled() {
            "yes"
        } else {
            "no"
        };

        Self {
            options: vec![
                MpvOption::new("config", discovered_config),
                MpvOption::new("load-scripts", discovered_scripts),
                // External URL resolvers remain disabled in both policies.
                // Trusted users can still configure mpv-native functionality,
                // but Ferrex does not package or invoke yt-dlp implicitly.
                MpvOption::new("ytdl", "no"),
                MpvOption::new("terminal", "no"),
                MpvOption::new("osc", "no"),
                MpvOption::new("input-default-bindings", "no"),
                MpvOption::new("input-vo-keyboard", "no"),
                MpvOption::new("vo", "gpu-next,gpu"),
                MpvOption::new("hwdec", "auto-safe"),
            ],
            log_level: MpvLogLevel::Info,
            node_limits: MpvNodeLimits::default(),
            config_policy,
        }
    }

    /// Append or override an mpv option. mpv applies repeated options in order.
    pub fn with_option(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.options.push(MpvOption::new(name, value));
        self
    }

    /// Set native log filtering.
    pub const fn with_log_level(mut self, level: MpvLogLevel) -> Self {
        self.log_level = level;
        self
    }

    /// Set bounds for copied event/node payloads.
    pub const fn with_node_limits(mut self, limits: MpvNodeLimits) -> Self {
        self.node_limits = limits;
        self
    }

    /// Ordered options passed before `mpv_initialize`.
    pub fn options(&self) -> &[MpvOption] {
        &self.options
    }

    /// User-config trust policy represented by this profile.
    pub const fn config_policy(&self) -> MpvConfigPolicy {
        self.config_policy
    }
}

/// Coalescing wakeup signal installed as libmpv's callback userdata.
///
/// The foreign callback performs only an atomic transition and `Thread::unpark`;
/// it never calls libmpv, allocates, locks, blocks, or unwinds.
pub struct MpvWakeupSignal {
    pending: AtomicBool,
    notifications: AtomicU64,
    owner: Thread,
}

impl std::fmt::Debug for MpvWakeupSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MpvWakeupSignal")
            .field("pending", &self.pending.load(Ordering::Relaxed))
            .field("notifications", &self.notifications.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl MpvWakeupSignal {
    fn for_current_thread() -> Arc<Self> {
        Arc::new(Self {
            pending: AtomicBool::new(false),
            notifications: AtomicU64::new(0),
            owner: thread::current(),
        })
    }

    /// Consume the current coalesced wakeup bit.
    pub fn take_pending(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }

    /// Number of callback invocations, saturated on overflow.
    pub fn notification_count(&self) -> u64 {
        self.notifications.load(Ordering::Relaxed)
    }

    fn notify(&self) {
        let _ = self.notifications.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |value| Some(value.saturating_add(1)),
        );
        if !self.pending.swap(true, Ordering::AcqRel) {
            self.owner.unpark();
        }
    }
}

unsafe extern "C" fn wakeup_callback(userdata: *mut c_void) {
    let Some(signal) = NonNull::new(userdata.cast::<MpvWakeupSignal>()) else {
        return;
    };
    // SAFETY: registration owns a strong Arc reference until after callback
    // removal and native handle destruction.
    unsafe { signal.as_ref() }.notify();
}

#[derive(Debug, Clone)]
struct PendingRequest {
    kind: MpvRequestKind,
    cancelled: bool,
}

#[derive(Debug, Clone)]
struct Observation {
    name: String,
    format: MpvFormat,
}

/// Completion of one asynchronous request.
#[derive(Debug, Clone, PartialEq)]
pub struct MpvAsyncReply {
    /// Correlated request identity.
    pub id: MpvRequestId,
    /// Submitted operation kind.
    pub kind: MpvRequestKind,
    /// Whether cancellation was requested before this reply was drained.
    pub cancellation_requested: bool,
    /// Native result or copied command/property value.
    pub result: Result<Option<MpvNode>, MpvError>,
}

/// Copied property observation event.
#[derive(Debug, Clone, PartialEq)]
pub struct MpvPropertyChange {
    /// Observation identity from registration.
    pub id: MpvObservationId,
    /// Copied property name.
    pub name: String,
    /// `None` means unavailable or notification-only format.
    pub value: Option<MpvNode>,
    /// Whether this identity is still known to the session.
    pub registered: bool,
}

/// Severity mapped from mpv's numeric and textual log levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpvMessageLevel {
    /// Fatal process/core condition.
    Fatal,
    /// Error.
    Error,
    /// Warning.
    Warn,
    /// Informational message.
    Info,
    /// Verbose informational message.
    Verbose,
    /// Debug message.
    Debug,
    /// Trace message.
    Trace,
    /// A future native level.
    Unknown(u32),
}

/// Redacted, Ferrex-owned native log message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpvLogMessage {
    /// Native component prefix.
    pub prefix: String,
    /// Mapped severity.
    pub level: MpvMessageLevel,
    /// Sanitized text with common credential forms removed.
    pub text: String,
}

/// Why one file ended according to mpv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpvEndFileReason {
    /// Normal end of file/range.
    Eof,
    /// Explicit stop command.
    Stop,
    /// Core quit/shutdown.
    Quit,
    /// Playback error.
    Error,
    /// Playlist redirect.
    Redirect,
    /// Future native reason.
    Unknown(u32),
}

/// Copied end-file details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpvEndFile {
    /// Native reason.
    pub reason: MpvEndFileReason,
    /// Native playback error when reason is `Error`.
    pub error: Option<MpvError>,
    /// Stable playlist entry identity.
    pub playlist_entry_id: i64,
    /// First inserted playlist identity for redirects.
    pub playlist_insert_id: i64,
    /// Number of inserted entries.
    pub playlist_insert_count: u32,
}

/// Copied hook event awaiting one continuation call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpvHook {
    /// Registration that produced the hook.
    pub registration: MpvHookRegistrationId,
    /// Native continuation identity.
    pub id: MpvHookId,
    /// Copied hook name.
    pub name: String,
}

/// Fully owned event; no variant borrows libmpv's event ring buffer.
#[derive(Debug, Clone, PartialEq)]
pub enum MpvEvent {
    /// Core is shutting down.
    Shutdown,
    /// Redacted native log message.
    Log(MpvLogMessage),
    /// Correlated asynchronous completion.
    AsyncReply(MpvAsyncReply),
    /// Reply did not match a pending request.
    UnmatchedAsyncReply {
        /// Native identity.
        id: MpvRequestId,
        /// Kind inferred from event ID.
        kind: MpvRequestKind,
        /// Native error, if any.
        error: Option<MpvError>,
    },
    /// Property observation changed.
    PropertyChanged(MpvPropertyChange),
    /// Playback is starting a playlist entry.
    StartFile {
        /// Stable native playlist entry identity.
        playlist_entry_id: i64,
    },
    /// Headers loaded and decoding can start.
    FileLoaded,
    /// Playback ended/unloaded a file.
    EndFile(MpvEndFile),
    /// Core entered idle mode.
    Idle,
    /// Script/client message with copied arguments.
    ClientMessage(Vec<String>),
    /// Video output was reconfigured.
    VideoReconfigured,
    /// Audio output was reconfigured.
    AudioReconfigured,
    /// Seek started.
    Seek,
    /// Playback restarted after load/seek.
    PlaybackRestart,
    /// Deprecated tick event retained for arbitrary event access.
    Tick,
    /// Native event queue overflowed.
    QueueOverflow,
    /// Hook awaiting continuation.
    Hook(MpvHook),
    /// Malformed native payload was copied into a safe diagnostic.
    ProtocolError {
        /// Native event ID.
        event_id: u32,
        /// Non-sensitive validation failure.
        message: String,
    },
    /// Future event ID not understood by this release.
    Unknown {
        /// Unrecognized native event identity.
        event_id: u32,
    },
}

/// Serialized owner of one initialized libmpv client handle.
///
/// The value is deliberately `!Send` through [`MpvHandle`]. All normal calls
/// require `&mut self`, and only this owner drains the event queue.
pub struct MpvSession {
    handle: Option<MpvHandle>,
    control: MpvControlApi,
    next_userdata: u64,
    pending: HashMap<u64, PendingRequest>,
    observations: HashMap<u64, Observation>,
    hook_registrations: HashMap<u64, String>,
    outstanding_hooks: HashSet<u64>,
    wakeup: Arc<MpvWakeupSignal>,
    wakeup_userdata: *const MpvWakeupSignal,
    node_limits: MpvNodeLimits,
    shutting_down: bool,
}

impl std::fmt::Debug for MpvSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MpvSession")
            .field("handle", &self.handle)
            .field("next_userdata", &self.next_userdata)
            .field("pending_requests", &self.pending.len())
            .field("observations", &self.observations.len())
            .field("hook_registrations", &self.hook_registrations.len())
            .field("outstanding_hooks", &self.outstanding_hooks.len())
            .field("wakeup", &self.wakeup)
            .field("shutting_down", &self.shutting_down)
            .finish_non_exhaustive()
    }
}

impl MpvSession {
    /// Create, configure, initialize, and install the signal-only wakeup callback.
    pub fn create(
        functions: MpvFunctionTable,
        config: MpvSessionConfig,
    ) -> Result<Self, MpvSessionError> {
        let control = functions
            .control_api()
            .ok_or(MpvSessionError::MissingControlApi)?;
        let mut handle = MpvHandle::create(functions)?;
        let raw = handle
            .raw_ptr()
            .expect("newly created mpv handle must be present")
            .as_ptr();

        for option in &config.options {
            let name = c_string(option.name(), "mpv option name")?;
            let value = c_string(&option.value, "mpv option value")?;
            // SAFETY: strings are live for this call and the table matches raw.
            let code = unsafe {
                (control.set_option_string)(raw, name.as_ptr(), value.as_ptr())
            };
            check_native("mpv_set_option_string", code)?;
        }

        let level = CString::new(config.log_level.as_str())
            .expect("built-in mpv log level has no NUL");
        // Request messages before initialization so runtime versions and the
        // compiled feature list are available to diagnostics. libmpv permits
        // this client-local event setting on a newly created handle.
        // SAFETY: the created handle and static-level CString are valid here.
        let code =
            unsafe { (control.request_log_messages)(raw, level.as_ptr()) };
        check_native("mpv_request_log_messages", code)?;
        handle.initialize()?;

        let wakeup = MpvWakeupSignal::for_current_thread();
        let wakeup_userdata = Arc::into_raw(Arc::clone(&wakeup));
        // SAFETY: the leaked Arc keeps userdata valid until explicit teardown;
        // callback only performs signal-safe Ferrex notification work.
        unsafe {
            (control.set_wakeup_callback)(
                raw,
                Some(wakeup_callback),
                wakeup_userdata.cast_mut().cast(),
            )
        };

        Ok(Self {
            handle: Some(handle),
            control,
            next_userdata: 1,
            pending: HashMap::new(),
            observations: HashMap::new(),
            hook_registrations: HashMap::new(),
            outstanding_hooks: HashSet::new(),
            wakeup,
            wakeup_userdata,
            node_limits: config.node_limits,
            shutting_down: false,
        })
    }

    /// Coalescing callback state for integration with an owner/runtime loop.
    pub fn wakeup_signal(&self) -> Arc<MpvWakeupSignal> {
        Arc::clone(&self.wakeup)
    }

    /// Park this owner thread until a callback arrives or `timeout` elapses.
    ///
    /// Commands from another queue should call `Thread::unpark` on their owner
    /// too; callers must always re-check both queues after this returns.
    pub fn wait_for_wakeup(&self, timeout: Duration) -> bool {
        if self.wakeup.take_pending() {
            return true;
        }
        thread::park_timeout(timeout);
        self.wakeup.take_pending()
    }

    /// Submit a standard pre-split mpv command.
    pub fn command_async<I, S>(
        &mut self,
        arguments: I,
    ) -> Result<MpvRequestId, MpvSessionError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.ensure_running()?;
        let arguments = arguments
            .into_iter()
            .map(|argument| c_string(argument.as_ref(), "mpv command argument"))
            .collect::<Result<Vec<_>, _>>()?;
        if arguments.is_empty() {
            return Err(MpvSessionError::EmptyCommand);
        }
        let mut pointers = arguments
            .iter()
            .map(|argument| argument.as_ptr())
            .chain(std::iter::once(ptr::null()))
            .collect::<Vec<_>>();
        let id = self.allocate_userdata()?;
        // SAFETY: pointer array is NUL-terminated and libmpv copies it before
        // returning from the asynchronous submission call.
        let code = unsafe {
            (self.control.command_async)(self.raw(), id, pointers.as_mut_ptr())
        };
        check_native("mpv_command_async", code)?;
        self.pending.insert(
            id,
            PendingRequest {
                kind: MpvRequestKind::Command,
                cancelled: false,
            },
        );
        Ok(MpvRequestId(id))
    }

    /// Submit an arbitrary array/map node command.
    pub fn command_node_async(
        &mut self,
        command: &MpvNode,
    ) -> Result<MpvRequestId, MpvSessionError> {
        self.ensure_running()?;
        if !matches!(command, MpvNode::Array(_) | MpvNode::Map(_)) {
            return Err(MpvSessionError::InvalidNodeCommand);
        }
        let mut arena = RawNodeArena::new(command)?;
        let id = self.allocate_userdata()?;
        // SAFETY: arena preserves every pointer until libmpv copies the command.
        let code = unsafe {
            (self.control.command_node_async)(self.raw(), id, arena.root_mut())
        };
        check_native("mpv_command_node_async", code)?;
        self.pending.insert(
            id,
            PendingRequest {
                kind: MpvRequestKind::NodeCommand,
                cancelled: false,
            },
        );
        Ok(MpvRequestId(id))
    }

    /// Request cancellation of an in-flight command. Completion still arrives
    /// through the normal correlated reply event.
    pub fn cancel_request(
        &mut self,
        id: MpvRequestId,
    ) -> Result<(), MpvSessionError> {
        let pending = self
            .pending
            .get_mut(&id.0)
            .ok_or(MpvSessionError::UnknownRequest(id.0))?;
        if !matches!(
            pending.kind,
            MpvRequestKind::Command | MpvRequestKind::NodeCommand
        ) {
            return Err(MpvSessionError::UnknownRequest(id.0));
        }
        pending.cancelled = true;
        // SAFETY: request identity belongs to this initialized handle.
        unsafe { (self.control.abort_async_command)(self.raw(), id.0) };
        Ok(())
    }

    /// Set a string, flag, integer, double, or arbitrary node property.
    pub fn set_property_async(
        &mut self,
        name: &str,
        value: &MpvNode,
    ) -> Result<MpvRequestId, MpvSessionError> {
        self.ensure_running()?;
        let name = c_string(name, "mpv property name")?;
        let id = self.allocate_userdata()?;

        let code = match value {
            MpvNode::String(value) => {
                let value = c_string(value, "mpv property string")?;
                let mut pointer = value.as_ptr();
                // SAFETY: libmpv copies data before this function returns.
                unsafe {
                    (self.control.set_property_async)(
                        self.raw(),
                        id,
                        name.as_ptr(),
                        FORMAT_STRING,
                        (&mut pointer as *mut *const c_char).cast(),
                    )
                }
            }
            MpvNode::Bool(value) => {
                let mut value = c_int::from(*value);
                // SAFETY: data points to a live native `int` value.
                unsafe {
                    (self.control.set_property_async)(
                        self.raw(),
                        id,
                        name.as_ptr(),
                        FORMAT_FLAG,
                        (&mut value as *mut c_int).cast(),
                    )
                }
            }
            MpvNode::Int(value) => {
                let mut value = *value;
                // SAFETY: data points to a live native `int64_t` value.
                unsafe {
                    (self.control.set_property_async)(
                        self.raw(),
                        id,
                        name.as_ptr(),
                        FORMAT_INT64,
                        (&mut value as *mut i64).cast(),
                    )
                }
            }
            MpvNode::Double(value) => {
                let mut value = *value;
                // SAFETY: data points to a live native `double` value.
                unsafe {
                    (self.control.set_property_async)(
                        self.raw(),
                        id,
                        name.as_ptr(),
                        FORMAT_DOUBLE,
                        (&mut value as *mut f64).cast(),
                    )
                }
            }
            MpvNode::Null
            | MpvNode::Array(_)
            | MpvNode::Map(_)
            | MpvNode::Bytes(_) => {
                let mut arena = RawNodeArena::new(value)?;
                // SAFETY: arena remains live until libmpv copies the node.
                unsafe {
                    (self.control.set_property_async)(
                        self.raw(),
                        id,
                        name.as_ptr(),
                        FORMAT_NODE,
                        arena.root_mut().cast(),
                    )
                }
            }
        };
        check_native("mpv_set_property_async", code)?;
        self.pending.insert(
            id,
            PendingRequest {
                kind: MpvRequestKind::SetProperty,
                cancelled: false,
            },
        );
        Ok(MpvRequestId(id))
    }

    /// Read a property in any supported typed or node format.
    pub fn get_property_async(
        &mut self,
        name: &str,
        format: MpvFormat,
    ) -> Result<MpvRequestId, MpvSessionError> {
        self.ensure_running()?;
        let name = c_string(name, "mpv property name")?;
        let id = self.allocate_userdata()?;
        // SAFETY: name is live for the call; no output pointer is retained.
        let code = unsafe {
            (self.control.get_property_async)(
                self.raw(),
                id,
                name.as_ptr(),
                format.raw(),
            )
        };
        check_native("mpv_get_property_async", code)?;
        self.pending.insert(
            id,
            PendingRequest {
                kind: MpvRequestKind::GetProperty,
                cancelled: false,
            },
        );
        Ok(MpvRequestId(id))
    }

    /// Observe an arbitrary property with a stable identity.
    pub fn observe_property(
        &mut self,
        name: &str,
        format: MpvFormat,
    ) -> Result<MpvObservationId, MpvSessionError> {
        self.ensure_running()?;
        let native_name = c_string(name, "mpv property name")?;
        let id = self.allocate_userdata()?;
        // SAFETY: libmpv copies the property name during registration.
        let code = unsafe {
            (self.control.observe_property)(
                self.raw(),
                id,
                native_name.as_ptr(),
                format.raw(),
            )
        };
        check_native("mpv_observe_property", code)?;
        self.observations.insert(
            id,
            Observation {
                name: name.to_owned(),
                format,
            },
        );
        Ok(MpvObservationId(id))
    }

    /// Remove one property observation.
    pub fn unobserve_property(
        &mut self,
        id: MpvObservationId,
    ) -> Result<(), MpvSessionError> {
        if !self.observations.contains_key(&id.0) {
            return Err(MpvSessionError::UnknownObservation(id.0));
        }
        // SAFETY: observation identity belongs to this handle.
        let code =
            unsafe { (self.control.unobserve_property)(self.raw(), id.0) };
        check_native("mpv_unobserve_property", code)?;
        self.observations.remove(&id.0);
        Ok(())
    }

    /// Register a documented or future mpv hook.
    pub fn add_hook(
        &mut self,
        name: &str,
        priority: i32,
    ) -> Result<MpvHookRegistrationId, MpvSessionError> {
        self.ensure_running()?;
        let native_name = c_string(name, "mpv hook name")?;
        let id = self.allocate_userdata()?;
        // SAFETY: libmpv copies the hook name during registration.
        let code = unsafe {
            (self.control.hook_add)(
                self.raw(),
                id,
                native_name.as_ptr(),
                priority,
            )
        };
        check_native("mpv_hook_add", code)?;
        self.hook_registrations.insert(id, name.to_owned());
        Ok(MpvHookRegistrationId(id))
    }

    /// Continue one received hook exactly once.
    pub fn continue_hook(
        &mut self,
        id: MpvHookId,
    ) -> Result<(), MpvSessionError> {
        if !self.outstanding_hooks.contains(&id.0) {
            return Err(MpvSessionError::UnknownHook(id.0));
        }
        // SAFETY: ID was copied from one outstanding hook on this handle.
        let code = unsafe { (self.control.hook_continue)(self.raw(), id.0) };
        check_native("mpv_hook_continue", code)?;
        self.outstanding_hooks.remove(&id.0);
        Ok(())
    }

    /// Enable/disable an arbitrary native event ID.
    pub fn set_event_enabled(
        &mut self,
        event_id: u32,
        enabled: bool,
    ) -> Result<(), MpvSessionError> {
        self.ensure_running()?;
        // SAFETY: libmpv validates unknown event IDs and enable is 0/1.
        let code = unsafe {
            (self.control.request_event)(
                self.raw(),
                event_id,
                c_int::from(enabled),
            )
        };
        check_native("mpv_request_event", code)
    }

    /// Change native log filtering at runtime.
    pub fn set_log_level(
        &mut self,
        level: MpvLogLevel,
    ) -> Result<(), MpvSessionError> {
        self.ensure_running()?;
        let level = CString::new(level.as_str())
            .expect("built-in mpv log level has no NUL");
        // SAFETY: level is live for the call and handle is initialized.
        let code = unsafe {
            (self.control.request_log_messages)(self.raw(), level.as_ptr())
        };
        check_native("mpv_request_log_messages", code)
    }

    /// Drain all currently queued events and copy every payload before asking
    /// libmpv for the next event.
    pub fn drain_events(&mut self) -> Result<Vec<MpvEvent>, MpvSessionError> {
        let mut events = Vec::new();
        loop {
            // SAFETY: this serialized owner is the only event-queue consumer.
            let raw = unsafe { (self.control.wait_event)(self.raw(), 0.0) };
            let raw = NonNull::new(raw).ok_or(MpvSessionError::NullEvent)?;
            // SAFETY: pointer remains valid until the next `wait_event`; this
            // method completes the owned copy before continuing the loop.
            let raw = unsafe { raw.as_ref() };
            if raw.event_id == EVENT_NONE {
                self.wakeup.take_pending();
                break;
            }
            events.push(self.copy_event(raw));
        }
        Ok(events)
    }

    /// Number of requests still awaiting native replies.
    pub fn pending_request_count(&self) -> usize {
        self.pending.len()
    }

    /// Number of active property observations.
    pub fn observation_count(&self) -> usize {
        self.observations.len()
    }

    /// Whether a native shutdown event has been observed.
    pub const fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    /// Explicit raw/unsafe extension boundary for APIs not yet represented.
    ///
    /// # Safety
    ///
    /// The callback must not destroy or retain the handle, call
    /// `mpv_wait_event`, race this serialized owner, alter the wakeup callback,
    /// or violate any libmpv API contract.
    pub unsafe fn with_raw_handle<R>(
        &mut self,
        callback: impl FnOnce(NonNull<c_void>) -> R,
    ) -> R {
        // SAFETY: caller accepts the extension-boundary obligations above.
        let raw = unsafe {
            self.handle
                .as_ref()
                .and_then(|handle| handle.as_raw())
                .expect("live mpv session must own a handle")
        };
        callback(raw)
    }

    fn copy_event(&mut self, event: &RawMpvEvent) -> MpvEvent {
        match self.try_copy_event(event) {
            Ok(event) => event,
            Err(error) => MpvEvent::ProtocolError {
                event_id: event.event_id,
                message: error.to_string(),
            },
        }
    }

    fn try_copy_event(
        &mut self,
        event: &RawMpvEvent,
    ) -> Result<MpvEvent, MpvNodeError> {
        match event.event_id {
            EVENT_SHUTDOWN => {
                self.shutting_down = true;
                Ok(MpvEvent::Shutdown)
            }
            EVENT_LOG_MESSAGE => self.copy_log_event(event),
            EVENT_GET_PROPERTY_REPLY
            | EVENT_SET_PROPERTY_REPLY
            | EVENT_COMMAND_REPLY => self.copy_async_reply(event),
            EVENT_PROPERTY_CHANGE => self.copy_property_change(event),
            EVENT_START_FILE => {
                let data = event_data::<RawMpvEventStartFile>(event)?;
                Ok(MpvEvent::StartFile {
                    playlist_entry_id: data.playlist_entry_id,
                })
            }
            EVENT_FILE_LOADED => Ok(MpvEvent::FileLoaded),
            EVENT_END_FILE => {
                let data = event_data::<RawMpvEventEndFile>(event)?;
                let reason = match data.reason {
                    END_FILE_REASON_EOF => MpvEndFileReason::Eof,
                    END_FILE_REASON_STOP => MpvEndFileReason::Stop,
                    END_FILE_REASON_QUIT => MpvEndFileReason::Quit,
                    END_FILE_REASON_ERROR => MpvEndFileReason::Error,
                    END_FILE_REASON_REDIRECT => MpvEndFileReason::Redirect,
                    unknown => MpvEndFileReason::Unknown(unknown),
                };
                Ok(MpvEvent::EndFile(MpvEndFile {
                    reason,
                    error: (data.error < 0)
                        .then(|| MpvError::from_code(data.error)),
                    playlist_entry_id: data.playlist_entry_id,
                    playlist_insert_id: data.playlist_insert_id,
                    playlist_insert_count: u32::try_from(
                        data.playlist_insert_num_entries,
                    )
                    .unwrap_or(0),
                }))
            }
            EVENT_IDLE => Ok(MpvEvent::Idle),
            EVENT_CLIENT_MESSAGE => self.copy_client_message(event),
            EVENT_VIDEO_RECONFIG => Ok(MpvEvent::VideoReconfigured),
            EVENT_AUDIO_RECONFIG => Ok(MpvEvent::AudioReconfigured),
            EVENT_SEEK => Ok(MpvEvent::Seek),
            EVENT_PLAYBACK_RESTART => Ok(MpvEvent::PlaybackRestart),
            EVENT_TICK => Ok(MpvEvent::Tick),
            EVENT_QUEUE_OVERFLOW => Ok(MpvEvent::QueueOverflow),
            EVENT_HOOK => self.copy_hook(event),
            event_id => Ok(MpvEvent::Unknown { event_id }),
        }
    }

    fn copy_log_event(
        &self,
        event: &RawMpvEvent,
    ) -> Result<MpvEvent, MpvNodeError> {
        let data = event_data::<RawMpvEventLogMessage>(event)?;
        let prefix = copy_event_c_string(data.prefix, "log prefix")?;
        let text = copy_event_c_string(data.text, "log text")?;
        let level = match data.log_level {
            10 => MpvMessageLevel::Fatal,
            20 => MpvMessageLevel::Error,
            30 => MpvMessageLevel::Warn,
            40 => MpvMessageLevel::Info,
            50 => MpvMessageLevel::Verbose,
            60 => MpvMessageLevel::Debug,
            70 => MpvMessageLevel::Trace,
            unknown => MpvMessageLevel::Unknown(unknown),
        };
        Ok(MpvEvent::Log(MpvLogMessage {
            prefix,
            level,
            text: redact_log_text(&text),
        }))
    }

    fn copy_async_reply(
        &mut self,
        event: &RawMpvEvent,
    ) -> Result<MpvEvent, MpvNodeError> {
        let inferred_kind = match event.event_id {
            EVENT_GET_PROPERTY_REPLY => MpvRequestKind::GetProperty,
            EVENT_SET_PROPERTY_REPLY => MpvRequestKind::SetProperty,
            EVENT_COMMAND_REPLY => MpvRequestKind::Command,
            _ => unreachable!(),
        };
        let id = MpvRequestId(event.reply_userdata);
        let Some(pending) = self.pending.remove(&event.reply_userdata) else {
            return Ok(MpvEvent::UnmatchedAsyncReply {
                id,
                kind: inferred_kind,
                error: (event.error < 0)
                    .then(|| MpvError::from_code(event.error)),
            });
        };

        let kind_matches = match event.event_id {
            EVENT_GET_PROPERTY_REPLY => {
                pending.kind == MpvRequestKind::GetProperty
            }
            EVENT_SET_PROPERTY_REPLY => {
                pending.kind == MpvRequestKind::SetProperty
            }
            EVENT_COMMAND_REPLY => matches!(
                pending.kind,
                MpvRequestKind::Command | MpvRequestKind::NodeCommand
            ),
            _ => false,
        };
        if !kind_matches {
            return Ok(MpvEvent::ProtocolError {
                event_id: event.event_id,
                message: format!(
                    "reply kind {inferred_kind:?} did not match pending {:?}",
                    pending.kind
                ),
            });
        }

        let result = if event.error < 0 {
            Err(MpvError::from_code(event.error))
        } else {
            let value = match event.event_id {
                EVENT_GET_PROPERTY_REPLY => {
                    let property = event_data::<RawMpvEventProperty>(event)?;
                    // SAFETY: property payload remains valid for this event.
                    unsafe {
                        copy_raw_event_value(
                            property.format,
                            property.data,
                            self.node_limits,
                        )?
                    }
                }
                EVENT_SET_PROPERTY_REPLY => None,
                EVENT_COMMAND_REPLY => {
                    if event.data.is_null() {
                        None
                    } else {
                        let command = event_data::<RawMpvEventCommand>(event)?;
                        // SAFETY: command node remains valid for this event.
                        let value = unsafe {
                            copy_raw_node(&command.result, self.node_limits)?
                        };
                        (value != MpvNode::Null).then_some(value)
                    }
                }
                _ => unreachable!(),
            };
            Ok(value)
        };

        Ok(MpvEvent::AsyncReply(MpvAsyncReply {
            id,
            kind: pending.kind,
            cancellation_requested: pending.cancelled,
            result,
        }))
    }

    fn copy_property_change(
        &self,
        event: &RawMpvEvent,
    ) -> Result<MpvEvent, MpvNodeError> {
        let property = event_data::<RawMpvEventProperty>(event)?;
        let observation = self.observations.get(&event.reply_userdata);
        let native_name = if property.name.is_null() {
            None
        } else {
            Some(copy_event_c_string(property.name, "property name")?)
        };
        let name = native_name
            .or_else(|| observation.map(|observation| observation.name.clone()))
            .unwrap_or_else(|| "<unknown>".to_owned());

        // SAFETY: property data remains valid until the next wait call.
        let value = unsafe {
            copy_raw_event_value(
                property.format,
                property.data,
                self.node_limits,
            )?
        };
        if let Some(observation) = observation {
            // Keep the requested format read so diagnostics and future strict
            // format checks cannot silently lose registration metadata.
            let _requested_format = observation.format;
        }
        Ok(MpvEvent::PropertyChanged(MpvPropertyChange {
            id: MpvObservationId(event.reply_userdata),
            name,
            value,
            registered: observation.is_some(),
        }))
    }

    fn copy_client_message(
        &self,
        event: &RawMpvEvent,
    ) -> Result<MpvEvent, MpvNodeError> {
        let data = event_data::<RawMpvEventClientMessage>(event)?;
        let count = usize::try_from(data.count)
            .map_err(|_| MpvNodeError::InvalidListCount(data.count.into()))?;
        if count > self.node_limits.max_items {
            return Err(MpvNodeError::LimitExceeded {
                kind: "item-count",
                limit: self.node_limits.max_items,
            });
        }
        if count > 0 && data.args.is_null() {
            return Err(MpvNodeError::NullPointer("client-message arguments"));
        }
        let mut arguments = Vec::with_capacity(count);
        let mut bytes = 0usize;
        for index in 0..count {
            // SAFETY: native argument array contains `count` pointers.
            let argument = unsafe { *data.args.add(index) };
            let argument =
                copy_event_c_string(argument, "client-message argument")?;
            bytes = bytes.saturating_add(argument.len());
            if bytes > self.node_limits.max_bytes {
                return Err(MpvNodeError::LimitExceeded {
                    kind: "byte-count",
                    limit: self.node_limits.max_bytes,
                });
            }
            arguments.push(argument);
        }
        Ok(MpvEvent::ClientMessage(arguments))
    }

    fn copy_hook(
        &mut self,
        event: &RawMpvEvent,
    ) -> Result<MpvEvent, MpvNodeError> {
        let data = event_data::<RawMpvEventHook>(event)?;
        let name = if data.name.is_null() {
            self.hook_registrations
                .get(&event.reply_userdata)
                .cloned()
                .unwrap_or_else(|| "<unknown>".to_owned())
        } else {
            copy_event_c_string(data.name, "hook name")?
        };
        self.outstanding_hooks.insert(data.id);
        Ok(MpvEvent::Hook(MpvHook {
            registration: MpvHookRegistrationId(event.reply_userdata),
            id: MpvHookId(data.id),
            name,
        }))
    }

    fn raw(&self) -> *mut RawMpvHandle {
        self.handle
            .as_ref()
            .and_then(MpvHandle::raw_ptr)
            .expect("live mpv session must own a handle")
            .as_ptr()
    }

    fn ensure_running(&self) -> Result<(), MpvSessionError> {
        if self.shutting_down {
            Err(MpvSessionError::ShuttingDown)
        } else {
            Ok(())
        }
    }

    fn allocate_userdata(&mut self) -> Result<u64, MpvSessionError> {
        let current = self.next_userdata;
        if current == 0 {
            return Err(MpvSessionError::UserdataExhausted);
        }
        self.next_userdata = current
            .checked_add(1)
            .ok_or(MpvSessionError::UserdataExhausted)?;
        Ok(current)
    }
}

impl Drop for MpvSession {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.as_ref()
            && let Some(raw) = handle.raw_ptr()
        {
            // SAFETY: clearing the callback prevents future invocations;
            // userdata Arc remains alive through native destruction below.
            unsafe {
                (self.control.set_wakeup_callback)(
                    raw.as_ptr(),
                    None,
                    ptr::null_mut(),
                )
            };
        }

        // Destroy/terminate while callback userdata still owns a strong Arc.
        drop(self.handle.take());
        // SAFETY: this exactly reclaims the strong reference created by
        // `Arc::into_raw` after native code can no longer invoke the callback.
        unsafe { drop(Arc::from_raw(self.wakeup_userdata)) };
    }
}

fn check_native(
    operation: &'static str,
    code: i32,
) -> Result<(), MpvSessionError> {
    if code < 0 {
        Err(MpvSessionError::NativeCall {
            operation,
            error: MpvError::from_code(code),
        })
    } else {
        Ok(())
    }
}

fn c_string(
    value: &str,
    category: &'static str,
) -> Result<CString, MpvSessionError> {
    CString::new(value.as_bytes())
        .map_err(|_| MpvSessionError::InteriorNul { category })
}

fn event_data<T>(event: &RawMpvEvent) -> Result<&T, MpvNodeError> {
    // SAFETY: each caller chooses `T` from the native event ID. Null is checked.
    unsafe { event.data.cast::<T>().as_ref() }
        .ok_or(MpvNodeError::NullPointer("event data"))
}

fn copy_event_c_string(
    pointer: *const c_char,
    category: &'static str,
) -> Result<String, MpvNodeError> {
    if pointer.is_null() {
        return Err(MpvNodeError::NullPointer(category));
    }
    // SAFETY: libmpv event strings are NUL-terminated for the event lifetime.
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

fn redact_log_text(input: &str) -> String {
    let mut output = input.to_owned();
    for marker in [
        "access_token=",
        "api_key=",
        "apikey=",
        "token=",
        "ticket=",
        "signature=",
        "sig=",
        "auth=",
        "session=",
    ] {
        output = redact_after_marker(&output, marker);
    }
    for marker in ["authorization:", "cookie:", "http-header-fields="] {
        output = redact_line_after_marker(&output, marker);
    }
    redact_url_userinfo(&output)
}

fn redact_after_marker(input: &str, marker: &str) -> String {
    let lowercase = input.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut source_offset = 0usize;
    let mut search_offset = 0usize;

    while let Some(relative) = lowercase[search_offset..].find(marker) {
        let marker_start = search_offset + relative;
        let value_start = marker_start + marker.len();
        output.push_str(&input[source_offset..value_start]);
        output.push_str("<redacted>");
        let remainder = &input[value_start..];
        let value_len = remainder
            .find(|character: char| {
                matches!(
                    character,
                    '&' | '#'
                        | ' '
                        | '\t'
                        | '\r'
                        | '\n'
                        | '"'
                        | '\''
                        | ')'
                        | ']'
                        | '}'
                        | ','
                )
            })
            .unwrap_or(remainder.len());
        source_offset = value_start + value_len;
        search_offset = source_offset;
    }
    output.push_str(&input[source_offset..]);
    output
}

fn redact_line_after_marker(input: &str, marker: &str) -> String {
    let lowercase = input.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut source_offset = 0usize;
    let mut search_offset = 0usize;

    while let Some(relative) = lowercase[search_offset..].find(marker) {
        let marker_start = search_offset + relative;
        let value_start = marker_start + marker.len();
        output.push_str(&input[source_offset..value_start]);
        output.push_str("<redacted>");
        let remainder = &input[value_start..];
        let value_len = remainder.find(['\r', '\n']).unwrap_or(remainder.len());
        source_offset = value_start + value_len;
        search_offset = source_offset;
    }
    output.push_str(&input[source_offset..]);
    output
}

fn redact_url_userinfo(input: &str) -> String {
    let mut output = input.to_owned();
    let mut offset = 0usize;
    while let Some(relative_scheme) = output[offset..].find("://") {
        let authority_start = offset + relative_scheme + 3;
        let authority_end = output[authority_start..]
            .find(['/', '?', '#', ' ', '\t', '\r', '\n'])
            .map_or(output.len(), |relative| authority_start + relative);
        let Some(relative_at) =
            output[authority_start..authority_end].rfind('@')
        else {
            offset = authority_end;
            continue;
        };
        let at = authority_start + relative_at;
        output.replace_range(authority_start..at, "<redacted>");
        offset = authority_start + "<redacted>@".len();
    }
    output
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque};

    use super::*;
    use crate::raw::{FORMAT_NONE, MpvControlApi, RawMpvNode, WakeupCallback};

    enum FakeQueuedEvent {
        CommandReply {
            id: u64,
            result: Option<MpvNode>,
        },
        GetReply {
            id: u64,
            name: String,
            value: MpvNode,
        },
        SetReply {
            id: u64,
        },
        PropertyChange {
            id: u64,
            name: String,
            value: MpvNode,
        },
        Log {
            prefix: String,
            text: String,
            level: u32,
        },
        ClientMessage(Vec<String>),
        Hook {
            registration: u64,
            name: String,
            hook_id: u64,
        },
        Shutdown,
    }

    struct FakeEventStorage {
        event: Box<RawMpvEvent>,
        _property: Option<Box<RawMpvEventProperty>>,
        _command: Option<Box<RawMpvEventCommand>>,
        _log: Option<Box<RawMpvEventLogMessage>>,
        _client: Option<Box<RawMpvEventClientMessage>>,
        _hook: Option<Box<RawMpvEventHook>>,
        _strings: Vec<CString>,
        _string_pointer: Option<Box<*const c_char>>,
        _argument_pointers: Option<Box<[*const c_char]>>,
        _flag: Option<Box<c_int>>,
        _integer: Option<Box<i64>>,
        _double: Option<Box<f64>>,
        _arena: Option<Box<RawNodeArena>>,
    }

    impl FakeEventStorage {
        fn none() -> Self {
            Self::empty(EVENT_NONE, 0, 0)
        }

        fn empty(event_id: u32, error: i32, userdata: u64) -> Self {
            Self {
                event: Box::new(RawMpvEvent {
                    event_id,
                    error,
                    reply_userdata: userdata,
                    data: ptr::null_mut(),
                }),
                _property: None,
                _command: None,
                _log: None,
                _client: None,
                _hook: None,
                _strings: Vec::new(),
                _string_pointer: None,
                _argument_pointers: None,
                _flag: None,
                _integer: None,
                _double: None,
                _arena: None,
            }
        }

        fn from_queued(queued: FakeQueuedEvent) -> Self {
            match queued {
                FakeQueuedEvent::CommandReply { id, result } => {
                    let mut storage = Self::empty(EVENT_COMMAND_REPLY, 0, id);
                    if let Some(result) = result {
                        let mut arena =
                            Box::new(RawNodeArena::new(&result).unwrap());
                        // SAFETY: root is initialized and copied by value while
                        // arena retains all pointer-backed children.
                        let result = unsafe { *arena.root_mut() };
                        let mut command =
                            Box::new(RawMpvEventCommand { result });
                        storage.event.data = command.as_mut()
                            as *mut RawMpvEventCommand
                            as *mut c_void;
                        storage._command = Some(command);
                        storage._arena = Some(arena);
                    }
                    storage
                }
                FakeQueuedEvent::GetReply { id, name, value } => {
                    Self::property_event(
                        EVENT_GET_PROPERTY_REPLY,
                        id,
                        name,
                        value,
                    )
                }
                FakeQueuedEvent::SetReply { id } => {
                    Self::empty(EVENT_SET_PROPERTY_REPLY, 0, id)
                }
                FakeQueuedEvent::PropertyChange { id, name, value } => {
                    Self::property_event(EVENT_PROPERTY_CHANGE, id, name, value)
                }
                FakeQueuedEvent::Log {
                    prefix,
                    text,
                    level,
                } => {
                    let mut storage = Self::empty(EVENT_LOG_MESSAGE, 0, 0);
                    let prefix = CString::new(prefix).unwrap();
                    let text = CString::new(text).unwrap();
                    let level_name = CString::new("info").unwrap();
                    let mut log = Box::new(RawMpvEventLogMessage {
                        prefix: prefix.as_ptr(),
                        level: level_name.as_ptr(),
                        text: text.as_ptr(),
                        log_level: level,
                    });
                    storage.event.data = log.as_mut()
                        as *mut RawMpvEventLogMessage
                        as *mut c_void;
                    storage._strings = vec![prefix, text, level_name];
                    storage._log = Some(log);
                    storage
                }
                FakeQueuedEvent::ClientMessage(arguments) => {
                    let mut storage = Self::empty(EVENT_CLIENT_MESSAGE, 0, 0);
                    let strings = arguments
                        .into_iter()
                        .map(|argument| CString::new(argument).unwrap())
                        .collect::<Vec<_>>();
                    let mut pointers = strings
                        .iter()
                        .map(|argument| argument.as_ptr())
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    let mut client = Box::new(RawMpvEventClientMessage {
                        count: c_int::try_from(pointers.len()).unwrap(),
                        args: pointers.as_mut_ptr(),
                    });
                    storage.event.data = client.as_mut()
                        as *mut RawMpvEventClientMessage
                        as *mut c_void;
                    storage._strings = strings;
                    storage._argument_pointers = Some(pointers);
                    storage._client = Some(client);
                    storage
                }
                FakeQueuedEvent::Hook {
                    registration,
                    name,
                    hook_id,
                } => {
                    let mut storage = Self::empty(EVENT_HOOK, 0, registration);
                    let name = CString::new(name).unwrap();
                    let mut hook = Box::new(RawMpvEventHook {
                        name: name.as_ptr(),
                        id: hook_id,
                    });
                    storage.event.data =
                        hook.as_mut() as *mut RawMpvEventHook as *mut c_void;
                    storage._strings.push(name);
                    storage._hook = Some(hook);
                    storage
                }
                FakeQueuedEvent::Shutdown => Self::empty(EVENT_SHUTDOWN, 0, 0),
            }
        }

        fn property_event(
            event_id: u32,
            id: u64,
            name: String,
            value: MpvNode,
        ) -> Self {
            let mut storage = Self::empty(event_id, 0, id);
            let name = CString::new(name).unwrap();
            let (format, data) = match value {
                MpvNode::Null => (FORMAT_NONE, ptr::null_mut()),
                MpvNode::String(value) => {
                    let value = CString::new(value).unwrap();
                    let mut pointer = Box::new(value.as_ptr());
                    let data =
                        pointer.as_mut() as *mut *const c_char as *mut c_void;
                    storage._strings.push(value);
                    storage._string_pointer = Some(pointer);
                    (FORMAT_STRING, data)
                }
                MpvNode::Bool(value) => {
                    let mut value = Box::new(c_int::from(value));
                    let data = value.as_mut() as *mut c_int as *mut c_void;
                    storage._flag = Some(value);
                    (FORMAT_FLAG, data)
                }
                MpvNode::Int(value) => {
                    let mut value = Box::new(value);
                    let data = value.as_mut() as *mut i64 as *mut c_void;
                    storage._integer = Some(value);
                    (FORMAT_INT64, data)
                }
                MpvNode::Double(value) => {
                    let mut value = Box::new(value);
                    let data = value.as_mut() as *mut f64 as *mut c_void;
                    storage._double = Some(value);
                    (FORMAT_DOUBLE, data)
                }
                node @ (MpvNode::Array(_)
                | MpvNode::Map(_)
                | MpvNode::Bytes(_)) => {
                    let mut arena = Box::new(RawNodeArena::new(&node).unwrap());
                    let data = arena.root_mut().cast();
                    storage._arena = Some(arena);
                    (FORMAT_NODE, data)
                }
            };
            let mut property = Box::new(RawMpvEventProperty {
                name: name.as_ptr(),
                format,
                data,
            });
            storage.event.data =
                property.as_mut() as *mut RawMpvEventProperty as *mut c_void;
            storage._strings.push(name);
            storage._property = Some(property);
            storage
        }
    }

    struct FakeState {
        options: Vec<(String, String)>,
        initialized_after_option_count: Option<usize>,
        logs_requested_before_initialize: Option<bool>,
        commands: Vec<Vec<String>>,
        node_commands: Vec<MpvNode>,
        property_sets: Vec<(String, MpvNode)>,
        aborted: Vec<u64>,
        observations: HashMap<u64, (String, u32)>,
        unobserved: Vec<u64>,
        hooks: HashMap<u64, String>,
        continued_hooks: Vec<u64>,
        requested_events: Vec<(u32, bool)>,
        log_levels: Vec<String>,
        events: VecDeque<FakeQueuedEvent>,
        current: Option<FakeEventStorage>,
        callback: WakeupCallback,
        callback_userdata: *mut c_void,
        destroy_count: usize,
        terminate_count: usize,
    }

    impl Default for FakeState {
        fn default() -> Self {
            Self {
                options: Vec::new(),
                initialized_after_option_count: None,
                logs_requested_before_initialize: None,
                commands: Vec::new(),
                node_commands: Vec::new(),
                property_sets: Vec::new(),
                aborted: Vec::new(),
                observations: HashMap::new(),
                unobserved: Vec::new(),
                hooks: HashMap::new(),
                continued_hooks: Vec::new(),
                requested_events: Vec::new(),
                log_levels: Vec::new(),
                events: VecDeque::new(),
                current: None,
                callback: None,
                callback_userdata: ptr::null_mut(),
                destroy_count: 0,
                terminate_count: 0,
            }
        }
    }

    thread_local! {
        static FAKE: RefCell<FakeState> = RefCell::new(FakeState::default());
    }

    fn reset_fake() {
        FAKE.with(|state| *state.borrow_mut() = FakeState::default());
    }

    fn enqueue(event: FakeQueuedEvent) {
        let (callback, userdata) = FAKE.with(|state| {
            let mut state = state.borrow_mut();
            state.events.push_back(event);
            (state.callback, state.callback_userdata)
        });
        if let Some(callback) = callback {
            // SAFETY: fake registration preserves the callback contract.
            unsafe { callback(userdata) };
        }
    }

    unsafe extern "C" fn fake_version() -> std::ffi::c_ulong {
        crate::MpvClientApiVersion::new(2, 5).packed() as std::ffi::c_ulong
    }

    unsafe extern "C" fn fake_create() -> *mut RawMpvHandle {
        NonNull::<u8>::dangling().as_ptr().cast()
    }

    unsafe extern "C" fn fake_initialize(_handle: *mut RawMpvHandle) -> c_int {
        FAKE.with(|state| {
            let mut state = state.borrow_mut();
            state.initialized_after_option_count = Some(state.options.len());
            state.logs_requested_before_initialize =
                Some(!state.log_levels.is_empty());
        });
        0
    }

    unsafe extern "C" fn fake_destroy(_handle: *mut RawMpvHandle) {
        FAKE.with(|state| state.borrow_mut().destroy_count += 1);
    }

    unsafe extern "C" fn fake_terminate(_handle: *mut RawMpvHandle) {
        FAKE.with(|state| state.borrow_mut().terminate_count += 1);
    }

    unsafe extern "C" fn fake_set_option_string(
        _handle: *mut RawMpvHandle,
        name: *const c_char,
        value: *const c_char,
    ) -> c_int {
        // SAFETY: session passes live C strings for the duration of this call.
        let name = unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: same as `name`.
        let value = unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        FAKE.with(|state| state.borrow_mut().options.push((name, value)));
        0
    }

    unsafe extern "C" fn fake_command_async(
        _handle: *mut RawMpvHandle,
        id: u64,
        arguments: *mut *const c_char,
    ) -> c_int {
        let mut copied = Vec::new();
        for index in 0..128 {
            // SAFETY: session passes a NUL-terminated pointer vector.
            let argument = unsafe { *arguments.add(index) };
            if argument.is_null() {
                break;
            }
            // SAFETY: each non-null entry is a live C string.
            copied.push(
                unsafe { CStr::from_ptr(argument) }
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        FAKE.with(|state| state.borrow_mut().commands.push(copied));
        enqueue(FakeQueuedEvent::CommandReply { id, result: None });
        0
    }

    unsafe extern "C" fn fake_command_node_async(
        _handle: *mut RawMpvHandle,
        id: u64,
        command: *mut RawMpvNode,
    ) -> c_int {
        // SAFETY: session's arena keeps the full node live for this call.
        let command = unsafe {
            copy_raw_node(&*command, MpvNodeLimits::default()).unwrap()
        };
        FAKE.with(|state| {
            state.borrow_mut().node_commands.push(command.clone())
        });
        enqueue(FakeQueuedEvent::CommandReply {
            id,
            result: Some(command),
        });
        0
    }

    unsafe extern "C" fn fake_abort(_handle: *mut RawMpvHandle, id: u64) {
        FAKE.with(|state| state.borrow_mut().aborted.push(id));
    }

    unsafe extern "C" fn fake_set_property_async(
        _handle: *mut RawMpvHandle,
        id: u64,
        name: *const c_char,
        format: u32,
        data: *mut c_void,
    ) -> c_int {
        // SAFETY: session passes a live property name.
        let name = unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: format/data follow the native property ABI for this call.
        let value = unsafe {
            copy_raw_event_value(format, data, MpvNodeLimits::default())
                .unwrap()
                .unwrap_or(MpvNode::Null)
        };
        FAKE.with(|state| state.borrow_mut().property_sets.push((name, value)));
        enqueue(FakeQueuedEvent::SetReply { id });
        0
    }

    unsafe extern "C" fn fake_get_property_async(
        _handle: *mut RawMpvHandle,
        id: u64,
        name: *const c_char,
        format: u32,
    ) -> c_int {
        // SAFETY: session passes a live property name.
        let name = unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned();
        let value = match format {
            FORMAT_STRING => MpvNode::String("value".into()),
            FORMAT_FLAG => MpvNode::Bool(true),
            FORMAT_INT64 => MpvNode::Int(42),
            FORMAT_DOUBLE => MpvNode::Double(12.5),
            FORMAT_NODE => MpvNode::Map(vec![(
                "array".into(),
                MpvNode::Array(vec![MpvNode::Null, MpvNode::Int(7)]),
            )]),
            _ => MpvNode::Null,
        };
        enqueue(FakeQueuedEvent::GetReply { id, name, value });
        0
    }

    unsafe extern "C" fn fake_observe_property(
        _handle: *mut RawMpvHandle,
        id: u64,
        name: *const c_char,
        format: u32,
    ) -> c_int {
        // SAFETY: session passes a live property name.
        let name = unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned();
        FAKE.with(|state| {
            state
                .borrow_mut()
                .observations
                .insert(id, (name.clone(), format));
        });
        enqueue(FakeQueuedEvent::PropertyChange {
            id,
            name,
            value: MpvNode::Double(1.25),
        });
        0
    }

    unsafe extern "C" fn fake_unobserve_property(
        _handle: *mut RawMpvHandle,
        id: u64,
    ) -> c_int {
        FAKE.with(|state| {
            let mut state = state.borrow_mut();
            state.observations.remove(&id);
            state.unobserved.push(id);
        });
        1
    }

    unsafe extern "C" fn fake_request_logs(
        _handle: *mut RawMpvHandle,
        level: *const c_char,
    ) -> c_int {
        // SAFETY: session passes a live level C string.
        let level = unsafe { CStr::from_ptr(level) }
            .to_string_lossy()
            .into_owned();
        FAKE.with(|state| state.borrow_mut().log_levels.push(level));
        0
    }

    unsafe extern "C" fn fake_wait_event(
        _handle: *mut RawMpvHandle,
        _timeout: f64,
    ) -> *mut RawMpvEvent {
        FAKE.with(|state| {
            let mut state = state.borrow_mut();
            let storage = state
                .events
                .pop_front()
                .map(FakeEventStorage::from_queued)
                .unwrap_or_else(FakeEventStorage::none);
            state.current = Some(storage);
            state.current.as_mut().unwrap().event.as_mut() as *mut RawMpvEvent
        })
    }

    unsafe extern "C" fn fake_set_wakeup_callback(
        _handle: *mut RawMpvHandle,
        callback: WakeupCallback,
        userdata: *mut c_void,
    ) {
        FAKE.with(|state| {
            let mut state = state.borrow_mut();
            state.callback = callback;
            state.callback_userdata = userdata;
        });
    }

    unsafe extern "C" fn fake_request_event(
        _handle: *mut RawMpvHandle,
        event: u32,
        enabled: c_int,
    ) -> c_int {
        FAKE.with(|state| {
            state
                .borrow_mut()
                .requested_events
                .push((event, enabled != 0))
        });
        0
    }

    unsafe extern "C" fn fake_hook_add(
        _handle: *mut RawMpvHandle,
        id: u64,
        name: *const c_char,
        _priority: c_int,
    ) -> c_int {
        // SAFETY: session passes a live hook name.
        let name = unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned();
        FAKE.with(|state| {
            state.borrow_mut().hooks.insert(id, name.clone());
        });
        enqueue(FakeQueuedEvent::Hook {
            registration: id,
            name,
            hook_id: 77,
        });
        0
    }

    unsafe extern "C" fn fake_hook_continue(
        _handle: *mut RawMpvHandle,
        hook_id: u64,
    ) -> c_int {
        FAKE.with(|state| state.borrow_mut().continued_hooks.push(hook_id));
        0
    }

    fn fake_table() -> MpvFunctionTable {
        // SAFETY: every fake uses the matching ABI and one thread-local state.
        unsafe {
            let control = MpvControlApi::from_raw_parts(
                fake_set_option_string,
                fake_command_async,
                fake_command_node_async,
                fake_abort,
                fake_set_property_async,
                fake_get_property_async,
                fake_observe_property,
                fake_unobserve_property,
                fake_request_logs,
                fake_wait_event,
                fake_set_wakeup_callback,
                fake_request_event,
                fake_hook_add,
                fake_hook_continue,
            );
            MpvFunctionTable::from_raw_parts(
                fake_version,
                fake_create,
                fake_initialize,
                fake_destroy,
                fake_terminate,
            )
            .with_control_api(control)
        }
    }

    fn fake_session() -> MpvSession {
        MpvSession::create(fake_table(), MpvSessionConfig::default()).unwrap()
    }

    #[test]
    fn deterministic_options_precede_initialization_and_drop_terminates() {
        reset_fake();
        let config = MpvSessionConfig::default();
        let expected_options = config.options().len();
        let session = MpvSession::create(fake_table(), config).unwrap();

        FAKE.with(|state| {
            let state = state.borrow();
            assert_eq!(state.options.len(), expected_options);
            assert_eq!(
                state.initialized_after_option_count,
                Some(expected_options)
            );
            assert_eq!(state.log_levels, ["info"]);
            assert_eq!(state.logs_requested_before_initialize, Some(true));
            assert!(state.callback.is_some());
        });
        drop(session);
        FAKE.with(|state| {
            let state = state.borrow();
            assert!(state.callback.is_none());
            assert_eq!(state.destroy_count, 0);
            assert_eq!(state.terminate_count, 1);
        });
    }

    #[test]
    fn user_config_and_scripts_require_an_explicit_trusted_policy() {
        let deterministic = MpvSessionConfig::native_window();
        assert_eq!(
            deterministic.config_policy(),
            MpvConfigPolicy::Deterministic
        );
        assert_eq!(
            deterministic
                .options
                .iter()
                .find(|option| option.name == "config")
                .map(|option| option.value.as_str()),
            Some("no")
        );
        assert_eq!(
            deterministic
                .options
                .iter()
                .find(|option| option.name == "load-scripts")
                .map(|option| option.value.as_str()),
            Some("no")
        );

        let trusted = MpvSessionConfig::native_window_with_config_policy(
            MpvConfigPolicy::TrustedUser,
        );
        assert_eq!(trusted.config_policy(), MpvConfigPolicy::TrustedUser);
        assert_eq!(trusted.config_policy().as_str(), "trusted-user");
        assert_eq!(
            trusted
                .options
                .iter()
                .find(|option| option.name == "config")
                .map(|option| option.value.as_str()),
            Some("yes")
        );
        assert_eq!(
            trusted
                .options
                .iter()
                .find(|option| option.name == "load-scripts")
                .map(|option| option.value.as_str()),
            Some("yes")
        );
        assert_eq!(
            trusted
                .options
                .iter()
                .find(|option| option.name == "ytdl")
                .map(|option| option.value.as_str()),
            Some("no"),
            "trusted config does not implicitly package an external resolver"
        );
    }

    #[test]
    fn typed_properties_nodes_async_replies_and_cancellation_correlate() {
        reset_fake();
        let mut session = fake_session();

        let command = session.command_async(["seek", "5", "relative"]).unwrap();
        session.cancel_request(command).unwrap();
        let node_command = MpvNode::Array(vec![
            MpvNode::String("expand-text".into()),
            MpvNode::String("${mpv-version}".into()),
        ]);
        let node_command_id =
            session.command_node_async(&node_command).unwrap();

        let get_ids = [
            (
                session
                    .get_property_async("string", MpvFormat::String)
                    .unwrap(),
                MpvNode::String("value".into()),
            ),
            (
                session.get_property_async("flag", MpvFormat::Flag).unwrap(),
                MpvNode::Bool(true),
            ),
            (
                session
                    .get_property_async("integer", MpvFormat::Int64)
                    .unwrap(),
                MpvNode::Int(42),
            ),
            (
                session
                    .get_property_async("double", MpvFormat::Double)
                    .unwrap(),
                MpvNode::Double(12.5),
            ),
            (
                session.get_property_async("node", MpvFormat::Node).unwrap(),
                MpvNode::Map(vec![(
                    "array".into(),
                    MpvNode::Array(vec![MpvNode::Null, MpvNode::Int(7)]),
                )]),
            ),
        ];

        let property_values = [
            MpvNode::String("text".into()),
            MpvNode::Bool(false),
            MpvNode::Int(9),
            MpvNode::Double(1.5),
            MpvNode::Map(vec![("nested".into(), MpvNode::Null)]),
        ];
        for (index, value) in property_values.iter().enumerate() {
            session
                .set_property_async(&format!("property-{index}"), value)
                .unwrap();
        }

        let events = session.drain_events().unwrap();
        assert_eq!(session.pending_request_count(), 0);

        let replies = events
            .into_iter()
            .filter_map(|event| match event {
                MpvEvent::AsyncReply(reply) => Some(reply),
                _ => None,
            })
            .collect::<Vec<_>>();
        let cancelled =
            replies.iter().find(|reply| reply.id == command).unwrap();
        assert!(cancelled.cancellation_requested);
        assert_eq!(cancelled.result, Ok(None));
        let node_reply = replies
            .iter()
            .find(|reply| reply.id == node_command_id)
            .unwrap();
        assert_eq!(node_reply.result, Ok(Some(node_command.clone())));
        for (id, expected) in get_ids {
            let reply = replies.iter().find(|reply| reply.id == id).unwrap();
            assert_eq!(reply.result, Ok(Some(expected)));
        }

        FAKE.with(|state| {
            let state = state.borrow();
            assert_eq!(state.commands, [vec!["seek", "5", "relative"]]);
            assert_eq!(state.node_commands, [node_command]);
            assert_eq!(state.aborted, [command.get()]);
            assert_eq!(
                state
                    .property_sets
                    .iter()
                    .map(|(_, value)| value)
                    .collect::<Vec<_>>(),
                property_values.iter().collect::<Vec<_>>()
            );
        });
    }

    #[test]
    fn event_payloads_are_copied_before_the_next_wait_invalidates_them() {
        reset_fake();
        let mut session = fake_session();
        let observation = session
            .observe_property("time-pos", MpvFormat::Double)
            .unwrap();
        let events = session.drain_events().unwrap();

        // `drain_events` fetched EVENT_NONE after the property event, which
        // dropped the fake's pointer-backed property storage.
        assert_eq!(
            events,
            [MpvEvent::PropertyChanged(MpvPropertyChange {
                id: observation,
                name: "time-pos".into(),
                value: Some(MpvNode::Double(1.25)),
                registered: true,
            })]
        );
        session.unobserve_property(observation).unwrap();
        assert_eq!(session.observation_count(), 0);
    }

    #[test]
    fn logs_client_messages_hooks_and_shutdown_are_owned_and_safe() {
        reset_fake();
        let mut session = fake_session();
        let registration = session.add_hook("on_load", 0).unwrap();
        enqueue(FakeQueuedEvent::Log {
            prefix: "ffmpeg".into(),
            text: "Opening https://user:pass@example.test/v?access_token=query-secret Authorization: Bearer-secret".into(),
            level: 30,
        });
        enqueue(FakeQueuedEvent::ClientMessage(vec![
            "ferrex".into(),
            "ready".into(),
        ]));
        enqueue(FakeQueuedEvent::Shutdown);

        let events = session.drain_events().unwrap();
        let hook = events
            .iter()
            .find_map(|event| match event {
                MpvEvent::Hook(hook) => Some(hook.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(hook.registration, registration);
        session.continue_hook(hook.id).unwrap();
        assert!(matches!(
            session.continue_hook(hook.id),
            Err(MpvSessionError::UnknownHook(77))
        ));

        let log = events
            .iter()
            .find_map(|event| match event {
                MpvEvent::Log(log) => Some(log),
                _ => None,
            })
            .unwrap();
        for secret in ["user:pass", "query-secret", "Bearer-secret"] {
            assert!(!log.text.contains(secret));
        }
        assert!(events.contains(&MpvEvent::ClientMessage(vec![
            "ferrex".into(),
            "ready".into()
        ])));
        assert!(events.contains(&MpvEvent::Shutdown));
        assert!(session.is_shutting_down());
        assert!(matches!(
            session.command_async(["stop"]),
            Err(MpvSessionError::ShuttingDown)
        ));
    }

    #[test]
    fn wakeup_storm_coalesces_without_losing_notification_count() {
        let signal = MpvWakeupSignal::for_current_thread();
        let raw = Arc::into_raw(Arc::clone(&signal));
        for _ in 0..1_000 {
            // SAFETY: raw owns one strong Arc and callback accepts this type.
            unsafe { wakeup_callback(raw.cast_mut().cast()) };
        }

        assert!(signal.take_pending());
        assert!(!signal.take_pending());
        assert_eq!(signal.notification_count(), 1_000);
        // SAFETY: reclaim exactly the reference leaked above.
        unsafe { drop(Arc::from_raw(raw)) };
    }

    #[test]
    fn repeated_sessions_clear_callbacks_and_destroy_once() {
        reset_fake();
        for _ in 0..50 {
            drop(fake_session());
        }
        FAKE.with(|state| {
            let state = state.borrow();
            assert!(state.callback.is_none());
            assert_eq!(state.terminate_count, 50);
            assert_eq!(state.destroy_count, 0);
        });
    }

    #[test]
    fn log_redaction_removes_query_headers_cookies_and_userinfo() {
        let input = "https://user:pass@example.test/v?access_token=secret&x=1 Authorization: Bearer-secret Cookie: sid=cookie-secret";
        let redacted = redact_log_text(input);

        for secret in ["user:pass", "secret", "Bearer-secret", "cookie-secret"]
        {
            assert!(!redacted.contains(secret), "leaked {secret}: {redacted}");
        }
        assert!(redacted.contains("example.test"));
        assert!(redacted.contains("access_token=<redacted>"));
    }

    #[cfg(feature = "linked")]
    #[test]
    fn linked_session_initializes_and_correlates_a_real_property_reply() {
        use std::time::Instant;

        let mut session = MpvSession::create(
            MpvFunctionTable::linked(),
            MpvSessionConfig::default(),
        )
        .unwrap();
        let id = session
            .get_property_async("mpv-version", MpvFormat::String)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);

        loop {
            if let Some(reply) = session
                .drain_events()
                .unwrap()
                .into_iter()
                .find_map(|event| match event {
                    MpvEvent::AsyncReply(reply) if reply.id == id => {
                        Some(reply)
                    }
                    _ => None,
                })
            {
                let Some(MpvNode::String(version)) = reply.result.unwrap()
                else {
                    panic!("mpv-version did not return a string");
                };
                assert!(
                    version.to_ascii_lowercase().contains("mpv"),
                    "{version}"
                );
                break;
            }
            assert!(Instant::now() < deadline, "timed out waiting for libmpv");
            session.wait_for_wakeup(Duration::from_millis(20));
        }
    }
}
