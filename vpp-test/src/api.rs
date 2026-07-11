//! Typed binary-API client for tests, built on ayourtch/vpp-api
//! (transport + message traits) and ayourtch/latest-vpp-api (generated
//! message types).
//!
//! The message-id/CRC negotiation happens at `connect()` — ids are
//! resolved by name+CRC against the running VPP, so compatibility is
//! per-message, not per-release. `Api` factors out the repetitive
//! client_index / context bookkeeping so a test reads as one call per
//! operation (this wrapper is the design sketch for a future reworked
//! vpp-api surface).

use crate::Vpp;
use latest_vpp_api::interface::{SwInterfaceDetails, SwInterfaceDump};
use latest_vpp_api::interface::{SwInterfaceSetFlags, SwInterfaceSetFlagsReply};
use latest_vpp_api::interface_types::IfStatusFlags;
use latest_vpp_api::vlib::{CliInband, CliInbandReply};
use std::convert::TryInto;
use vpp_api_transport::afunix;
use vpp_api_transport::reqrecv::{send_bulk_msg, send_recv_one};
use vpp_api_transport::VppApiTransport;

pub struct Api {
    t: Box<dyn VppApiTransport>,
    next_context: u32,
}

impl Vpp {
    /// Connect a typed binary-API client to this instance.
    pub fn api(&self, client_name: &str) -> Api {
        let mut t: Box<dyn VppApiTransport> = Box::new(afunix::Transport::new(
            self.api_sock().to_str().unwrap(),
        ));
        t.connect(client_name, None, 256).expect("api connect failed");
        Api { t, next_context: 1 }
    }
}

impl Api {
    fn ctx(&mut self) -> u32 {
        let c = self.next_context;
        self.next_context += 1;
        c
    }

    fn client_index(&self) -> u32 {
        self.t.get_client_index()
    }

    /// Run a CLI command over the binary API (cli_inband).
    pub fn cli(&mut self, cmd: &str) -> String {
        let context = self.ctx();
        let reply: CliInbandReply = send_recv_one(
            &CliInband {
                client_index: self.client_index(),
                context,
                cmd: cmd.try_into().unwrap(),
            },
            &mut *self.t,
        )
        .expect("cli_inband failed");
        assert_eq!(reply.retval, 0, "cli_inband '{cmd}' retval {}", reply.retval);
        reply.reply.to_string_lossy()
    }

    /// Dump all interfaces.
    pub fn interfaces(&mut self) -> Vec<SwInterfaceDetails> {
        let context = self.ctx();
        send_bulk_msg(
            &SwInterfaceDump::get_message_name_and_crc(),
            &SwInterfaceDump {
                client_index: self.client_index(),
                context,
                sw_if_index: u32::MAX,
                name_filter_valid: false,
                name_filter: "".try_into().unwrap(),
            },
            &mut *self.t,
            &SwInterfaceDetails::get_message_name_and_crc(),
        )
    }

    /// Find one interface by name.
    pub fn interface(&mut self, name: &str) -> Option<SwInterfaceDetails> {
        self.interfaces()
            .into_iter()
            .find(|i| i.interface_name == name)
    }

    /// Set an interface admin-up (typed sw_interface_set_flags).
    pub fn set_interface_up(&mut self, sw_if_index: u32) {
        let context = self.ctx();
        let reply: SwInterfaceSetFlagsReply = send_recv_one(
            &SwInterfaceSetFlags {
                client_index: self.client_index(),
                context,
                sw_if_index,
                flags: vec![IfStatusFlags::IF_STATUS_API_FLAG_ADMIN_UP]
                    .try_into()
                    .unwrap(),
            },
            &mut *self.t,
        )
        .expect("sw_interface_set_flags failed");
        assert_eq!(reply.retval, 0, "set_flags retval {}", reply.retval);
    }
}

/// "aa:bb:cc:dd:ee:ff" for a MacAddress ([u8; 6]).
pub fn mac_string(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":")
}
