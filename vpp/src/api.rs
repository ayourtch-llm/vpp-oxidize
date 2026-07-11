//! Server-side binary API for Rust plugins: register plugin messages
//! with VPP's api infrastructure and send replies.
//!
//! VPP's binary API model, condensed:
//! - message IDs are dynamic: a plugin asks for a contiguous ID range
//!   (`vl_msg_api_get_msg_ids`) and publishes each message under a
//!   `"name_crc"` string so clients resolve IDs by name+CRC at connect
//!   time. There is no vppapigen here — the `"crc"` suffix is just a
//!   version stamp you bump on any wire-format change, and the message
//!   structs are written by hand on both sides.
//! - on the wire everything after the 2-byte (big-endian) message ID is
//!   the message struct, packed, fields big-endian by convention;
//!   `client_index` and `context` are opaque echoes (do not byte-swap).
//! - registration must happen from an init function (`define_init!`),
//!   not a dlopen ctor: the api infrastructure has to exist first.
//!
//! Handlers run on the main thread with the worker barrier held
//! (is_mp_safe = 0), so plain reads/writes of plugin config are safe.

use core::ffi::{c_char, c_void, CStr};

use crate::sys;

/// One message: its resolution string and handler.
/// `size`/`calc_size` come from the [`api_message!`] helper.
pub struct MsgDef {
    /// `"name_crc"` string clients resolve the ID with, e.g.
    /// `c"ttlgate_enable_disable_01234567"`.
    pub name_crc: &'static CStr,
    /// `void handler(void *msg)` — cast the pointer to your message
    /// struct; byte-swap multi-byte fields. `None` for replies: they
    /// occupy an ID (clients resolve them by name+CRC) but are never
    /// dispatched on the server.
    pub handler: Option<unsafe extern "C" fn(*mut c_void)>,
    /// Wire size of the fixed-size message struct.
    pub size: usize,
    /// Returns the expected size given the raw message (dispatch
    /// drops truncated messages based on this).
    pub calc_size: unsafe extern "C" fn(*mut c_void) -> sys::uword,
}

/// Expands to a [`MsgDef`] for a fixed-size `#[repr(C, packed)]`
/// message struct:
/// `api_message!(c"ttlgate_set_v1_00000001", TtlgateSet, handler_fn)`
/// for requests, `api_message!(c"..._reply_...", TtlgateSetReply)` for
/// replies.
#[macro_export]
macro_rules! api_message {
    ($name_crc:expr, $ty:ty, $handler:path $(,)?) => {
        $crate::__api_message_impl!($name_crc, $ty, Some($handler))
    };
    ($name_crc:expr, $ty:ty $(,)?) => {
        $crate::__api_message_impl!($name_crc, $ty, None)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __api_message_impl {
    ($name_crc:expr, $ty:ty, $handler:expr) => {{
        unsafe extern "C" fn __calc_size(_m: *mut core::ffi::c_void) -> $crate::sys::uword {
            core::mem::size_of::<$ty>() as $crate::sys::uword
        }
        $crate::api::MsgDef {
            name_crc: $name_crc,
            handler: $handler,
            size: core::mem::size_of::<$ty>(),
            calc_size: __calc_size,
        }
    }};
}

/// Allocate a message-ID range for the plugin and register every
/// message in it. `range_name` identifies the plugin's ID block (the
/// conventional form is `"<plugin>_<version-stamp>"`). Returns the
/// first assigned ID; message `i` of `msgs` got ID `first + i`.
///
/// Call from an init function ([`crate::define_init!`]).
///
/// # Safety
/// Requires VPP's api infrastructure to be initialized (init-function
/// time or later, main thread).
pub unsafe fn register_messages(range_name: &CStr, msgs: &[MsgDef]) -> u16 {
    unsafe {
        let first = sys::vl_msg_api_get_msg_ids(range_name.as_ptr(), msgs.len() as i32);
        let am = sys::vlibapi_get_main();
        for (i, m) in msgs.iter().enumerate() {
            let id = first + i as u16;
            sys::vl_msg_api_add_msg_name_crc(am, m.name_crc.as_ptr(), id as u32);
            let Some(handler) = m.handler else {
                continue; // reply: name+ID only, never dispatched here
            };
            let mut cfg: sys::vl_msg_api_msg_config_t = core::mem::zeroed();
            cfg.id = id as i32;
            cfg.name = m.name_crc.as_ptr() as *mut c_char;
            cfg.handler = handler as *mut c_void;
            cfg.calc_size = m.calc_size as *mut c_void;
            cfg.size = m.size as i32;
            // no endian/format/json functions: handlers do their own
            // byte swaps (is_autoendian = 0), and the message is not
            // api-traceable without vppapigen-generated helpers
            cfg.set_traced(0);
            cfg.set_replay(0);
            cfg.set_message_bounce(0);
            cfg.set_is_mp_safe(0);
            cfg.set_is_autoendian(0);
            sys::vl_msg_api_config(&mut cfg);
        }
        first
    }
}

/// Allocate, fill and send a reply to the client identified by the
/// request's (opaque) `client_index`. `msg_id` is the reply's assigned
/// ID (`first + offset`); `fill` sees a zeroed `T` with the ID already
/// stamped — byte-swap what you put in.
///
/// # Safety
/// Main-thread only (API handler context). `T` must be the packed wire
/// struct whose first field is the 2-byte message ID.
pub unsafe fn send_reply<T>(client_index: u32, msg_id: u16, fill: impl FnOnce(&mut T)) {
    unsafe {
        let reg = sys::vl_api_client_index_to_registration(client_index);
        if reg.is_null() {
            return; // client vanished; nothing to reply to
        }
        let mp = sys::vl_msg_api_alloc_zero(core::mem::size_of::<T>() as i32) as *mut T;
        (mp as *mut u16).write(msg_id.to_be());
        fill(&mut *mp);
        sys::vl_api_send_msg(reg, mp as *mut sys::u8_);
    }
}
