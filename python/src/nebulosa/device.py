"""The ``Nebulosa`` device: commands, streaming, flash and software update."""

from __future__ import annotations

import collections
import struct
import threading
import time
from typing import Any, Callable, Deque, Iterator, List, Optional, Union

from .constants import (
    BLOCK_HDR_LEN,
    DATA,
    FLASH_CHUNK,
    FLASH_ERASE_CHUNK,
    FLASH_SECTOR,
    MAX_PAYLOAD,
    TCP_PORT,
    TEST_ALL,
    BoardSet,
    Cmd,
    DecimMode,
    StartFlag,
    UpdateOp,
    rsp,
)
from .errors import ImageError, LinkClosed, NebulosaError, TimeoutError, VerifyError
from .frame import Frame, encode
from .images import PathOrBytes, bitstream, crc32, firmware_check, load
from .records import (
    Block,
    BlockHeader,
    BoardStatus,
    Decim,
    Info,
    RangeState,
    Stats,
    UpdateInfo,
)
from .stream import Calibration, Sample, SampleArrays, StreamCounters, StreamProcessor, _numpy
from .transport import TcpTransport, Transport, UsbTransport

DEFAULT_TIMEOUT = 1.5
TIMEOUTS = {
    Cmd.BOARD_TEST: 15.0,
    Cmd.FPGA_CONFIG: 10.0,
    Cmd.FLASH_ERASE: 5.0,
    Cmd.UPDATE: 30.0,
}

Progress = Callable[[str, int, int], Any]
"""``progress(stage, done, total)``: stage is ``"erase"``, ``"write"``,
``"verify"`` or ``"install"``."""


class Nebulosa:
    """A connection to one Nebulosa.

    Open it with ``Nebulosa.tcp(host)`` or ``Nebulosa.usb()``; use it as a
    context manager or call ``close()``. One command is in flight at a time;
    the methods are serialised by a lock, so a stream reader thread and a
    command thread may share the object.

    DATA frames that arrive while a command waits for its response are
    queued for the stream reader, never dropped (the queue is unbounded:
    keep reading while streaming).
    """

    def __init__(self, transport: Transport, calibration: Optional[Calibration] = None) -> None:
        self.transport = transport
        self._lock = threading.RLock()
        self._seq = 0
        self._data: Deque[bytes] = collections.deque()  # queued DATA payloads
        self._pend: List[Sample] = []  # decoded samples not yet returned
        self._pend_arrays: List[SampleArrays] = []
        self._proc = StreamProcessor(calibration)
        self._info: Optional[Info] = None
        self.streaming = False
        self.decode = True  # False: the stream is format 4 (raw words only)
        self.discarded = 0  # stale responses discarded

    # ---- opening ---------------------------------------------------------
    @classmethod
    def tcp(cls, host: str, port: int = TCP_PORT, timeout: float = 3.0,
            calibration: Optional[Calibration] = None) -> "Nebulosa":
        """Connect over TCP (Ethernet). The device takes one client at a time:
        close the Companion first."""
        return cls(TcpTransport(host, port, timeout), calibration)

    @classmethod
    def usb(cls, serial: Optional[str] = None, index: int = 0,
            calibration: Optional[Calibration] = None) -> "Nebulosa":
        """Open over USB (needs pyusb). ``serial`` picks a device by serial
        number; else the ``index``-th one found."""
        return cls(UsbTransport(serial, index), calibration)

    def close(self) -> None:
        """Stop a stream this connection started (best effort) and close."""
        with self._lock:
            if self.streaming:
                try:
                    self.stop_stream()
                except NebulosaError:
                    pass
            self.transport.close()

    def __enter__(self) -> "Nebulosa":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    def __repr__(self) -> str:
        return "<Nebulosa %s>" % self.transport.name

    @property
    def calibration(self) -> Calibration:
        """Code -> amps / volts conversion; assign a new ``Calibration`` to replace it."""
        return self._proc.calibration

    @calibration.setter
    def calibration(self, cal: Calibration) -> None:
        self._proc.calibration = cal

    # ---- request / response ----------------------------------------------
    def _queue(self, f: Frame) -> bool:
        if f.type == DATA:
            self._data.append(f.payload)
            return True
        return False

    def request(self, cmd: int, payload: bytes = b"", timeout: Optional[float] = None) -> bytes:
        """Send one command and wait for its response; the response payload.

        Raises ``NebulosaError`` when the status isn't OK (with ``.payload``)
        and ``nebulosa.TimeoutError`` when no response arrives in time.
        """
        if len(payload) > MAX_PAYLOAD:
            raise ValueError("payload too long (%d > %d)" % (len(payload), MAX_PAYLOAD))
        if timeout is None:
            timeout = TIMEOUTS.get(cmd, DEFAULT_TIMEOUT)  # type: ignore[call-overload]
        with self._lock:
            self._seq = (self._seq + 1) & 0xFFFF
            seq = self._seq
            want = rsp(cmd)
            self.transport.write(encode(cmd, seq, payload))
            deadline = time.monotonic() + timeout
            while True:
                left = deadline - time.monotonic()
                if left <= 0:
                    raise TimeoutError("command 0x%02X: no response in %.1f s" % (cmd, timeout), cmd=cmd)
                got: Optional[Frame] = None
                # go through the whole batch: DATA after the response is queued too
                for f in self.transport.poll(min(left, 0.05)):
                    if self._queue(f):
                        continue
                    if got is None and f.type == want and f.seq == seq:
                        got = f
                    else:
                        self.discarded += 1
                if got is not None:
                    if got.status != 0:
                        raise NebulosaError(got.status, cmd=cmd, payload=got.payload)
                    return got.payload

    # ---- general ---------------------------------------------------------
    def ping(self, data: bytes = b"") -> bytes:
        """Echo ``data`` (<= 504 bytes)."""
        return self.request(Cmd.PING, bytes(data))

    def get_info(self) -> Info:
        """Device / firmware / link information."""
        info = Info.parse(self.request(Cmd.GET_INFO))
        self._info = info
        return info

    @property
    def info(self) -> Info:
        """``get_info()``, cached after the first call."""
        if self._info is None:
            return self.get_info()
        return self._info

    def reg_read(self, index: int) -> int:
        """Read FPGA register ``index``."""
        p = self.request(Cmd.REG_READ, struct.pack("<B3xI", index, 0))
        if len(p) < 8:
            raise NebulosaError(None, "REG_READ: short response", cmd=Cmd.REG_READ, payload=p)
        return struct.unpack_from("<I", p, 4)[0]

    def reg_write(self, index: int, value: int) -> None:
        """Write FPGA register ``index``."""
        self.request(Cmd.REG_WRITE, struct.pack("<B3xI", index, value & 0xFFFFFFFF))

    def reset(self) -> None:
        """Restart the device (~50 ms after the response; the connection drops)."""
        self.request(Cmd.RESET)
        self.streaming = False

    # ---- stream control --------------------------------------------------
    def start_stream(self, flush: bool = True, link_test: bool = False) -> None:
        """Start DATA on this link. Clears what was queued from a previous stream."""
        with self._lock:
            if self._info is None:
                try:
                    self.get_info()
                except NebulosaError:
                    pass
            if self._info is not None:
                self.decode = self._info.stream_format == 5
            self._data.clear()
            self._pend = []
            self._pend_arrays = []
            self._proc.reset()
            flags = (StartFlag.FLUSH if flush else 0) | (StartFlag.LINK_TEST if link_test else 0)
            self.request(Cmd.STREAM_START, struct.pack("<I", int(flags)))
            self.streaming = True

    def stop_stream(self) -> None:
        """Stop the stream. DATA already in flight is still queued and readable."""
        self.request(Cmd.STREAM_STOP)
        self.streaming = False

    def get_stats(self) -> Stats:
        return Stats.parse(self.request(Cmd.GET_STATS))

    def set_source(self, src: int) -> None:
        """``Source.STUB_RANDOM`` / ``STUB_PATTERN`` / ``AD7387``."""
        self.request(Cmd.SET_SOURCE, struct.pack("<B3x", src))

    def set_range_auto(self, min: int = 0, max: int = 5) -> None:
        """Automatic ranging between ranges ``min`` (2 A) and ``max`` (20 uA)."""
        self.request(Cmd.SET_RANGE, bytes([1, 0, min, max]))

    def set_range_manual(self, range: int) -> None:
        """Fixed range 0 (2 A) .. 5 (20 uA)."""
        self.request(Cmd.SET_RANGE, bytes([0, range, 0, 5]))

    def get_range(self) -> RangeState:
        return RangeState.parse(self.request(Cmd.GET_RANGE))

    def set_range_timing(self, up_filt: int, down_filt: int, overlap: int, hold: int) -> None:
        """Range switch timing, in FPGA clock cycles (``reg_read(REG_CLK_HZ)``)."""
        self.request(Cmd.SET_RANGE_TIMING, struct.pack("<BBBxHH", up_filt, down_filt, overlap, hold, 0))

    def set_range_force(self, enable: bool, value: int = 0) -> None:
        """Force the range comparators: ``value`` bits {down_c, up_c, down_b, up_b, down_a, up_a}."""
        self.request(Cmd.SET_RANGE_FORCE, struct.pack("<BB2x", int(bool(enable)), value))

    def set_decimation(self, log2n: int, mode: int = DecimMode.AVERAGE) -> Decim:
        """Decimate by N = 2^log2n (0..8); the setting as applied."""
        return Decim.parse(self.request(Cmd.SET_DECIMATION, struct.pack("<BB2xI", log2n, mode, 0)))

    # ---- board -----------------------------------------------------------
    def board_status(self) -> BoardStatus:
        return BoardStatus.parse(self.request(Cmd.BOARD_STATUS))

    def board_set(self, item: int, value: int) -> BoardStatus:
        """Change a board setting (``BoardSet``); the status after the change."""
        return BoardStatus.parse(self.request(Cmd.BOARD_SET, struct.pack("<B3xI", item, value & 0xFFFFFFFF)))

    def board_test(self, mask: int = TEST_ALL) -> BoardStatus:
        """Run board self-tests (bits of ``Test``); see ``BoardStatus.test_result``."""
        return BoardStatus.parse(self.request(Cmd.BOARD_TEST, struct.pack("<I", mask & TEST_ALL)))

    def set_leds(self, mask: int) -> BoardStatus:
        """Set LED0..4 manually (bit n = LEDn)."""
        return self.board_set(BoardSet.LEDS, (mask & 0x1F) | 0x100)

    def leds_auto(self) -> BoardStatus:
        """LEDs back to the firmware's status pattern."""
        return self.board_set(BoardSet.LEDS, 0)

    def ui_key(self, key: int) -> BoardStatus:
        """Press a key (``Key``) on the on-board UI."""
        return self.board_set(BoardSet.UI_KEY, key)

    # ---- flash -----------------------------------------------------------
    def flash_read(self, addr: int, length: int) -> bytes:
        """Read ``length`` (<= 256) bytes of the H417's SPI flash."""
        if not 0 <= length <= FLASH_CHUNK:
            raise ValueError("flash_read: len must be 0..256")
        p = self.request(Cmd.FLASH_READ, struct.pack("<II", addr, length))
        if len(p) < 8 + length:
            raise NebulosaError(None, "FLASH_READ: short response", cmd=Cmd.FLASH_READ, payload=p)
        return p[8 : 8 + length]

    def flash_write(self, addr: int, data: bytes) -> None:
        """Program ``data`` within one 256-byte page (erase first)."""
        data = bytes(data)
        if len(data) > FLASH_CHUNK or (addr & 0xFF) + len(data) > FLASH_CHUNK:
            raise ValueError("flash_write: data must lie within one 256-byte page")
        self.request(Cmd.FLASH_WRITE, struct.pack("<II", addr, len(data)) + data)

    def flash_erase(self, addr: int, length: int, timeout: Optional[float] = None) -> None:
        """Erase ``length`` bytes at ``addr`` (both 4 KB aligned)."""
        if addr % FLASH_SECTOR or length % FLASH_SECTOR:
            raise ValueError("flash_erase: addr and len must be 4 KB aligned")
        self.request(Cmd.FLASH_ERASE, struct.pack("<II", addr, length), timeout)

    def fpga_config(self, addr: int, length: int, flags: int = 0) -> BoardStatus:
        """Load the FPGA from a bitstream in the flash; the status after."""
        return BoardStatus.parse(self.request(Cmd.FPGA_CONFIG, struct.pack("<III", addr, length, flags)))

    def upload_flash(self, addr: int, data: bytes, progress: Optional[Progress] = None,
                     verify: bool = True) -> None:
        """Erase, write and verify ``data`` at ``addr`` (4 KB aligned).

        Erases in 64 KB commands (the last one shorter, a 4 KB multiple),
        writes 256-byte pages (pages that are all 0xFF are skipped: erased
        flash already reads so), then reads everything back and compares
        (``VerifyError`` on a mismatch).
        """
        data = bytes(data)
        if addr % FLASH_SECTOR:
            raise ValueError("upload_flash: addr must be 4 KB aligned")
        n = len(data)
        end = addr + -(-n // FLASH_SECTOR) * FLASH_SECTOR
        cb = progress or (lambda s, d, t: None)
        a = addr
        while a < end:
            ln = min(FLASH_ERASE_CHUNK, end - a)
            self.flash_erase(a, ln, timeout=5.0)
            a += ln
            cb("erase", a - addr, end - addr)
        for off in range(0, n, FLASH_CHUNK):
            chunk = data[off : off + FLASH_CHUNK]
            if chunk.count(0xFF) != len(chunk):
                self.flash_write(addr + off, chunk)
            cb("write", min(off + FLASH_CHUNK, n), n)
        if verify:
            for off in range(0, n, FLASH_CHUNK):
                want = data[off : off + FLASH_CHUNK]
                got = self.flash_read(addr + off, len(want))
                if got != want:
                    k = next(i for i in range(len(want)) if got[i] != want[i])
                    raise VerifyError("verify: byte 0x%X reads 0x%02X not 0x%02X"
                                      % (addr + off + k, got[k], want[k]), addr + off + k)
                cb("verify", min(off + FLASH_CHUNK, n), n)

    # ---- software update -------------------------------------------------
    def update(self, op: int, slot: int = 0, length: int = 0, crc: int = 0,
               timeout: Optional[float] = None) -> UpdateInfo:
        """NBP_CMD_UPDATE. When the device answers an error status, the raised
        ``NebulosaError`` has the ``UpdateInfo`` it sent in ``.info`` and its
        ``msg`` in the message."""
        try:
            p = self.request(Cmd.UPDATE, struct.pack("<BB2xII", op, slot, length, crc), timeout)
        except NebulosaError as e:
            if e.status is not None and len(e.payload) >= UpdateInfo.LEN:
                info = UpdateInfo.parse(e.payload)
                e.info = info
                if info.msg:
                    e.args = ("%s: %s" % (e.args[0], info.msg),)
            raise
        return UpdateInfo.parse(p)

    def update_info(self) -> UpdateInfo:
        """What runs and what is in the update slots."""
        return self.update(UpdateOp.INFO, timeout=DEFAULT_TIMEOUT)

    def _install(self, op: int, image: bytes, progress: Optional[Progress], timeout: float) -> UpdateInfo:
        info = self.update_info()
        if not image or len(image) > info.slot_size:
            raise ImageError("the image is %d bytes; the slot holds %d" % (len(image), info.slot_size))
        slot = info.update_slot
        self.upload_flash(slot * info.slot_size, image, progress)
        if progress:
            progress("install", 0, 1)
        r = self.update(op, slot, len(image), crc32(image), timeout=timeout)
        if r.result != 0:
            raise NebulosaError(r.result, "update failed: %s" % (r.msg or r.result), cmd=Cmd.UPDATE, info=r)
        if progress:
            progress("install", 1, 1)
        return r

    def update_gateware(self, src: PathOrBytes, progress: Optional[Progress] = None) -> UpdateInfo:
        """Install a gateware (.bit or .bin, path or bytes): written to the
        update slot, then tested on the FPGA and made active (else the old
        one is kept and ``NebulosaError`` raised)."""
        return self._install(UpdateOp.GATEWARE, bitstream(load(src)), progress, 15.0)

    def update_firmware(self, src: PathOrBytes, progress: Optional[Progress] = None) -> UpdateInfo:
        """Install a firmware image (.nbfw, path or bytes): checked here,
        written to the update slot, checked by the device, which then
        restarts (~0.5 s) and its bootloader installs it. The connection
        drops; reconnect and ``update_info()`` shows the new version."""
        img = firmware_check(load(src)).data
        r = self._install(UpdateOp.FIRMWARE, img, progress, 10.0)
        self.streaming = False
        return r

    def update_revert(self) -> UpdateInfo:
        """Make the previous gateware (the other slot) active again."""
        return self.update(UpdateOp.REVERT, timeout=15.0)

    # ---- stream reading --------------------------------------------------
    def _fill(self, timeout: float) -> bool:
        """Pull frames from the transport into the DATA queue; True if any DATA came."""
        got = False
        for f in self.transport.poll(timeout):
            if self._queue(f):
                got = True
            else:
                self.discarded += 1
        return got

    def _check_decode(self) -> None:
        if not self.decode:
            raise NebulosaError(None, "this FPGA (ID 0x%08X) streams format 4, which isn't decoded: "
                                "use read_blocks()" % (self._info.fpga_id if self._info else 0))

    def read_samples(self, max: int = 65536, timeout: float = 1.0) -> List[Sample]:
        """Samples available now (up to ``max``): queued DATA first, then the
        link, waiting up to ``timeout`` seconds for at least one. An empty
        list means none arrived in time."""
        self._check_decode()
        with self._lock:
            out: List[Sample] = []
            if self._pend_arrays:
                for a in self._pend_arrays:
                    self._pend.extend(a.to_samples())
                self._pend_arrays = []
            if self._pend:
                out, self._pend = self._pend[:max], self._pend[max:]
                if len(out) >= max:
                    return out
            deadline = time.monotonic() + timeout
            while True:
                while self._data and len(out) < max:
                    self._proc.decode(self._data.popleft(), out)
                if len(out) >= max:
                    self._pend = out[max:] + self._pend
                    return out[:max]
                if out and not self._data:
                    # take what else is already there without waiting
                    if not self._fill(0.0):
                        return out
                    continue
                left = deadline - time.monotonic()
                if left <= 0:
                    return out
                self._fill(min(left, 0.05))

    def samples(self, timeout: float = 1.0) -> Iterator[Sample]:
        """Iterate over samples while streaming; stops when none arrive
        within ``timeout`` seconds."""
        while True:
            batch = self.read_samples(timeout=timeout)
            if not batch:
                return
            for s in batch:
                yield s

    def read_arrays(self, max: int = 1 << 20, timeout: float = 1.0) -> SampleArrays:
        """Like ``read_samples`` but numpy arrays (vectorised decode; needs numpy)."""
        _numpy()
        self._check_decode()
        with self._lock:
            parts: List[SampleArrays] = list(self._pend_arrays)
            self._pend_arrays = []
            if self._pend:
                parts.insert(0, SampleArrays.from_samples(self._pend))
                self._pend = []
            have = sum(len(p) for p in parts)
            deadline = time.monotonic() + timeout
            while have < max:
                if self._data:
                    payloads = []
                    words = 0
                    while self._data and have + words < max:
                        payloads.append(self._data.popleft())
                        words += 507
                    a = self._proc.decode_arrays(payloads)
                    parts.append(a)
                    have += len(a)
                    continue
                if have:
                    if not self._fill(0.0):
                        break
                    continue
                left = deadline - time.monotonic()
                if left <= 0:
                    break
                self._fill(min(left, 0.05))
            a = SampleArrays.concat(parts)
            if len(a) > max:
                self._pend_arrays = [a.slice(max)]
                a = a.slice(0, max)
            return a

    def capture(self, seconds: float, numpy: bool = True) -> Union[SampleArrays, List[Sample]]:
        """Read the running stream for ``seconds`` of wall time; all samples
        (``SampleArrays`` with numpy, else a list of ``Sample``)."""
        end = time.monotonic() + seconds
        if numpy:
            parts = []
            while True:
                left = end - time.monotonic()
                if left <= 0:
                    break
                parts.append(self.read_arrays(timeout=min(left, 0.2)))
            return SampleArrays.concat(parts)
        out: List[Sample] = []
        while True:
            left = end - time.monotonic()
            if left <= 0:
                break
            out.extend(self.read_samples(timeout=min(left, 0.2)))
        return out

    def read_blocks(self, max: int = 64, timeout: float = 1.0) -> List[Block]:
        """Raw DATA blocks (header + host-order FPGA words), bypassing the
        sample decoder: for format-4 FPGAs, or to do your own decoding.
        Block counters (lost / stall / gap / test) are still kept. Don't mix
        with ``read_samples`` on the same stream: blocks go to one or the other."""
        with self._lock:
            out: List[Block] = []
            deadline = time.monotonic() + timeout
            while True:
                while self._data and len(out) < max:
                    p = self._data.popleft()
                    if len(p) < BLOCK_HDR_LEN:
                        continue
                    h = BlockHeader.parse(p)
                    self._proc.block_start(p)
                    n = min(h.nwords, (len(p) - BLOCK_HDR_LEN) // 4)
                    out.append(Block(h, list(struct.unpack_from(">%dI" % n, p, BLOCK_HDR_LEN))))
                if len(out) >= max:
                    return out
                if out:
                    if not self._fill(0.0):
                        return out
                    continue
                left = deadline - time.monotonic()
                if left <= 0:
                    return out
                self._fill(min(left, 0.05))

    def stream_counters(self) -> StreamCounters:
        """Decoder counters plus data_blocks, lost_blocks, stall_flags,
        gap_flags, test_blocks and the framing resyncs."""
        return self._proc.counters(self.transport.resyncs)

    @property
    def queued_blocks(self) -> int:
        """DATA blocks received but not yet decoded."""
        return len(self._data)


__all__ = ["Nebulosa", "Progress", "DEFAULT_TIMEOUT", "TIMEOUTS", "LinkClosed"]
