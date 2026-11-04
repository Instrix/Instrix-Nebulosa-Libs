#!/usr/bin/env python3
"""
Fake Nebulosa device: speaks NBP (spec/nbp.h) over TCP and answers UDP
discovery, so the libraries and your scripts can be tested without hardware.

    python3 tools/nebulosa_sim.py                # TCP 20034, UDP 20035
    python3 tools/nebulosa_sim.py --port 21000 --no-udp

The stream is FPGA format 5 (spec/neb_stream.h): the current is a slow
log-sweep from ~2 uA to ~1 A (so the auto range walks through every range),
the voltage a ramp, decimation honoured. Throughput is capped by Python
(a few hundred thousand samples/s); the timestamps stay consistent.

Standard library only.
"""

import argparse
import math
import select
import socket
import struct
import threading
import time

MAGIC = 0x424E
HDR = struct.Struct("<HBBHH")
DATA = 0x80
BLOCK_WORDS = 507
FPGA_ID = 0x4E42000B
BASE_RATE = 2_000_000
I_FS = [2.0, 0.2, 20e-3, 2e-3, 200e-6, 20e-6]

ST_OK, ST_UNKNOWN, ST_BAD_LEN, ST_BAD_ARG, ST_BUSY, ST_HW, ST_PROTECTED = 0, 1, 2, 3, 5, 6, 8
SLOT_SIZE = 448 * 1024


def frame(ty, seq, payload=b"", status=0):
    return HDR.pack(MAGIC, ty, status, seq & 0xFFFF, len(payload)) + payload


def crc32(b):
    import zlib
    return zlib.crc32(b) & 0xFFFFFFFF


class Device:
    def __init__(self, link=3):
        self.t0 = time.monotonic()
        self.link = link
        self.regs = [0] * 45
        self.regs[0] = FPGA_ID
        self.regs[41] = 100_000_000
        self.source = 2
        self.range_auto = True
        self.range_manual = 0
        self.range_min, self.range_max = 0, 5
        self.log2n, self.dmode = 0, 0
        self.flash = bytearray(b"\xff" * (1 << 20))
        self.leds, self.led_manual = 0, 0
        self.rtc = 0
        self.streaming = False
        self.ts = 0  # FPGA timestamp: counts since "FPGA reset", across streams
        self.stats_blocks = 0
        self.stats_words = 0
        self.reset_stream_state()

    def uptime_ms(self):
        return int((time.monotonic() - self.t0) * 1000) & 0xFFFFFFFF

    # ---- stream generation ------------------------------------------------
    def reset_stream_state(self):
        self.nbp_seq = 0
        self.fpga_seq = 0
        self.ts_base = self.ts
        self.pending_words = []
        self.first = True
        self.stream_t0 = time.monotonic()
        self.cur_range = None

    def sample_at(self, ts):
        t = ts * 500e-9
        # log sweep 2 uA .. 1 A, period 4 s, plus a 1 kHz ripple
        lg = -5.7 + 5.7 * (0.5 + 0.5 * math.sin(2 * math.pi * t / 4.0))
        i = 10 ** lg * (1 + 0.05 * math.sin(2 * math.pi * 1000 * t))
        v = 3.3 + 0.2 * math.sin(2 * math.pi * 0.5 * t)
        if self.range_auto:
            r = self.range_min
            for k in range(self.range_max, self.range_min - 1, -1):
                if i < 0.9 * I_FS[k]:
                    r = k
                    break
        else:
            r = self.range_manual
        ic = max(0, min(16383, int(round(i / I_FS[r] * 16384))))
        vc = max(0, min(16383, int(round(v / 1e-3))))
        return r, ic, vc

    def make_fpga_block(self):
        """One FPGA block (header + up to 256 samples of one range)."""
        step = 1 << self.log2n
        r0, _, _ = self.sample_at(self.ts)
        samples = []
        ts = self.ts
        while len(samples) < 256:
            r, ic, vc = self.sample_at(ts)
            if r != r0:
                break
            dirty = 1 if (len(samples) < 2 and self.cur_range is not None and r0 != self.cur_range) else 0
            samples.append((1 << 31) | (dirty << 30) | (ic << 14) | vc)
            ts += step
        src = 1 if self.source == 2 else 0
        hdr = ((2 << 29) | ((self.fpga_seq & 0xFFFF) << 11) | (self.dmode << 9)
               | (self.log2n << 5) | (r0 << 2) | (src << 1))
        words = [hdr,
                 (2 << 29) | (1 << 27) | ((self.ts >> 24) & 0xFFFFFF),
                 0,  # an empty word may fall between header words
                 (2 << 29) | (2 << 27) | (self.ts & 0xFFFFFF)]
        words += samples
        self.fpga_seq += 1
        self.cur_range = r0
        self.ts = ts
        return words

    def due(self):
        """How far ahead of real time the stream is allowed to be."""
        return (time.monotonic() - self.stream_t0) / 500e-9 - (self.ts - self.ts_base)

    def next_data_frames(self, max_frames=8):
        out = []
        while len(out) < max_frames and self.due() > 0:
            while len(self.pending_words) < BLOCK_WORDS:
                self.pending_words += self.make_fpga_block()
            words, self.pending_words = self.pending_words[:BLOCK_WORDS], self.pending_words[BLOCK_WORDS:]
            flags = 1 if self.first else 0
            self.first = False
            t_us = int((time.monotonic() - self.t0) * 1e6) & 0xFFFFFFFF
            body = struct.pack("<IIHH", self.nbp_seq, t_us, len(words), flags)
            body += struct.pack(">%dI" % len(words), *words)
            body += b"\0" * (2040 - len(body))
            out.append(frame(DATA, self.nbp_seq, body))
            self.nbp_seq = (self.nbp_seq + 1) & 0xFFFFFFFF
            self.stats_blocks += 1
            self.stats_words += len(words)
        return out

    # ---- payloads ---------------------------------------------------------
    def info(self):
        build = b"sim " + time.strftime("%Y-%m-%d %H:%M:%S").encode()
        return struct.pack("<8I32s6s2sI", 1, (1 << 16) | (4 << 8) | 0, FPGA_ID, BASE_RATE,
                           self.link, 1460 if self.link == 3 else 512, BLOCK_WORDS,
                           0b111111, build, bytes([0x02, 0x4E, 0x42, 0, 0, 1]), b"\0\0",
                           0x0100007F if self.link == 3 else 0)

    def stats(self):
        return struct.pack("<16I", self.uptime_ms(), self.link if self.streaming else 0,
                           self.stats_blocks, self.stats_words, 0, 0, 0, self.cur_range or 0,
                           (self.dmode << 4) | self.log2n, self.source, 0b111, 0x0100007F,
                           0, 0, 0, 0)

    def board_status(self):
        b = bytearray(200)
        struct.pack_into("<6I", b, 0, 0x4E440001, self.uptime_ms(), 100_000_000, 100_000_000,
                         # HSE LSE RTC LCD FPGA_DONE FPGA_INIT_B ETH_LINK USB_C APWR_OK
                         (1 << 0) | (1 << 1) | (1 << 2) | (1 << 10) | (1 << 11) | (1 << 14) | (1 << 15)
                         | (1 << 19) | (1 << 20), self.rtc)
        struct.pack_into("<3H", b, 24, 12050, 5020, 11900)
        struct.pack_into("<7B", b, 32, self.leds, self.led_manual, 0, 0b0101, 1, 1, 3)
        struct.pack_into("<7I", b, 40, 120, 0, FPGA_ID, 0xEF4014, 1 << 20, 0, 0)
        struct.pack_into("<I", b, 80, 0x555555)  # all 12 tests pass
        struct.pack_into("<BBBBHHHB", b, 132, 4, 1, 2, 3, 12000, 3000, 12000, 3)
        # fixed PDOs: [19:10] 50 mV, [9:0] 10 mA: 5 V 3 A, 9 V 3 A, 12 V 3 A
        struct.pack_into("<3I", b, 144, *[((mv // 50) << 10) | 300 for mv in (5000, 9000, 12000)])
        return bytes(b)

    def update_info(self, result=0, msg=b"ok"):
        b = bytearray(136)
        struct.pack_into("<4I24sI4BI", b, 0, (1 << 16) | (4 << 8), 0xDEADBEEF, 200_000, 0x12345678,
                         b"2026-11-03", 0x10000, 0, 1, 0, result, SLOT_SIZE)
        struct.pack_into("<B3xIII", b, 52, 1, 340_000, 0xCAFEF00D, FPGA_ID)
        struct.pack_into("<I48s", b, 84, 0, msg)
        return bytes(b)

    # ---- commands ---------------------------------------------------------
    def handle(self, ty, seq, p):
        """Returns (status, payload) or None for no response."""
        n = len(p)

        def need(k):
            return n == k

        if ty == 0x01:
            return ST_OK, p
        if ty == 0x02:
            return (ST_OK, self.info()) if need(0) else (ST_BAD_LEN, b"")
        if ty == 0x03:
            if not need(8):
                return ST_BAD_LEN, b""
            i = p[0]
            if i >= len(self.regs):
                return ST_BAD_ARG, b""
            return ST_OK, struct.pack("<B3xI", i, self.regs[i])
        if ty == 0x04:
            if not need(8):
                return ST_BAD_LEN, b""
            i, v = struct.unpack("<B3xI", p)
            if i >= len(self.regs):
                return ST_BAD_ARG, b""
            if i not in (0, 41):
                self.regs[i] = v
            return ST_OK, b""
        if ty == 0x05:
            return ST_OK, b""
        if ty == 0x10:
            if not need(4):
                return ST_BAD_LEN, b""
            self.reset_stream_state()
            self.streaming = True
            self.stats_blocks = self.stats_words = 0
            return ST_OK, b""
        if ty == 0x11:
            self.streaming = False
            return ST_OK, b""
        if ty == 0x12:
            return ST_OK, self.stats()
        if ty == 0x20:
            if not need(4):
                return ST_BAD_LEN, b""
            if p[0] > 2:
                return ST_BAD_ARG, b""
            self.source = p[0]
            return ST_OK, b""
        if ty == 0x21:
            if not need(4):
                return ST_BAD_LEN, b""
            a, m, lo, hi = p
            if m > 5 or lo > 5 or hi > 5 or lo > hi:
                return ST_BAD_ARG, b""
            self.range_auto, self.range_manual, self.range_min, self.range_max = bool(a), m, lo, hi
            return ST_OK, b""
        if ty == 0x22:
            return ST_OK, struct.pack("<4I", self.cur_range or 0, 0, int(self.range_auto), 0)
        if ty in (0x23, 0x24):
            return (ST_OK, b"") if need(8 if ty == 0x23 else 4) else (ST_BAD_LEN, b"")
        if ty == 0x30:
            if not need(8):
                return ST_BAD_LEN, b""
            if p[0] > 8 or p[1] > 1:
                return ST_BAD_ARG, b""
            self.log2n, self.dmode = p[0], p[1]
            return ST_OK, struct.pack("<BB2xI", self.log2n, self.dmode, BASE_RATE >> self.log2n)
        if ty == 0x38:
            return ST_OK, self.board_status()
        if ty == 0x39:
            if not need(8):
                return ST_BAD_LEN, b""
            item, v = struct.unpack("<B3xI", p)
            if item == 0:
                self.leds, self.led_manual = v & 0x1F, (v >> 8) & 1
            elif item == 8:
                self.rtc = v
            elif item > 10:
                return ST_BAD_ARG, b""
            return ST_OK, self.board_status()
        if ty == 0x3A:
            return (ST_OK, self.board_status()) if need(4) else (ST_BAD_LEN, b"")
        if ty == 0x3B:
            if not need(8):
                return ST_BAD_LEN, b""
            a, ln = struct.unpack("<II", p)
            if ln > 256 or a + ln > len(self.flash):
                return ST_BAD_ARG, b""
            return ST_OK, p + bytes(self.flash[a:a + ln])
        if ty == 0x3C:
            if n < 8:
                return ST_BAD_LEN, b""
            a, ln = struct.unpack_from("<II", p)
            if ln != n - 8:
                return ST_BAD_LEN, b""
            if ln > 256 or (a & 0xFF) + ln > 256:
                return ST_BAD_ARG, b""
            for k, byte in enumerate(p[8:]):
                self.flash[a + k] &= byte  # NOR: program only clears bits
            return ST_OK, b""
        if ty == 0x3D:
            if not need(8):
                return ST_BAD_LEN, b""
            a, ln = struct.unpack("<II", p)
            if a % 4096 or ln % 4096 or a + ln > len(self.flash):
                return ST_BAD_ARG, b""
            self.flash[a:a + ln] = b"\xff" * ln
            return ST_OK, b""
        if ty == 0x3E:
            return (ST_OK, self.board_status()) if need(12) else (ST_BAD_LEN, b"")
        if ty == 0x3F:
            if not need(12):
                return ST_BAD_LEN, b""
            op, slot, ln, c = struct.unpack("<BB2xII", p)
            if op == 0:
                return ST_OK, self.update_info()
            if op in (1, 2):
                base = slot * SLOT_SIZE
                ok = crc32(bytes(self.flash[base:base + ln])) == c
                if not ok:
                    return ST_HW, self.update_info(ST_HW, b"sim: crc mismatch")
                return ST_OK, self.update_info(0, b"sim: crc ok")
            return ST_OK, self.update_info(0, b"sim: op ignored")
        return ST_UNKNOWN, b""


def serve_client(conn, addr, dev, verbose):
    conn.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    buf = b""
    if verbose:
        print("client", addr)
    try:
        while True:
            timeout = 0.002 if dev.streaming else 0.2
            r, _, _ = select.select([conn], [], [], timeout)
            if r:
                chunk = conn.recv(65536)
                if not chunk:
                    break
                buf += chunk
                while True:
                    k = buf.find(b"NB")
                    if k < 0:
                        buf = buf[-1:] if buf.endswith(b"N") else b""
                        break
                    buf = buf[k:]
                    if len(buf) < 8:
                        break
                    _, ty, _, seq, ln = HDR.unpack_from(buf)
                    if ln > 504:
                        buf = buf[1:]
                        continue
                    if len(buf) < 8 + ln:
                        break
                    p, buf = buf[8:8 + ln], buf[8 + ln:]
                    st, rp = dev.handle(ty, seq, p)
                    if verbose:
                        print("cmd 0x%02X seq %d len %d -> st %d len %d" % (ty, seq, ln, st, len(rp)))
                    conn.sendall(frame(ty | 0x40, seq, rp, st))
                    if ty == 0x05:
                        raise ConnectionResetError("reset")
            if dev.streaming:
                for f in dev.next_data_frames():
                    conn.sendall(f)
    except (ConnectionResetError, BrokenPipeError):
        pass
    finally:
        dev.streaming = False  # closing the connection stops its stream
        conn.close()
        if verbose:
            print("client gone", addr)


def udp_socket(port):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("", port))
    return s


def udp_discovery(s, dev):
    while True:
        data, addr = s.recvfrom(1500)
        if len(data) >= 8 and data[:2] == b"NB" and data[2] == 0x02:
            seq = struct.unpack_from("<H", data, 4)[0]
            s.sendto(frame(0x42, seq, dev.info()), addr)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="0.0.0.0")
    ap.add_argument("--port", type=int, default=20034)
    ap.add_argument("--udp-port", type=int, default=20035)
    ap.add_argument("--no-udp", action="store_true")
    ap.add_argument("-v", "--verbose", action="store_true")
    a = ap.parse_args()

    dev = Device()
    if not a.no_udp:
        threading.Thread(target=udp_discovery, args=(udp_socket(a.udp_port), dev), daemon=True).start()  # bound before 'ready'
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind((a.host, a.port))
    srv.listen(4)
    print("nebulosa-sim: TCP %s:%d%s" % (a.host, a.port, "" if a.no_udp else ", UDP discovery %d" % a.udp_port),
          flush=True)
    busy = threading.Lock()

    def run(conn, addr):
        try:
            serve_client(conn, addr, dev, a.verbose)
        finally:
            busy.release()

    try:
        while True:
            conn, addr = srv.accept()
            if not busy.acquire(blocking=False):
                conn.close()  # one client at a time, like the device: refuse the next
                continue
            threading.Thread(target=run, args=(conn, addr), daemon=True).start()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
