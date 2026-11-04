# Odin guide

!!! warning "Experimental"
    The Odin package was written without an Odin compiler at hand. CI builds
    and tests it on every push (with the latest Odin release); until that
    job is green, expect small fixes on your first build. Reports and patches
    welcome.

The `nebulosa` package does TCP, UDP discovery and, with
`-define:NEBULOSA_USB=true`, USB through libusb-1.0 (asynchronous transfers
on every IN endpoint, like the C library).

## Set up

Point a collection named `nebulosa` at the repository's `odin/` directory
and import the package:

```odin
import nb "nebulosa:nebulosa"
```

```sh
odin build . -collection:nebulosa=path/to/Instrix-Nebulosa-Libs/odin
odin build . -collection:nebulosa=path/to/Instrix-Nebulosa-Libs/odin -define:NEBULOSA_USB=true
```

USB links `libusb-1.0` (`system:usb-1.0`); add
`-extra-linker-flags:"-L/path/to/lib"` if the linker can't find it. Without
the define, `connect_usb` and `list_usb` return `.Usb_Disabled`.

## Conventions

- Every fallible procedure returns `Error` last; `nil` is success.
  `Error :: union { General_Error, Status }`: a `Status` is the device
  refusing (`st, ok := err.(nb.Status)`); `error_string(err)` names either.
- Records (`Info`, `Stats`, `Board_Status`, `Update_Info`, ...) are parsed
  field by field from the little-endian payload.
- Allocating procedures take an `allocator` (default `context.allocator`):
  `connect_*`, `ping`, `flash_read`, `discover`, `list_usb`.
- A `Device` is single-threaded. `close` stops a stream it started.

## Connect

```odin
package main

import "core:fmt"
import nb "nebulosa:nebulosa"

main :: proc() {
	dev, err := nb.connect_tcp("192.168.1.20")     // port 20034; or nb.connect_usb()
	if err != nil {
		fmt.eprintln("connect:", nb.error_string(err))
		return
	}
	defer nb.close(dev)

	info, ierr := nb.get_info(dev)
	if ierr != nil { return }
	major, minor, patch := nb.version_parts(info.fw_version)
	fmt.printfln("firmware %d.%d.%d, fpga %8x", major, minor, patch, info.fpga_id)
}
```

Discovery:

```odin
found, derr := nb.discover()                       // 600 ms broadcast
defer delete(found)
for d in found { fmt.println(d) }
```

## Measure

```odin
import "core:time"

_ = nb.set_range_auto(dev)
d, _ := nb.set_decimation(dev, 4)                   // .Average; 125 kS/s
if nb.start_stream(dev) != nil { return }

buf: [8192]nb.Sample
sum: f64
count: int
for count < int(d.rate_hz) * 5 {                    // 5 seconds
	n, rerr := nb.read_samples(dev, buf[:], 500 * time.Millisecond)
	if rerr != nil { fmt.eprintln(nb.error_string(rerr)); break }
	for s in buf[:n] { sum += s.current }           // s.voltage, s.range, s.dirty, nb.sample_time_s(s)
	count += n
}
_ = nb.stop_stream(dev)

c := nb.stream_counters(dev)
fmt.printfln("mean %.6g A, lost blocks %d", sum / f64(count), c.lost_blocks)
```

`read_samples` returns `n = 0, err = nil` on timeout and
`.Unsupported_Format` for format-4 FPGAs; use `read_block` for raw words.

## Control the board

```odin
b, _ := nb.board_status(dev)
fmt.println(b.v_main_mv, "mV")
_, _ = nb.set_leds(dev, 0b10101)
_, _ = nb.leds_auto(dev)
_ = nb.set_source(dev, .AD7387)
t, _ := nb.board_test(dev)                          // stop streaming first
fmt.println(nb.board_test_result(t, .FPGA_Done))
```

Flash: `flash_read`, `flash_write`, `flash_erase`, `upload_flash` (erase +
write + verify), `fpga_config`, `update`, `update_info`, `crc32`.

## Examples and tests

```sh
cd odin/examples/average_current
odin build . -collection:nebulosa=../.. && ./average_current 192.168.1.20

odin test odin/tests -collection:nebulosa=odin                 # unit tests
python3 tools/nebulosa_sim.py --port 21000 --no-udp &
NEBULOSA_SIM_PORT=21000 odin test odin/tests -collection:nebulosa=odin
```
