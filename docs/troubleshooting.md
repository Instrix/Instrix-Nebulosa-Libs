# Troubleshooting

**Connection refused / timed out over TCP**
: Another client is connected (the Companion, another script): the device
  takes one at a time. Close it. Check the IP on the device screen or with
  discovery.

**Discovery finds nothing**
: Discovery is a UDP broadcast: it doesn't cross routers or VPNs and some
  firewalls block it. Allow UDP 20035, or connect by IP.

**USB: permission denied / access denied (Linux)**
: Install `tools/99-nebulosa.rules` (see [Getting started](getting-started.md#2-usb-permissions))
  and replug.

**USB: device not found (Windows)**
: Bind WinUSB to the NBP interface with Zadig.

**`NO_POWER` when switching the analog supplies on**
: They need 12 V: barrel jack, or a USB-PD source offering 12 V.

**Current reads random values between 0 and 2 A**
: The stub source is selected (no 12 V for the analog supplies). Power the
  board from 12 V and `set_source(AD7387)`.

**`BUSY`**
: The command can't run while a stream is on (flash, self-tests, FPGA
  configuration), or the device is serving its USB drive. Stop the stream.

**`lost_blocks` / `stall_flags` / `get_stats().stalls` grow**
: The host doesn't read fast enough. Decimate (`set_decimation(4)` or more),
  read in larger batches, use numpy in Python, build Rust in `--release`.
  Over USB, use the Type-A (SuperSpeed) port rather than USB-C (Full-Speed).

**The stream stops when my program exits**
: Closing a TCP connection stops its stream; the libraries also send
  STREAM_STOP on close. That's intended.

**Timestamps don't start at zero**
: They count from the FPGA reset. Subtract the first sample's time.

**Format-4 FPGA / "unsupported format"**
: FPGA IDs below `0x4E42000B` use the older stream format, which the
  decoders don't handle. Update the gateware, or read raw blocks.
