//! Raw libmpv client ABI used by the safe Ferrex wrapper.
//!
//! These declarations intentionally mirror the small subset of `client.h`
//! needed by the control plane. Constructing a function table is unsafe; normal
//! callers should use [`crate::MpvFunctionTable::linked`] instead.

use std::ffi::{c_char, c_double, c_int, c_uint, c_void};

pub(crate) type RawMpvHandle = c_void;

pub(crate) const FORMAT_NONE: c_uint = 0;
pub(crate) const FORMAT_STRING: c_uint = 1;
pub(crate) const FORMAT_OSD_STRING: c_uint = 2;
pub(crate) const FORMAT_FLAG: c_uint = 3;
pub(crate) const FORMAT_INT64: c_uint = 4;
pub(crate) const FORMAT_DOUBLE: c_uint = 5;
pub(crate) const FORMAT_NODE: c_uint = 6;
pub(crate) const FORMAT_NODE_ARRAY: c_uint = 7;
pub(crate) const FORMAT_NODE_MAP: c_uint = 8;
pub(crate) const FORMAT_BYTE_ARRAY: c_uint = 9;

pub(crate) const EVENT_NONE: c_uint = 0;
pub(crate) const EVENT_SHUTDOWN: c_uint = 1;
pub(crate) const EVENT_LOG_MESSAGE: c_uint = 2;
pub(crate) const EVENT_GET_PROPERTY_REPLY: c_uint = 3;
pub(crate) const EVENT_SET_PROPERTY_REPLY: c_uint = 4;
pub(crate) const EVENT_COMMAND_REPLY: c_uint = 5;
pub(crate) const EVENT_START_FILE: c_uint = 6;
pub(crate) const EVENT_END_FILE: c_uint = 7;
pub(crate) const EVENT_FILE_LOADED: c_uint = 8;
pub(crate) const EVENT_IDLE: c_uint = 11;
pub(crate) const EVENT_TICK: c_uint = 14;
pub(crate) const EVENT_CLIENT_MESSAGE: c_uint = 16;
pub(crate) const EVENT_VIDEO_RECONFIG: c_uint = 17;
pub(crate) const EVENT_AUDIO_RECONFIG: c_uint = 18;
pub(crate) const EVENT_SEEK: c_uint = 20;
pub(crate) const EVENT_PLAYBACK_RESTART: c_uint = 21;
pub(crate) const EVENT_PROPERTY_CHANGE: c_uint = 22;
pub(crate) const EVENT_QUEUE_OVERFLOW: c_uint = 24;
pub(crate) const EVENT_HOOK: c_uint = 25;

pub(crate) const END_FILE_REASON_EOF: c_uint = 0;
pub(crate) const END_FILE_REASON_STOP: c_uint = 2;
pub(crate) const END_FILE_REASON_QUIT: c_uint = 3;
pub(crate) const END_FILE_REASON_ERROR: c_uint = 4;
pub(crate) const END_FILE_REASON_REDIRECT: c_uint = 5;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvNode {
    pub(crate) value: RawMpvNodeValue,
    pub(crate) format: c_uint,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union RawMpvNodeValue {
    pub(crate) string: *mut c_char,
    pub(crate) flag: c_int,
    pub(crate) int64: i64,
    pub(crate) double_: c_double,
    pub(crate) list: *mut RawMpvNodeList,
    pub(crate) bytes: *mut RawMpvByteArray,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvNodeList {
    pub(crate) count: c_int,
    pub(crate) values: *mut RawMpvNode,
    pub(crate) keys: *mut *mut c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvByteArray {
    pub(crate) data: *mut c_void,
    pub(crate) size: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvEvent {
    pub(crate) event_id: c_uint,
    pub(crate) error: c_int,
    pub(crate) reply_userdata: u64,
    pub(crate) data: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvEventProperty {
    pub(crate) name: *const c_char,
    pub(crate) format: c_uint,
    pub(crate) data: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvEventLogMessage {
    pub(crate) prefix: *const c_char,
    pub(crate) level: *const c_char,
    pub(crate) text: *const c_char,
    pub(crate) log_level: c_uint,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvEventStartFile {
    pub(crate) playlist_entry_id: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvEventEndFile {
    pub(crate) reason: c_uint,
    pub(crate) error: c_int,
    pub(crate) playlist_entry_id: i64,
    pub(crate) playlist_insert_id: i64,
    pub(crate) playlist_insert_num_entries: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvEventClientMessage {
    pub(crate) count: c_int,
    pub(crate) args: *mut *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvEventHook {
    pub(crate) name: *const c_char,
    pub(crate) id: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawMpvEventCommand {
    pub(crate) result: RawMpvNode,
}

pub(crate) type SetOptionStringFn = unsafe extern "C" fn(
    *mut RawMpvHandle,
    *const c_char,
    *const c_char,
) -> c_int;
pub(crate) type CommandAsyncFn =
    unsafe extern "C" fn(*mut RawMpvHandle, u64, *mut *const c_char) -> c_int;
pub(crate) type CommandNodeAsyncFn =
    unsafe extern "C" fn(*mut RawMpvHandle, u64, *mut RawMpvNode) -> c_int;
pub(crate) type AbortAsyncCommandFn =
    unsafe extern "C" fn(*mut RawMpvHandle, u64);
pub(crate) type SetPropertyAsyncFn = unsafe extern "C" fn(
    *mut RawMpvHandle,
    u64,
    *const c_char,
    c_uint,
    *mut c_void,
) -> c_int;
pub(crate) type GetPropertyAsyncFn = unsafe extern "C" fn(
    *mut RawMpvHandle,
    u64,
    *const c_char,
    c_uint,
) -> c_int;
pub(crate) type ObservePropertyFn = unsafe extern "C" fn(
    *mut RawMpvHandle,
    u64,
    *const c_char,
    c_uint,
) -> c_int;
pub(crate) type UnobservePropertyFn =
    unsafe extern "C" fn(*mut RawMpvHandle, u64) -> c_int;
pub(crate) type RequestLogMessagesFn =
    unsafe extern "C" fn(*mut RawMpvHandle, *const c_char) -> c_int;
pub(crate) type WaitEventFn =
    unsafe extern "C" fn(*mut RawMpvHandle, c_double) -> *mut RawMpvEvent;
pub(crate) type WakeupCallback = Option<unsafe extern "C" fn(*mut c_void)>;
pub(crate) type SetWakeupCallbackFn =
    unsafe extern "C" fn(*mut RawMpvHandle, WakeupCallback, *mut c_void);
pub(crate) type RequestEventFn =
    unsafe extern "C" fn(*mut RawMpvHandle, c_uint, c_int) -> c_int;
pub(crate) type HookAddFn =
    unsafe extern "C" fn(*mut RawMpvHandle, u64, *const c_char, c_int) -> c_int;
pub(crate) type HookContinueFn =
    unsafe extern "C" fn(*mut RawMpvHandle, u64) -> c_int;

/// Raw control-plane symbols from one ABI-compatible libmpv instance.
#[derive(Clone, Copy)]
pub(crate) struct MpvControlApi {
    pub(crate) set_option_string: SetOptionStringFn,
    pub(crate) command_async: CommandAsyncFn,
    pub(crate) command_node_async: CommandNodeAsyncFn,
    pub(crate) abort_async_command: AbortAsyncCommandFn,
    pub(crate) set_property_async: SetPropertyAsyncFn,
    pub(crate) get_property_async: GetPropertyAsyncFn,
    pub(crate) observe_property: ObservePropertyFn,
    pub(crate) unobserve_property: UnobservePropertyFn,
    pub(crate) request_log_messages: RequestLogMessagesFn,
    pub(crate) wait_event: WaitEventFn,
    pub(crate) set_wakeup_callback: SetWakeupCallbackFn,
    pub(crate) request_event: RequestEventFn,
    pub(crate) hook_add: HookAddFn,
    pub(crate) hook_continue: HookContinueFn,
}

impl std::fmt::Debug for MpvControlApi {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MpvControlApi")
            .finish_non_exhaustive()
    }
}

impl MpvControlApi {
    /// Construct a control table from trusted ABI-compatible symbols.
    ///
    /// # Safety
    ///
    /// Every symbol must implement the matching libmpv client API function,
    /// remain valid for all handles used with this table, and originate from
    /// the same library instance as the lifecycle symbols.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) const unsafe fn from_raw_parts(
        set_option_string: SetOptionStringFn,
        command_async: CommandAsyncFn,
        command_node_async: CommandNodeAsyncFn,
        abort_async_command: AbortAsyncCommandFn,
        set_property_async: SetPropertyAsyncFn,
        get_property_async: GetPropertyAsyncFn,
        observe_property: ObservePropertyFn,
        unobserve_property: UnobservePropertyFn,
        request_log_messages: RequestLogMessagesFn,
        wait_event: WaitEventFn,
        set_wakeup_callback: SetWakeupCallbackFn,
        request_event: RequestEventFn,
        hook_add: HookAddFn,
        hook_continue: HookContinueFn,
    ) -> Self {
        Self {
            set_option_string,
            command_async,
            command_node_async,
            abort_async_command,
            set_property_async,
            get_property_async,
            observe_property,
            unobserve_property,
            request_log_messages,
            wait_event,
            set_wakeup_callback,
            request_event,
            hook_add,
            hook_continue,
        }
    }

    #[cfg(feature = "linked")]
    pub(crate) const fn linked() -> Self {
        Self {
            set_option_string: linked_set_option_string,
            command_async: linked_command_async,
            command_node_async: linked_command_node_async,
            abort_async_command: linked_abort_async_command,
            set_property_async: linked_set_property_async,
            get_property_async: linked_get_property_async,
            observe_property: linked_observe_property,
            unobserve_property: linked_unobserve_property,
            request_log_messages: linked_request_log_messages,
            wait_event: linked_wait_event,
            set_wakeup_callback: linked_set_wakeup_callback,
            request_event: linked_request_event,
            hook_add: linked_hook_add,
            hook_continue: linked_hook_continue,
        }
    }
}

#[cfg(feature = "linked")]
const _: () = {
    use std::mem::{align_of, offset_of, size_of};

    assert!(size_of::<RawMpvNode>() == size_of::<libmpv2_sys::mpv_node>());
    assert!(align_of::<RawMpvNode>() == align_of::<libmpv2_sys::mpv_node>());
    assert!(
        offset_of!(RawMpvNode, value) == offset_of!(libmpv2_sys::mpv_node, u)
    );
    assert!(
        offset_of!(RawMpvNode, format)
            == offset_of!(libmpv2_sys::mpv_node, format)
    );

    assert!(
        size_of::<RawMpvNodeList>() == size_of::<libmpv2_sys::mpv_node_list>()
    );
    assert!(
        align_of::<RawMpvNodeList>()
            == align_of::<libmpv2_sys::mpv_node_list>()
    );
    assert!(
        offset_of!(RawMpvNodeList, count)
            == offset_of!(libmpv2_sys::mpv_node_list, num)
    );
    assert!(
        offset_of!(RawMpvNodeList, values)
            == offset_of!(libmpv2_sys::mpv_node_list, values)
    );
    assert!(
        offset_of!(RawMpvNodeList, keys)
            == offset_of!(libmpv2_sys::mpv_node_list, keys)
    );

    assert!(
        size_of::<RawMpvByteArray>()
            == size_of::<libmpv2_sys::mpv_byte_array>()
    );
    assert!(
        align_of::<RawMpvByteArray>()
            == align_of::<libmpv2_sys::mpv_byte_array>()
    );
    assert!(size_of::<RawMpvEvent>() == size_of::<libmpv2_sys::mpv_event>());
    assert!(align_of::<RawMpvEvent>() == align_of::<libmpv2_sys::mpv_event>());
    assert!(
        offset_of!(RawMpvEvent, data)
            == offset_of!(libmpv2_sys::mpv_event, data)
    );
    assert!(
        size_of::<RawMpvEventProperty>()
            == size_of::<libmpv2_sys::mpv_event_property>()
    );
    assert!(
        align_of::<RawMpvEventProperty>()
            == align_of::<libmpv2_sys::mpv_event_property>()
    );
    assert!(
        size_of::<RawMpvEventLogMessage>()
            == size_of::<libmpv2_sys::mpv_event_log_message>()
    );
    assert!(
        align_of::<RawMpvEventLogMessage>()
            == align_of::<libmpv2_sys::mpv_event_log_message>()
    );
    assert!(
        size_of::<RawMpvEventEndFile>()
            == size_of::<libmpv2_sys::mpv_event_end_file>()
    );
    assert!(
        align_of::<RawMpvEventEndFile>()
            == align_of::<libmpv2_sys::mpv_event_end_file>()
    );
};

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_set_option_string(
    handle: *mut RawMpvHandle,
    name: *const c_char,
    value: *const c_char,
) -> c_int {
    // SAFETY: the wrapper preserves the libmpv ABI and opaque handle identity.
    unsafe { libmpv2_sys::mpv_set_option_string(handle.cast(), name, value) }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_command_async(
    handle: *mut RawMpvHandle,
    userdata: u64,
    args: *mut *const c_char,
) -> c_int {
    // SAFETY: arguments follow `mpv_command_async` and are copied by libmpv.
    unsafe { libmpv2_sys::mpv_command_async(handle.cast(), userdata, args) }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_command_node_async(
    handle: *mut RawMpvHandle,
    userdata: u64,
    args: *mut RawMpvNode,
) -> c_int {
    // SAFETY: raw node layouts are asserted above and libmpv copies arguments.
    unsafe {
        libmpv2_sys::mpv_command_node_async(
            handle.cast(),
            userdata,
            args.cast(),
        )
    }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_abort_async_command(
    handle: *mut RawMpvHandle,
    userdata: u64,
) {
    // SAFETY: forwarded to the matching handle's abort function.
    unsafe { libmpv2_sys::mpv_abort_async_command(handle.cast(), userdata) };
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_set_property_async(
    handle: *mut RawMpvHandle,
    userdata: u64,
    name: *const c_char,
    format: c_uint,
    data: *mut c_void,
) -> c_int {
    // SAFETY: format/data are built according to `client.h` and copied by mpv.
    unsafe {
        libmpv2_sys::mpv_set_property_async(
            handle.cast(),
            userdata,
            name,
            format,
            data,
        )
    }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_get_property_async(
    handle: *mut RawMpvHandle,
    userdata: u64,
    name: *const c_char,
    format: c_uint,
) -> c_int {
    // SAFETY: forwarded with an ABI-compatible format value.
    unsafe {
        libmpv2_sys::mpv_get_property_async(
            handle.cast(),
            userdata,
            name,
            format,
        )
    }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_observe_property(
    handle: *mut RawMpvHandle,
    userdata: u64,
    name: *const c_char,
    format: c_uint,
) -> c_int {
    // SAFETY: forwarded with an ABI-compatible format value.
    unsafe {
        libmpv2_sys::mpv_observe_property(handle.cast(), userdata, name, format)
    }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_unobserve_property(
    handle: *mut RawMpvHandle,
    userdata: u64,
) -> c_int {
    // SAFETY: userdata belongs to an observation on this handle.
    unsafe { libmpv2_sys::mpv_unobserve_property(handle.cast(), userdata) }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_request_log_messages(
    handle: *mut RawMpvHandle,
    level: *const c_char,
) -> c_int {
    // SAFETY: level is a live NUL-terminated string for the duration of call.
    unsafe { libmpv2_sys::mpv_request_log_messages(handle.cast(), level) }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_wait_event(
    handle: *mut RawMpvHandle,
    timeout: c_double,
) -> *mut RawMpvEvent {
    // SAFETY: event layout is asserted above; lifetime remains owned by mpv.
    unsafe { libmpv2_sys::mpv_wait_event(handle.cast(), timeout).cast() }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_set_wakeup_callback(
    handle: *mut RawMpvHandle,
    callback: WakeupCallback,
    userdata: *mut c_void,
) {
    // SAFETY: callback ABI exactly matches the libmpv declaration.
    unsafe {
        libmpv2_sys::mpv_set_wakeup_callback(handle.cast(), callback, userdata)
    };
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_request_event(
    handle: *mut RawMpvHandle,
    event: c_uint,
    enabled: c_int,
) -> c_int {
    // SAFETY: event IDs and enable values use the native representation.
    unsafe { libmpv2_sys::mpv_request_event(handle.cast(), event, enabled) }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_hook_add(
    handle: *mut RawMpvHandle,
    userdata: u64,
    name: *const c_char,
    priority: c_int,
) -> c_int {
    // SAFETY: name is live for the call and libmpv copies it.
    unsafe {
        libmpv2_sys::mpv_hook_add(handle.cast(), userdata, name, priority)
    }
}

#[cfg(feature = "linked")]
unsafe extern "C" fn linked_hook_continue(
    handle: *mut RawMpvHandle,
    id: u64,
) -> c_int {
    // SAFETY: callers validate that the hook ID is outstanding on this handle.
    unsafe { libmpv2_sys::mpv_hook_continue(handle.cast(), id) }
}
