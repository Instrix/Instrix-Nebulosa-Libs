#!/usr/bin/env python3
"""Measure mean / min / max current, mean voltage, charge and energy over T seconds."""

import sys
import time

from _connect import connect, nebulosa, parser


def main():
    ap = parser(__doc__)
    ap.add_argument("-t", "--seconds", type=float, default=2.0)
    ap.add_argument("-d", "--log2n", type=int, default=None,
                    help="decimate by 2^log2n (default: 0 with numpy, 6 without)")
    args = ap.parse_args()
    try:
        import numpy  # noqa: F401
        have_np = True
    except ImportError:
        have_np = False
    log2n = args.log2n if args.log2n is not None else (0 if have_np else 6)

    with connect(args) as dev:
        dev.set_decimation(log2n)
        dev.start_stream()
        try:
            data = dev.capture(args.seconds, numpy=have_np)
        finally:
            dev.stop_stream()
        c = dev.stream_counters()
    if not len(data):
        sys.exit("no samples")
    if have_np:
        i, v, step = data.current, data.voltage, data.step
        n = len(i)
        mean_i, min_i, max_i, mean_v = float(i.mean()), float(i.min()), float(i.max()), float(v.mean())
        dt = step * nebulosa.TS_PERIOD_S
        charge = float((i * dt).sum())
        energy = float((i * v * dt).sum())
    else:
        n = len(data)
        cur = [s.current for s in data]
        mean_i, min_i, max_i = sum(cur) / n, min(cur), max(cur)
        mean_v = sum(s.voltage for s in data) / n
        charge = sum(s.current * s.step * nebulosa.TS_PERIOD_S for s in data)
        energy = sum(s.current * s.voltage * s.step * nebulosa.TS_PERIOD_S for s in data)
    print("samples   %d (2^%d decimation)" % (n, log2n))
    print("current   mean %.6g A, min %.6g A, max %.6g A" % (mean_i, min_i, max_i))
    print("voltage   mean %.5g V" % mean_v)
    print("charge    %.6g C (%.6g mAh)" % (charge, charge / 3.6))
    print("energy    %.6g J" % energy)
    if c.lost_blocks or c.seq_errors or c.malformed:
        print("warning: lost blocks %d, seq errors %d, malformed %d (decimate more?)"
              % (c.lost_blocks, c.seq_errors, c.malformed), file=sys.stderr)


if __name__ == "__main__":
    main()
