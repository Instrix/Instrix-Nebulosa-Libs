"""Response records (the packed payload structs of ``nbp.h``).

Every ``parse`` accepts a payload longer than the struct (newer firmware may
append fields) and rejects a shorter one with ``ValueError``.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field
from typing import List, Optional, Tuple

from .constants import (
    FPGA_ID_FORMAT5,
    BoardFlag,
    Cap,
    Link,
    TestResult,
    TEST_COUNT,
)


def _need(p: bytes, n: int, what: str) -> None:
    if len(p) < n:
        raise ValueError("%s: payload is %d bytes, need %d" % (what, len(p), n))


def _cstr(b: bytes) -> str:
    end = b.find(b"\0")
    if end >= 0:
        b = b[:end]
    return b.decode("utf-8", "replace")


def version_str(v: int) -> str:
    """``major << 16 | minor << 8 | patch`` as ``"major.minor.patch"``."""
    return "%d.%d.%d" % (v >> 16, (v >> 8) & 0xFF, v & 0xFF)


@dataclass
class Info:
    """``nbp_info_t`` (76 bytes): ``get_info()``."""

    LEN = 76

    proto_version: int
    fw_version: int
    fpga_id: int
    base_rate_hz: int
    link: int
    link_unit: int
    block_words: int
    caps: int
    build: str
    mac: bytes
    ip: str  # dotted IPv4, "" without one

    @classmethod
    def parse(cls, p: bytes) -> "Info":
        _need(p, cls.LEN, "Info")
        v = struct.unpack_from("<8I", p, 0)
        ip = bytes(p[72:76])
        return cls(
            proto_version=v[0],
            fw_version=v[1],
            fpga_id=v[2],
            base_rate_hz=v[3],
            link=v[4],
            link_unit=v[5],
            block_words=v[6],
            caps=v[7],
            build=_cstr(bytes(p[32:64])),
            mac=bytes(p[64:70]),
            ip="" if ip == b"\0\0\0\0" else "%d.%d.%d.%d" % tuple(ip),
        )

    @property
    def fw_version_str(self) -> str:
        return version_str(self.fw_version)

    @property
    def mac_str(self) -> str:
        """``"aa:bb:cc:dd:ee:ff"``, ``""`` without Ethernet."""
        if not any(self.mac):
            return ""
        return ":".join("%02x" % b for b in self.mac)

    @property
    def link_name(self) -> str:
        try:
            return Link(self.link).name
        except ValueError:
            return "?"

    @property
    def capabilities(self) -> Cap:
        return Cap(self.caps & 0x3F)

    @property
    def stream_format(self) -> int:
        """FPGA stream format: 5 (decoded by this library) or 4 (raw words only)."""
        return 5 if self.fpga_id >= FPGA_ID_FORMAT5 else 4


@dataclass
class Stats:
    """``nbp_stats_t`` (64 bytes): ``get_stats()``."""

    LEN = 64

    uptime_ms: int
    stream_link: int
    blocks: int
    words: int
    stalls: int
    fpga_drops: int
    fifo_level: int
    range_status: int
    decim: int
    adc_ctrl: int
    eth: int
    eth_ip: int
    reserved: Tuple[int, int, int, int] = (0, 0, 0, 0)

    @classmethod
    def parse(cls, p: bytes) -> "Stats":
        _need(p, cls.LEN, "Stats")
        v = struct.unpack_from("<16I", p, 0)
        return cls(*v[:12], reserved=tuple(v[12:16]))  # type: ignore[arg-type]

    @property
    def eth_ip_str(self) -> str:
        if not self.eth_ip:
            return ""
        return "%d.%d.%d.%d" % tuple(struct.pack("<I", self.eth_ip))


@dataclass
class RangeState:
    """``nbp_range_t`` (16 bytes): ``get_range()`` (FPGA registers 35..38)."""

    LEN = 16

    status: int
    count: int
    ctrl: int
    timing: int

    @classmethod
    def parse(cls, p: bytes) -> "RangeState":
        _need(p, cls.LEN, "RangeState")
        return cls(*struct.unpack_from("<4I", p, 0))


@dataclass
class Decim:
    """``nbp_decim_t`` (8 bytes): ``set_decimation()``, as applied."""

    LEN = 8

    log2n: int
    mode: int
    rate_hz: int

    @classmethod
    def parse(cls, p: bytes) -> "Decim":
        _need(p, cls.LEN, "Decim")
        log2n, mode, rate = struct.unpack_from("<BB2xI", p, 0)
        return cls(log2n, mode, rate)


@dataclass
class Pdo:
    """A USB PD source capability. Non-fixed supplies have ``mv = ma = 0``."""

    raw: int
    fixed: bool
    mv: int
    ma: int


@dataclass
class BoardStatus:
    """``nbp_board_status_t`` (200 bytes): ``board_status()`` and friends."""

    LEN = 200

    board_id: int
    uptime_ms: int
    core_hz: int
    hclk_hz: int
    flags: int
    rtc_s: int
    v_bj_mv: int
    v_vbus_mv: int
    v_main_mv: int
    leds: int
    led_manual: int
    pwr_sel: int
    cc: int
    fpga_mode: int
    lcd_mode: int
    sd_type: int
    fpga_cfg_ms: int
    fpga_cfg_bytes: int
    fpga_id: int
    flash_jedec: int
    flash_bytes: int
    sd_blocks: int
    sd_cid: List[int]
    test_results: int
    test_detail: List[int]
    pd_state: int
    pd_cc_line: int
    pd_spec_rev: int
    pd_npdo: int
    pd_mv: int
    pd_ma: int
    pd_target_mv: int
    pd_pdo_sel: int
    pd_pdo: List[int]
    pd_contracts: int
    pd_hard_resets: int
    pd_errors: int

    @classmethod
    def parse(cls, p: bytes) -> "BoardStatus":
        _need(p, cls.LEN, "BoardStatus")
        u32 = lambda o: struct.unpack_from("<I", p, o)[0]  # noqa: E731
        u16 = lambda o: struct.unpack_from("<H", p, o)[0]  # noqa: E731
        return cls(
            board_id=u32(0),
            uptime_ms=u32(4),
            core_hz=u32(8),
            hclk_hz=u32(12),
            flags=u32(16),
            rtc_s=u32(20),
            v_bj_mv=u16(24),
            v_vbus_mv=u16(26),
            v_main_mv=u16(28),
            leds=p[32],
            led_manual=p[33],
            pwr_sel=p[34],
            cc=p[35],
            fpga_mode=p[36],
            lcd_mode=p[37],
            sd_type=p[38],
            fpga_cfg_ms=u32(40),
            fpga_cfg_bytes=u32(44),
            fpga_id=u32(48),
            flash_jedec=u32(52),
            flash_bytes=u32(56),
            sd_blocks=u32(60),
            sd_cid=list(struct.unpack_from("<4I", p, 64)),
            test_results=u32(80),
            test_detail=list(struct.unpack_from("<%dI" % TEST_COUNT, p, 84)),
            pd_state=p[132],
            pd_cc_line=p[133],
            pd_spec_rev=p[134],
            pd_npdo=p[135],
            pd_mv=u16(136),
            pd_ma=u16(138),
            pd_target_mv=u16(140),
            pd_pdo_sel=p[142],
            pd_pdo=list(struct.unpack_from("<7I", p, 144)),
            pd_contracts=u32(172),
            pd_hard_resets=u32(176),
            pd_errors=u32(180),
        )

    def test_result(self, t: int) -> TestResult:
        """Result of board test ``t`` (a ``Test``): ``(test_results >> 2t) & 3``."""
        return TestResult((self.test_results >> (2 * int(t))) & 3)

    def has(self, flag: int) -> bool:
        """True when every bit of ``flag`` (``BoardFlag``) is set."""
        return self.flags & int(flag) == int(flag)

    @property
    def board_flags(self) -> BoardFlag:
        return BoardFlag(self.flags & 0x3FFFFF)

    @property
    def pdos(self) -> List[Pdo]:
        """The source's PDOs (``pd_npdo`` of them) decoded."""
        out = []
        for w in self.pd_pdo[: min(self.pd_npdo, 7)]:
            fixed = (w >> 30) == 0
            out.append(Pdo(w, fixed, ((w >> 10) & 0x3FF) * 50 if fixed else 0, (w & 0x3FF) * 10 if fixed else 0))
        return out


@dataclass
class Slot:
    """One update slot in ``UpdateInfo``."""

    type: int  # SlotType
    len: int
    crc: int
    id: int  # gateware: IP core ID; firmware: version


@dataclass
class UpdateInfo:
    """``nbp_update_info_t`` (136 bytes): ``update()`` / ``update_info()``."""

    LEN = 136

    fw_version: int
    fw_build_id: int
    fw_len: int
    fw_crc: int
    fw_date: str
    boot_version: int
    active: int
    update_slot: int
    fw_pending: int
    result: int
    slot_size: int
    slot: List[Slot] = field(default_factory=list)
    installs: int = 0
    msg: str = ""

    @classmethod
    def parse(cls, p: bytes) -> "UpdateInfo":
        _need(p, cls.LEN, "UpdateInfo")
        fw_version, build, fw_len, fw_crc = struct.unpack_from("<4I", p, 0)
        boot, active, upd, pending, result, slot_size = struct.unpack_from("<I4BI", p, 40)
        slots = [Slot(*struct.unpack_from("<B3xIII", p, o)) for o in (52, 68)]
        return cls(
            fw_version=fw_version,
            fw_build_id=build,
            fw_len=fw_len,
            fw_crc=fw_crc,
            fw_date=_cstr(bytes(p[16:40])),
            boot_version=boot,
            active=active,
            update_slot=upd,
            fw_pending=pending,
            result=result,
            slot_size=slot_size,
            slot=slots,
            installs=struct.unpack_from("<I", p, 84)[0],
            msg=_cstr(bytes(p[88:136])),
        )

    @property
    def fw_version_str(self) -> str:
        return version_str(self.fw_version)


@dataclass
class BlockHeader:
    """``nbp_block_t`` without its words."""

    LEN = 12

    seq: int
    t_us: int
    nwords: int
    flags: int

    @classmethod
    def parse(cls, p: bytes) -> "BlockHeader":
        _need(p, cls.LEN, "BlockHeader")
        return cls(*struct.unpack_from("<IIHH", p, 0))


@dataclass
class Block:
    """A raw DATA block: header and its FPGA words (host order)."""

    header: BlockHeader
    words: List[int]

    @property
    def seq(self) -> int:
        return self.header.seq

    @property
    def flags(self) -> int:
        return self.header.flags


@dataclass
class UsbDevice:
    """A Nebulosa found on USB (``list_usb()``)."""

    bus: int
    address: int
    vid: int
    pid: int
    serial: Optional[str]
    product: Optional[str]
    full_speed: bool  # the USB-C (Full-Speed) interface

    def __str__(self) -> str:
        return "USB %d:%d %04x:%04x %s%s" % (
            self.bus,
            self.address,
            self.vid,
            self.pid,
            "(USB-C, Full-Speed)" if self.full_speed else "(USB 3 port)",
            " serial %s" % self.serial if self.serial else "",
        )


@dataclass
class Discovered:
    """A device that answered UDP discovery (``discover()``)."""

    host: str
    info: Info
    port: int = 20034
