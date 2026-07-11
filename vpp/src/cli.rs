//! CLI command registration and helpers.

use crate::sys;
use core::cell::UnsafeCell;
use core::ffi::{c_char, CStr};
use std::ffi::CString;

pub type CliHandler = unsafe extern "C" fn(
    vm: *mut sys::vlib_main_t,
    input: *mut sys::unformat_input_t,
    cmd: *mut sys::vlib_cli_command_t,
) -> *mut sys::clib_error_t;

pub struct CliCell(UnsafeCell<sys::vlib_cli_command_t>);
unsafe impl Sync for CliCell {}

impl CliCell {
    pub const fn new() -> Self {
        CliCell(UnsafeCell::new(unsafe {
            core::mem::MaybeUninit::zeroed().assume_init()
        }))
    }
}

/// Register a CLI command. Call from a `vpp::ctor!` constructor.
///
/// # Safety
/// Must run at dlopen time on the single loading thread.
pub unsafe fn register_cli(
    cell: &'static CliCell,
    path: &'static CStr,
    short_help: &'static CStr,
    handler: CliHandler,
) {
    unsafe {
        let c = &mut *cell.0.get();
        c.path = path.as_ptr() as *mut c_char;
        c.short_help = short_help.as_ptr() as *mut c_char;
        c.function = Some(handler);
        sys::vlib_cli_command_registration_helper(c);
    }
}

/// Print a line on the current CLI session.
pub fn output(vm: *mut sys::vlib_main_t, text: &str) {
    let c = CString::new(text).unwrap_or_default();
    unsafe { sys::vlib_cli_output(vm, c"%s".as_ptr() as *mut c_char, c.as_ptr()) };
}

/// Return a CLI error (shows as "rateguard: <msg>" in red on the CLI).
pub fn error(msg: &CStr) -> *mut sys::clib_error_t {
    unsafe {
        sys::_clib_error_return(
            core::ptr::null_mut(),
            0,
            0,
            core::ptr::null(),
            c"%s".as_ptr(),
            msg.as_ptr(),
        )
    }
}

pub const END_OF_INPUT: sys::uword = !0;

/// True while there is more CLI input to parse.
pub fn more_input(input: *mut sys::unformat_input_t) -> bool {
    unsafe { sys::unformat_check_input(input) != END_OF_INPUT }
}
