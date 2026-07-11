//! vlib_buffer_t access.
//!
//! `vlib_buffer_template_t` is the first member of the `vlib_buffer_t`
//! union (asserted by bindgen's layout tests), so metadata access is a
//! pointer cast.

use crate::sys;

pub const IS_TRACED: u32 = sys::VLIB_BUFFER_IS_TRACED as u32;

/// A borrowed VPP buffer. Wraps the raw pointer; the caller must ensure
/// exclusive access for the duration of node processing (guaranteed by
/// VPP's dispatch model: a buffer belongs to exactly one in-flight frame).
pub struct Buffer(*mut sys::vlib_buffer_t);

impl Buffer {
    /// # Safety
    /// `bi` must be a valid buffer index owned by the current frame.
    pub unsafe fn from_index(vm: *mut sys::vlib_main_t, bi: u32) -> Buffer {
        Buffer(unsafe { sys::vlib_get_buffer(vm, bi) })
    }

    pub fn raw(&self) -> *mut sys::vlib_buffer_t {
        self.0
    }

    fn tmpl(&self) -> *mut sys::vlib_buffer_template_t {
        // template is at offset 0 of the vlib_buffer_t union
        self.0.cast()
    }

    pub fn flags(&self) -> u32 {
        unsafe { (*self.tmpl()).flags }
    }

    pub fn is_traced(&self) -> bool {
        self.flags() & IS_TRACED != 0
    }

    pub fn current_length(&self) -> u16 {
        unsafe { (*self.tmpl()).current_length }
    }

    /// Set the per-buffer error code (counter attribution on drop paths).
    /// `error` is `node_runtime.errors[code]`.
    pub fn set_error(&mut self, error: u16) {
        unsafe { (*self.tmpl()).error = error }
    }

    /// Pointer to the current parse position (e.g. the IPv4 header when
    /// running as an ip4-unicast feature).
    ///
    /// # Safety
    /// Caller asserts at least `size_of::<T>()` bytes are present at the
    /// current position (check `current_length` first for parsers).
    pub unsafe fn current<T>(&self) -> *mut T {
        unsafe { sys::vlib_buffer_get_current(self.0) as *mut T }
    }
}
