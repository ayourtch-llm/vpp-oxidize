//! Bridge to VPP's existing Python test framework, so migration can be
//! incremental: Rust and Python tests run under one runner (cargo
//! test / nextest), and Python suites get retired one by one as their
//! Rust ports land.
//!
//! The Python framework runs against the SAME VPP install the Rust
//! tests use (see [`crate::vpp_prefix`]) — nothing is rebuilt. The
//! framework is driven through its supported external-VPP knobs,
//! validated against both build trees and /usr package installs:
//! - `WS_ROOT`/`BR`/`TEST_DIR` make vars (top-level Makefile exports
//!   that a direct `make -C test` does not get)
//! - `--vpp`/`--vpp-install-dir`/`--vpp-plugin-dir` injected via the
//!   PLUGIN_PATH_ARGS / TEST_PLUGIN_PATH_ARGS hooks
//! - `EXTERN_APIDIR` for the .api.json files
//! - PYTHON is deliberately NOT set: an absolute interpreter path
//!   bypasses the activated test venv and breaks the vpp_papi import.
//!
//! Python tests are heavyweight (first run builds a venv), so bridge
//! tests only execute when `VPP_PYTHON_TESTS=1` is set; otherwise they
//! log a skip note. Point `VPP_SRC` at the VPP source tree if it is
//! not the default sibling checkout.

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

/// Run Python tests matching `filter` (run_tests --filter syntax, e.g.
/// "test_punt" or "acl*") against the installed VPP — without building
/// VPP. Panics on test failure; no-op (with a note) when not enabled.
pub fn make_test(filter: &str) {
    if !enabled() {
        eprintln!("skipping python tests '{filter}' (set VPP_PYTHON_TESTS=1 to run)");
        return;
    }
    // CI convenience: override every bridge filter from the environment
    // (meaningful while there is a single bridge test; revisit when the
    // in-repo migration list grows).
    let env_filter = std::env::var("VPP_PYTHON_TEST_FILTER").ok();
    let filter = env_filter.as_deref().unwrap_or(filter);
    let src = vpp_src().canonicalize().expect("VPP source tree not found");
    let prefix = crate::vpp_prefix();
    let libdir = crate::vpp_libdir(&prefix);
    // config.py derives paths as {install_dir}/vpp/...; for a build
    // tree the parent of ".../vpp" fits that shape, for /usr it does
    // not matter since every derived path is overridden explicitly.
    let install_dir = if prefix.ends_with("vpp") {
        prefix.parent().unwrap().to_path_buf()
    } else {
        prefix.clone()
    };
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get());

    let status = Command::new("make")
        .current_dir(src.join("test"))
        .arg("test")
        .arg(format!("WS_ROOT={}", src.display()))
        .arg(format!("BR={}/build-root", src.display()))
        .arg(format!("TEST_DIR={}/test", src.display()))
        .arg(format!("TEST={filter}"))
        .arg(format!("TEST_JOBS={jobs}"))
        .arg(format!("EXTERN_APIDIR={}/share/vpp/api", prefix.display()))
        .arg(format!(
            "PLUGIN_PATH_ARGS=--vpp={p}/bin/vpp --vpp-install-dir={i} --vpp-plugin-dir={l}/vpp_plugins",
            p = prefix.display(),
            i = install_dir.display(),
            l = libdir.display(),
        ))
        .arg(format!(
            "TEST_PLUGIN_PATH_ARGS=--vpp-test-plugin-dir={}/vpp_api_test_plugins",
            libdir.display()
        ))
        .status()
        .expect("failed to run make -C test");
    assert!(status.success(), "python tests '{filter}' failed");
}

/// Declare a cargo test that runs Python tests matching a filter.
#[macro_export]
macro_rules! python_test {
    ($name:ident, $filter:literal) => {
        #[test]
        fn $name() {
            $crate::python::make_test($filter);
        }
    };
}
