//! Drive a real VPP process from Rust tests.
//!
//! Each `Vpp::start()` gets an isolated runtime directory (own CLI and
//! API sockets), so tests can run in parallel under `cargo test` /
//! `cargo nextest`. The instance is killed and cleaned up on drop.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

static INSTANCE_SEQ: AtomicU32 = AtomicU32::new(0);

/// Locate the VPP install tree: $VPP_PREFIX, or well-known sibling paths.
pub fn vpp_prefix() -> PathBuf {
    if let Ok(p) = std::env::var("VPP_PREFIX") {
        return PathBuf::from(p);
    }
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vpp/build-root");
    for cand in [
        "install-vpp_debug-native/vpp",
        "install-vpp-native/vpp",
        "install-oxidize/vpp",
    ] {
        let p = base.join(cand);
        if p.join("bin/vpp").exists() {
            return p.canonicalize().unwrap();
        }
    }
    panic!("no VPP install tree found; set VPP_PREFIX");
}

/// Directory that holds staged Rust plugins (<name>_plugin.so).
/// Copies target/debug/lib<name>_plugin.so into the instance dir.
fn stage_plugin(workdir: &Path, name: &str) -> PathBuf {
    let plugdir = workdir.join("plugins");
    std::fs::create_dir_all(&plugdir).unwrap();
    let built = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../target/debug/lib{}_plugin.so", name));
    let dst = plugdir.join(format!("{}_plugin.so", name));
    std::fs::copy(&built, &dst).unwrap_or_else(|e| {
        panic!(
            "cannot stage plugin {} ({}): build it first (cargo build)",
            built.display(),
            e
        )
    });
    plugdir
}

pub struct Vpp {
    child: Child,
    workdir: PathBuf,
    cli_sock: PathBuf,
    prefix: PathBuf,
}

impl Vpp {
    /// Start VPP with the given Rust plugins (by crate plugin name, e.g.
    /// "rateguard") staged into the plugin path.
    pub fn start(plugins: &[&str]) -> Vpp {
        let prefix = vpp_prefix();
        let seq = INSTANCE_SEQ.fetch_add(1, Ordering::Relaxed);
        // short base path: unix socket paths are limited to ~108 chars
        let workdir = PathBuf::from(format!("/tmp/vpptest-{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&workdir).unwrap();

        let mut plugin_path = format!(
            "{}/lib/x86_64-linux-gnu/vpp_plugins",
            prefix.display()
        );
        for p in plugins {
            let dir = stage_plugin(&workdir, p);
            plugin_path = format!("{}:{}", plugin_path, dir.display());
        }

        let cli_sock = workdir.join("cli.sock");
        let conf = format!(
            "unix {{ nodaemon cli-listen {cli} runtime-dir {run} }}\n\
             socksvr {{ socket-name {api} }}\n\
             api-segment {{ prefix vpptest-{pid}-{seq} }}\n\
             plugins {{ path {pp} plugin dpdk_plugin.so {{ disable }} }}\n",
            cli = cli_sock.display(),
            run = workdir.display(),
            api = workdir.join("api.sock").display(),
            pid = std::process::id(),
            pp = plugin_path,
        );
        let conf_path = workdir.join("startup.conf");
        std::fs::write(&conf_path, conf).unwrap();

        let child = Command::new(prefix.join("bin/vpp"))
            .arg("-c")
            .arg(&conf_path)
            .stdout(std::fs::File::create(workdir.join("vpp.log")).unwrap())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to spawn vpp");

        let vpp = Vpp {
            child,
            workdir,
            cli_sock,
            prefix,
        };
        vpp.wait_ready();
        vpp
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if self.try_ctl("show version").is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!(
            "vpp did not come up; log:\n{}",
            std::fs::read_to_string(self.workdir.join("vpp.log")).unwrap_or_default()
        );
    }

    fn try_ctl(&self, cmd: &str) -> Option<String> {
        let out = Command::new(self.prefix.join("bin/vppctl"))
            .arg("-s")
            .arg(&self.cli_sock)
            .args(cmd.split_whitespace())
            .output()
            .ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            None
        }
    }

    /// Run a CLI command and return its output. Panics on transport failure.
    pub fn ctl(&self, cmd: &str) -> String {
        self.try_ctl(cmd)
            .unwrap_or_else(|| panic!("vppctl failed for: {}", cmd))
    }

    /// MAC address of an interface, in pg "aabb.ccdd.eeff" format.
    pub fn mac_of(&self, ifname: &str) -> String {
        let hw = self.ctl(&format!("show hardware {}", ifname));
        let mac = hw
            .lines()
            .find_map(|l| l.trim().strip_prefix("Ethernet address "))
            .unwrap_or_else(|| panic!("no MAC in: {}", hw))
            .trim()
            .replace(':', "");
        format!("{}.{}.{}", &mac[0..4], &mac[4..8], &mac[8..12])
    }
}

impl Drop for Vpp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if std::env::var_os("VPP_TEST_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&self.workdir);
        } else {
            eprintln!("keeping {}", self.workdir.display());
        }
    }
}
