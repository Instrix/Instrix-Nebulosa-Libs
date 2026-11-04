//! Tests on a real Nebulosa over USB. Harmless only: info, stats, LEDs,
//! decimation and short streams. Run with a device attached:
//!
//!     cargo test --release --features usb --test hardware -- --ignored --test-threads 1
#![cfg(feature = "usb")]

use nebulosa::{msg, Device, StreamOptions, UsbSelector};
use std::time::{Duration, Instant};

#[test]
#[ignore = "needs a Nebulosa on USB"]
fn usb_info_and_leds() {
    let mut d = Device::usb(UsbSelector::Any).unwrap();
    let i = d.get_info().unwrap();
    assert_eq!(i.proto_version, msg::VERSION);
    assert!(matches!(i.link, msg::link::USB_FS | msg::link::USB_HS | msg::link::USB_SS));
    d.get_stats().unwrap();
    if i.has_caps(msg::caps::BOARD) {
        assert_eq!(d.set_leds(0b00101).unwrap().leds & 0x1F, 0b00101);
        assert_eq!(d.leds_auto().unwrap().led_manual, 0);
        d.board_status().unwrap();
    }
    assert_eq!(d.ping(&[1, 2, 3]).unwrap(), [1, 2, 3]);
}

#[test]
#[ignore = "needs a Nebulosa on USB"]
fn usb_stream_2msps() {
    let mut d = Device::usb(UsbSelector::Any).unwrap();
    let i = d.get_info().unwrap();
    assert!(i.is_format5(), "FPGA 0x{:08X} is not format 5", i.fpga_id);
    d.set_decimation(0, msg::decim::AVERAGE).unwrap();
    let before = d.get_stats().unwrap();
    d.start_stream(StreamOptions::default()).unwrap();
    let t0 = Instant::now();
    let mut buf = Vec::with_capacity(1 << 16);
    let mut n = 0u64;
    let mut mid = None;
    while t0.elapsed() < Duration::from_secs(3) {
        buf.clear();
        n += d.read_samples(&mut buf, 1 << 16, Duration::from_millis(500)).unwrap() as u64;
        if mid.is_none() && t0.elapsed() > Duration::from_millis(1500) {
            mid = Some(d.get_stats().unwrap()); // a request mid-stream
        }
    }
    let during = d.get_stats().unwrap();
    d.stop_stream().unwrap();
    let c = d.stream_counters();
    eprintln!("{n} samples in 3 s, {c:?}");
    assert!(n as f64 > 2e6 * 2.5, "{n} samples");
    assert_eq!((c.lost_blocks, c.stall_flags, c.seq_errors, c.malformed), (0, 0, 0, 0));
    assert_eq!(during.stalls, 0);
    assert_eq!(during.fpga_drops, before.fpga_drops, "the FPGA dropped samples");
}
