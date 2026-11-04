# Instrix-Nebulosa-Libs

Developer libraries for the **INSTRIX Nebulosa** power profiler: control the
device and stream its samples from your own scripts and programs, over TCP or
USB, without the Companion app.

| Language | Directory | Transports | Status |
|----------|-----------|------------|--------|
| Python 3.8+ | [`python/`](python) | TCP, UDP discovery, USB (pyusb) | tested: simulator + hardware (USB) |
| C99 | [`c/`](c) | TCP, UDP discovery, USB (libusb-1.0) | tested: simulator + hardware (USB, 2 MSPS lossless) |
| C++17 | [`cpp/`](cpp) | header-only wrapper over the C library | tested: simulator + hardware |
| Rust | [`rust/`](rust) | TCP, UDP discovery, USB (`usb` feature, rusb) | tested: simulator + hardware (USB, 2 MSPS lossless) |
| Odin | [`odin/`](odin) | TCP, UDP discovery, USB (libusb-1.0, async) | written, **not yet compiled** (no odin toolchain here) |

Every binding implements the same model, described in
[`docs/API.md`](docs/API.md):

- **connect**: `tcp(host)`, `usb(serial)`, `discover()` (UDP broadcast), `list_usb()`
- **commands**: info, stats, registers, source, range (auto / manual / timing /
  force), decimation, board status / set / tests, LEDs, UI keys, SPI flash,
  FPGA config, software update (gateware `.bit`/`.bin`, firmware `.nbfw`)
- **stream**: `start_stream()`, `read_samples()` -> timestamped samples with
  current (A), voltage (V), range and dirty flag; raw blocks; loss counters
- **calibration**: per-range full scale and offsets (defaults: the firmware's scaling)

## Quick look

```python
import nebulosa
with nebulosa.Nebulosa.usb() as dev:            # or Nebulosa.tcp("192.168.1.20")
    print(dev.get_info())
    dev.set_decimation(4)                        # 125 kS/s
    dev.start_stream()
    for s in dev.read_samples(timeout=1.0)[:5]:
        print(s.time_s, s.current, s.voltage, s.range)
    dev.stop_stream()
```

## Repository layout

```
spec/        nbp.h, neb_stream.h: the protocol definition, copied verbatim from
             Instrix-Nebulosa-Firmware (common/nbp, common/fpga)
docs/API.md  the common API contract + wire details for binding authors
tools/       nebulosa_sim.py (a fake device for tests), 99-nebulosa.rules (Linux udev)
python/ c/ cpp/ rust/ odin/
```

## Testing without hardware

```sh
python3 tools/nebulosa_sim.py          # TCP 20034 + UDP discovery 20035
```

It answers every command and streams format-5 data (a log-swept current that
walks through all ranges). Each library's test suite starts it on a free port.

## Things to know

- **TCP: one client at a time.** Close the Companion (or any other script)
  first. Closing a TCP connection stops a stream it started.
- **USB on Linux**: `sudo cp tools/99-nebulosa.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger`.
  Windows needs the WinUSB driver bound to the NBP interface.
- **Rate**: 2 MSPS full rate is 8 MB/s. C, C++ and Rust keep up over
  USB; Python keeps up with numpy (`read_arrays`) or decimation.
- **Real measurements need the analog supplies**, which need a 12 V source
  (barrel jack or a 12 V USB-PD contract). Without one the data is the stub source.
- **Keeping in sync with firmware**: when `common/nbp/nbp.h` changes in the
  firmware, copy it to `spec/` and update the bindings.
