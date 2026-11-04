#!/usr/bin/env python3
"""Stream for N seconds at a decimation and write time, current, voltage, range as CSV."""

import csv
import sys
import time

from _connect import connect, nebulosa, parser


def main():
    ap = parser(__doc__)
    ap.add_argument("-t", "--seconds", type=float, default=5.0)
    ap.add_argument("-d", "--log2n", type=int, default=8, help="decimate by 2^log2n (0..8, default 8: 7.8 kS/s)")
    ap.add_argument("-o", "--out", default="-", help="CSV file (default stdout)")
    args = ap.parse_args()

    out = sys.stdout if args.out == "-" else open(args.out, "w", newline="")
    w = csv.writer(out)
    w.writerow(["time_s", "current_A", "voltage_V", "range", "dirty"])
    with connect(args) as dev:
        dec = dev.set_decimation(args.log2n)
        print("%d S/s for %.1f s" % (dec.rate_hz, args.seconds), file=sys.stderr)
        dev.start_stream()
        n = 0
        t0 = None
        end = time.monotonic() + args.seconds
        try:
            while time.monotonic() < end:
                for s in dev.read_samples(timeout=0.5):
                    if t0 is None:
                        t0 = s.time_s
                    w.writerow(["%.7f" % (s.time_s - t0), "%.6e" % s.current, "%.5f" % s.voltage, s.range, int(s.dirty)])
                    n += 1
        finally:
            dev.stop_stream()
        c = dev.stream_counters()
        print("%d samples; lost blocks %d, seq errors %d, malformed %d"
              % (n, c.lost_blocks, c.seq_errors, c.malformed), file=sys.stderr)
    if out is not sys.stdout:
        out.close()


if __name__ == "__main__":
    main()
