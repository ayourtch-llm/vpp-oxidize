//! Legacy Python tests pulled into the Rust runner, so the suite can
//! migrate incrementally: keep an entry here until its Rust port lands,
//! then delete the line.
//!
//! `python_test_attached!` runs the filtered tests against a VPP that
//! RUST spawns and owns (the framework's --use-running-vpp mode): the
//! framework does no process management, only the test logic is Python,
//! and cargo provides the parallelism (one VPP per entry).
//!
//! Granularity rule: one entry per TEST CLASS (file.Class filter). In
//! attached mode the framework never restarts VPP, so interface/config
//! state persists — two classes sharing one VPP collide (learned the
//! hard way: create_vlan_subif returns -56 for the second class).
//! Single-class modules can use the bare module name.
//!
//! These only execute with VPP_PYTHON_TESTS=1 (heavyweight); otherwise
//! they log a skip and pass.

use vpp_test::python_test_attached;

python_test_attached!(python_acl_plugin, "test_acl_plugin.TestACLplugin");
python_test_attached!(python_acl_conns, "test_acl_plugin_conns.ACLPluginConnTestCase");
python_test_attached!(python_acl_l2l3, "test_acl_plugin_l2l3.TestACLpluginL2L3");
python_test_attached!(python_acl_macip_ip4, "test_acl_plugin_macip.TestMACIP_IP4");
python_test_attached!(python_acl_macip_ip6, "test_acl_plugin_macip.TestMACIP_IP6");
python_test_attached!(python_acl_macip, "test_acl_plugin_macip.TestMACIP");
python_test_attached!(python_acl_dot1q_bridged, "test_acl_plugin_macip.TestACL_dot1q_bridged");
python_test_attached!(python_acl_dot1ad_bridged, "test_acl_plugin_macip.TestACL_dot1ad_bridged");
python_test_attached!(python_acl_dot1q_routed, "test_acl_plugin_macip.TestACL_dot1q_routed");
python_test_attached!(python_acl_dot1ad_routed, "test_acl_plugin_macip.TestACL_dot1ad_routed");

// Escape hatch: run an arbitrary filter (classic mode, framework spawns
// its own VPPs) via VPP_PYTHON_TEST_FILTER — used by CI manual dispatch.
vpp_test::python_test!(python_custom_filter, "");
