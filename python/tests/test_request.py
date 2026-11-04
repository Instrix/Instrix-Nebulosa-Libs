"""request() against a scripted transport: DATA queued, stale responses
discarded, timeouts."""

import struct

import pytest

import nebulosa as n
from nebulosa.frame import Frame, FrameParser
from nebulosa.transport import Transport


class Fake(Transport):
    name = "fake"

    def __init__(self, script):
        self.script = script  # callable(cmd, seq, payload) -> list of batches of frames
        self.batches = []
        self.parser = FrameParser()

    def write(self, data):
        self.parser.push(data)
        for f in self.parser.frames():
            self.batches.extend(self.script(f.type, f.seq, f.payload))

    def poll(self, timeout):
        return self.batches.pop(0) if self.batches else []

    def close(self):
        pass


def data(seq):
    return Frame(0x80, 0, seq & 0xFFFF, struct.pack("<IIHH", seq, 0, 0, 1 if seq == 0 else 0))


def test_data_kept_stale_dropped():
    def script(cmd, seq, p):
        return [[data(0), Frame(cmd | 0x40, 0, (seq - 1) & 0xFFFF, b"old")],
                [data(1), Frame(cmd | 0x40, 0, seq, p), data(2)]]

    d = n.Nebulosa(Fake(script))
    assert d.ping(b"abc") == b"abc"
    assert d.queued_blocks == 3
    assert d.discarded == 1
    assert d.read_samples(timeout=0) == []
    assert d.stream_counters().data_blocks == 3
    assert d.stream_counters().lost_blocks == 0


def test_status_error_and_timeout():
    d = n.Nebulosa(Fake(lambda c, s, p: [[Frame(c | 0x40, n.Status.BUSY, s, b"")]] if c != 0x12 else []))
    with pytest.raises(n.NebulosaError) as e:
        d.stop_stream()
    assert e.value.status == n.Status.BUSY and e.value.name == "BUSY"
    with pytest.raises(n.TimeoutError):
        d.request(0x12, timeout=0.1)
    with pytest.raises(TimeoutError):  # the built-in one too
        d.request(0x12, timeout=0.05)


def test_update_error_carries_info():
    info = bytearray(136)
    info[47] = n.Status.HW
    info[88:100] = b"DONE stuck 0"

    d = n.Nebulosa(Fake(lambda c, s, p: [[Frame(c | 0x40, n.Status.HW, s, bytes(info))]]))
    with pytest.raises(n.NebulosaError) as e:
        d.update(n.UpdateOp.GATEWARE, 1, 10, 0)
    assert e.value.info.msg == "DONE stuck 0" and "DONE stuck 0" in str(e.value)
