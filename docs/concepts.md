# Concepts

## Samples

A decoded sample carries:

| Field | Meaning |
|-------|---------|
| `ts` | timestamp in ADC periods of **500 ns** since the FPGA reset; `time_s = ts × 500e-9` |
| `step` | 2^log2n: the `ts` distance to the next sample |
| `current` | amps, calibrated |
| `voltage` | volts, calibrated |
| `current_code`, `voltage_code` | the raw 14-bit codes |
| `range` | current range the sample was taken in (below) |
| `dirty` | the sample may be unreliable (taken during a range switch); drop or interpolate it |

Timestamps keep counting across streams (they come from the FPGA, not from
`start_stream`). Subtract the first sample's time when you want "seconds
since start".

## Current ranges

| Range | Full scale |
|-------|-----------|
| 0 | 2 A |
| 1 | 200 mA |
| 2 | 20 mA |
| 3 | 2 mA |
| 4 | 200 µA |
| 5 | 20 µA |

In **auto** mode (`set_range_auto(min, max)`) the FPGA switches between
`min` and `max` as the current changes, within microseconds. **Manual**
mode (`set_range_manual(r)`) fixes one range. Every sample tells you its
range, so you never have to track it yourself.

## Sample rate and decimation

The ADC runs at **2 MSPS**. `set_decimation(log2n, mode)` reduces that by
2^log2n (log2n 0..8):

| log2n | rate | typical use |
|-------|------|-------------|
| 0 | 2 MS/s | transients, wake-up spikes |
| 4 | 125 kS/s | general profiling |
| 8 | 7.8 kS/s | long battery-life runs |

Mode `AVERAGE` (default) averages the 2^log2n samples, so decimation also
lowers noise; `SUBSAMPLE` keeps every N-th.

Full rate is 8 MB/s of data. C, C++ and Rust keep up over USB. Python keeps
up with numpy (`read_arrays`); plain `read_samples` handles a few hundred
kS/s.

## Did I lose data?

After a stream, check the counters:

| Counter | Non-zero means |
|---------|----------------|
| `lost_blocks` | DATA blocks never arrived (sequence jump) |
| `stall_flags` | the device's buffer filled up: the host wasn't reading fast enough |
| `ovf_blocks` | the FPGA FIFO overflowed (consequence of a stall) |
| `gap_periods` | sample periods missing between blocks |
| `seq_errors`, `malformed` | corrupted stream (should never happen) |

The device-side view is `get_stats()`: `stalls`, `fpga_drops`. If they grow,
decimate more, read more often, or use a faster language.

## Calibration

Default scaling is the firmware's: `current = code × full_scale[range] / 16384`,
`voltage = code × 1 mV`. Replace it with your own per-range full scale and
offset (`Calibration`) when you have calibration data. Raw codes stay
available on every sample.

## Commands while streaming

You can send commands (stats, range changes, LEDs...) while a stream runs.
DATA that arrives while a command waits for its answer is queued and
handed to the next `read_samples`, never dropped. Keep reading, though:
that queue grows if you don't.

Some board operations answer `BUSY` while a stream runs (flash, self-tests,
FPGA configuration): stop the stream first.

## Errors

Two kinds, in every language:

- **device status**: the device answered and refused: `UNKNOWN`,
  `BAD_LEN`, `BAD_ARG`, `FPGA`, `BUSY`, `HW`, `NO_POWER` (analog supplies
  need 12 V), `PROTECTED` (flash in use);
- **library errors**: I/O, timeout, connection closed, malformed response.
