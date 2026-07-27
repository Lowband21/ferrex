//! Minimal, fakeable libmpv ABI boundary.
//!
//! Lifecycle symbols stay here while the P3 command/property/event symbols
//! live in the crate-private raw control table. Neither table exposes binding
//! types to the playback domain.

use std::{
    ffi::{c_int, c_ulong, c_void},
    fmt,
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
};

use crate::raw::{MpvControlApi, RawMpvHandle};

/// Client API version represented by libmpv's packed major/minor integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MpvClientApiVersion {
    /// ABI major version.
    pub major: u16,
    /// Backwards-compatible API minor version.
    pub minor: u16,
}

impl MpvClientApiVersion {
    /// Construct an API version.
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Decode the value returned by `mpv_client_api_version`.
    pub const fn from_packed(value: u64) -> Self {
        Self {
            major: ((value >> 16) & 0xffff) as u16,
            minor: (value & 0xffff) as u16,
        }
    }

    /// Encode this version using libmpv's `MPV_MAKE_VERSION` layout.
    pub const fn packed(self) -> u64 {
        ((self.major as u64) << 16) | self.minor as u64
    }

    /// Whether this runtime can satisfy the required client ABI.
    pub const fn satisfies(self, required: Self) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

impl fmt::Display for MpvClientApiVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

/// Client API represented by the selected `libmpv2-sys` 4.0.1 bindings.
pub const BINDINGS_CLIENT_API: MpvClientApiVersion =
    MpvClientApiVersion::new(2, 5);

/// Oldest runtime accepted by the initial Ferrex libmpv integration.
///
/// API 2.2 corresponds to mpv 0.37.0 and already contains every client symbol
/// required by P3. Release artifacts still target the selected mpv 0.41.0
/// baseline (API 2.5) so this compatibility floor does not weaken packaging.
pub const MINIMUM_CLIENT_API: MpvClientApiVersion =
    MpvClientApiVersion::new(2, 2);

/// Runtime and binding compatibility details suitable for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpvCompatibilityReport {
    /// API used to generate the raw Rust bindings.
    pub bindings: MpvClientApiVersion,
    /// API reported by the loaded libmpv runtime.
    pub runtime: MpvClientApiVersion,
    /// Minimum API accepted by Ferrex.
    pub minimum: MpvClientApiVersion,
    /// Whether the runtime satisfies the Ferrex requirement.
    pub compatible: bool,
}

/// Errors produced before the full mpv event/error mapper is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MpvFfiError {
    /// The loaded runtime has an incompatible client API.
    #[error(
        "incompatible libmpv client API {runtime}; Ferrex requires {minimum}"
    )]
    IncompatibleClientApi {
        /// Version reported by the runtime.
        runtime: MpvClientApiVersion,
        /// Minimum version required by Ferrex.
        minimum: MpvClientApiVersion,
    },
    /// `mpv_create` returned a null handle.
    #[error("libmpv could not allocate a client handle")]
    CreateFailed,
    /// `mpv_initialize` returned a native error code.
    #[error("libmpv initialization failed with error code {code}")]
    InitializationFailed {
        /// Native negative `mpv_error` value.
        code: i32,
    },
    /// Initialization was requested more than once.
    #[error("libmpv handle is already initialized")]
    AlreadyInitialized,
    /// The handle was already consumed by failed initialization or teardown.
    #[error("libmpv handle is no longer usable")]
    Destroyed,
}

type ClientApiVersionFn = unsafe extern "C" fn() -> c_ulong;
type CreateFn = unsafe extern "C" fn() -> *mut RawMpvHandle;
type InitializeFn = unsafe extern "C" fn(*mut RawMpvHandle) -> c_int;
type DestroyFn = unsafe extern "C" fn(*mut RawMpvHandle);

/// Required libmpv symbols, isolated so tests can supply a fake ABI.
///
/// The associated control table covers commands, properties, nodes, events,
/// hooks, logs, and wakeup delivery. Keeping both tables by value avoids
/// process-global mock state in production code.
#[derive(Clone, Copy)]
pub struct MpvFunctionTable {
    client_api_version: ClientApiVersionFn,
    create: CreateFn,
    initialize: InitializeFn,
    destroy: DestroyFn,
    terminate_destroy: DestroyFn,
    control: Option<MpvControlApi>,
}

impl fmt::Debug for MpvFunctionTable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MpvFunctionTable")
            .finish_non_exhaustive()
    }
}

impl MpvFunctionTable {
    /// Construct a table from a trusted set of ABI-compatible symbols.
    ///
    /// # Safety
    ///
    /// Every function must obey the corresponding libmpv client API contract,
    /// remain valid for every handle created from this table, and originate
    /// from one ABI-compatible library instance.
    pub const unsafe fn from_raw_parts(
        client_api_version: ClientApiVersionFn,
        create: CreateFn,
        initialize: InitializeFn,
        destroy: DestroyFn,
        terminate_destroy: DestroyFn,
    ) -> Self {
        Self {
            client_api_version,
            create,
            initialize,
            destroy,
            terminate_destroy,
            control: None,
        }
    }

    #[cfg(test)]
    pub(crate) const unsafe fn with_control_api(
        mut self,
        control: MpvControlApi,
    ) -> Self {
        self.control = Some(control);
        self
    }

    pub(crate) const fn control_api(self) -> Option<MpvControlApi> {
        self.control
    }

    /// Report compatibility without allocating an mpv handle.
    pub fn compatibility_report(self) -> MpvCompatibilityReport {
        // SAFETY: table construction guarantees a valid, argument-free version
        // function for the lifetime of this copied table.
        // `c_ulong` is u64 on LP64 targets and u32 on Windows/32-bit targets.
        #[allow(clippy::unnecessary_cast)]
        let packed = unsafe { (self.client_api_version)() } as u64;
        let runtime = MpvClientApiVersion::from_packed(packed);
        MpvCompatibilityReport {
            bindings: BINDINGS_CLIENT_API,
            runtime,
            minimum: MINIMUM_CLIENT_API,
            compatible: runtime.satisfies(MINIMUM_CLIENT_API),
        }
    }

    /// Use symbols linked from `libmpv2-sys`.
    #[cfg(feature = "linked")]
    pub const fn linked() -> Self {
        Self {
            client_api_version: linked_client_api_version,
            create: linked_create,
            initialize: linked_initialize,
            destroy: linked_destroy,
            terminate_destroy: linked_terminate_destroy,
            control: Some(MpvControlApi::linked()),
        }
    }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_client_api_version() -> c_ulong {
    // SAFETY: forwarded directly to the linked libmpv symbol.
    unsafe { libmpv2_sys::mpv_client_api_version() }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_create() -> *mut RawMpvHandle {
    // SAFETY: forwarded directly to the linked libmpv symbol; the opaque
    // pointer representation is preserved by the cast.
    unsafe { libmpv2_sys::mpv_create().cast() }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_initialize(handle: *mut RawMpvHandle) -> c_int {
    // SAFETY: callers only pass handles returned by `linked_create`.
    unsafe { libmpv2_sys::mpv_initialize(handle.cast()) }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_destroy(handle: *mut RawMpvHandle) {
    // SAFETY: callers only pass live handles returned by `linked_create`.
    unsafe { libmpv2_sys::mpv_destroy(handle.cast()) };
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_terminate_destroy(handle: *mut RawMpvHandle) {
    // SAFETY: callers only pass live handles returned by `linked_create`.
    unsafe { libmpv2_sys::mpv_terminate_destroy(handle.cast()) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandleState {
    Created,
    Initialized,
    Destroyed,
}

/// RAII owner for one `mpv_handle`.
///
/// The owner is deliberately `!Send` and `!Sync`: P3 creates it on the
/// serialized backend owner thread (or platform main thread) instead of moving
/// an initialized native handle between executors.
pub struct MpvHandle {
    raw: Option<NonNull<RawMpvHandle>>,
    api: MpvFunctionTable,
    report: MpvCompatibilityReport,
    state: HandleState,
    thread_affinity: PhantomData<Rc<()>>,
}

impl fmt::Debug for MpvHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MpvHandle")
            .field("report", &self.report)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl MpvHandle {
    /// Validate the runtime and call `mpv_create` without initializing it.
    ///
    /// Keeping creation and initialization separate leaves room for
    /// deterministic pre-initialization options in P3.
    pub fn create(api: MpvFunctionTable) -> Result<Self, MpvFfiError> {
        let report = api.compatibility_report();
        if !report.compatible {
            return Err(MpvFfiError::IncompatibleClientApi {
                runtime: report.runtime,
                minimum: report.minimum,
            });
        }

        // SAFETY: `MpvFunctionTable` guarantees this is the matching create
        // symbol and that it remains valid for the resulting owner.
        let raw = NonNull::new(unsafe { (api.create)() })
            .ok_or(MpvFfiError::CreateFailed)?;

        Ok(Self {
            raw: Some(raw),
            api,
            report,
            state: HandleState::Created,
            thread_affinity: PhantomData,
        })
    }

    /// Initialize the playback core after all required options are set.
    pub fn initialize(&mut self) -> Result<(), MpvFfiError> {
        match self.state {
            HandleState::Initialized => {
                return Err(MpvFfiError::AlreadyInitialized);
            }
            HandleState::Destroyed => return Err(MpvFfiError::Destroyed),
            HandleState::Created => {}
        }

        let raw = self.raw.ok_or(MpvFfiError::Destroyed)?;
        // SAFETY: `raw` is a live handle created by this exact table and no
        // other owner can initialize or destroy it.
        let code = unsafe { (self.api.initialize)(raw.as_ptr()) };
        if code < 0 {
            // mpv's initialization contract requires terminate-destroy after
            // an initialization failure, not the weaker uninitialized destroy.
            // SAFETY: `raw` is still exclusively owned and live here.
            unsafe { (self.api.terminate_destroy)(raw.as_ptr()) };
            self.raw = None;
            self.state = HandleState::Destroyed;
            return Err(MpvFfiError::InitializationFailed { code });
        }

        self.state = HandleState::Initialized;
        Ok(())
    }

    /// Compatibility details captured before handle allocation.
    pub const fn compatibility_report(&self) -> MpvCompatibilityReport {
        self.report
    }

    /// Whether `mpv_initialize` completed successfully.
    pub const fn is_initialized(&self) -> bool {
        matches!(self.state, HandleState::Initialized)
    }

    /// Borrow the opaque native pointer through the raw extension boundary.
    ///
    /// # Safety
    ///
    /// The caller must not destroy the handle, retain it beyond this borrow,
    /// race the serialized owner, or violate any libmpv client API contract.
    pub const unsafe fn as_raw(&self) -> Option<NonNull<c_void>> {
        self.raw
    }

    pub(crate) const fn raw_ptr(&self) -> Option<NonNull<RawMpvHandle>> {
        self.raw
    }
}

impl Drop for MpvHandle {
    fn drop(&mut self) {
        let Some(raw) = self.raw.take() else {
            return;
        };

        // SAFETY: this owner has exclusive ownership of the live handle. The
        // correct destructor depends on whether initialization succeeded.
        unsafe {
            match self.state {
                HandleState::Created => (self.api.destroy)(raw.as_ptr()),
                HandleState::Initialized => {
                    (self.api.terminate_destroy)(raw.as_ptr());
                }
                HandleState::Destroyed => {}
            }
        }
        self.state = HandleState::Destroyed;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());
    static CREATE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static INITIALIZE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DESTROY_COUNT: AtomicUsize = AtomicUsize::new(0);
    static TERMINATE_COUNT: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn compatible_version() -> c_ulong {
        MpvClientApiVersion::new(2, 7).packed() as c_ulong
    }

    unsafe extern "C" fn old_version() -> c_ulong {
        MpvClientApiVersion::new(2, 1).packed() as c_ulong
    }

    unsafe extern "C" fn wrong_major_version() -> c_ulong {
        MpvClientApiVersion::new(3, 0).packed() as c_ulong
    }

    unsafe extern "C" fn fake_create() -> *mut RawMpvHandle {
        CREATE_COUNT.fetch_add(1, Ordering::SeqCst);
        NonNull::<u8>::dangling().as_ptr().cast()
    }

    unsafe extern "C" fn fake_initialize(_handle: *mut RawMpvHandle) -> c_int {
        INITIALIZE_COUNT.fetch_add(1, Ordering::SeqCst);
        0
    }

    unsafe extern "C" fn failing_initialize(
        _handle: *mut RawMpvHandle,
    ) -> c_int {
        INITIALIZE_COUNT.fetch_add(1, Ordering::SeqCst);
        -3
    }

    unsafe extern "C" fn fake_destroy(_handle: *mut RawMpvHandle) {
        DESTROY_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    unsafe extern "C" fn fake_terminate(_handle: *mut RawMpvHandle) {
        TERMINATE_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    fn table(version: ClientApiVersionFn) -> MpvFunctionTable {
        // SAFETY: every fake follows the required ABI and accepts the opaque
        // non-null sentinel returned by `fake_create`.
        unsafe {
            MpvFunctionTable::from_raw_parts(
                version,
                fake_create,
                fake_initialize,
                fake_destroy,
                fake_terminate,
            )
        }
    }

    fn reset_counts() {
        CREATE_COUNT.store(0, Ordering::SeqCst);
        INITIALIZE_COUNT.store(0, Ordering::SeqCst);
        DESTROY_COUNT.store(0, Ordering::SeqCst);
        TERMINATE_COUNT.store(0, Ordering::SeqCst);
    }

    #[test]
    fn client_api_version_round_trips_packed_layout() {
        let version = MpvClientApiVersion::new(2, 5);
        assert_eq!(MpvClientApiVersion::from_packed(version.packed()), version);
        assert!(MpvClientApiVersion::new(2, 7).satisfies(version));
        assert!(!MpvClientApiVersion::new(2, 4).satisfies(version));
        assert!(!MpvClientApiVersion::new(3, 5).satisfies(version));
    }

    #[test]
    fn incompatible_runtime_is_rejected_before_allocation() {
        assert!(matches!(
            MpvHandle::create(table(old_version)),
            Err(MpvFfiError::IncompatibleClientApi { .. })
        ));
        assert!(matches!(
            MpvHandle::create(table(wrong_major_version)),
            Err(MpvFfiError::IncompatibleClientApi { .. })
        ));
    }

    #[test]
    fn lifecycle_uses_the_destructor_matching_initialization_state() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_counts();

        drop(MpvHandle::create(table(compatible_version)).unwrap());
        assert_eq!(CREATE_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(DESTROY_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(TERMINATE_COUNT.load(Ordering::SeqCst), 0);

        let mut initialized =
            MpvHandle::create(table(compatible_version)).unwrap();
        initialized.initialize().unwrap();
        assert!(initialized.is_initialized());
        drop(initialized);

        assert_eq!(CREATE_COUNT.load(Ordering::SeqCst), 2);
        assert_eq!(INITIALIZE_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(DESTROY_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(TERMINATE_COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_initialization_terminates_once_and_invalidates_handle() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_counts();
        // SAFETY: these fake symbols obey the same contract as `table`, with
        // initialization deliberately returning a native error.
        let api = unsafe {
            MpvFunctionTable::from_raw_parts(
                compatible_version,
                fake_create,
                failing_initialize,
                fake_destroy,
                fake_terminate,
            )
        };
        let mut handle = MpvHandle::create(api).unwrap();

        assert_eq!(
            handle.initialize(),
            Err(MpvFfiError::InitializationFailed { code: -3 })
        );
        assert_eq!(handle.initialize(), Err(MpvFfiError::Destroyed));
        drop(handle);

        assert_eq!(TERMINATE_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(DESTROY_COUNT.load(Ordering::SeqCst), 0);
    }

    #[cfg(feature = "linked")]
    #[test]
    fn linked_libmpv_creates_initializes_and_destroys_a_handle() {
        let api = MpvFunctionTable::linked();
        let report = api.compatibility_report();
        assert!(report.compatible, "{report:?}");

        let mut handle = MpvHandle::create(api).unwrap();
        handle.initialize().unwrap();
        assert!(handle.is_initialized());
    }
}
