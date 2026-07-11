//! Safe(ish) building blocks for writing VPP plugins in Rust.
//!
//! Design: all `unsafe` FFI plumbing lives here (and in `vpp-sys`);
//! plugin crates should mostly contain safe logic. The registration
//! model mirrors what VPP's C macros do — constructor functions run at
//! `dlopen()` time and link registration structs into `vlib_global_main`
//! / `feature_main`, before VPP's init processes those lists.

pub use vpp_sys as sys;

pub mod buffer;
pub mod cli;
pub mod feature;
pub mod node;
pub mod plugin;
pub mod trace;

/// Register a constructor to run at plugin `dlopen()` time — the same
/// mechanism VPP's `VLIB_REGISTER_NODE` etc. use in C.
#[macro_export]
macro_rules! ctor {
    ($f:path) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".init_array")]
            static CTOR: unsafe extern "C" fn() = $f;
        };
    };
}

/// Current time in seconds (VPP's per-main-loop clock).
pub fn time_now(vm: *mut sys::vlib_main_t) -> f64 {
    unsafe { sys::vlib_time_now(vm) }
}

/// Index of the current VPP thread (0 = main, 1.. = workers).
pub fn thread_index() -> u32 {
    unsafe { sys::vlib_get_thread_index() as u32 }
}
