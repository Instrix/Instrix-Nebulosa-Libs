# libnebulosa (C)

C99 library for the INSTRIX Nebulosa: TCP, UDP discovery and USB
(libusb-1.0) transports, every NBP command, and the format-5 sample stream
decoded to amps / volts. One `.c` file, one header (plus `spec/nbp.h`).

Tested on Linux against `tools/nebulosa_sim.py` and real hardware (USB
SuperSpeed: 2 MSPS, 0 lost blocks).

## Build

```sh
cmake -S c -B c/build -DCMAKE_BUILD_TYPE=Release   # -DNEBULOSA_USB=AUTO|ON|OFF
cmake --build c/build
(cd c/build && ctest)                              # unit + simulator tests
```

USB needs libusb-1.0 (`apt install libusb-1.0-0-dev`, `brew install libusb`,
vcpkg `libusb` on Windows with the WinUSB driver). Without it you get TCP
only. On Linux install `tools/99-nebulosa.rules` for non-root access.

Vendoring instead of CMake: copy `c/include/nebulosa.h`, `c/src/nebulosa.c`
and `spec/nbp.h`; compile with `-DNEBULOSA_WITH_USB` and `-lusb-1.0` for USB
(`ws2_32` on Windows).

## Use

```c
#include "nebulosa.h"

nebulosa_t *d;
if (nebulosa_open_tcp(&d, "192.168.1.20", 0, 3000))   /* or nebulosa_open_usb(&d, NULL, 0) */
    return 1;
nbp_info_t info;
nebulosa_get_info(d, &info);

uint32_t rate;
nebulosa_set_decimation(d, 4, NBP_DECIM_AVERAGE, &rate);   /* 125 kS/s */
nebulosa_start_stream(d, NBP_START_FLUSH);
nebulosa_sample_t s[4096];
int n = nebulosa_read_samples(d, s, 4096, 500);          /* count, 0 on timeout, < 0 error */
for (int i = 0; i < n; i++)
    printf("%.7f s  %.6g A  %.4f V  range %u\n", nebulosa_sample_time(&s[i]), s[i].current, s[i].voltage, s[i].range);
nebulosa_stop_stream(d);
nebulosa_close(d);
```

- Return codes: `0` OK, `< 0` `NEBULOSA_E_*` (I/O, timeout, ...), `> 0` the
  device's `NBP_ST_*` (BUSY, BAD_ARG, ...). `nebulosa_strerror()` names them.
- Records are `nbp.h`'s packed structs (`nbp_info_t`, `nbp_board_status_t`, ...).
- Commands sent while streaming are fine: DATA arriving meanwhile is queued.
- Calibration: `nebulosa_set_calibration()`; defaults are the firmware's scaling.
- `nebulosa_read_block()` gives raw words (format-4 FPGAs, own decoding);
  `nebulosa_decoder_*` decodes recorded words offline.
- Board / flash / update: `nebulosa_board_status`, `nebulosa_set_leds`,
  `nebulosa_upload_flash`, `nebulosa_update_gateware` (.bit/.bin),
  `nebulosa_update_firmware` (.nbfw).
- Not thread safe: one thread per `nebulosa_t`.
- TCP: one client at a time (close the Companion). Closing a connection
  stops a stream it started.

Examples (`c/examples`, built as `nebulosa_*`): `info`, `stream_csv`,
`average_current`, `board_leds`. Each takes `--host H`, `--usb [SERIAL]`, or
nothing (UDP discovery).
