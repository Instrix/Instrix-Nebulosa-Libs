//! Integration tests against the fake device, `tools/nebulosa_sim.py`.

use nebulosa::msg::{self, board, status, upd};
use nebulosa::{Device, Error, StreamOptions};
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

struct Sim {
    child: Child,
    _out: BufReader<ChildStdout>,
    port: u16,
}

impl Drop for Sim {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_tcp_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn free_udp_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn sim_with(extra: &[String]) -> Sim {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../tools/nebulosa_sim.py");
    let port = free_tcp_port();
    let mut child = Command::new("python3")
        .arg(script)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(extra)
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 tools/nebulosa_sim.py");
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    assert!(line.starts_with("nebulosa-sim"), "simulator said {line:?}");
    Sim { child, _out: out, port }
}

fn sim() -> (Sim, Device) {
    let s = sim_with(&["--no-udp".into()]);
    let d = Device::tcp(&format!("127.0.0.1:{}", s.port)).unwrap();
    (s, d)
}

#[test]
fn every_command() {
    let (_sim, mut d) = sim();

    assert_eq!(d.ping(b"hello nebulosa").unwrap(), b"hello nebulosa");
    assert_eq!(d.ping(&[]).unwrap(), b"");

    let i = d.get_info().unwrap();
    assert_eq!(i.proto_version, msg::VERSION);
    assert_eq!(i.link, msg::link::TCP);
    assert!(i.is_format5());
    assert_eq!((i.base_rate_hz, i.block_words), (2_000_000, 507));
    assert!(i.has_caps(msg::caps::BOARD | msg::caps::ETH));
    assert_eq!(i.ip.to_string(), "127.0.0.1");
    assert!(i.build.starts_with("sim"));

    assert_eq!(d.reg_read(0).unwrap(), msg::FPGA_ID_FORMAT5);
    assert_eq!(d.reg_read(41).unwrap(), 100_000_000);
    d.reg_write(7, 0xDEAD_BEEF).unwrap();
    assert_eq!(d.reg_read(7).unwrap(), 0xDEAD_BEEF);
    let e = d.reg_read(200).unwrap_err();
    assert!(matches!(e, Error::Status { cmd: msg::cmd::REG_READ, status: status::BAD_ARG }), "{e}");
    assert!(e.to_string().contains("BAD_ARG"));

    let s = d.get_stats().unwrap();
    assert_eq!(s.stream_link, msg::link::NONE);
    assert!(s.uptime_ms < 60_000);

    d.set_source(msg::source::AD7387).unwrap();
    assert!(matches!(d.set_source(7), Err(Error::Status { status: status::BAD_ARG, .. })));
    d.set_range_auto(0, 5).unwrap();
    assert!(matches!(d.set_range_auto(4, 2), Err(Error::Status { status: status::BAD_ARG, .. })));
    d.set_range_manual(3).unwrap();
    assert!(matches!(d.set_range_manual(6), Err(Error::Status { status: status::BAD_ARG, .. })));
    let r = d.get_range().unwrap();
    assert_eq!(r.ctrl, 0); // manual
    d.set_range_auto(0, 5).unwrap();
    assert_eq!(d.get_range().unwrap().ctrl, 1);
    d.set_range_timing(10, 20, 3, 1000).unwrap();
    d.set_range_force(false, 0).unwrap();

    let dm = d.set_decimation(4, msg::decim::SUBSAMPLE).unwrap();
    assert_eq!((dm.log2n, dm.mode, dm.rate_hz), (4, 1, 125_000));
    assert!(matches!(d.set_decimation(9, 0), Err(Error::Status { status: status::BAD_ARG, .. })));
    d.set_decimation(0, msg::decim::AVERAGE).unwrap();

    let b = d.board_status().unwrap();
    assert_eq!(b.board_id, msg::BOARD_DIGITAL);
    assert_eq!((b.v_bj_mv, b.v_main_mv, b.sd_type, b.pd_state, b.pd_mv), (12050, 11900, 3, 4, 12000));
    assert_eq!(b.fpga_id, msg::FPGA_ID_FORMAT5);
    let b = d.board_set(board::set::RTC, 1_700_000_000).unwrap();
    assert_eq!(b.rtc_s, 1_700_000_000);
    assert!(matches!(d.board_set(99, 0), Err(Error::Status { status: status::BAD_ARG, .. })));
    let b = d.set_leds(0b10101).unwrap();
    assert_eq!((b.leds, b.led_manual), (0b10101, 1));
    let b = d.leds_auto().unwrap();
    assert_eq!(b.led_manual, 0);
    d.ui_key(board::key::DOWN).unwrap();
    let b = d.board_test(board::test::ALL).unwrap();
    for t in 0..board::test::COUNT {
        assert_eq!(b.test_result(t), msg::TestResult::Pass);
    }

    // flash: erase, write a page, read it back
    d.flash_erase(0x8_0000, 4096).unwrap();
    assert_eq!(d.flash_read(0x8_0000, 16).unwrap(), [0xFF; 16]);
    let page: Vec<u8> = (0..=255u8).collect();
    d.flash_write(0x8_0000, &page).unwrap();
    assert_eq!(d.flash_read(0x8_0000, 256).unwrap(), page);
    assert_eq!(d.flash_read(0x8_0010, 4).unwrap(), [16, 17, 18, 19]);
    d.flash_write(0x8_0100, &[0x12, 0x34]).unwrap();
    assert_eq!(d.flash_read(0x8_00FF, 3).unwrap(), [255, 0x12, 0x34]);
    assert!(matches!(d.flash_read(0xFF_FFFF, 256), Err(Error::Status { status: status::BAD_ARG, .. })));
    let b = d.fpga_config(0, 340_000).unwrap();
    assert_eq!(b.board_id, msg::BOARD_DIGITAL);

    // update
    let u = d.update_info().unwrap();
    assert_eq!((u.update_slot, u.slot_size), (1, 448 * 1024));
    assert_eq!(u.update_addr(), 448 * 1024);
    assert_eq!(u.fw_version_string(), "1.4.0");
    assert_eq!(u.slots[0].kind, upd::SLOT_GATEWARE);

    // upload with progress, unaligned start rejected
    let data: Vec<u8> = (0..10_000u32).map(|k| (k * 7 + k / 256) as u8).collect();
    let mut stages = vec![];
    d.upload_flash(0x9_0000, &data, |p| {
        if stages.last() != Some(&p.stage) {
            stages.push(p.stage);
        }
        assert!(p.done <= p.total);
    })
    .unwrap();
    assert_eq!(stages, [nebulosa::UploadStage::Erasing, nebulosa::UploadStage::Writing, nebulosa::UploadStage::Verifying]);
    assert_eq!(d.flash_read(0x9_0000 + 9_000, 8).unwrap(), data[9_000..9_008]);
    assert!(matches!(d.upload_flash(0x9_0100, &data, |_| {}), Err(Error::Invalid(_))));

    // gateware: a .bin with a sync word; the sim checks the CRC of the slot
    let mut gw = vec![0xFFu8; 300];
    gw.extend_from_slice(&nebulosa::update::SYNC_WORD);
    gw.extend((0..20_000u32).map(|k| (k ^ (k >> 5)) as u8));
    let u = d.update_gateware(&gw).unwrap();
    assert_eq!(u.result, status::OK);
    assert!(u.msg.contains("crc ok"), "{}", u.msg);
    assert_eq!(d.flash_read(448 * 1024 + 300, 4).unwrap(), nebulosa::update::SYNC_WORD);
    assert!(matches!(d.update_gateware(b"not a bitstream"), Err(Error::Invalid(_))));
    // a wrong CRC: HW, with the device's update info and message
    match d.update(upd::GATEWARE, 1, gw.len() as u32, 0x1234_5678) {
        Err(e @ Error::Update { status: status::HW, .. }) => {
            assert!(e.to_string().contains("mismatch"), "{e}");
            if let Error::Update { info, .. } = e {
                assert_eq!(info.result, status::HW);
            }
        }
        // older simulator: status OK, the failure only in `result`
        Ok(u) => assert_eq!(u.result, status::HW),
        other => panic!("wrong CRC: {other:?}"),
    }
    // too big for the slot
    let mut big = nebulosa::update::SYNC_WORD.to_vec();
    big.resize(448 * 1024 + 1, 0);
    assert!(matches!(d.update_gateware(&big), Err(Error::Invalid(_))));

    // firmware: checked first
    let fw = nebulosa::update::make_nbfw(&[0x5A; 5000], 0x0001_0500, msg::BOARD_DIGITAL);
    let u = d.update_firmware(&fw).unwrap();
    assert!(u.msg.contains("crc ok"), "{}", u.msg);
    let bad = nebulosa::update::make_nbfw(&[0x5A; 100], 1, 0x1234);
    assert!(matches!(d.update_firmware(&bad), Err(Error::Invalid(m)) if m.contains("another board")));
    d.update_revert().unwrap();

    // raw request: an unknown command
    assert!(matches!(d.request(0x7E, &[], Duration::from_secs(1)), Err(Error::Status { cmd: 0x7E, status: status::UNKNOWN })));

    // reset: the connection drops
    d.reset().unwrap();
    let mut closed = false;
    for _ in 0..3 {
        match d.ping(b"x") {
            Err(Error::Closed) | Err(Error::Io(_)) => {
                closed = true;
                break;
            }
            Err(Error::Timeout { .. }) => {}
            other => panic!("after reset: {other:?}"),
        }
    }
    assert!(closed);
}

#[test]
fn stream_one_second() {
    let (_sim, mut d) = sim();
    let dm = d.set_decimation(3, msg::decim::AVERAGE).unwrap();
    assert_eq!(dm.rate_hz, 250_000);
    d.set_range_auto(0, 5).unwrap();
    d.start_stream(StreamOptions::default()).unwrap();

    let mut all = Vec::new();
    let t0 = Instant::now();
    let mut asked_stats = false;
    while t0.elapsed() < Duration::from_secs(1) {
        d.read_samples(&mut all, 50_000, Duration::from_millis(500)).unwrap();
        if !asked_stats && t0.elapsed() > Duration::from_millis(400) {
            // DATA arriving while waiting for this response is queued
            let s = d.get_stats().unwrap();
            assert_eq!(s.stream_link, msg::link::TCP);
            assert!(s.blocks > 0);
            asked_stats = true;
        }
    }
    d.stop_stream().unwrap();
    // drain what was in flight
    while d.read_samples(&mut all, 1 << 20, Duration::from_millis(100)).unwrap() > 0 {}
    assert_eq!(d.get_stats().unwrap().stream_link, msg::link::NONE);

    assert!(all.len() > 10_000, "only {} samples", all.len());
    assert_eq!(all[0].ts, 0);
    for w in all.windows(2) {
        assert_eq!(w[1].ts, w[0].ts + w[0].step as u64, "timestamps not contiguous");
    }
    assert!(all.iter().all(|s| s.step == 8 && s.range <= 5));
    let ranges: std::collections::BTreeSet<u8> = all.iter().map(|s| s.range).collect();
    assert!(ranges.len() > 1, "range never changed: {ranges:?}");
    // calibrated values: the sim's voltage is 3.3 V +- 0.2 V
    assert!(all.iter().all(|s| s.voltage > 3.0 && s.voltage < 3.6));
    assert!(all.iter().all(|s| s.current > 0.0 && s.current < 2.0));

    let c = d.stream_counters();
    assert_eq!(c.samples, all.len() as u64);
    assert!(c.range_changes > 0);
    assert_eq!((c.seq_errors, c.malformed, c.lost_blocks, c.unframed, c.gap_periods), (0, 0, 0, 0, 0));
    assert_eq!((c.stall_flags, c.gap_flags, c.test_blocks, c.resyncs, c.bad_blocks), (0, 0, 0, 0, 0));
    assert!(c.data_blocks > 10);
    assert!(c.dirty > 0);
}

#[test]
fn iterator_restart_and_raw_blocks() {
    let (_sim, mut d) = sim();
    d.set_decimation(2, msg::decim::AVERAGE).unwrap();
    d.start_stream(StreamOptions::default()).unwrap();
    let v: Vec<_> = d.samples(Duration::from_secs(2)).take(5000).collect::<Result<_, _>>().unwrap();
    assert_eq!(v.len(), 5000);
    assert!(v.windows(2).all(|w| w[1].ts == w[0].ts + 4));
    d.stop_stream().unwrap();

    // a second stream starts over: FIRST resets the counters
    d.start_stream(StreamOptions::default()).unwrap();
    let mut blocks = Vec::new();
    while blocks.len() < 5 {
        let want = 5 - blocks.len();
        assert!(d.read_blocks(&mut blocks, want, Duration::from_secs(2)).unwrap() > 0);
    }
    d.stop_stream().unwrap();
    assert_eq!(blocks[0].seq, 0);
    assert!(blocks[0].flags & msg::blk::FIRST != 0);
    assert!(blocks.windows(2).all(|w| w[1].seq == w[0].seq + 1));
    assert!(blocks.iter().all(|b| b.words.len() == msg::BLOCK_WORDS));
    // the first FPGA word is a BLOCK header (bits 31:29 = 010, type 0)
    assert_eq!(blocks[0].words[0] >> 27, 0b01000);
    let c = d.stream_counters();
    assert_eq!((c.data_blocks, c.lost_blocks), (5, 0));
}

#[test]
fn udp_discovery() {
    let udp = free_udp_port();
    let _s = sim_with(&["--udp-port".into(), udp.to_string()]);
    let target: SocketAddr = format!("127.0.0.1:{udp}").parse().unwrap();
    // the simulator's UDP thread may bind just after it says it is up
    let mut found = vec![];
    for _ in 0..5 {
        found = nebulosa::discover_at(target, Duration::from_millis(600)).unwrap();
        if !found.is_empty() {
            break;
        }
    }
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].addr, "127.0.0.1:20034".parse().unwrap());
    assert!(found[0].info.is_format5());
}
