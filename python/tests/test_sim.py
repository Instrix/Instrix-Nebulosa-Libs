"""Integration: every command and a stream against tools/nebulosa_sim.py."""

import struct
import time

import pytest

import nebulosa as n


def test_info_and_general(dev):
    i = dev.get_info()
    assert i.proto_version == 1 and i.link == n.Link.TCP
    assert i.fpga_id == n.FPGA_ID_FORMAT5 and i.stream_format == 5
    assert i.base_rate_hz == 2_000_000 and i.block_words == 507
    assert i.ip == "127.0.0.1" and i.mac_str
    assert dev.ping(b"hello" * 100) == b"hello" * 100
    assert dev.ping() == b""
    dev.reg_write(5, 0xDEADBEEF)
    assert dev.reg_read(5) == 0xDEADBEEF
    assert dev.reg_read(0) == n.FPGA_ID_FORMAT5
    assert dev.reg_read(n.REG_CLK_HZ) == 100_000_000
    with pytest.raises(n.NebulosaError) as e:
        dev.reg_read(200)
    assert e.value.name == "BAD_ARG"
    with pytest.raises(n.NebulosaError) as e:
        dev.request(0x77)
    assert e.value.status == n.Status.UNKNOWN


def test_settings(dev):
    dev.set_source(n.Source.AD7387)
    with pytest.raises(n.NebulosaError):
        dev.set_source(7)
    dev.set_range_manual(2)
    dev.set_range_auto(1, 4)
    dev.set_range_auto()
    assert isinstance(dev.get_range(), n.RangeState)
    dev.set_range_timing(10, 20, 3, 1000)
    dev.set_range_force(True, 0b000001)
    dev.set_range_force(False)
    r = dev.set_decimation(4, n.DecimMode.SUBSAMPLE)
    assert (r.log2n, r.mode, r.rate_hz) == (4, 1, 125_000)
    with pytest.raises(n.NebulosaError):
        dev.set_decimation(9)
    st = dev.get_stats()
    assert st.decim == (1 << 4) | 4 and st.adc_ctrl == n.Source.AD7387
    dev.set_decimation(0)


def test_board(dev):
    b = dev.board_status()
    assert b.board_id == n.BOARD_DIGITAL
    assert b.has(n.BoardFlag.HSE) and b.flash_jedec == 0xEF4014
    b = dev.board_test()
    assert all(b.test_result(t) == n.TestResult.PASS for t in n.Test)
    b = dev.set_leds(0b10101)
    assert (b.leds, b.led_manual) == (0b10101, 1)
    b = dev.leds_auto()
    assert b.led_manual == 0
    b = dev.board_set(n.BoardSet.RTC, 123456)
    assert b.rtc_s == 123456
    dev.ui_key(n.Key.DOWN)
    assert isinstance(dev.fpga_config(0, 1000), n.BoardStatus)


def test_flash(dev):
    dev.flash_erase(0x80000, 4096)
    assert dev.flash_read(0x80000, 16) == b"\xff" * 16
    dev.flash_write(0x80010, b"abc")
    assert dev.flash_read(0x80010, 3) == b"abc"
    with pytest.raises(ValueError):
        dev.flash_write(0x800F0, bytes(32))  # crosses a page
    with pytest.raises(ValueError):
        dev.flash_erase(0x80001, 4096)
    data = bytes(range(256)) * 20 + b"\xff" * 256 + b"tail"
    seen = []
    dev.upload_flash(0x90000, data, progress=lambda s, d, t: seen.append(s))
    assert dev.flash_read(0x90000 + 5120 + 256, 4) == b"tail"
    assert {"erase", "write", "verify"} <= set(seen)


def test_update(dev):
    u = dev.update_info()
    assert u.slot_size == 448 * 1024 and u.update_slot == 1
    gw = b"\xff" * 16 + b"\xaa\x99\x55\x66" + bytes(range(256)) * 10
    r = dev.update_gateware(gw)
    assert r.result == 0 and "ok" in r.msg
    assert dev.flash_read(u.update_slot * u.slot_size, 20) == gw[:20]

    from test_images import nbfw
    r = dev.update_firmware(nbfw(b"firmware body" * 50, n.BOARD_DIGITAL))
    assert r.result == 0
    with pytest.raises(n.ImageError):
        dev.update_firmware(b"not firmware")
    with pytest.raises(n.ImageError):
        dev.update_gateware(b"no sync word here")


def test_stream_one_second(dev):
    dev.set_range_auto()
    dev.set_decimation(6)  # 31.25 kS/s: the sim keeps up in real time
    dev.start_stream()
    samples = []
    t_end = time.monotonic() + 1.0
    stats = None
    while time.monotonic() < t_end:
        samples += dev.read_samples(timeout=0.2)
        if stats is None:
            stats = dev.get_stats()  # a request while DATA flows: DATA is queued
    dev.stop_stream()
    samples += dev.read_samples(timeout=0.2)  # what was in flight
    assert stats.stream_link == n.Link.TCP
    assert len(samples) > 10000
    ts = [s.ts for s in samples]
    assert all(b > a for a, b in zip(ts, ts[1:]))
    assert all(b.ts - a.ts == a.step for a, b in zip(samples, samples[1:]))
    assert samples[0].ts == 0 and samples[0].step == 64
    assert len({s.range for s in samples}) >= 2
    c = dev.stream_counters()
    assert c.seq_errors == 0 and c.malformed == 0 and c.lost_blocks == 0
    assert c.unframed == 0 and c.resyncs == 0 and c.gap_periods == 0
    assert c.range_changes > 0 and c.samples == len(samples)
    for s in samples[:1000]:
        assert s.current == pytest.approx(s.current_code * dev.calibration.i_fs[s.range] / 16384)
        assert s.voltage == pytest.approx(s.voltage_code * 1e-3)
    assert 3.0 < samples[0].voltage < 3.6


def test_stream_arrays_and_blocks(dev):
    np = pytest.importorskip("numpy")
    dev.set_decimation(5)
    dev.start_stream()
    a = dev.capture(0.5)
    b = dev.read_arrays(max=100, timeout=1.0)
    assert len(b) == 100
    first = dev.read_samples(max=10)
    blocks = []
    while len(blocks) < 4:
        got = dev.read_blocks(max=4 - len(blocks))
        assert got
        blocks += got
    dev.stop_stream()
    assert len(a) > 5000
    assert np.all(np.diff(a.ts.astype(np.int64)) == 32)
    assert int(b.ts[0]) == int(a.ts[-1]) + 32 and int(first[0].ts) == int(b.ts[-1]) + 32
    assert a.current.dtype == np.float64 and a.time_s[1] - a.time_s[0] == pytest.approx(32 * 500e-9)
    assert len(blocks) == 4 and all(len(x.words) == 507 for x in blocks)
    assert dev.stream_counters().seq_errors == 0
    dev.set_decimation(0)


def test_samples_iterator_and_custom_calibration(dev):
    dev.set_decimation(7)
    dev.calibration = n.Calibration(v_scale=2e-3, v_offset=0.5)
    dev.start_stream()
    got = []
    for s in dev.samples(timeout=1.0):
        got.append(s)
        if len(got) >= 2000:
            break
    dev.stop_stream()
    assert len(got) == 2000
    assert got[0].voltage == pytest.approx(got[0].voltage_code * 2e-3 + 0.5)
    dev.set_decimation(0)


def test_discover(sim):
    host, _, udp = sim
    found = n.discover(timeout=0.5, address=host, port=udp)
    assert len(found) == 1
    assert found[0].host == "127.0.0.1" and found[0].info.fpga_id == n.FPGA_ID_FORMAT5


def test_reset_closes(sim):
    host, port, _ = sim
    d = n.Nebulosa.tcp(host, port)
    d.reset()
    with pytest.raises(n.LinkClosed):
        for _ in range(20):
            d.get_info()
    d.close()
    time.sleep(0.1)
