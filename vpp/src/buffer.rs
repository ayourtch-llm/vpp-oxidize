//! vlib_buffer_t access.
//!
//! `vlib_buffer_template_t` is the first member of the `vlib_buffer_t`
//! union (asserted by bindgen's layout tests), so metadata access is a
//! pointer cast. Since VPP 297bd92f2 the template itself is a union whose
//! first member is the struct of fields; `vpp-sys` resolves the right
//! struct type per VPP tree as `vlib_buffer_template_fields_t`.

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

    fn tmpl(&self) -> *mut sys::vlib_buffer_template_fields_t {
        // template fields are at offset 0 of the vlib_buffer_t union
        const _: () = assert!(
            core::mem::size_of::<sys::vlib_buffer_template_fields_t>() == 64,
            "vlib_buffer_t template fields must span exactly the first cacheline"
        );
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

    /// Next node along the feature arc for this buffer — the native
    /// reimplementation of `vnet_feature_next()`: the buffer's
    /// `current_config_index` points into the shared feature config
    /// heap (a u32 vector); the entry is the next-node index and the
    /// config index advances past it. Debug builds cross-check against
    /// the C shim.
    #[inline]
    pub fn feature_next(&mut self) -> u32 {
        unsafe {
            let t = self.tmpl();
            let i = (*t).__bindgen_anon_1.current_config_index;
            let heap = (*(&raw const sys::feature_main)).shared_feature_config_heap;
            let next = *heap.add(i as usize);
            (*t).__bindgen_anon_1.current_config_index = i + 1;
            #[cfg(debug_assertions)]
            {
                // rerun through the C shim and compare both outputs
                (*t).__bindgen_anon_1.current_config_index = i;
                let mut c_next: u32 = 0;
                sys::vnet_feature_next(&mut c_next, self.0);
                debug_assert_eq!(next, c_next);
                debug_assert_eq!((*t).__bindgen_anon_1.current_config_index, i + 1);
            }
            next
        }
    }

    /// Prefetch the buffer metadata (first cacheline) of `bi` — call a
    /// few packets ahead of using `from_index`.
    ///
    /// # Safety
    /// `bi` must be a valid buffer index (prefetching a bad address is
    /// harmless on the hardware, but don't hand this garbage).
    #[inline(always)]
    pub unsafe fn prefetch_header(vm: *mut sys::vlib_main_t, bi: u32) {
        unsafe {
            let bm = (*vm).buffer_main;
            let p = ((*bm).buffer_mem_start + ((bi as sys::uword) << sys::CLIB_LOG2_CACHE_LINE_BYTES))
                as *const u8;
            prefetch_read(p);
        }
    }

    /// Prefetch the packet data at the current parse position.
    #[inline(always)]
    pub fn prefetch_data(&self) {
        prefetch_read(unsafe { self.current::<u8>() as *const u8 });
    }
}

/// Best-effort read prefetch into L1 (no-op on other architectures).
#[inline(always)]
pub fn prefetch_read(p: *const u8) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_mm_prefetch::<{ core::arch::x86_64::_MM_HINT_T0 }>(p as *const i8)
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("prfm pldl1keep, [{0}]", in(reg) p, options(nostack, preserves_flags, readonly))
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    let _ = p;
}
