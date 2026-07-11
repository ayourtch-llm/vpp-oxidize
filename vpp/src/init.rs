//! VLIB init-function registration — the Rust mirror of
//! `VLIB_INIT_FUNCTION`. Nodes/features/CLI register pure data at
//! dlopen time; anything that must CALL into VPP (like binary-API
//! message registration) belongs in an init function, which VPP runs
//! once the relevant subsystems exist.

use core::cell::UnsafeCell;
use core::ffi::CStr;

use crate::sys;

/// Init function: return NULL on success, a clib_error_t on failure.
pub type InitFn = unsafe extern "C" fn(*mut sys::vlib_main_t) -> *mut sys::clib_error_t;

pub struct InitCell(UnsafeCell<sys::_vlib_init_function_list_elt>);
unsafe impl Sync for InitCell {}

impl InitCell {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        InitCell(UnsafeCell::new(unsafe {
            core::mem::MaybeUninit::zeroed().assume_init()
        }))
    }
}

/// Link an init function into `vgm->init_function_registrations` — the
/// same list insertion VLIB_INIT_FUNCTION's constructor does.
///
/// # Safety
/// Must run at dlopen time on the single loading thread.
pub unsafe fn register_init(cell: &'static InitCell, name: &'static CStr, f: InitFn) {
    unsafe {
        let r = &mut *cell.0.get();
        r.f = Some(f);
        r.name = name.as_ptr() as *mut core::ffi::c_char;
        let vgm = sys::vlib_get_global_main();
        r.next_init_function = (*vgm).init_function_registrations;
        (*vgm).init_function_registrations = r;
    }
}

/// Register a plugin init function, run by VPP after core init:
///
/// ```ignore
/// vpp::define_init! { static INIT: c"my_plugin_api_init", handler api_init_fn }
/// ```
#[macro_export]
macro_rules! define_init {
    ($vis:vis static $cell:ident: $name:expr, handler $f:path $(,)?) => {
        $vis static $cell: $crate::init::InitCell = $crate::init::InitCell::new();
        const _: () = {
            unsafe extern "C" fn __register_init() {
                unsafe { $crate::init::register_init(&$cell, $name, $f) };
            }
            #[used]
            #[unsafe(link_section = ".init_array")]
            static CTOR: unsafe extern "C" fn() = __register_init;
        };
    };
}
