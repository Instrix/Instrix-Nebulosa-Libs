//! # nebulosa
//!
//! Developer library for the INSTRIX Nebulosa power profiler. It speaks NBP
//! (the Nebulosa Bulk Protocol, `spec/nbp.h`) directly to a device over TCP
//! or USB, without the Companion app, and decodes the FPGA sample stream
//! (format 5, `spec/neb_stream.h`) into calibrated [`Sample`]s.
//!
//! ```no_run
//! use std::time::Duration;
//! use nebulosa::{Device, StreamOptions};
//!
//! let mut dev = Device::tcp("192.168.1.50")?;
//! let info = dev.get_info()?;
//! println!("firmware {} FPGA {:08x}", info.fw_version_string(), info.fpga_id);
//! dev.set_decimation(4, nebulosa::msg::decim::AVERAGE)?;
//! dev.start_stream(StreamOptions::default())?;
//! let mut buf = Vec::new();
//! dev.read_samples(&mut buf, 10_000, Duration::from_secs(1))?;
//! for s in &buf {
//!     println!("{:.9} {:e} A {:.3} V", s.time_s(), s.current, s.voltage);
//! }
//! dev.stop_stream()?;
//! # Ok::<(), nebulosa::Error>(())
//! ```
//!
//! Without features the crate has no dependencies (blocking std TCP / UDP).
//! The `usb` feature adds the USB transport (through `rusb` / libusb).
#![deny(missing_docs)]

pub mod calibration;
pub mod device;
pub mod discovery;
pub mod error;
pub mod frame;
pub mod msg;
pub mod stream;
pub mod transport;
pub mod update;
#[cfg(feature = "usb")]
pub mod usb;

pub use calibration::Calibration;
pub use device::{Block, Device, Samples, StreamCounters, StreamOptions, Timeouts, UploadProgress, UploadStage};
pub use discovery::{discover, discover_at, Discovered};
pub use error::{Error, Result};
pub use frame::{Frame, FrameParser};
pub use msg::{BoardStatus, Decim, Info, RangeState, Stats, UpdateInfo};
pub use stream::{DecoderCounters, RawSample, Sample, StreamDecoder};
pub use transport::{TcpTransport, Transport};
pub use update::crc32;
#[cfg(feature = "usb")]
pub use usb::{list_usb, UsbDeviceInfo, UsbSelector, UsbTransport};

#[inline]
pub(crate) fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

#[inline]
pub(crate) fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
