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
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::convert::TryInto;
use vpp_api_transport::afunix;
use vpp_api_transport::reqrecv::{send_bulk_msg, send_recv_one};
use vpp_api_transport::VppApiTransport;

// Re-exported so plugin test crates can define their own message
// structs (plugin custom APIs) against the same trait/derive versions.
pub use vpp_api_message::VppApiMessage;

pub struct Api {
    t: Box<dyn VppApiTransport>,
    next_context: u32,
    // held for the client's lifetime; see Vpp::api
    _exclusive: std::sync::MutexGuard<'static, ()>,
}

/// vpp-api-transport's afunix::Transport panics if two instances exist
/// in one process (a shmem-compat guard), so parallel tests must take
/// turns holding an Api client.
static API_CLIENT_SLOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl Vpp {
    /// Connect a typed binary-API client to this instance. Only one
    /// `Api` can exist per test process — concurrent tests block here
    /// until the current holder drops theirs.
    pub fn api(&self, client_name: &str) -> Api {
        let exclusive = API_CLIENT_SLOT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut t: Box<dyn VppApiTransport> = Box::new(afunix::Transport::new(
            self.api_sock().to_str().unwrap(),
        ));
        t.connect(client_name, None, 256).expect("api connect failed");
        Api {
            t,
            next_context: 1,
            _exclusive: exclusive,
        }
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

    /// Send any request and wait for its reply — for custom plugin
    /// messages. The closure gets `(client_index, context)` to embed in
    /// the request; the reply type is resolved by its own name+CRC.
    pub fn send_recv<T, R>(&mut self, build: impl FnOnce(u32, u32) -> T) -> R
    where
        T: Serialize + DeserializeOwned + VppApiMessage,
        R: Serialize + DeserializeOwned + VppApiMessage,
    {
        let context = self.ctx();
        let req = build(self.client_index(), context);
        send_recv_one(&req, &mut *self.t)
            .unwrap_or_else(|e| panic!("{} failed: {e:?}", T::get_message_name_and_crc()))
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
