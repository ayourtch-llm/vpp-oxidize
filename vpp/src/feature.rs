//! Feature arc registration (VNET_FEATURE_INIT equivalent) and
//! enable/disable.

use crate::sys;
use core::cell::UnsafeCell;
use core::ffi::{c_char, CStr};

pub struct FeatureCell(UnsafeCell<sys::vnet_feature_registration_t>);
unsafe impl Sync for FeatureCell {}

impl FeatureCell {
    pub const fn new() -> Self {
        FeatureCell(UnsafeCell::new(unsafe {
            core::mem::MaybeUninit::zeroed().assume_init()
        }))
    }
}

/// Register `node` on feature arc `arc`, running before the given nodes.
/// Call from a `vpp::ctor!` constructor.
///
/// # Safety
/// Must run at dlopen time on the single loading thread.
pub unsafe fn register_feature(
    cell: &'static FeatureCell,
    arc: &'static CStr,
    node: &'static CStr,
    runs_before: &[&'static CStr],
) {
    unsafe {
        let r = &mut *cell.0.get();
        r.arc_name = arc.as_ptr() as *mut c_char;
        r.node_name = node.as_ptr() as *mut c_char;
        // NULL-terminated array, same shape as the C initializer
        let mut rb: Vec<*mut c_char> = runs_before
            .iter()
            .map(|s| s.as_ptr() as *mut c_char)
            .collect();
        rb.push(core::ptr::null_mut());
        r.runs_before = Box::leak(rb.into_boxed_slice()).as_mut_ptr();
        let fm = &raw mut sys::feature_main;
        r.next = (*fm).next_feature;
        (*fm).next_feature = r;
    }
}

/// Enable or disable a feature node on an interface.
pub fn enable_disable(
    arc: &CStr,
    node: &CStr,
    sw_if_index: u32,
    enable: bool,
) -> Result<(), i32> {
    let rv = unsafe {
        sys::vnet_feature_enable_disable(
            arc.as_ptr(),
            node.as_ptr(),
            sw_if_index,
            enable as i32,
            core::ptr::null_mut(),
            0,
        )
    };
    if rv == 0 {
        Ok(())
    } else {
        Err(rv)
    }
}
