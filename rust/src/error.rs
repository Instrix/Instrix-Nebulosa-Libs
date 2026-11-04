//! The error type of every device operation.

use crate::msg::{status, UpdateInfo};
use std::fmt;
use std::io;

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What went wrong talking to a device.
#[derive(Debug)]
pub enum Error {
    /// An I/O error from the transport (socket, USB).
    Io(io::Error),
    /// No response to command `cmd` within the timeout.
    Timeout {
        /// The command code (`msg::cmd::*`).
        cmd: u8,
    },
    /// The device answered command `cmd` with a status other than OK.
    Status {
        /// The command code (`msg::cmd::*`).
        cmd: u8,
        /// The `msg::status::*` code; [`status::name`] gives its name.
        status: u8,
    },
    /// A software update operation failed; the device's own report
    /// (`info.msg`) says why.
    Update {
        /// The `msg::status::*` code of the operation.
        status: u8,
        /// The `nbp_update_info_t` the device sent back with it.
        info: Box<UpdateInfo>,
    },
    /// A response too short for its command, or otherwise not understood.
    BadResponse(String),
    /// The connection was closed (by the device, or it went away).
    Closed,
    /// An argument the protocol cannot carry (payload too long, an
    /// unaligned flash address, an image that fails its checks, ...).
    Invalid(String),
    /// Flash read back after writing differs.
    Verify {
        /// First flash address that reads back wrong.
        addr: u32,
        /// What was written.
        expected: u8,
        /// What reads back.
        got: u8,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Timeout { cmd } => write!(f, "command 0x{cmd:02X}: no response (timeout)"),
            Error::Status { cmd, status: st } => {
                write!(f, "command 0x{cmd:02X}: device answered {} ({st})", status::name(*st))
            }
            Error::Update { status: st, info } => {
                write!(f, "update failed: {} ({st})", status::name(*st))?;
                if !info.msg.is_empty() {
                    write!(f, ": {}", info.msg)?;
                }
                Ok(())
            }
            Error::BadResponse(s) => write!(f, "bad response: {s}"),
            Error::Closed => write!(f, "connection closed"),
            Error::Invalid(s) => write!(f, "invalid argument: {s}"),
            Error::Verify { addr, expected, got } => {
                write!(f, "flash verify: 0x{addr:06X} reads 0x{got:02X}, wrote 0x{expected:02X}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe => Error::Closed,
            _ => Error::Io(e),
        }
    }
}
