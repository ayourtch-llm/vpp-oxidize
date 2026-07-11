//! Example of pulling existing Python tests into the Rust runner, so
//! the suite can migrate incrementally: keep the Python module in the
//! mix until its Rust port lands, then delete the bridge line.
//!
//! These only execute with VPP_PYTHON_TESTS=1 (they are heavyweight);
//! otherwise they log a skip note and pass.

vpp_test::python_test!(python_test_punt, "test_punt");
