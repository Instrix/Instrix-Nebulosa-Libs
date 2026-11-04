import pytest

from nebulosa import FrameParser, encode
from nebulosa.frame import Frame


def feed(stream, chunk):
    p = FrameParser()
    out = []
    for i in range(0, len(stream), chunk):
        p.push(stream[i:i + chunk])
        out.extend(p.frames())
    return p, out


def test_roundtrip_split_and_garbage():
    # stream.rs / frame.rs test, ported
    a = encode(0x41, 7, b"hello")
    b = encode(0x42, 8, b"")
    stream = bytes([0xFF, ord("N"), 0x00]) + a + b
    p, out = feed(stream, 3)
    assert len(out) == 2
    assert out[0] == Frame(0x41, 0, 7, b"hello")
    assert out[1].type == 0x42 and out[1].payload == b""
    assert p.resyncs > 0


@pytest.mark.parametrize("chunk", [1, 2, 5, 7, 64, 4096])
def test_every_split(chunk):
    msgs = [encode(0x40 | k, k, bytes(range(k * 3))) for k in range(1, 20)]
    stream = b"".join(msgs)
    p, out = feed(stream, chunk)
    assert [f.seq for f in out] == list(range(1, 20))
    assert all(f.payload == bytes(range(f.seq * 3)) for f in out)
    assert p.resyncs == 0
    assert p.pending == 0


def test_trailing_n_is_kept():
    p = FrameParser()
    m = encode(0x42, 3, b"xy")
    p.push(b"garbageN")
    assert p.next_frame() is None
    p.push(m[1:])  # the 'N' came with the garbage
    f = p.next_frame()
    assert f is not None and f.seq == 3 and f.payload == b"xy"


def test_len_too_big_resyncs():
    bad = b"NB" + bytes([0x42, 0, 1, 0]) + (2041).to_bytes(2, "little")
    good = encode(0x43, 9, b"ok")
    p, out = feed(bad + good, 1000)
    assert [f.seq for f in out] == [9]
    assert p.resyncs >= 1


def test_data_frame_size_accepted():
    payload = bytes(2040)
    raw = b"NB" + bytes([0x80, 0, 5, 0]) + (2040).to_bytes(2, "little") + payload
    p, out = feed(raw, 300)
    assert len(out) == 1 and out[0].type == 0x80 and len(out[0].payload) == 2040


def test_encode_limits():
    assert encode(1, 0x1_0002, b"") == b"NB\x01\x00\x02\x00\x00\x00"
    with pytest.raises(ValueError):
        encode(1, 0, bytes(505))
