"""FPGA sample stream, format 5 (FPGA ID 0x4E42000B on).

A port of ``spec/neb_stream.h`` / the Companion's ``nbp-core/src/stream.rs``
(same state machine, same counters). Every FIFO word (host order, after
byte-swapping) is one of, by bits 31:29::

    sample  1 d 0   [30] DIRTY  [27:14] current  [13:0] voltage (14-bit codes)
    header  0 1 0   [28:27] type, then:
      BLOCK (0)  [26:11] block seq  [10] OVF  [9] decim mode  [8:5] log2 N
                 [4:2] range  [1] source (0 stub, 1 ADC)
      TS_HI (1)  [23:0] timestamp[47:24]
      TS_LO (2)  [23:0] timestamp[23:0]
    empty   0x00000000           late   0x20000000

A block is BLOCK, TS_HI, TS_LO (empty words may fall between), then 1..256
samples; it ends where the next BLOCK begins. Sample k of a block was taken
at timestamp + k * 2^log2N, in 500 ns ADC periods.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field, fields
from typing import Any, Callable, List, NamedTuple, Optional, Sequence, Tuple

from .constants import BLOCK_HDR_LEN, TS_PERIOD_S, BlockFlag

LATE_WORD = 0x20000000
HDR_BLOCK, HDR_TS_HI, HDR_TS_LO = 0, 1, 2

# what one word was (neb_sw_t)
SKIP, SAMPLE, BLOCK, ERROR = 0, 1, 2, -1


class RawSample(NamedTuple):
    """A decoded sample before calibration (``neb_sample_t`` plus ``step``)."""

    ts: int
    step: int
    current: int
    voltage: int
    range: int
    dirty: bool


class Sample(NamedTuple):
    """A calibrated sample."""

    ts: int
    """ADC periods of 500 ns since FPGA reset."""
    step: int
    """2^log2N: ts distance to the next sample."""
    current_code: int
    voltage_code: int
    range: int
    """0 = highest current range (2 A) .. 5 (20 uA)."""
    dirty: bool
    """The sample may be unreliable (range switching)."""
    current: float
    """Amps."""
    voltage: float
    """Volts."""

    @property
    def time_s(self) -> float:
        """``ts`` in seconds since FPGA reset."""
        return self.ts * TS_PERIOD_S


@dataclass
class Calibration:
    """Code -> physical units.

    ``current = code * i_fs[range] / 16384 + i_offset[range]``,
    ``voltage = code * v_scale + v_offset``.
    """

    i_fs: List[float] = field(default_factory=lambda: [2.0, 0.2, 20e-3, 2e-3, 200e-6, 20e-6, 0.0, 0.0])
    i_offset: List[float] = field(default_factory=lambda: [0.0] * 8)
    v_scale: float = 1e-3
    v_offset: float = 0.0

    def __post_init__(self) -> None:
        self.i_fs = (list(self.i_fs) + [0.0] * 8)[:8]
        self.i_offset = (list(self.i_offset) + [0.0] * 8)[:8]

    def current(self, code: int, range: int) -> float:
        return code * self.i_fs[range] / 16384.0 + self.i_offset[range]

    def voltage(self, code: int) -> float:
        return code * self.v_scale + self.v_offset


@dataclass
class DecoderCounters:
    """``neb_stream_t`` counters."""

    samples: int = 0
    dirty: int = 0
    blocks: int = 0
    empty: int = 0
    late: int = 0
    ovf_blocks: int = 0
    range_changes: int = 0
    seq_errors: int = 0
    malformed: int = 0
    unframed: int = 0
    gap_periods: int = 0


@dataclass
class StreamCounters(DecoderCounters):
    """Decoder counters plus the DATA-block counters (``stream_counters()``)."""

    data_blocks: int = 0
    lost_blocks: int = 0
    stall_flags: int = 0
    gap_flags: int = 0
    test_blocks: int = 0
    resyncs: int = 0

    def as_dict(self) -> dict:
        return {f.name: getattr(self, f.name) for f in fields(self)}


class StreamDecoder:
    """Format-5 word decoder: feed one host-order word at a time."""

    def __init__(self) -> None:
        self.ts0 = 0
        self.ts_next = 0
        self.step = 0
        self.seq = 0
        self.k = 0
        self.range = 0
        self.dmode = 0
        self.log2n = 0
        self.src = 0
        self.ovf = False
        self.hdr_need = 0
        self.synced = False
        self.have_prev = False
        self.prev_seq = 0
        self.prev_ts_next = 0
        self.c = DecoderCounters()

    def desync(self) -> None:
        """Words were lost upstream: drop samples until the next BLOCK header
        and don't count that header's sequence jump again."""
        self.synced = False
        self.have_prev = False

    def feed(self, w: int) -> Tuple[int, Optional[RawSample]]:
        """Decode one word. Returns ``(kind, sample)``: kind is ``SKIP``,
        ``SAMPLE`` (sample set), ``BLOCK`` (a block header completed) or
        ``ERROR`` (malformed, counted)."""
        c = self.c
        if w >> 31:
            if not self.synced:
                c.unframed += 1
                return SKIP, None
            if self.hdr_need:
                c.malformed += 1
                return ERROR, None
            dirty = bool((w >> 30) & 1)
            s = RawSample(self.ts_next, self.step, (w >> 14) & 0x3FFF, w & 0x3FFF, self.range, dirty)
            self.ts_next += self.step
            self.k = (self.k + 1) & 0xFFFF
            c.samples += 1
            c.dirty += dirty
            return SAMPLE, s
        if w >> 29 == 2:
            return self._header(w), None
        if w == 0:
            c.empty += 1
        elif w == LATE_WORD:
            c.late += 1
        else:
            c.malformed += 1
            return ERROR, None
        return SKIP, None

    def _header(self, w: int) -> int:
        c = self.c
        t = (w >> 27) & 3
        if t == HDR_BLOCK:
            if self.synced and self.hdr_need:
                c.malformed += 1  # the last header never completed
            if self.synced:
                self.have_prev = True
                self.prev_seq = self.seq
                self.prev_ts_next = self.ts_next
            seq = (w >> 11) & 0xFFFF
            rng = (w >> 2) & 7
            if self.have_prev and seq != (self.prev_seq + 1) & 0xFFFF:
                c.seq_errors += 1
            if self.synced and rng != self.range:
                c.range_changes += 1
            self.seq = seq
            self.ovf = bool((w >> 10) & 1)
            self.dmode = (w >> 9) & 1
            self.log2n = (w >> 5) & 0xF
            self.range = rng
            self.src = (w >> 1) & 1
            self.step = 1 << self.log2n
            self.k = 0
            self.hdr_need = 2
            self.synced = True
            c.ovf_blocks += self.ovf
            return SKIP
        if t == HDR_TS_HI:
            if not self.synced:
                return SKIP
            if self.hdr_need != 2:
                c.malformed += 1
                return ERROR
            self.ts0 = (w & 0xFFFFFF) << 24
            self.hdr_need = 1
            return SKIP
        if t == HDR_TS_LO:
            if not self.synced:
                return SKIP
            if self.hdr_need != 1:
                c.malformed += 1
                return ERROR
            self.ts0 |= w & 0xFFFFFF
            self.ts_next = self.ts0
            self.hdr_need = 0
            c.blocks += 1
            if self.have_prev:
                if self.ts0 >= self.prev_ts_next:
                    c.gap_periods += self.ts0 - self.prev_ts_next
                else:
                    c.malformed += 1  # overlaps the previous block
            return BLOCK
        c.malformed += 1
        return ERROR

    def run(self, n: int, ndirty: int) -> Optional[Tuple[int, int, int]]:
        """Account for ``n`` consecutive sample words at once (``ndirty`` of
        them DIRTY), exactly as ``n`` calls of ``feed`` would. Returns
        ``(ts_first, step, range)`` when they are samples, ``None`` when they
        are dropped (unframed / malformed)."""
        c = self.c
        if not self.synced:
            c.unframed += n
            return None
        if self.hdr_need:
            c.malformed += n
            return None
        ts = self.ts_next
        self.ts_next += self.step * n
        self.k = (self.k + n) & 0xFFFF
        c.samples += n
        c.dirty += ndirty
        return ts, self.step, self.range


class SampleArrays:
    """Decoded samples as numpy arrays (``read_arrays()``)."""

    __slots__ = ("ts", "step", "current_code", "voltage_code", "range", "dirty", "current", "voltage")

    def __init__(self, ts: Any, step: Any, current_code: Any, voltage_code: Any, range: Any, dirty: Any,
                 current: Any, voltage: Any) -> None:
        self.ts = ts  # uint64
        self.step = step  # uint32
        self.current_code = current_code  # uint16
        self.voltage_code = voltage_code  # uint16
        self.range = range  # uint8
        self.dirty = dirty  # bool
        self.current = current  # float64, A
        self.voltage = voltage  # float64, V

    def __len__(self) -> int:
        return int(self.ts.shape[0])

    def __repr__(self) -> str:
        return "SampleArrays(%d samples)" % len(self)

    @property
    def time_s(self) -> Any:
        """``ts`` in seconds (float64)."""
        return self.ts * TS_PERIOD_S

    @classmethod
    def empty(cls) -> "SampleArrays":
        np = _numpy()
        return cls(np.zeros(0, np.uint64), np.zeros(0, np.uint32), np.zeros(0, np.uint16),
                   np.zeros(0, np.uint16), np.zeros(0, np.uint8), np.zeros(0, bool),
                   np.zeros(0, np.float64), np.zeros(0, np.float64))

    @classmethod
    def concat(cls, parts: Sequence["SampleArrays"]) -> "SampleArrays":
        parts = [p for p in parts if len(p)]
        if not parts:
            return cls.empty()
        if len(parts) == 1:
            return parts[0]
        np = _numpy()
        return cls(*(np.concatenate([getattr(p, n) for p in parts]) for n in cls.__slots__))

    def slice(self, a: int, b: Optional[int] = None) -> "SampleArrays":
        return SampleArrays(*(getattr(self, n)[a:b] for n in self.__slots__))

    def to_samples(self) -> List[Sample]:
        cols = [getattr(self, n).tolist() for n in self.__slots__]
        return [Sample(*row) for row in zip(*cols)]

    @classmethod
    def from_samples(cls, samples: Sequence[Sample]) -> "SampleArrays":
        np = _numpy()
        if not samples:
            return cls.empty()
        cols = list(zip(*samples))
        types = (np.uint64, np.uint32, np.uint16, np.uint16, np.uint8, bool, np.float64, np.float64)
        return cls(*(np.array(c, dtype=t) for c, t in zip(cols, types)))


def _numpy() -> Any:
    try:
        import numpy
    except ImportError:  # pragma: no cover - depends on the environment
        raise ImportError("this needs numpy: pip install 'nebulosa[numpy]'") from None
    return numpy


class StreamProcessor:
    """Turns DATA payloads (``nbp_block_t``) into samples: block flags,
    sequence check, desync, then the word decoder (API.md section 5)."""

    def __init__(self, calibration: Optional[Calibration] = None) -> None:
        self.calibration = calibration or Calibration()
        self.reset()

    def reset(self) -> None:
        """New stream: reset the decoder and the counters."""
        self.decoder = StreamDecoder()
        self.expected: Optional[int] = None
        self.data_blocks = 0
        self.lost_blocks = 0
        self.stall_flags = 0
        self.gap_flags = 0
        self.test_blocks = 0

    def counters(self, resyncs: int = 0) -> StreamCounters:
        d = self.decoder.c
        return StreamCounters(
            **{f.name: getattr(d, f.name) for f in fields(DecoderCounters)},
            data_blocks=self.data_blocks,
            lost_blocks=self.lost_blocks,
            stall_flags=self.stall_flags,
            gap_flags=self.gap_flags,
            test_blocks=self.test_blocks,
            resyncs=resyncs,
        )

    def block_start(self, payload: bytes) -> Optional[Tuple[int, int, int, int]]:
        """Steps 1..4 for one DATA payload. Returns ``(seq, t_us, nwords,
        flags)`` when its words should be decoded, ``None`` when not (test
        block, or too short)."""
        if len(payload) < BLOCK_HDR_LEN:
            return None
        seq, t_us, nwords, flags = struct.unpack_from("<IIHH", payload, 0)
        nwords = min(nwords, (len(payload) - BLOCK_HDR_LEN) // 4)
        if flags & BlockFlag.FIRST:
            self.reset()
        self.data_blocks += 1
        if self.expected is not None and seq != self.expected:
            self.lost_blocks += (seq - self.expected) & 0xFFFFFFFF
            self.decoder.desync()
        self.expected = (seq + 1) & 0xFFFFFFFF
        if flags & BlockFlag.TEST:
            self.test_blocks += 1
            return None
        if flags & (BlockFlag.STALL | BlockFlag.GAP):
            if flags & BlockFlag.STALL:
                self.stall_flags += 1
            if flags & BlockFlag.GAP:
                self.gap_flags += 1
            self.decoder.desync()
        return seq, t_us, nwords, flags

    def decode(self, payload: bytes, out: List[Sample]) -> None:
        """Decode one DATA payload into calibrated ``Sample`` s appended to ``out``."""
        h = self.block_start(payload)
        if h is None:
            return
        nwords = h[2]
        words = struct.unpack_from(">%dI" % nwords, payload, BLOCK_HDR_LEN)
        d = self.decoder
        cal = self.calibration
        i_fs = cal.i_fs
        i_off = cal.i_offset
        vs, vo = cal.v_scale, cal.v_offset
        feed = d.feed
        append = out.append
        c = d.c
        for w in words:
            if w >> 31 and d.synced and not d.hdr_need:
                # inline fast path of feed() for a sample word
                r = d.range
                ic = (w >> 14) & 0x3FFF
                vc = w & 0x3FFF
                dirty = bool((w >> 30) & 1)
                ts = d.ts_next
                d.ts_next = ts + d.step
                d.k = (d.k + 1) & 0xFFFF
                c.samples += 1
                c.dirty += dirty
                append(Sample(ts, d.step, ic, vc, r, dirty, ic * i_fs[r] / 16384.0 + i_off[r], vc * vs + vo))
            else:
                feed(w)

    def decode_arrays(self, payloads: Sequence[bytes]) -> "SampleArrays":
        """Decode DATA payloads with numpy: the non-sample words go through
        the scalar state machine, the runs of sample words between them are
        handled in bulk (``StreamDecoder.run``), which is exact."""
        np = _numpy()
        parts: List[SampleArrays] = []
        batch: List[Any] = []
        for p in payloads:
            # a desync must take effect between the previous words and these
            h_flags = struct.unpack_from("<H", p, 10)[0] if len(p) >= BLOCK_HDR_LEN else 0
            seq = struct.unpack_from("<I", p, 0)[0] if len(p) >= BLOCK_HDR_LEN else 0
            event = (h_flags & (BlockFlag.FIRST | BlockFlag.STALL | BlockFlag.GAP | BlockFlag.TEST)) or (
                self.expected is not None and seq != self.expected)
            if event and batch:
                parts.append(self._decode_words(np, np.concatenate(batch)))
                batch = []
            h = self.block_start(p)
            if h is None:
                continue
            batch.append(np.frombuffer(p, dtype=">u4", count=h[2], offset=BLOCK_HDR_LEN).astype(np.uint32))
        if batch:
            parts.append(self._decode_words(np, np.concatenate(batch)))
        return SampleArrays.concat(parts)

    def _decode_words(self, np: Any, w: Any) -> SampleArrays:
        d = self.decoder
        is_sample = w >= 0x80000000
        other = np.flatnonzero(~is_sample)
        other_words = w[other].tolist()
        dirty_cum = np.concatenate(([0], np.cumsum((w >> 30) & 1 & is_sample)))
        idx_parts: List[Any] = []
        ts_parts: List[Any] = []
        meta: List[Tuple[int, int, int]] = []  # (n, step, range)
        prev = 0
        bounds = other.tolist() + [len(w)]
        ow = other_words + [None]
        for pos, word in zip(bounds, ow):
            n = pos - prev
            if n:
                r = d.run(n, int(dirty_cum[pos] - dirty_cum[prev]))
                if r is not None:
                    ts0, step, rng = r
                    idx_parts.append(np.arange(prev, pos))
                    ts_parts.append(np.uint64(ts0) + np.arange(n, dtype=np.uint64) * np.uint64(step))
                    meta.append((n, step, rng))
            if word is not None:
                d.feed(word)
            prev = pos + 1
        if not idx_parts:
            return SampleArrays.empty()
        idx = np.concatenate(idx_parts)
        ts = np.concatenate(ts_parts)
        counts = [m[0] for m in meta]
        step = np.repeat(np.array([m[1] for m in meta], dtype=np.uint32), counts)
        rng = np.repeat(np.array([m[2] for m in meta], dtype=np.uint8), counts)
        sw = w[idx]
        ic = ((sw >> 14) & 0x3FFF).astype(np.uint16)
        vc = (sw & 0x3FFF).astype(np.uint16)
        dirty = ((sw >> 30) & 1).astype(bool)
        cal = self.calibration
        i_fs = np.asarray(cal.i_fs, dtype=np.float64)
        i_off = np.asarray(cal.i_offset, dtype=np.float64)
        cur = ic * i_fs[rng] / 16384.0 + i_off[rng]
        volt = vc * cal.v_scale + cal.v_offset
        return SampleArrays(ts, step, ic, vc, rng, dirty, cur, volt)


def decode_words(words: Sequence[int], decoder: Optional[StreamDecoder] = None) -> List[RawSample]:
    """Decode host-order words with a (new) ``StreamDecoder``; the samples."""
    d = decoder or StreamDecoder()
    out = []
    for w in words:
        kind, s = d.feed(w)
        if kind == SAMPLE:
            out.append(s)
    return out  # type: ignore[return-value]


SampleCallback = Callable[[List[Sample]], Any]
