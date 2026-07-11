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
    #[inline]
    pub unsafe fn from_index(vm: *mut sys::vlib_main_t, bi: u32) -> Buffer {
        // native reimplementation of the vlib_get_buffer() inline:
        // buffer_mem_start + (bi << log2-cache-line)
        unsafe {
            let bm = (*vm).buffer_main;
            let b = ((*bm).buffer_mem_start
                + ((bi as sys::uword) << sys::CLIB_LOG2_CACHE_LINE_BYTES))
                as *mut sys::vlib_buffer_t;
            debug_assert_eq!(b, sys::vlib_get_buffer(vm, bi));
            Buffer(b)
        }
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
    #[inline]
    pub unsafe fn current<T>(&self) -> *mut T {
        // native reimplementation of vlib_buffer_get_current():
        // b->data + b->current_data
        unsafe {
            let data =
                (self.0 as *mut u8).add(core::mem::offset_of!(sys::vlib_buffer_t__bindgen_ty_1, data));
            let p = data.offset((*self.tmpl()).current_data as isize);
            debug_assert_eq!(p, sys::vlib_buffer_get_current(self.0) as *mut u8);
            p as *mut T
        }
    }
}
