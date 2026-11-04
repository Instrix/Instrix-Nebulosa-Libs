"""Update images: CRC-32, gateware bitstreams (.bit / .bin), firmware (.nbfw).

Ports of the Companion's ``nbp-core/src/update.rs`` checks.
"""

from __future__ import annotations

import os
import struct
import zlib
from dataclasses import dataclass
from typing import Union

from .constants import BOARD_DIGITAL
from .errors import ImageError
from .records import version_str

PathOrBytes = Union[str, "os.PathLike[str]", bytes, bytearray, memoryview]

NBFW_MAGIC = 0x5746424E  # "NBFW"
NBFW_HDR_LEN = 64
SYNC_WORD = b"\xAA\x99\x55\x66"


def crc32(data: bytes) -> int:
    """zlib's CRC-32 (what NBP_CMD_UPDATE checks)."""
    return zlib.crc32(bytes(data)) & 0xFFFFFFFF


def load(src: PathOrBytes) -> bytes:
    """Bytes of a file path, or the bytes themselves."""
    if isinstance(src, (bytes, bytearray, memoryview)):
        return bytes(src)
    with open(src, "rb") as f:
        return f.read()


def bitstream(d: bytes) -> bytes:
    """A gateware file to the raw bitstream the FPGA loads: a Xilinx .bit's
    'e' field, or a .bin as it is. Raises ``ImageError`` without a sync word
    (0xAA995566)."""
    d = bytes(d)
    if SYNC_WORD not in d:
        raise ImageError("no sync word (0xAA995566): not a bitstream")
    # .bit: fields 'a'..'d' (16-bit lengths), then 'e' with a 32-bit length
    if len(d) > 13 and d[0] == 0x00 and d[1] == 0x09:
        p = 2 + 9 + 2
        while p < len(d):
            key = d[p]
            p += 1
            if key == ord("e"):
                if p + 4 > len(d):
                    break
                n = struct.unpack_from(">I", d, p)[0]
                p += 4
                if p + n > len(d):
                    break
                return d[p : p + n]
            if p + 2 > len(d):
                break
            p += 2 + ((d[p] << 8) | d[p + 1])
        raise ImageError("malformed .bit header")
    return d


@dataclass
class FirmwareImage:
    """A checked .nbfw image."""

    data: bytes
    version: int
    board: int

    @property
    def version_str(self) -> str:
        return version_str(self.version)


def firmware_check(img: bytes) -> FirmwareImage:
    """Check an .nbfw firmware image (header, CRCs, length, board); raises
    ``ImageError`` saying what is wrong."""
    img = bytes(img)

    def u32(o: int) -> int:
        return struct.unpack_from("<I", img, o)[0]

    if len(img) < NBFW_HDR_LEN or u32(0) != NBFW_MAGIC:
        raise ImageError("not an .nbfw firmware image (no NBFW header)")
    if struct.unpack_from("<H", img, 4)[0] != NBFW_HDR_LEN:
        raise ImageError("unknown header size")
    if crc32(img[:60]) != u32(60):
        raise ImageError("header CRC wrong")
    if u32(16) != len(img):
        raise ImageError("the length in the header doesn't match the file")
    if crc32(img[NBFW_HDR_LEN:]) != u32(20):
        raise ImageError("image CRC wrong: the file is damaged")
    if u32(56) != BOARD_DIGITAL:
        raise ImageError("an image for another board")
    return FirmwareImage(img, u32(24), u32(56))
