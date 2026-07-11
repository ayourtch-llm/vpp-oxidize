//! End-to-end tests: a real (unmodified) VPP loads the Rust rateguard
//! plugin and rate-limits traffic. Packets are built with oside.

use oside::protocols::all::*;
use oside::*;
use vpp_test::pg::Stream;
use vpp_test::Vpp;

fn setup() -> Vpp {
    let vpp = Vpp::start(&["rateguard"]);
    vpp.ctl("create packet-generator interface pg0");
    vpp.ctl("set interface ip address pg0 10.0.0.1/24");
    vpp.ctl("set interface state pg0 up");
    vpp
}

#[test]
fn rateguard_limits_single_source_burst() {
    let vpp = setup();
    assert!(
        vpp.ctl("show plugins").contains("rateguard_plugin.so"),
        "plugin not loaded"
    );
    vpp.ctl("set rateguard rate 100 burst 10");
    vpp.ctl("rateguard interface pg0");

    let pkt = Ether!(src = "00:0a:0a:0a:0a:0a", dst = vpp.mac_of("pg0").as_str())
        / IP!(src = "10.0.0.2", dst = "10.0.0.1")
        / UDP!(sport = 1234, dport = 2345);
    vpp.pg_stream(Stream::new("rg", "pg0", pkt).count(100).rate(1e6));

    vpp.ctl("trace add pg-input 5");
    vpp.pg_run();

    let show = vpp.ctl("show rateguard");
    assert!(
        show.contains("10 allowed, 90 dropped"),
        "unexpected rateguard stats:\n{show}"
    );

    let errors = vpp.ctl("show errors");
    let drops: u64 = errors
        .lines()
        .find(|l| l.contains("rate limited"))
        .and_then(|l| l.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert_eq!(drops, 90, "error counter mismatch:\n{errors}");

    let trace = vpp.ctl("show trace max 1");
    assert!(
        trace.contains("rateguard: src 10.0.0.2"),
        "no rateguard trace record:\n{trace}"
    );
}

#[test]
fn rateguard_independent_sources() {
    let vpp = setup();
    vpp.ctl("set rateguard rate 100 burst 10");
    vpp.ctl("rateguard interface pg0");

    // two sources, one burst each: each gets its own bucket
    for (i, src) in ["10.0.0.2", "10.0.0.3"].iter().enumerate() {
        let pkt = Ether!(src = "00:0a:0a:0a:0a:0a", dst = vpp.mac_of("pg0").as_str())
            / IP!(src = *src, dst = "10.0.0.1")
            / UDP!(sport = 1234, dport = 2345);
        vpp.pg_stream(Stream::new(&format!("s{i}"), "pg0", pkt).count(50).rate(1e6));
    }
    vpp.pg_run();

    let show = vpp.ctl("show rateguard");
    assert!(
        show.contains("2 active flow(s), 20 allowed, 80 dropped"),
        "unexpected stats:\n{show}"
    );
}

#[test]
fn rateguard_cli_validation() {
    let vpp = Vpp::start(&["rateguard"]);
    let out = vpp.ctl("set rateguard rate 0");
    assert!(
        out.contains("rate must be"),
        "expected validation error, got: {out}"
    );
    let out = vpp.ctl("rateguard interface");
    assert!(
        out.contains("specify an interface"),
        "expected interface error, got: {out}"
    );
}

/// Perf smoke test: pushes a large burst and reports VPP's own
/// clocks/packet measurement for the rateguard node. Run explicitly
/// (ideally against a release build) with:
///   cargo test -p rateguard --test e2e -- --ignored --nocapture
#[test]
#[ignore = "perf measurement; run explicitly with --ignored"]
fn rateguard_perf_smoke() {
    let vpp = setup();
    vpp.ctl("set rateguard rate 1000000000 burst 100000000");
    vpp.ctl("rateguard interface pg0");

    let pkt = Ether!(src = "00:0a:0a:0a:0a:0a", dst = vpp.mac_of("pg0").as_str())
        / IP!(src = "10.0.0.2", dst = "10.0.0.1")
        / UDP!(sport = 1234, dport = 2345);
    vpp.pg_stream(Stream::new("perf", "pg0", pkt).count(1_000_000).rate(1e9));
    vpp.pg_run();

    let rt = vpp
        .node_runtime("rateguard")
        .expect("rateguard node did not run");
    eprintln!(
        "rateguard: {} pkts in {} calls, {:.1} clocks/pkt, {:.1} vectors/call",
        rt.vectors, rt.calls, rt.clocks_per_vector, rt.vectors_per_call
    );
    assert_eq!(rt.vectors, 1_000_000);
}
