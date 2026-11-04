import random
import struct

import pytest

import nebulosa as n
from nebulosa.stream import BLOCK, ERROR, SAMPLE, SKIP, StreamDecoder, StreamProcessor, decode_words


def block_words(seq, rng, log2n, ts, samples):
    """stream.rs tests::block_words, ported."""
    v = [
        (2 << 29) | (seq << 11) | (log2n << 5) | (rng << 2) | 2,
        (2 << 29) | (1 << 27) | ((ts >> 24) & 0xFFFFFF),
        0,  # empty words may fall between header words
        (2 << 29) | (2 << 27) | (ts & 0xFFFFFF),
    ]
    for i, u, d in samples:
        v.append((1 << 31) | (int(d) << 30) | (i << 14) | u)
    return v


def test_decodes_blocks():
    # stream.rs decodes_blocks, ported
    d = StreamDecoder()
    w = block_words(10, 3, 1, 0x123456789A, [(100, 200, False), (101, 201, True)])
    w += block_words(11, 4, 1, 0x123456789A + 4, [(5, 6, False)])
    s = decode_words(w, d)
    assert len(s) == 3
    assert s[0].ts == 0x123456789A
    assert s[1].ts == 0x123456789A + 2
    assert s[1].dirty
    assert (s[2].range, s[2].current, s[2].voltage) == (4, 5, 6)
    assert d.c.seq_errors == 0
    assert d.c.range_changes == 1
    assert d.c.gap_periods == 0
    assert d.c.malformed == 0
    assert (d.c.samples, d.c.dirty, d.c.blocks, d.c.empty) == (3, 1, 2, 2)


def test_word_kinds():
    d = StreamDecoder()
    assert d.feed(0x80000000)[0] == SKIP and d.c.unframed == 1  # before any header
    w = block_words(0, 0, 0, 0, [(1, 2, False)])
    assert d.feed(w[0])[0] == SKIP
    assert d.feed(0x80000001)[0] == ERROR and d.c.malformed == 1  # sample inside the header
    assert d.feed(w[1])[0] == SKIP
    assert d.feed(w[3])[0] == BLOCK
    k, s = d.feed(w[4])
    assert k == SAMPLE and (s.current, s.voltage) == (1, 2)
    assert d.feed(n.stream.LATE_WORD)[0] == SKIP and d.c.late == 1
    assert d.feed(0x12345678)[0] == ERROR  # neither sample, header, empty nor late
    assert d.feed((2 << 29) | (3 << 27))[0] == ERROR  # header type 3


def test_seq_gap_and_desync():
    d = StreamDecoder()
    w = block_words(1, 2, 0, 100, [(1, 1, False)] * 4)
    w += block_words(3, 2, 0, 110, [(1, 1, False)])  # seq jump, 6 periods missing
    decode_words(w, d)
    assert d.c.seq_errors == 1 and d.c.gap_periods == 6
    d.desync()
    s = decode_words([0x80000000] + block_words(9, 2, 0, 200, [(1, 1, False)]), d)
    assert d.c.seq_errors == 1  # a desync doesn't count the next jump
    assert d.c.unframed == 1 and len(s) == 1
    # a block overlapping the previous one is malformed
    decode_words(block_words(10, 2, 0, 100, [(1, 1, False)]), d)
    assert d.c.malformed == 1


def test_header_never_completed():
    d = StreamDecoder()
    w = block_words(1, 0, 0, 0, [(1, 1, False)])
    decode_words(w[:2] + block_words(2, 0, 0, 5, [(1, 1, False)]), d)
    assert d.c.malformed == 1


def nbp_block(seq, words, flags=0):
    return struct.pack("<IIHH", seq, 0, len(words), flags) + struct.pack(">%dI" % len(words), *words)


def test_processor_flags_and_lost_blocks():
    p = StreamProcessor()
    out = []
    p.decode(nbp_block(0, block_words(0, 1, 2, 0, [(8192, 3300, False)] * 3), n.BlockFlag.FIRST), out)
    assert len(out) == 3
    assert out[1].ts == 4 and out[1].step == 4
    assert out[0].current == pytest.approx(0.1) and out[0].voltage == pytest.approx(3.3)
    assert out[0].time_s == 0.0 and out[1].time_s == pytest.approx(2e-6)
    # block 1 lost: the decoder desyncs, so these samples are unframed
    p.decode(nbp_block(2, [0x80000000 | 5]), out)
    c = p.counters()
    assert (c.lost_blocks, c.unframed, c.data_blocks) == (1, 1, 2)
    p.decode(nbp_block(3, [], n.BlockFlag.STALL | n.BlockFlag.GAP), out)
    p.decode(nbp_block(4, [1, 2, 3], n.BlockFlag.TEST), out)
    c = p.counters()
    assert (c.stall_flags, c.gap_flags, c.test_blocks) == (1, 1, 1)
    assert c.malformed == 0  # test block words are not decoded
    p.decode(nbp_block(0, [], n.BlockFlag.FIRST), out)
    assert p.counters().data_blocks == 1 and p.counters().samples == 0


def test_calibration():
    cal = n.Calibration(i_fs=[1.0] * 6, i_offset=[0.5] * 6, v_scale=2e-3, v_offset=-1)
    assert len(cal.i_fs) == 8
    p = StreamProcessor(cal)
    out = []
    p.decode(nbp_block(0, block_words(0, 0, 0, 0, [(16384 // 2, 1000, False)]), 1), out)
    assert out[0].current == pytest.approx(1.0) and out[0].voltage == pytest.approx(1.0)


def random_stream(rng, nblocks=200):
    """Words as the FPGA would send them, with garbage sprinkled in."""
    words = []
    ts = 1000
    for seq in range(nblocks):
        log2n = rng.randrange(0, 9)
        k = rng.randrange(1, 257)
        samples = [(rng.randrange(16384), rng.randrange(16384), rng.random() < 0.1) for _ in range(k)]
        words += block_words(seq & 0xFFFF, rng.randrange(6), log2n, ts, samples)
        ts += k << log2n
        if rng.random() < 0.2:
            ts += rng.randrange(100)
        r = rng.random()
        if r < 0.05:
            words.append(0x20000000)
        elif r < 0.08:
            words.append(rng.randrange(1 << 32))  # anything
        elif r < 0.10:
            del words[-rng.randrange(1, 4)]
    return words


def chunks(words, size=507):
    return [words[i:i + size] for i in range(0, len(words), size)]


@pytest.mark.parametrize("seed", range(5))
def test_numpy_decode_matches_scalar(seed):
    pytest.importorskip("numpy")
    rng = random.Random(seed)
    words = random_stream(rng)
    payloads = []
    for k, c in enumerate(chunks(words)):
        flags = n.BlockFlag.FIRST if k == 0 else 0
        seq = k + (1 if k > 5 else 0)  # one lost block
        if k == 9:
            flags |= n.BlockFlag.STALL
        payloads.append(nbp_block(seq, c, flags))
    a, b = StreamProcessor(), StreamProcessor()
    out = []
    for p in payloads:
        a.decode(p, out)
    arr = b.decode_arrays(payloads)
    assert len(arr) == len(out) > 0
    assert arr.to_samples() == out
    assert a.counters() == b.counters()
    assert a.counters().malformed > 0 or a.counters().unframed > 0
