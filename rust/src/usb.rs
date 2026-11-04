//! NBP over USB (feature `usb`, through `rusb` / libusb): the vendor
//! interface (class FF / 4E / 01). Its bulk OUT endpoint takes commands;
//! every bulk IN endpoint is read continuously by its own thread
//! (High-Speed: one carries DATA, one responses; Full-Speed: one carries
//! both), each its own byte stream (a transport source).

use crate::msg;
use crate::transport::Transport;
use rusb::{Context, Device as RusbDevice, DeviceHandle, Direction, TransferType, UsbContext};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Bytes per IN transfer (a multiple of every max packet size).
const IN_TRANSFER: usize = 64 * 1024;
/// How often reader threads look at the stop flag when idle.
const IN_POLL: Duration = Duration::from_millis(100);
const OUT_TIMEOUT: Duration = Duration::from_secs(2);

/// Which USB device to open.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UsbSelector {
    /// The first Nebulosa found (High-Speed port preferred).
    #[default]
    Any,
    /// The one with this serial number.
    Serial(String),
    /// The one at this bus number and device address.
    BusAddress {
        /// Bus number.
        bus: u8,
        /// Device address.
        address: u8,
    },
}

/// A Nebulosa on USB ([`list_usb`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDeviceInfo {
    /// Bus number.
    pub bus: u8,
    /// Device address.
    pub address: u8,
    /// Product ID: `msg::USB_PID_HS` (Type-A port) or `msg::USB_PID_FS` (USB-C).
    pub product_id: u16,
    /// Negotiated speed ("High", "Super", "Full", ...).
    pub speed: String,
    /// Manufacturer string, if it could be read (needs access rights).
    pub manufacturer: Option<String>,
    /// Product string, if it could be read.
    pub product: Option<String>,
    /// Serial number, if it could be read.
    pub serial: Option<String>,
}

fn is_nbp<T: UsbContext>(d: &RusbDevice<T>) -> bool {
    d.device_descriptor()
        .map(|x| x.vendor_id() == msg::USB_VID && (x.product_id() == msg::USB_PID_HS || x.product_id() == msg::USB_PID_FS))
        .unwrap_or(false)
}

fn usb_err(e: rusb::Error) -> io::Error {
    let kind = match e {
        rusb::Error::Timeout => io::ErrorKind::TimedOut,
        rusb::Error::NoDevice => io::ErrorKind::UnexpectedEof,
        rusb::Error::Access => io::ErrorKind::PermissionDenied,
        rusb::Error::NotFound => io::ErrorKind::NotFound,
        rusb::Error::Busy => io::ErrorKind::AddrInUse,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, format!("USB: {e}"))
}

fn describe_dev<T: UsbContext>(d: &RusbDevice<T>) -> UsbDeviceInfo {
    let desc = d.device_descriptor().ok();
    let (mut manufacturer, mut product, mut serial) = (None, None, None);
    if let (Some(desc), Ok(h)) = (&desc, d.open()) {
        manufacturer = h.read_manufacturer_string_ascii(desc).ok();
        product = h.read_product_string_ascii(desc).ok();
        serial = h.read_serial_number_string_ascii(desc).ok();
    }
    UsbDeviceInfo {
        bus: d.bus_number(),
        address: d.address(),
        product_id: desc.map(|x| x.product_id()).unwrap_or(0),
        speed: format!("{:?}", d.speed()),
        manufacturer,
        product,
        serial,
    }
}

/// The Nebulosas on USB. Strings are `None` where the device can't be
/// opened (Linux: install `tools/99-nebulosa.rules`).
pub fn list_usb() -> io::Result<Vec<UsbDeviceInfo>> {
    let ctx = Context::new().map_err(usb_err)?;
    let list = ctx.devices().map_err(usb_err)?;
    Ok(list.iter().filter(is_nbp).map(|d| describe_dev(&d)).collect())
}

enum Msg {
    Data(usize, Vec<u8>),
    Err(io::Error),
}

/// NBP over USB: [`Transport`] with one source per bulk IN endpoint.
pub struct UsbTransport {
    handle: Arc<DeviceHandle<Context>>,
    iface: u8,
    out_ep: u8,
    rx: Receiver<Msg>,
    pending: Option<(usize, Vec<u8>, usize)>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    label: String,
}

impl std::fmt::Debug for UsbTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsbTransport").field("device", &self.label).finish_non_exhaustive()
    }
}

impl UsbTransport {
    /// Opens the selected device, claims the NBP interface (only that one)
    /// and starts a reader thread per bulk IN endpoint.
    pub fn open(sel: &UsbSelector) -> io::Result<Self> {
        let ctx = Context::new().map_err(usb_err)?;
        let mut devs: Vec<RusbDevice<Context>> = ctx.devices().map_err(usb_err)?.iter().filter(is_nbp).collect();
        // the High-Speed / SuperSpeed product first
        devs.sort_by_key(|d| d.device_descriptor().map(|x| x.product_id() != msg::USB_PID_HS).unwrap_or(true));
        let dev = devs
            .into_iter()
            .find(|d| match sel {
                UsbSelector::Any => true,
                UsbSelector::BusAddress { bus, address } => d.bus_number() == *bus && d.address() == *address,
                UsbSelector::Serial(s) => describe_dev(d).serial.as_deref() == Some(s.as_str()),
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no Nebulosa on USB (or not the one selected)"))?;

        let cfg = dev.active_config_descriptor().map_err(usb_err)?;
        let mut found = None;
        for i in cfg.interfaces() {
            for a in i.descriptors() {
                if a.class_code() == msg::USB_CLASS
                    && a.sub_class_code() == msg::USB_SUBCLASS
                    && a.protocol_code() == msg::USB_PROTOCOL
                {
                    let bulk: Vec<(u8, Direction)> = a
                        .endpoint_descriptors()
                        .filter(|e| e.transfer_type() == TransferType::Bulk)
                        .map(|e| (e.address(), e.direction()))
                        .collect();
                    found = Some((a.interface_number(), a.setting_number(), bulk));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let (iface, alt, bulk) =
            found.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no NBP interface on this device"))?;
        let out_ep = bulk
            .iter()
            .find(|e| e.1 == Direction::Out)
            .map(|e| e.0)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no bulk OUT endpoint"))?;
        let in_eps: Vec<u8> = bulk.iter().filter(|e| e.1 == Direction::In).map(|e| e.0).collect();
        if in_eps.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no bulk IN endpoint"));
        }

        let h = dev.open().map_err(|e| {
            let mut err = usb_err(e);
            if err.kind() == io::ErrorKind::PermissionDenied {
                err = io::Error::new(err.kind(), format!("{err} (Linux: install tools/99-nebulosa.rules)"));
            }
            err
        })?;
        let _ = h.set_auto_detach_kernel_driver(true);
        h.claim_interface(iface).map_err(usb_err)?;
        if alt != 0 {
            h.set_alternate_setting(iface, alt).map_err(usb_err)?;
        }
        let handle = Arc::new(h);
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let threads = in_eps
            .iter()
            .enumerate()
            .map(|(src, &ep)| {
                let (handle, stop, tx) = (handle.clone(), stop.clone(), tx.clone());
                std::thread::Builder::new()
                    .name(format!("nebulosa-usb-in-{ep:02x}"))
                    .spawn(move || {
                        // a read is always outstanding on this endpoint: the
                        // device's ring must never wait for the host
                        while !stop.load(Ordering::Relaxed) {
                            let mut buf = vec![0u8; IN_TRANSFER];
                            match handle.read_bulk(ep, &mut buf, IN_POLL) {
                                Ok(n) => {
                                    buf.truncate(n);
                                    if n > 0 && tx.send(Msg::Data(src, buf)).is_err() {
                                        break;
                                    }
                                }
                                Err(rusb::Error::Timeout) | Err(rusb::Error::Interrupted) => {}
                                Err(e) => {
                                    if !stop.load(Ordering::Relaxed) {
                                        let _ = tx.send(Msg::Err(io::Error::new(
                                            usb_err(e).kind(),
                                            format!("USB IN 0x{ep:02X}: {e}"),
                                        )));
                                    }
                                    break;
                                }
                            }
                        }
                    })
                    .map_err(io::Error::other)
            })
            .collect::<io::Result<Vec<_>>>()?;
        let label = format!("USB {}:{} {:?}", dev.bus_number(), dev.address(), dev.speed());
        Ok(Self { handle, iface, out_ep, rx, pending: None, stop, threads, label })
    }
}

impl Transport for UsbTransport {
    fn write_all(&mut self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let n = self.handle.write_bulk(self.out_ep, data, OUT_TIMEOUT).map_err(usb_err)?;
            data = &data[n..];
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<Option<(usize, usize)>> {
        let (src, v, off) = match self.pending.take() {
            Some(p) => p,
            None => match self.rx.recv_timeout(timeout) {
                Ok(Msg::Data(src, v)) => (src, v, 0),
                Ok(Msg::Err(e)) => return Err(e),
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => return Err(io::ErrorKind::UnexpectedEof.into()),
            },
        };
        let n = (v.len() - off).min(buf.len());
        buf[..n].copy_from_slice(&v[off..off + n]);
        if off + n < v.len() {
            self.pending = Some((src, v, off + n));
        }
        Ok(Some((src, n)))
    }

    fn describe(&self) -> String {
        self.label.clone()
    }
}

impl Drop for UsbTransport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        let _ = self.handle.release_interface(self.iface);
    }
}
