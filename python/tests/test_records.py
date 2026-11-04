import struct

import pytest

import nebulosa as n
from nebulosa.records import BoardStatus, Decim, Info, RangeState, Stats, UpdateInfo


def test_sizes():
    assert (Info.LEN, Stats.LEN, RangeState.LEN, Decim.LEN, BoardStatus.LEN, UpdateInfo.LEN) == (
        76, 64, 16, 8, 200, 136)


@pytest.mark.parametrize("cls", [Info, Stats, RangeState, Decim, BoardStatus, UpdateInfo])
def test_short_rejected_long_accepted(cls):
    with pytest.raises(ValueError):
        cls.parse(bytes(cls.LEN - 1))
    cls.parse(bytes(cls.LEN + 32))  # newer firmware may append


def test_info_layout():
    p = struct.pack("<8I32s6s2s4s", 1, 0x010400, 0x4E42000B, 2_000_000, 3, 1460, 507, 0x3F,
                    b"build x", b"\x02\x4e\x42\x00\x00\x01", b"\0\0", bytes([192, 168, 1, 50]))
    i = Info.parse(p)
    assert i.fw_version_str == "1.4.0"
    assert i.link == n.Link.TCP and i.link_name == "TCP"
    assert i.build == "build x"
    assert i.mac_str == "02:4e:42:00:00:01"
    assert i.ip == "192.168.1.50"
    assert i.stream_format == 5
    assert n.Cap.BOARD in i.capabilities
    p0 = bytearray(p)
    p0[8:12] = struct.pack("<I", 0x4E42000A)
    p0[72:76] = b"\0\0\0\0"
    i0 = Info.parse(bytes(p0))
    assert i0.stream_format == 4 and i0.ip == ""


def test_stats_and_small():
    s = Stats.parse(struct.pack("<16I", *range(16)))
    assert (s.uptime_ms, s.stream_link, s.eth_ip, s.reserved) == (0, 1, 11, (12, 13, 14, 15))
    assert RangeState.parse(struct.pack("<4I", 1, 2, 3, 4)) == RangeState(1, 2, 3, 4)
    assert Decim.parse(struct.pack("<BB2xI", 6, 1, 31250)) == Decim(6, 1, 31250)


def test_board_status_layout():
    # board.rs status_layout, ported
    p = bytearray(200)
    p[0:4] = struct.pack("<I", 0x4E440001)
    p[16:20] = struct.pack("<I", n.BoardFlag.APWR_OK | n.BoardFlag.APWR_H | n.BoardFlag.USB_SS)
    p[24:26] = struct.pack("<H", 12050)
    p[28:30] = struct.pack("<H", 11900)
    p[80:84] = struct.pack("<I", 1 | 2 << 2 | 3 << 12)
    p[84:88] = struct.pack("<I", 150_000_000)
    p[132] = 4
    p[135] = 2
    p[136:138] = struct.pack("<H", 12000)
    p[144:148] = struct.pack("<I", (100 << 10) | 300)
    p[148:152] = struct.pack("<I", (240 << 10) | 225)
    p[180:184] = struct.pack("<I", 7)
    s = BoardStatus.parse(bytes(p))
    assert s.board_id == n.BOARD_DIGITAL
    assert s.has(n.BoardFlag.APWR_OK) and s.has(n.BoardFlag.APWR_H) and not s.has(n.BoardFlag.APWR_L)
    assert (s.v_bj_mv, s.v_main_mv) == (12050, 11900)
    assert s.test_result(n.Test.HSE) == n.TestResult.PASS
    assert s.test_result(n.Test.LSE) == n.TestResult.FAIL
    assert s.test_result(n.Test.SD) == n.TestResult.SKIP
    assert s.test_result(n.Test.RTC) == n.TestResult.NONE
    assert s.test_detail[0] == 150_000_000
    assert (s.pd_state, s.pd_mv, s.pd_errors) == (n.PdState.READY, 12000, 7)
    assert [(q.mv, q.ma) for q in s.pdos] == [(5000, 3000), (12000, 2250)]


def test_update_info_layout():
    # update.rs info_layout, ported
    p = bytearray(136)
    p[0:4] = struct.pack("<I", 0x00020000)
    p[16:21] = b"Nov 3"
    p[45] = 1
    p[48:52] = struct.pack("<I", 448 * 1024)
    p[68] = n.SlotType.FIRMWARE
    p[72:76] = struct.pack("<I", 1234)
    p[88:92] = b"done"
    i = UpdateInfo.parse(bytes(p))
    assert (i.fw_version_str, i.update_slot, i.slot_size) == ("2.0.0", 1, 448 * 1024)
    assert i.slot[1].type == n.SlotType.FIRMWARE and i.slot[1].len == 1234
    assert (i.fw_date, i.msg) == ("Nov 3", "done")


def test_errors():
    e = n.NebulosaError(n.Status.BAD_ARG, cmd=0x30)
    assert e.status == 3 and e.name == "BAD_ARG" and "0x30" in str(e)
    t = n.TimeoutError("x")
    assert isinstance(t, n.NebulosaError) and isinstance(t, TimeoutError) and t.name == "TIMEOUT"
    assert n.NebulosaError(99).name == "?"
