//! Bridge to VPP's existing Python test framework, so migration can be
//! incremental: Rust and Python tests run under one runner (cargo
//! test / nextest), and Python suites get retired one by one as their
//! Rust ports land.
//!
//! Python tests are heavyweight (they build a venv on first run), so
//! bridge tests only run when `VPP_PYTHON_TESTS=1` is set; otherwise
//! they're skipped with a note. Point `VPP_SRC` at the VPP source tree
//! if it is not the default sibling checkout.

use std::path::PathBuf;
use std::process::Command;

fn vpp_src() -> PathBuf {
    if let Ok(p) = std::env::var("VPP_SRC") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vpp")
}

/// True when python bridge tests should actually execute.
pub fn enabled() -> bool {
    std::env::var("VPP_PYTHON_TESTS").as_deref() == Ok("1")
}

/// Run one Python test module (e.g. "test_arp") through `make test`.
/// Panics on test failure; no-op (with a note) when not enabled.
pub fn make_test(module: &str) {
    if !enabled() {
        eprintln!("skipping python test {module} (set VPP_PYTHON_TESTS=1 to run)");
        return;
    }
    let src = vpp_src();
    let status = Command::new("make")
        .current_dir(&src)
        .arg("test")
        .arg(format!("TEST={module}"))
        // the stray uv python otherwise hijacks cmake's interpreter pick
        .env("vpp_cmake_args", "-DPython3_EXECUTABLE=/usr/bin/python3")
        .status()
        .expect("failed to run make test");
    assert!(status.success(), "python test {module} failed");
}

/// Declare a cargo test that runs a Python test module.
#[macro_export]
macro_rules! python_test {
    ($name:ident, $module:literal) => {
        #[test]
        fn $name() {
            $crate::python::make_test($module);
        }
    };
}
