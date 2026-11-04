//! Byte transports NBP runs on, and the TCP one.
//!
//! Implement [`Transport`] to run NBP over anything else (a serial bridge,
//! a recorded capture, a test double) and pass it to
//! [`crate::Device::with_transport`].

use crate::msg::TCP_PORT;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// One ordered byte stream to the device and one or more from it.
///
/// Each source (USB IN endpoint) is its own byte stream with its own frame
/// parser; a message never spans sources. TCP has one source, 0.
pub trait Transport: Send {
    /// Sends all of `data`.
    fn write_all(&mut self, data: &[u8]) -> io::Result<()>;

    /// Reads whatever is available into `buf`, waiting up to `timeout`.
    ///
    /// Returns `Ok(Some((source, n)))` with `n > 0` bytes from stream
    /// `source`, `Ok(None)` when nothing came within `timeout`, and an
    /// error of kind `UnexpectedEof` (or `ConnectionReset`, ...) once the
    /// connection is closed.
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<Option<(usize, usize)>>;

    /// A short description ("TCP 192.168.1.50:20034", "USB 3:7").
    fn describe(&self) -> String {
        String::from("custom transport")
    }
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        (**self).write_all(data)
    }
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<Option<(usize, usize)>> {
        (**self).read(buf, timeout)
    }
    fn describe(&self) -> String {
        (**self).describe()
    }
}

/// NBP over TCP (port 20034). The device takes one client at a time.
#[derive(Debug)]
pub struct TcpTransport {
    stream: TcpStream,
    peer: SocketAddr,
    timeout: Option<Duration>,
}

/// Resolves `"host"`, `"host:port"`, `"1.2.3.4"`, `"[::1]:20034"`, `"::1"`;
/// the port defaults to 20034.
pub fn resolve(addr: &str) -> io::Result<Vec<SocketAddr>> {
    if let Ok(sa) = addr.parse::<SocketAddr>() {
        return Ok(vec![sa]);
    }
    if let Ok(ip) = addr.trim_matches(|c| c == '[' || c == ']').parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, TCP_PORT)]);
    }
    let has_port = addr.rsplit_once(':').is_some_and(|(_, p)| p.parse::<u16>().is_ok());
    let v: Vec<SocketAddr> =
        if has_port { addr.to_socket_addrs()?.collect() } else { (addr, TCP_PORT).to_socket_addrs()?.collect() };
    if v.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{addr}: no address")));
    }
    Ok(v)
}

impl TcpTransport {
    /// Connects to `addr` (see [`resolve`]) with a 3 s connect timeout.
    pub fn connect(addr: &str) -> io::Result<Self> {
        Self::connect_timeout(addr, Duration::from_secs(3))
    }

    /// Connects to `addr` (see [`resolve`]).
    pub fn connect_timeout(addr: &str, timeout: Duration) -> io::Result<Self> {
        let mut last = None;
        for sa in resolve(addr)? {
            match TcpStream::connect_timeout(&sa, timeout) {
                Ok(s) => return Self::from_stream(s),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address")))
    }

    /// Wraps a connected stream (sets `TCP_NODELAY`).
    pub fn from_stream(stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        let peer = stream.peer_addr()?;
        Ok(Self { stream, peer, timeout: None })
    }

    /// The device's address.
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer
    }
}

impl Transport for TcpTransport {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.stream.write_all(data)
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<Option<(usize, usize)>> {
        let t = Some(timeout.max(Duration::from_millis(1)));
        if self.timeout != t {
            self.stream.set_read_timeout(t)?;
            self.timeout = t;
        }
        match self.stream.read(buf) {
            Ok(0) => Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => Ok(Some((0, n))),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    fn describe(&self) -> String {
        format!("TCP {}", self.peer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_default_port() {
        assert_eq!(resolve("10.0.0.2").unwrap()[0], "10.0.0.2:20034".parse().unwrap());
        assert_eq!(resolve("10.0.0.2:99").unwrap()[0], "10.0.0.2:99".parse().unwrap());
        assert_eq!(resolve("::1").unwrap()[0], "[::1]:20034".parse().unwrap());
        assert_eq!(resolve("[::1]:5").unwrap()[0], "[::1]:5".parse().unwrap());
        assert_eq!(resolve("localhost").unwrap()[0].port(), 20034);
    }
}
