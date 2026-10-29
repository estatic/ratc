//! Console lines (and follow-me car commands, see `atc_proto::overlay_command`) to the X-Plane
//! overlay plugin (`crates/atc-xplane-plugin`): one UTF-8 UDP datagram per line. Fire and
//! forget — nothing listening is not an error.

use std::net::{SocketAddr, UdpSocket};

/// Where the plugin listens unless `overlay` in the config says otherwise.
pub const DEFAULT_ADDR: &str = "127.0.0.1:48010";

pub struct Overlay {
    socket: UdpSocket,
    to: SocketAddr,
}

impl Overlay {
    pub fn new(to: SocketAddr) -> std::io::Result<Self> {
        let any: SocketAddr = if to.is_ipv4() {
            ([0, 0, 0, 0], 0).into()
        } else {
            (std::net::Ipv6Addr::UNSPECIFIED, 0).into()
        };
        let socket = UdpSocket::bind(any)?;
        socket.set_nonblocking(true)?;
        Ok(Overlay { socket, to })
    }

    pub fn send(&self, line: &str) {
        let _ = self.socket.send_to(line.as_bytes(), self.to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_line_is_one_datagram() {
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let o = Overlay::new(rx.local_addr().unwrap()).unwrap();
        o.send("[Kraków Tower] Sierra Bravo Charlie, cleared for take-off");
        let mut buf = [0u8; 256];
        let n = rx.recv(&mut buf).unwrap();
        assert_eq!(
            std::str::from_utf8(&buf[..n]).unwrap(),
            "[Kraków Tower] Sierra Bravo Charlie, cleared for take-off"
        );
    }
}
