//! Structured access to `show runtime` — the foundation for perf tests.
//!
//! VPP already measures clocks/packet per node with rdtsc precision;
//! we just parse it. Run perf tests against a release VPP + release
//! plugin build for meaningful numbers.

use crate::Vpp;

#[derive(Debug, Clone, PartialEq)]
pub struct NodeRuntime {
    pub name: String,
    pub calls: u64,
    pub vectors: u64,
    pub suspends: u64,
    pub clocks_per_vector: f64,
    pub vectors_per_call: f64,
}

impl Vpp {
    /// Parse `show runtime` into per-node stats.
    pub fn runtime_stats(&self) -> Vec<NodeRuntime> {
        let out = self.ctl("show runtime");
        let mut stats = Vec::new();
        for line in out.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            // Name State Calls Vectors Suspends Clocks Vectors/Call
            if f.len() >= 7 {
                let numeric_tail: Option<(u64, u64, u64, f64, f64)> = (|| {
                    let n = f.len();
                    Some((
                        f[n - 5].parse().ok()?,
                        f[n - 4].parse().ok()?,
                        f[n - 3].parse().ok()?,
                        f[n - 2].parse().ok()?,
                        f[n - 1].parse().ok()?,
                    ))
                })();
                if let Some((calls, vectors, suspends, clocks, vpc)) = numeric_tail {
                    // name may contain no spaces; state occupies the rest
                    stats.push(NodeRuntime {
                        name: f[0].to_string(),
                        calls,
                        vectors,
                        suspends,
                        clocks_per_vector: clocks,
                        vectors_per_call: vpc,
                    });
                }
            }
        }
        stats
    }

    /// Runtime stats for one node, if it ran.
    pub fn node_runtime(&self, node: &str) -> Option<NodeRuntime> {
        self.runtime_stats().into_iter().find(|n| n.name == node)
    }
}
