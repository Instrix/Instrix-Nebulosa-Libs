# Nebulosa for Odin

An Odin package (`nebulosa`) for scripting the INSTRIX Nebulosa power profiler
directly, over TCP or USB, without the Companion app. It implements the shared
contract in [`docs/API.md`](../docs/API.md): framing, request/response, every
command, the format-5 sample stream decoder (a port of the Companion's
`stream.rs`), and calibration.

> **Status: not compiled yet.** No Odin compiler was available when this package
> was written, so it has **not been built or run** yet. It targets the
> 2025/2026 `core` library (`core:net`, `core:encoding/endian`, `core:time`,
> `core:testing`, `core:os`). Expect a few small fixes the first time you
> compile it. Please report them, or send a patch.

## Layout

```
odin/
  nebulosa/            the package
    protocol.odin      constants, commands, enums, bit_sets
    errors.odin        Error (General_Error | Status)
    frame.odin         encode, resyncing Frame_Parser, LE/BE byte helpers
    records.odin       Info, Stats, Range_State, Decim, Board_Status, Update_Info
    stream.odin        Stream_Decoder (format 5), Calibration, Sample
    device.odin        Device, connect_tcp, close, request
    commands.odin      every command in API.md section 4, plus helpers (crc32, LEDs, flash upload)
    streaming.odin     start_stream, stop_stream, read_samples, read_block, stream_counters
    discovery.odin     UDP discovery
    libusb.odin        minimal libusb-1.0 binding (only with NEBULOSA_USB)
    usb.odin           USB transport (async IN transfers), or stubs
  examples/info/ examples/stream_csv/ examples/average_current/
  tests/               core:testing unit tests + simulator integration test
```

## Using it

Give the compiler a collection named `nebulosa` that points at this `odin/`
directory, then import the package:

```odin
package main

import "core:fmt"
import "core:time"
import nb "nebulosa:nebulosa"

main :: proc() {
	dev, err := nb.connect_tcp("192.168.1.50")    // or nb.connect_usb()
	if err != nil { fmt.eprintln(nb.error_string(err)); return }
	defer nb.close(dev)

	_, _ = nb.set_decimation(dev, 4)               // 2 MS/s / 16 = 125 kS/s
	if nb.start_stream(dev) != nil { return }
	buf: [4096]nb.Sample
	n, _ := nb.read_samples(dev, buf[:], 500 * time.Millisecond)
	for s in buf[:n] { fmt.println(nb.sample_time_s(s), s.current, s.voltage) }
}
```

Build:

```sh
cd odin/examples/info
odin build . -collection:nebulosa=<path>/odin     # e.g. -collection:nebulosa=../..
./info 192.168.1.50        # or no argument: UDP discovery
```

### Conventions

- Every procedure that can fail returns `Error` as its last value; `nil` means
  success. `Error` is `union { General_Error, Status }`. A device answering
  with a non-zero status gives a `Status` (`st, ok := err.(nb.Status)`);
  `error_string(err)` names it. Device errors never panic.
- Payload records are parsed field by field at the documented offsets
  (little-endian), never by casting structs. A longer payload is accepted, a
  shorter one is `.Short_Payload`.
- Allocations take an explicit `allocator` parameter (defaulting to
  `context.allocator`): `connect_*` (the `Device`), `ping`, `flash_read`,
  `discover`, `list_usb`.
- `request` returns a payload pointing into the parser's buffer. It stays valid
  until the next call on the device.
- A `Device` is not thread-safe. Keep one command in flight, from one thread.

### Streaming

`start_stream` sends STREAM_START. `read_samples(dev, buf, timeout)` fills `buf`
with whatever samples are available: first the DATA frames that were queued
during requests (they are never dropped), then the link. It waits up to
`timeout` for the first one, and `n = 0, err = nil` means nothing arrived in
time. `stream_counters` returns the decoder counters plus `data_blocks`,
`lost_blocks`, `stall_flags`, `gap_flags`, `test_blocks` and `resyncs`.
`Device.calibration` (default from `default_calibration()`) converts codes to
amps and volts, and you can replace it.

Format-4 FPGAs (`fpga_id < 0x4E42000B`) are not decoded, and `read_samples`
returns `.Unsupported_Format` for them. Use `read_block` to get the raw words
(host order) of each DATA block. The same goes for link tests
(`start_stream(dev, link_test = true)`).

At 2 MS/s (`log2n = 0`) the stream is 8 MB/s of words. Use decimation unless
you need the full rate.

## TCP

The device listens on port 20034 and accepts **one client at a time**. A second
connection is refused, so close the Companion first (`.Connect_Failed`).
Closing the connection stops a stream it started. `TCP_NODELAY` is set, and
request timeouts use the socket receive timeout. `discover()` broadcasts a
GET_INFO header to UDP port 20035 and collects answers for 600 ms.

## USB

USB support is compiled in only on request:

```sh
odin build . -collection:nebulosa=../.. -define:NEBULOSA_USB=true
```

The package links `libusb-1.0` through `foreign import libusb "system:usb-1.0"`
(that is, `-lusb-1.0`). If the linker cannot find it, add
`-extra-linker-flags:"-L/path/to/lib -lusb-1.0"`, or edit the foreign import
path in `nebulosa/libusb.odin`. Without the define, `connect_usb` and `list_usb`
return `.Usb_Disabled` and libusb is not needed.

The library claims only the NBP vendor interface (class `FF`, subclass `4E`,
protocol `01`), so the CDC log interface of the Full-Speed device is left alone.
It keeps 8 asynchronous 32 KB transfers queued on **every** bulk IN endpoint,
and each endpoint feeds its own frame parser. On High-Speed, DATA comes on one
endpoint and responses on another. Full-Speed has one endpoint for both. Events
are handled while you wait in a request or `read_samples`. OUT is a synchronous
bulk write.

On Linux, normal users need the udev rule:

```sh
sudo cp tools/99-nebulosa.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

## Tests

```sh
# unit tests (framing, decoder, record layouts, CRC)
odin test odin/tests -collection:nebulosa=odin

# plus the integration test against the simulator
python3 tools/nebulosa_sim.py --port 21000 --no-udp &
NEBULOSA_SIM_PORT=21000 odin test odin/tests -collection:nebulosa=odin
```

Without `NEBULOSA_SIM_PORT` the integration test logs that it was skipped and
passes. The simulator serves one client at a time, like the device.
