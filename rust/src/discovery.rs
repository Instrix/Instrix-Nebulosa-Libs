//! UDP discovery: a GET_INFO header to port 20035, broadcast or unicast;
//! every device answers with its GET_INFO response in one datagram.

use crate::frame::{FrameParser, HDR_LEN};
use crate::msg::{self, Info};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

/// A device that answered discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    /// Where to connect over TCP: the answer's source address, port 20034.
    pub addr: SocketAddr,
    /// Its GET_INFO.
    pub info: Info,
}

/// Default listen time.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(600);

/// Broadcasts discovery on the local network and collects the answers for
/// `timeout` (600 ms is usual).
pub fn discover(timeout: Duration) -> io::Result<Vec<Discovered>> {
    discover_at(SocketAddrV4::new(Ipv4Addr::BROADCAST, msg::UDP_DISCOVERY_PORT).into(), timeout)
}

/// Sends discovery to `target` (a broadcast or unicast address, normally
/// port 20035) and collects the answers for `timeout`, one per device.
pub fn discover_at(target: SocketAddr, timeout: Duration) -> io::Result<Vec<Discovered>> {
    let bind: SocketAddr = if target.is_ipv4() { "0.0.0.0:0".parse().unwrap() } else { "[::]:0".parse().unwrap() };
    let sock = UdpSocket::bind(bind)?;
    if target.is_ipv4() {
        sock.set_broadcast(true)?;
    }
    let q: [u8; HDR_LEN] = [0x4E, 0x42, msg::cmd::GET_INFO, 0, 0, 0, 0, 0];
    sock.send_to(&q, target)?;
    let deadline = Instant::now() + timeout;
    let mut out: Vec<Discovered> = Vec::new();
    let mut buf = [0u8; 1500];
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        sock.set_read_timeout(Some(deadline - now))?;
        let (n, from) = match sock.recv_from(&mut buf) {
            Ok(x) => x,
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => break,
            Err(e) => return Err(e),
        };
        let mut p = FrameParser::new();
        p.push(&buf[..n]);
        let Some(f) = p.next_frame() else { continue };
        if f.ty != msg::rsp(msg::cmd::GET_INFO) || f.status != 0 {
            continue;
        }
        let Some(info) = Info::parse(&f.payload) else { continue };
        let addr = SocketAddr::new(from.ip(), msg::TCP_PORT);
        if !out.iter().any(|d| d.addr == addr) {
            out.push(Discovered { addr, info });
        }
    }
    Ok(out)
}
