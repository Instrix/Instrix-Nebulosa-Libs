import struct

import pytest

import nebulosa as n
from nebulosa.images import bitstream, crc32, firmware_check


def test_crc_is_zlibs():
    assert crc32(b"123456789") == 0xCBF43926


def nbfw(body, board):
    h = bytearray(64)
    h[0:4] = struct.pack("<I", 0x5746424E)
    h[4:6] = struct.pack("<H", 64)
    h[16:20] = struct.pack("<I", 64 + len(body))
    h[20:24] = struct.pack("<I", crc32(body))
    h[24:28] = struct.pack("<I", 0x00020300)
    h[56:60] = struct.pack("<I", board)
    h[60:64] = struct.pack("<I", crc32(bytes(h[:60])))
    return bytes(h) + bytes(body)


def test_firmware_images():
    ok = firmware_check(nbfw(b"\1\2\3\4", n.BOARD_DIGITAL))
    assert ok.version_str == "2.3.0"
    damaged = bytearray(nbfw(b"\1\2\3\4", n.BOARD_DIGITAL))
    damaged[-1] ^= 1
    with pytest.raises(n.ImageError, match="damaged"):
        firmware_check(bytes(damaged))
    with pytest.raises(n.ImageError, match="another board"):
        firmware_check(nbfw(b"\1", 0x1234))
    with pytest.raises(n.ImageError, match="NBFW"):
        firmware_check(b"hello")


def test_bit_and_bin():
    raw = bytes([0xFF, 0xFF, 0xAA, 0x99, 0x55, 0x66, 0x30, 0x01])
    assert bitstream(raw) == raw
    bit = bytes([0x00, 0x09, 0x0F, 0xF0, 0x0F, 0xF0, 0x0F, 0xF0, 0x0F, 0xF0, 0x00, 0x00, 0x01])
    bit += bytes([ord("a"), 0, 1, ord("x"), ord("b"), 0, 1, ord("p"), ord("e")])
    bit += struct.pack(">I", len(raw)) + raw
    assert bitstream(bit) == raw
    with pytest.raises(n.ImageError):
        bitstream(bytes([0, 1, 2, 3]))
