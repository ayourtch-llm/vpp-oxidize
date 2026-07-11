//! Legacy Python tests pulled into the Rust runner, so the suite can
//! migrate incrementally: keep a filter in the mix until its Rust port
//! lands, then delete the bridge line.
//!
//! These run VPP's python framework against the SAME installed VPP the
//! Rust tests use (no VPP build). They only execute with
//! VPP_PYTHON_TESTS=1 (heavyweight); otherwise they log a skip and pass.

vpp_test::python_test!(python_acl_tests, "acl*");
