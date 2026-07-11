//! rateguard: per-source-IPv4 token-bucket rate limiter (VPP plugin in Rust).

vpp::plugin_register! {
    version: "0.1.0",
    description: c"Per-source-IPv4 token bucket rate limiter (Rust)",
}
