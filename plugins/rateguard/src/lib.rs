//! rateguard: per-source-IPv4 token-bucket rate limiter — a VPP plugin
//! written in Rust.
//!
//! Data plane: an `ip4-unicast` feature node. Each source address gets a
//! token bucket (rate/burst configurable); packets that find the bucket
//! empty are dropped with a counted error.
//!
//! CLI:
//!   set rateguard rate <pps> [burst <packets>]
//!   rateguard interface <ifname> [disable]
//!   show rateguard

use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use vpp::buffer::Buffer;
use vpp::node::{frame_vector, tracing_enabled, NextFrames};
use vpp::sys;

vpp::plugin_register! {
    version: "0.1.0",
    description: c"Per-source-IPv4 token bucket rate limiter (Rust)",
}

// ---------------------------------------------------------------------------
// configuration & state
// ---------------------------------------------------------------------------

/// pps allowed per source (f64 bits).
static RATE: AtomicU64 = AtomicU64::new(0);
/// bucket depth in packets (f64 bits).
static BURST: AtomicU64 = AtomicU64::new(0);

const DEFAULT_RATE: f64 = 100.0;
const DEFAULT_BURST: f64 = 10.0;

fn config() -> (f64, f64) {
    let r = f64::from_bits(RATE.load(Ordering::Relaxed));
    let b = f64::from_bits(BURST.load(Ordering::Relaxed));
    (
        if r > 0.0 { r } else { DEFAULT_RATE },
        if b > 0.0 { b } else { DEFAULT_BURST },
    )
}

fn set_config(rate: f64, burst: f64) {
    RATE.store(rate.to_bits(), Ordering::Relaxed);
    BURST.store(burst.to_bits(), Ordering::Relaxed);
}

struct Bucket {
    tokens: f64,
    last: f64,
}

#[derive(Default)]
struct PerThread {
    flows: HashMap<u32, Bucket>,
    allowed: u64,
    dropped: u64,
}

const MAX_THREADS: usize = 256;

/// Per-worker state. Each slot is written only by its owning thread
/// (VPP worker barrier model); `show rateguard` reads other threads'
/// counters racily, which is fine for a debug CLI.
struct Threads([UnsafeCell<Option<Box<PerThread>>>; MAX_THREADS]);
unsafe impl Sync for Threads {}

static THREADS: Threads = Threads([const { UnsafeCell::new(None) }; MAX_THREADS]);

fn per_thread() -> &'static mut PerThread {
    let ti = vpp::thread_index() as usize;
    assert!(ti < MAX_THREADS);
    unsafe {
        let slot = &mut *THREADS.0[ti].get();
        slot.get_or_insert_with(Box::default)
    }
}

/// Interfaces we enabled the feature on (control-plane bookkeeping only).
static ENABLED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// the graph node
// ---------------------------------------------------------------------------

const NEXT_DROP: u32 = 0;

#[repr(u32)]
enum Error {
    Dropped = 0,
    Allowed = 1,
}

#[repr(C)]
struct Trace {
    src: [u8; 4],
    tokens_after: f64,
    dropped: u8,
}

vpp::define_node! {
    static NODE: c"rateguard" => {
        function: rateguard_node_fn,
        format_trace: Some(format_trace),
        errors: [
            (c"dropped", c"rateguard: rate limited", Error),
            (c"allowed", c"rateguard: allowed", Info),
        ],
        next_nodes: [c"error-drop"],
    }
}

vpp::define_feature! {
    static FEATURE: arc c"ip4-unicast", node c"rateguard", runs_before [c"ip4-lookup"]
}

vpp::define_cli! {
    static CLI_SET: path c"set rateguard",
    help c"set rateguard rate <pps> [burst <packets>]", handler cli_set_fn
}
vpp::define_cli! {
    static CLI_IF: path c"rateguard interface",
    help c"rateguard interface <interface> [disable]", handler cli_interface_fn
}
vpp::define_cli! {
    static CLI_SHOW: path c"show rateguard", help c"show rateguard", handler cli_show_fn
}

unsafe extern "C" fn rateguard_node_fn(
    vm: *mut sys::vlib_main_t,
    node: *mut sys::vlib_node_runtime_t,
    frame: *mut sys::vlib_frame_t,
) -> sys::uword {
    let from = unsafe { frame_vector(frame) };
    let mut next_frames = unsafe { NextFrames::new(vm, node, NEXT_DROP) };
    let now = vpp::time_now(vm);
    let (rate, burst) = config();
    let tracing = tracing_enabled(node);
    let state = per_thread();
    let mut n_dropped: u64 = 0;

    for &bi in from {
        let mut b = unsafe { Buffer::from_index(vm, bi) };

        // default: continue along the feature arc
        let mut next: u32 = 0;
        unsafe { sys::vnet_feature_next(&mut next, b.raw()) };

        // src address lives at bytes 12..16 of the IPv4 header
        let src = unsafe { (b.current::<u8>() as *const u8).add(12).cast::<u32>().read_unaligned() };

        let bucket = state.flows.entry(src).or_insert(Bucket {
            tokens: burst,
            last: now,
        });
        bucket.tokens = (bucket.tokens + (now - bucket.last) * rate).min(burst);
        bucket.last = now;

        let dropped = bucket.tokens < 1.0;
        if dropped {
            next = NEXT_DROP;
            unsafe { b.set_error(*(*node).errors.add(Error::Dropped as usize)) };
            state.dropped += 1;
            n_dropped += 1;
        } else {
            bucket.tokens -= 1.0;
            state.allowed += 1;
        }

        if tracing && b.is_traced() {
            let t = unsafe { &mut *vpp::trace::add::<Trace>(vm, node, &b) };
            t.src = src.to_ne_bytes();
            t.tokens_after = bucket.tokens;
            t.dropped = dropped as u8;
        }

        next_frames.enqueue(bi, next);
    }
    next_frames.finish();

    // Dropped packets are counted through b->error when error-drop
    // processes them; count only the allowed ones explicitly.
    let n = unsafe { (*frame).n_vectors as u64 };
    unsafe {
        let node_index = (*node).node_index;
        sys::vlib_node_increment_counter(vm, node_index, Error::Allowed as u32, n - n_dropped);
    }
    n as sys::uword
}

unsafe extern "C" fn format_trace(
    s: *mut sys::u8_,
    args: *mut sys::va_list,
) -> *mut sys::u8_ {
    unsafe {
        let t = &*vpp::trace::format_args::<Trace>(args);
        sys::format(
            s,
            c"rateguard: src %d.%d.%d.%d %s (%.2f tokens left)".as_ptr(),
            t.src[0] as u32,
            t.src[1] as u32,
            t.src[2] as u32,
            t.src[3] as u32,
            if t.dropped != 0 {
                c"DROP".as_ptr()
            } else {
                c"PASS".as_ptr()
            },
            t.tokens_after,
        )
    }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

unsafe extern "C" fn cli_set_fn(
    _vm: *mut sys::vlib_main_t,
    input: *mut sys::unformat_input_t,
    _cmd: *mut sys::vlib_cli_command_t,
) -> *mut sys::clib_error_t {
    let (mut rate, mut burst) = config();
    while vpp::cli::more_input(input) {
        let matched = unsafe {
            sys::unformat(input, c"rate %f".as_ptr(), &mut rate as *mut f64) != 0
                || sys::unformat(input, c"burst %f".as_ptr(), &mut burst as *mut f64) != 0
        };
        if !matched {
            return vpp::cli::error(c"expected: rate <pps> [burst <packets>]");
        }
    }
    if rate <= 0.0 || burst < 1.0 {
        return vpp::cli::error(c"rate must be > 0, burst >= 1");
    }
    set_config(rate, burst);
    core::ptr::null_mut()
}

unsafe extern "C" fn cli_interface_fn(
    _vm: *mut sys::vlib_main_t,
    input: *mut sys::unformat_input_t,
    _cmd: *mut sys::vlib_cli_command_t,
) -> *mut sys::clib_error_t {
    let vnm = unsafe { sys::vnet_get_main() };
    let mut sw_if_index: u32 = !0;
    let mut enable = true;
    while vpp::cli::more_input(input) {
        let matched = unsafe {
            sys::unformat(
                input,
                c"%U".as_ptr(),
                sys::unformat_vnet_sw_interface
                    as unsafe extern "C" fn(*mut sys::unformat_input_t, *mut sys::va_list) -> sys::uword,
                vnm,
                &mut sw_if_index as *mut u32,
            ) != 0
                || {
                    let d = sys::unformat(input, c"disable".as_ptr()) != 0;
                    if d {
                        enable = false;
                    }
                    d
                }
        };
        if !matched {
            return vpp::cli::error(c"expected: <interface> [disable]");
        }
    }
    if sw_if_index == !0 {
        return vpp::cli::error(c"please specify an interface");
    }
    match vpp::feature::enable_disable(c"ip4-unicast", c"rateguard", sw_if_index, enable) {
        Ok(()) => {
            let mut en = ENABLED.lock().unwrap();
            en.retain(|&i| i != sw_if_index);
            if enable {
                en.push(sw_if_index);
            }
            core::ptr::null_mut()
        }
        Err(_) => vpp::cli::error(c"feature enable/disable failed"),
    }
}

unsafe extern "C" fn cli_show_fn(
    vm: *mut sys::vlib_main_t,
    _input: *mut sys::unformat_input_t,
    _cmd: *mut sys::vlib_cli_command_t,
) -> *mut sys::clib_error_t {
    let (rate, burst) = config();
    vpp::cli::output(vm, &format!("rateguard: rate {} pps, burst {}", rate, burst));
    let enabled = ENABLED.lock().unwrap().clone();
    vpp::cli::output(vm, &format!("enabled on {} interface(s): {:?}", enabled.len(), enabled));
    let mut total_flows = 0usize;
    let (mut allowed, mut dropped) = (0u64, 0u64);
    for slot in THREADS.0.iter() {
        // racy cross-thread read; debug CLI only
        if let Some(pt) = unsafe { (*slot.get()).as_ref() } {
            total_flows += pt.flows.len();
            allowed += pt.allowed;
            dropped += pt.dropped;
        }
    }
    vpp::cli::output(
        vm,
        &format!(
            "{} active flow(s), {} allowed, {} dropped",
            total_flows, allowed, dropped
        ),
    );
    core::ptr::null_mut()
}
