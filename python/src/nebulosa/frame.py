"""NBP message framing: the 8-byte header and a resyncing stream parser."""

from __future__ import annotations

import struct
from typing import Iterator, NamedTuple, Optional

from .constants import HDR_LEN, MAGIC, MAX_LEN, MAX_PAYLOAD

_HDR = struct.Struct("<HBBHH")


class Frame(NamedTuple):
    """One NBP message."""

    type: int
    status: int
    seq: int
    payload: bytes


def encode(type: int, seq: int, payload: bytes = b"", status: int = 0) -> bytes:
    """Encode one message (header + payload)."""
    if len(payload) > MAX_PAYLOAD:
        raise ValueError("NBP payload too long (%d > %d)" % (len(payload), MAX_PAYLOAD))
    return _HDR.pack(MAGIC, type & 0xFF, status & 0xFF, seq & 0xFFFF, len(payload)) + bytes(payload)


class FrameParser:
    """Cuts one direction's byte stream into messages.

    Parse by ``len``; transfer boundaries mean nothing. Garbage is skipped up
    to the next ``"NB"`` (a trailing ``'N'`` is kept) and counted in
    ``resyncs``; a header with ``len > 2040`` drops one byte and resyncs.
    """

    __slots__ = ("_buf", "_pos", "resyncs")

    def __init__(self) -> None:
        self._buf = bytearray()
        self._pos = 0
        self.resyncs = 0

    def push(self, data: bytes) -> None:
        """Append received bytes."""
        if self._pos and self._pos * 2 >= len(self._buf):
            del self._buf[: self._pos]
            self._pos = 0
        self._buf += data

    def clear(self) -> None:
        """Drop buffered bytes."""
        self._buf.clear()
        self._pos = 0

    @property
    def pending(self) -> int:
        """Bytes buffered but not yet a complete message."""
        return len(self._buf) - self._pos

    def next_frame(self) -> Optional[Frame]:
        """The next complete message, or ``None`` when more bytes are needed."""
        buf = self._buf
        while True:
            pos = self._pos
            n = len(buf) - pos
            if n < 2:
                return None
            if buf[pos] != 0x4E or buf[pos + 1] != 0x42:
                self.resyncs += 1
                i = buf.find(b"NB", pos + 1)
                if i >= 0:
                    self._pos = i
                elif buf[-1] == 0x4E:
                    self._pos = len(buf) - 1
                else:
                    self._pos = len(buf)
                continue
            if n < HDR_LEN:
                return None
            _, ty, st, seq, ln = _HDR.unpack_from(buf, pos)
            if ln > MAX_LEN:
                self.resyncs += 1
                self._pos = pos + 1
                continue
            if n < HDR_LEN + ln:
                return None
            payload = bytes(buf[pos + HDR_LEN : pos + HDR_LEN + ln])
            self._pos = pos + HDR_LEN + ln
            return Frame(ty, st, seq, payload)

    def frames(self) -> Iterator[Frame]:
        """Every complete message buffered."""
        while True:
            f = self.next_frame()
            if f is None:
                return
            yield f
