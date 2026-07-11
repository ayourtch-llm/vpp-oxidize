//! Packet injection via VPP's packet-generator, with packets built by
//! [oside](https://github.com/ayourtch/oside) layer stacks instead of
//! pg's text DSL.

use crate::Vpp;
use oside::LayerStack;

/// A packet-generator stream definition.
pub struct Stream {
    pub name: String,
    pub packet: LayerStack,
    pub count: u64,
    /// packets per second offered
    pub rate: f64,
    pub interface: String,
}

impl Stream {
    pub fn new(name: &str, interface: &str, packet: LayerStack) -> Stream {
        Stream {
            name: name.to_string(),
            packet,
            count: 100,
            rate: 1e6,
            interface: interface.to_string(),
        }
    }

    pub fn count(mut self, n: u64) -> Self {
        self.count = n;
        self
    }

    pub fn rate(mut self, pps: f64) -> Self {
        self.rate = pps;
        self
    }
}

impl Vpp {
    /// Define a pg stream from an oside layer stack (checksums/lengths
    /// are computed by `fill()`).
    pub fn pg_stream(&self, s: Stream) {
        let bytes = s.packet.fill().lencode();
        let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
        let size = bytes.len().max(60);
        self.ctl(&format!(
            "packet-generator new {{ name {name} limit {count} rate {rate} \
             node ethernet-input size {size}-{size} interface {ifn} \
             data {{ hex 0x{hex} }} }}",
            name = s.name,
            count = s.count,
            rate = s.rate,
            ifn = s.interface,
        ));
    }

    /// Enable all defined streams and wait until they finish sending.
    pub fn pg_run(&self) {
        self.ctl("packet-generator enable");
        for _ in 0..300 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let out = self.ctl("show packet-generator");
            // "Yes" in the Enabled column means still sending
            if !out.lines().skip(1).any(|l| l.split_whitespace().nth(1) == Some("Yes")) {
                return;
            }
        }
        panic!("packet-generator streams did not finish");
    }
}
