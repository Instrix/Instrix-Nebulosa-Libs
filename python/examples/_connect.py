"""Shared --host / --usb handling for the examples."""

import argparse
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src"))

import nebulosa  # noqa: E402


def parser(description):
    ap = argparse.ArgumentParser(description=description)
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--host", help="device IP / name (TCP); default: discover on the network")
    g.add_argument("--usb", nargs="?", const="", metavar="SERIAL", help="open over USB (optionally by serial)")
    ap.add_argument("--port", type=int, default=nebulosa.TCP_PORT, help="TCP port (default 20034)")
    return ap


def connect(args):
    if args.usb is not None:
        return nebulosa.Nebulosa.usb(serial=args.usb or None)
    host = args.host
    if host is None:
        found = nebulosa.discover()
        if not found:
            sys.exit("no device found by UDP discovery: give --host or --usb")
        host = found[0].host
        print("found %s (firmware %s)" % (host, found[0].info.fw_version_str), file=sys.stderr)
    return nebulosa.Nebulosa.tcp(host, args.port)
