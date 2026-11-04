# nebulosa (Python)

Script the INSTRIX Nebulosa power profiler directly over Ethernet (TCP) or
USB, without the Companion app. Implements the common contract in
[`docs/API.md`](../docs/API.md); the wire format is [`spec/nbp.h`](../spec/nbp.h)
and [`spec/neb_stream.h`](../spec/neb_stream.h).

- Python >= 3.8, no required dependencies (TCP and UDP discovery are pure stdlib)
- `usb` extra: [pyusb](https://pypi.org/project/pyusb/) >= 1.2 (plus the libusb-1.0 system library)
- `numpy` extra: vectorised stream decoding (`read_arrays`, `capture`)

## Install

```sh
pip install ./python[usb,numpy]     # from the repository root
pip install ./python                # TCP only, no dependencies
```

## Quickstart

```python
import nebulosa

found = nebulosa.discover()                 # UDP broadcast, 0.6 s
with nebulosa.Nebulosa.tcp(found[0].host) as dev:   # or Nebulosa.usb()
    info = dev.get_info()
    print(info.fw_version_str, info.link_name, hex(info.fpga_id))

    dev.set_range_auto()
    dev.set_decimation(6)                   # 2 MS/s / 64 = 31.25 kS/s
    dev.start_stream()
    try:
        for s in dev.read_samples(max=1000, timeout=1.0):
            print(s.time_s, s.current, s.voltage, s.range, s.dirty)
    finally:
        dev.stop_stream()
    print(dev.stream_counters())
```

With numpy:

```python
dev.set_decimation(0)                       # full rate
dev.start_stream()
a = dev.capture(2.0)                        # SampleArrays: ts, step, current, voltage, range, dirty, ...
dev.stop_stream()
print(a.current.mean(), a.current.max(), len(a))
```

## Examples

All take `--host HOST` (TCP), `--usb [SERIAL]`, or nothing (discover on the network).

| script | what it does |
|--------|--------------|
| `examples/info.py` | device info and board status |
| `examples/stream_csv.py -t 10 -d 8 -o out.csv` | stream N seconds at a decimation into a CSV (time, current, voltage, range) |
| `examples/average_current.py -t 5` | mean / min / max current, charge and energy over T seconds |
| `examples/board_leds.py` | LED chase, then LEDs back to the firmware |

Try them without hardware against the simulator:

```sh
python3 tools/nebulosa_sim.py --port 20034 &
python3 python/examples/info.py --host 127.0.0.1
```

## API overview

**Opening.** `Nebulosa.tcp(host, port=20034, timeout=3.0)`, `Nebulosa.usb(serial=None, index=0)`;
context manager or `close()` (which also stops a stream this connection started).
`discover(timeout=0.6, address="255.255.255.255")` returns `Discovered(host, info)`;
`list_usb()` returns `UsbDevice` records.

**Commands** (API.md section 4), all synchronous; one command in flight at a time
(the object is guarded by a lock, so a reader thread and a command thread can share it):

| method | returns |
|--------|---------|
| `ping(data)`, `get_info()`, `reg_read(i)`, `reg_write(i, v)`, `reset()` | bytes / `Info` / int / - |
| `start_stream(flush=True, link_test=False)`, `stop_stream()`, `get_stats()` | `Stats` |
| `set_source(Source.*)`, `set_range_auto(min=0, max=5)`, `set_range_manual(r)`, `get_range()` | `RangeState` |
| `set_range_timing(up, down, overlap, hold)`, `set_range_force(enable, value)` | - |
| `set_decimation(log2n, mode=DecimMode.AVERAGE)` | `Decim` (as applied) |
| `board_status()`, `board_set(BoardSet.*, value)`, `board_test(mask=TEST_ALL)` | `BoardStatus` (`.test_result(Test.X)`, `.has(BoardFlag.X)`, `.pdos`) |
| `set_leds(mask)`, `leds_auto()`, `ui_key(Key.*)` | `BoardStatus` |
| `flash_read(addr, n<=256)`, `flash_write(addr, data)`, `flash_erase(addr, len)`, `fpga_config(addr, len)` | bytes / - / - / `BoardStatus` |
| `update(op, slot, length, crc)`, `update_info()`, `update_revert()` | `UpdateInfo` |
| `upload_flash(addr, data, progress=None)` | erase 64 KB at a time, write 256-byte pages (blank ones skipped), read back and compare |
| `update_gateware(path_or_bytes)` | .bit or .bin to the update slot, then UPDATE GATEWARE (tested on the FPGA before it becomes active) |
| `update_firmware(path_or_bytes)` | .nbfw checked, to the update slot, then UPDATE FIRMWARE (the device restarts) |

Errors: `NebulosaError` (`.status`, `.name` such as `"BAD_ARG"`, `.payload`, and `.info`:
a failed `update()` attaches the `UpdateInfo` whose `msg` says why);
`nebulosa.TimeoutError` (also a built-in `TimeoutError`); `LinkClosed` (also a
`ConnectionError`); `VerifyError`; `ImageError` for unusable update files.
Default timeouts: 1.5 s; board_test 15 s; fpga_config 10 s; flash_erase 5 s; update 30 s
(gateware install 15 s, firmware 10 s). Every command takes the raw path too:
`dev.request(cmd, payload, timeout)`.

Constants are `IntEnum` / `IntFlag` classes: `Cmd`, `Status`, `Source`, `DecimMode`, `Cap`,
`Link`, `BoardFlag`, `BoardSet`, `Test`, `TestResult`, `Key`, `Lcd`, `PwrSel`, `PdState`,
`CC`, `FpgaMode`, `FpgaProgram`, `UpdateOp`, `SlotType`, `BlockFlag`, `StartFlag`, `EthFlag`.
Helpers: `crc32`, `bitstream` (.bit -> raw), `firmware_check` (.nbfw).

**Stream.** DATA frames that arrive while a command waits are queued, never dropped
(the queue is unbounded: keep reading while you stream). Reading pulls from the queue
first, then the link:

- `read_samples(max=65536, timeout=1.0)` -> `list[Sample]` (whatever is available; waits up to
  `timeout` for at least one). `Sample` is a named tuple: `ts` (500 ns ADC periods since FPGA
  reset), `step` (2^log2N), `current_code`, `voltage_code`, `range` (0 = 2 A .. 5 = 20 uA),
  `dirty`, `current` (A), `voltage` (V), and the `time_s` property.
- `samples(timeout=1.0)` -> an iterator; ends when nothing arrives within `timeout`.
- `read_arrays(max=1<<20, timeout=1.0)` -> `SampleArrays` of numpy arrays (vectorised:
  only header / empty words go through the Python state machine; counters are identical).
- `capture(seconds, numpy=True)` -> everything for a wall-clock duration.
- `read_blocks(max=64, timeout=1.0)` -> raw `Block`s (header + host-order FPGA words), for
  format-4 FPGAs (FPGA ID < 0x4E42000B, which the decoder refuses) or your own decoding.
  Use either the decoder or raw blocks on one stream, not both.
- `stream_counters()` -> `StreamCounters`: the decoder's (samples, dirty, blocks, empty, late,
  ovf_blocks, range_changes, seq_errors, malformed, unframed, gap_periods) plus data_blocks,
  lost_blocks, stall_flags, gap_flags, test_blocks, resyncs.
- `dev.calibration = Calibration(i_fs=[...], i_offset=[...], v_scale=1e-3, v_offset=0)`:
  `current = code * i_fs[range] / 16384 + i_offset[range]`, `voltage = code * v_scale + v_offset`.

The building blocks are public too: `FrameParser`, `encode`, `StreamDecoder` (a port of
`neb_stream.c` / the Companion's `stream.rs`), `StreamProcessor`.

## Throughput and decimation

The device produces 2 MS/s at `log2n = 0` (8 MB/s of FPGA words).

- `read_arrays` / `capture` with numpy keep up with the full rate over USB High-Speed /
  SuperSpeed on a typical PC (measured: 2.0 MS/s, no lost blocks, no device stalls).
- `read_samples` builds one Python object per sample: plan on a few hundred thousand
  samples/s. Use `set_decimation(2)` (500 kS/s) or more, or numpy.
- Decimation averages by default (`DecimMode.AVERAGE`), so it lowers noise as well as the
  rate; `log2n = 8` gives 7.8 kS/s, plenty for battery-life style measurements.
- Watch `stream_counters().lost_blocks` and `get_stats().stalls`: non-zero means the host
  did not keep up (blocks queued in the device ring overflowed). Decimate more.

## Transport notes

- **TCP: one client at a time.** The device refuses a second connection, so close the
  Companion (or any other script) first. Closing the connection stops the stream it started.
- **USB:** the library claims only the vendor interface (class FF / 4E / 01); the Full-Speed
  (USB-C) device's CDC log interface is left alone. Each bulk IN endpoint has its own reader
  thread keeping a read outstanding, with its own frame parser.
- **USB permissions (Linux):** install the udev rule, then replug:

  ```sh
  sudo cp tools/99-nebulosa.rules /etc/udev/rules.d/
  sudo udevadm control --reload && sudo udevadm trigger
  ```

## Tests

```sh
pip install -e ./python[numpy,test]
pytest python/tests
```

Unit tests cover the frame parser (garbage, splits), the stream decoder (the `stream.rs`
test, error cases, numpy vs scalar on random streams), record layouts and update images;
integration tests start `tools/nebulosa_sim.py` on a free port and exercise every command and
a 1 s stream.
