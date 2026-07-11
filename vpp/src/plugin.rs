//! Plugin registration: the exported `vlib_plugin_registration` symbol in
//! the `.vlib_plugin_registration` ELF section that the VPP loader looks for.

use core::ffi::c_char;

/// Mirror of `vlib_plugin_registration_t` that can be constructed in a
/// `const` context (the bindgen type has bitfields, which cannot).
/// Layout is asserted against the bindgen type in tests.
#[repr(C, align(64))]
pub struct PluginRegistration {
    /// bit 0: default_disabled, bit 1: deep_bind
    pub flags: u8,
    pub version: [u8; 64],
    pub version_required: [u8; 64],
    pub overrides: [u8; 256],
    pub early_init: *const c_char,
    pub description: *const c_char,
    pub load_after: *const c_char,
}

unsafe impl Sync for PluginRegistration {}

const fn copy_str<const N: usize>(s: &str) -> [u8; N] {
    let bytes = s.as_bytes();
    assert!(bytes.len() < N, "string too long for fixed field");
    let mut out = [0u8; N];
    let mut i = 0;
    while i < bytes.len() {
        out[i] = bytes[i];
        i += 1;
    }
    out
}

impl PluginRegistration {
    pub const fn new(version: &str, description: &'static core::ffi::CStr) -> Self {
        PluginRegistration {
            flags: 0,
            version: copy_str(version),
            version_required: [0; 64],
            overrides: [0; 256],
            early_init: core::ptr::null(),
            description: description.as_ptr(),
            load_after: core::ptr::null(),
        }
    }
}

/// Emit the plugin registration symbol VPP's loader expects.
///
/// ```ignore
/// vpp::plugin_register! {
///     version: "0.1.0",
///     description: c"my plugin does things",
/// }
/// ```
#[macro_export]
macro_rules! plugin_register {
    (version: $v:expr, description: $d:expr $(,)?) => {
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".vlib_plugin_registration")]
        #[used]
        pub static vlib_plugin_registration: $crate::plugin::PluginRegistration =
            $crate::plugin::PluginRegistration::new($v, $d);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_bindgen() {
        assert_eq!(
            core::mem::size_of::<PluginRegistration>(),
            core::mem::size_of::<vpp_sys::vlib_plugin_registration_t>()
        );
        assert_eq!(
            core::mem::align_of::<PluginRegistration>(),
            core::mem::align_of::<vpp_sys::vlib_plugin_registration_t>()
        );
        assert_eq!(core::mem::offset_of!(PluginRegistration, version), 1);
        assert_eq!(core::mem::offset_of!(PluginRegistration, version_required), 65);
        assert_eq!(core::mem::offset_of!(PluginRegistration, overrides), 129);
    }
}
