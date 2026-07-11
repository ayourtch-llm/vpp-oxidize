//! Packet trace support.
//!
//! A node's `format_trace` callback receives `(u8 *s, va_list *args)` where
//! the va_list holds `(vlib_main_t *, vlib_node_t *, T *trace_data)`. Rust
//! cannot `va_arg`, so a C shim extracts the pointers.

use crate::buffer::Buffer;
use crate::sys;
use core::ffi::c_void;

/// Reserve trace space for one buffer and return the typed trace record.
///
/// # Safety
/// Call only from a node function, for buffers with the traced flag set.
pub unsafe fn add<T>(
    vm: *mut sys::vlib_main_t,
    node: *mut sys::vlib_node_runtime_t,
    b: &Buffer,
) -> *mut T {
    unsafe { sys::vlib_add_trace(vm, node, b.raw(), core::mem::size_of::<T>() as u32) as *mut T }
}

/// Pull the trace-data pointer out of a `format_trace` va_list, skipping
/// the leading (vlib_main_t *, vlib_node_t *) arguments.
///
/// # Safety
/// Call exactly once, first thing, in a `format_trace` callback.
pub unsafe fn format_args<T>(args: *mut sys::va_list) -> *mut T {
    unsafe {
        let ap = args as *mut c_void;
        let _vm = sys::vppsys_va_arg_ptr(ap);
        let _node = sys::vppsys_va_arg_ptr(ap);
        sys::vppsys_va_arg_ptr(ap) as *mut T
    }
}
