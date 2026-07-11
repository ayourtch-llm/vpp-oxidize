//! End-to-end test: a real (unmodified) VPP loads the Rust rateguard
//! plugin and rate-limits a single-source burst to the configured
//! bucket size. Rust port of scripts/e2e-test.sh.

use vpp_test::Vpp;

#[test]
fn rateguard_limits_single_source_burst() {
    let vpp = Vpp::start(&["rateguard"]);

    assert!(
        vpp.ctl("show plugins").contains("rateguard_plugin.so"),
        "plugin not loaded"
    );

    vpp.ctl("create packet-generator interface pg0");
    vpp.ctl("set interface ip address pg0 10.0.0.1/24");
    vpp.ctl("set interface state pg0 up");
    vpp.ctl("set rateguard rate 100 burst 10");
    vpp.ctl("rateguard interface pg0");

    let mac = vpp.mac_of("pg0");
    vpp.ctl(&format!(
        "packet-generator new {{ name rg limit 100 rate 1e6 node ethernet-input \
         size 100-100 interface pg0 \
         data {{ IP4: 000a.0a0a.0a0a -> {mac} \
                 UDP: 10.0.0.2 -> 10.0.0.1 \
                 UDP: 1234 -> 2345 incrementing 8 }} }}"
    ));
    vpp.ctl("trace add pg-input 5");
    vpp.ctl("packet-generator enable");

    // wait for the stream to finish
    let mut done = false;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !vpp.ctl("show packet-generator").contains("Yes") {
            done = true;
            break;
        }
    }
    assert!(done, "packet-generator stream did not finish");

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
