# Python guide

`nebulosa` is a pure-Python package (3.8+) with no required dependencies.
TCP and discovery use the standard library; USB uses
[pyusb](https://pypi.org/project/pyusb/); numpy (optional) gives a fast,
vectorised stream decoder.

## Install

```sh
pip install "./python[usb,numpy]"     # from a clone of the repository
pip install ./python                  # TCP only, no dependencies
```

USB also needs the libusb-1.0 system library and, on Linux, the udev rule
(see [Getting started](../getting-started.md#2-usb-permissions)).

## Connect

```python
import nebulosa

dev = nebulosa.Nebulosa.usb()                    # first device; usb(serial="...") to pick one
dev = nebulosa.Nebulosa.tcp("192.168.1.20")      # port 20034 by default

for d in nebulosa.discover():                    # UDP broadcast, 0.6 s
    print(d.host, d.info.fw_version_str)
print(nebulosa.list_usb())
```

Use it as a context manager: `close()` also stops a stream the connection
started.

```python
with nebulosa.Nebulosa.usb() as dev:
    info = dev.get_info()
    print(info.fw_version_str, info.link_name, hex(info.fpga_id))
    print(dev.board_status())
```

## Measure

```python
import nebulosa

with nebulosa.Nebulosa.usb() as dev:
    dev.set_range_auto()                         # ranges 0..5
    rate = dev.set_decimation(6).rate_hz         # 2 MS/s / 64 = 31.25 kS/s
    dev.start_stream()
    try:
        total, n = 0.0, 0
        for s in dev.samples(timeout=1.0):       # iterator over Sample
            total += s.current
            n += 1
            if n == rate * 5:                    # 5 seconds
                break
    finally:
        dev.stop_stream()
    print(f"mean {total / n:.6g} A over {n} samples")
    print(dev.stream_counters())                 # lost_blocks, stall_flags, ...
```

Each `Sample` is a named tuple: `ts`, `step`, `current_code`,
`voltage_code`, `range`, `dirty`, `current` (A), `voltage` (V), plus the
`time_s` property. See [Concepts](../concepts.md#samples).

Reading options:

| Method | Returns | Use for |
|--------|---------|---------|
| `read_samples(max=65536, timeout=1.0)` | `list[Sample]` | simple loops, up to a few hundred kS/s |
| `samples(timeout=1.0)` | iterator, ends after `timeout` with no data | `for` loops |
| `read_arrays(max, timeout)` | `SampleArrays` of numpy arrays | full rate, analysis |
| `capture(seconds)` | `SampleArrays` for a wall-clock duration | "record N seconds" |
| `read_blocks(max, timeout)` | raw `Block`s | old FPGAs (format 4), own decoding |

## Full rate with numpy

```python
import numpy as np
import nebulosa

with nebulosa.Nebulosa.usb() as dev:
    dev.set_decimation(0)                        # 2 MS/s
    dev.start_stream()
    a = dev.capture(2.0)
    dev.stop_stream()

t = a.time_s - a.time_s[0]
print(len(a), a.current.mean(), a.current.max())
clean = a.current[~a.dirty]                      # drop range-switch samples
np.savez("capture.npz", t=t, i=a.current, v=a.voltage, r=a.range)
```

Measured on a PC over USB SuperSpeed: 2.0 MS/s with no lost blocks.
Without numpy, use `set_decimation(2)` (500 kS/s) or higher.

## Save to CSV / pandas

```python
import pandas as pd
df = pd.DataFrame({"t": a.time_s - a.time_s[0], "current": a.current,
                   "voltage": a.voltage, "range": a.range})
df.to_csv("capture.csv", index=False)
```

Or the bundled example: `python3 python/examples/stream_csv.py --usb -t 10 -d 8 -o out.csv`.

## Control the board

```python
from nebulosa import BoardSet, Key, Test, BoardFlag, Source

b = dev.board_status()
print(b.v_main_mv, b.pd_mv, b.has(BoardFlag.APWR_OK))
dev.set_leds(0b10101); dev.leds_auto()           # LEDs manual / back to firmware
dev.ui_key(Key.DOWN)                             # drive the on-board UI
dev.board_set(BoardSet.APWR_H, 1)                # analog supplies (need 12 V)
dev.set_source(Source.AD7387)                    # real ADC (stub sources for tests)
r = dev.board_test()                             # self-tests (stop streaming first)
print({t.name: r.test_result(t) for t in Test})
```

## Software update

```python
dev.update_gateware("nebulosa.bit")              # .bit or .bin; tested on the FPGA first
dev.update_firmware("firmware.nbfw")             # checked, installed after a restart
```

Both erase, write and verify the update slot, then ask the device to
install. A refused update raises `NebulosaError` with `.info.msg` saying
why. Pass `progress=lambda done, total: ...` to `upload_flash` for a
progress bar.

## Errors

```python
try:
    dev.set_range_manual(9)
except nebulosa.NebulosaError as e:
    print(e.status, e.name)                      # 3 BAD_ARG
except nebulosa.TimeoutError:                    # also a built-in TimeoutError
    ...
```

Also `LinkClosed` (a `ConnectionError`), `VerifyError`, `ImageError`.
Constants are `IntEnum` / `IntFlag` classes: `Cmd`, `Status`, `Source`,
`DecimMode`, `BoardSet`, `BoardFlag`, `Test`, `Key`, `Lcd`, `PwrSel`,
`UpdateOp`, ...

## Calibration

```python
dev.calibration = nebulosa.Calibration(
    i_fs=[2.0, 0.2, 20e-3, 2e-3, 200e-6, 20e-6, 0, 0],
    i_offset=[0, 0, 1.2e-6, 0, 0, 3e-9, 0, 0],
    v_scale=1e-3, v_offset=0.0)
```

## Threads

The `Nebulosa` object is guarded by a lock: a reader thread can stream while
another thread sends commands. Keep reading while you stream; DATA received
during commands is queued in memory.

## Examples

| Script | What |
|--------|------|
| `examples/info.py` | device info and board status |
| `examples/stream_csv.py -t 10 -d 8 -o out.csv` | stream to CSV |
| `examples/average_current.py -t 5` | mean / min / max current, charge, energy |
| `examples/board_leds.py` | LED chase |

All take `--host HOST`, `--usb [SERIAL]`, or nothing (discovery).

## Tests

```sh
pip install -e "./python[numpy,test]"
pytest python/tests                              # starts the simulator itself
```
