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

use std::path::{Path, PathBuf};
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
    // CI convenience: VPP_PYTHON_TEST_FILTER overrides the declared
    // filter. An empty declared filter runs ONLY when the env override
    // is present (escape hatch for ad-hoc CI dispatch runs).
    let env_filter = std::env::var("VPP_PYTHON_TEST_FILTER").ok();
    let filter = env_filter.as_deref().unwrap_or(filter);
    if filter.is_empty() {
        eprintln!("no python test filter given (set VPP_PYTHON_TEST_FILTER) — skipping");
        return;
    }
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get());
    run_framework(filter, jobs, &[]);
}

/// Run Python tests matching `filter` against a VPP instance that RUST
/// owns — the framework's `--use-running-vpp` mode. The framework does
/// no VPP process management at all: it attaches to the given sockets,
/// and only the test logic itself is Python. One instance per call, so
/// keep filters module-sized; parallelism happens at the cargo level
/// (each attached bridge test gets its own VPP).
pub fn run_attached(filter: &str) {
    if !enabled() {
        eprintln!("skipping python tests '{filter}' (set VPP_PYTHON_TESTS=1 to run)");
        return;
    }
    let src = vpp_src().canonicalize().expect("VPP source tree not found");
    ensure_venv(&src);

    let vpp = crate::Vpp::start(&[]);
    // Concurrent invocations otherwise collide on fixed /tmp paths
    // (e.g. every run has a SanityTestCase -> /tmp/vpp-unittest-SanityTestCase),
    // so each gets a private tmp dir inside the instance workdir.
    let tmp = vpp.socket_dir().join("test-tmp");
    std::fs::create_dir_all(&tmp).unwrap();

    let prefix = crate::vpp_prefix();
    let libdir = crate::vpp_libdir(&prefix);
    let venv = src.join("build-root/test/venv");
    let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("runner.py");

    // Rust is the scheduler: hand each attached run its own CPU,
    // round-robin. The framework pins the test process to it
    // (asfframework setUpClass), so parallel runs must not all get
    // CPU 0.
    static NEXT_CPU: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let ncpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let cpu = NEXT_CPU.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % ncpus;

    // Drive the framework directly with our minimal runner — no
    // run_tests.py, no test discovery: Rust owns process management
    // and scheduling; Python runs exactly one unittest name.
    let status = Command::new(venv.join("bin/python3"))
        .current_dir(src.join("test"))
        .arg(&runner)
        .arg("--use-running-vpp")
        .arg(format!("--socket-dir={}", vpp.socket_dir().display()))
        .arg(format!("--tmp-dir={}", tmp.display()))
        .arg(format!("--failed-dir={}", tmp.display()))
        .arg(format!("--venv-dir={}", venv.display()))
        .arg(format!("--vpp-ws-dir={}", src.display()))
        .arg("--vpp-tag=vpp") // required arg; derived paths all overridden
        .arg(format!("--extern-apidir={}/share/vpp/api", prefix.display()))
        .arg(format!("--vpp={}/bin/vpp", prefix.display()))
        .arg(format!("--vpp-plugin-dir={}/vpp_plugins", libdir.display()))
        .arg(format!(
            "--vpp-test-plugin-dir={}/vpp_api_test_plugins",
            libdir.display()
        ))
        .arg(format!("--cpus={cpu}"))
        .arg("--")
        .arg(filter)
        .status()
        .expect("failed to run test venv python");
    assert!(status.success(), "python tests '{filter}' failed");
}

/// Build the test venv once per process if it is missing (the make
/// stamps make re-runs cheap, but avoid concurrent builders).
fn ensure_venv(src: &Path) {
    use std::sync::Once;
    static VENV: Once = Once::new();
    VENV.call_once(|| {
        if src.join("build-root/test/venv/bin/python3").exists() {
            return;
        }
        let status = Command::new("make")
            .current_dir(src.join("test"))
            .arg("test-dep")
            .arg(format!("WS_ROOT={}", src.display()))
            .arg(format!("BR={}/build-root", src.display()))
            .arg(format!("TEST_DIR={}/test", src.display()))
            .status()
            .expect("failed to run make test-dep");
        assert!(status.success(), "building the test venv failed");
    });
}

fn run_framework(filter: &str, jobs: usize, extra_args: &[String]) {
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

    let mut plugin_path_args = format!(
        "--vpp={p}/bin/vpp --vpp-install-dir={i} --vpp-plugin-dir={l}/vpp_plugins",
        p = prefix.display(),
        i = install_dir.display(),
        l = libdir.display(),
    );
    for a in extra_args {
        plugin_path_args.push(' ');
        plugin_path_args.push_str(a);
    }

    let status = Command::new("make")
        .current_dir(src.join("test"))
        .arg("test")
        .arg(format!("WS_ROOT={}", src.display()))
        .arg(format!("BR={}/build-root", src.display()))
        .arg(format!("TEST_DIR={}/test", src.display()))
        .arg(format!("TEST={filter}"))
        .arg(format!("TEST_JOBS={jobs}"))
        .arg(format!("EXTERN_APIDIR={}/share/vpp/api", prefix.display()))
        .arg(format!("PLUGIN_PATH_ARGS={plugin_path_args}"))
        .arg(format!(
            "TEST_PLUGIN_PATH_ARGS=--vpp-test-plugin-dir={}/vpp_api_test_plugins",
            libdir.display()
        ))
        .status()
        .expect("failed to run make -C test");
    assert!(status.success(), "python tests '{filter}' failed");
}

/// Declare a cargo test that runs Python tests matching a filter
/// (framework spawns and manages its own VPP instances).
#[macro_export]
macro_rules! python_test {
    ($name:ident, $filter:literal) => {
        #[test]
        fn $name() {
            $crate::python::make_test($filter);
        }
    };
}

/// Declare a cargo test that runs Python tests against a Rust-owned
/// VPP instance (framework attaches, spawns nothing). Keep the filter
/// module-sized — everything it matches shares one VPP.
#[macro_export]
macro_rules! python_test_attached {
    ($name:ident, $filter:literal) => {
        #[test]
        fn $name() {
            $crate::python::run_attached($filter);
        }
    };
}
