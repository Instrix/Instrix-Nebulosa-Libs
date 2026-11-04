#!/usr/bin/env python3
"""Run a light chase on the board's five LEDs, then give them back to the firmware."""

import time

from _connect import connect, parser


def main():
    ap = parser(__doc__)
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--delay", type=float, default=0.1)
    args = ap.parse_args()
    with connect(args) as dev:
        try:
            for _ in range(args.rounds):
                for k in list(range(5)) + list(range(3, 0, -1)):
                    dev.set_leds(1 << k)
                    time.sleep(args.delay)
            dev.set_leds(0b11111)
            time.sleep(0.3)
        finally:
            dev.leds_auto()
        print("LEDs back to the status pattern")


if __name__ == "__main__":
    main()
