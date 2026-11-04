# C guide

`libnebulosa` is one C99 source file and one header (plus the protocol
header `spec/nbp.h`). It does TCP, UDP discovery and, with libusb-1.0, USB;
it keeps up with the full 2 MSPS over USB (measured: 0 lost blocks).

## Build

```sh
sudo apt install libusb-1.0-0-dev            # macOS: brew install libusb
cmake -S c -B c/build -DCMAKE_BUILD_TYPE=Release
cmake --build c/build
(cd c/build && ctest)                         # unit + simulator tests
```

`-DNEBULOSA_USB=AUTO` (default: USB when libusb is found), `ON` (required) or
`OFF` (TCP only). `-DBUILD_SHARED_LIBS=ON` for a shared library.

### In your CMake project

```cmake
add_subdirectory(path/to/Instrix-Nebulosa-Libs/c nebulosa)
target_link_libraries(myapp PRIVATE nebulosa::nebulosa)
```

### Without CMake

Copy `c/include/nebulosa.h`, `c/src/nebulosa.c` and `spec/nbp.h` into your
project:

```sh
cc -O2 -DNEBULOSA_WITH_USB $(pkg-config --cflags libusb-1.0) -c nebulosa.c
cc -O2 myapp.c nebulosa.o $(pkg-config --libs libusb-1.0) -o myapp
```

On Windows link `ws2_32`.

## Conventions

- Every call returns `int`: `0` OK, **negative** `NEBULOSA_E_*` (I/O,
  timeout, closed, ...), **positive** the device's `NBP_ST_*` (`BAD_ARG`,
  `BUSY`, ...). `nebulosa_strerror(e)` names both.
- Records are the protocol's packed structs from `nbp.h`: `nbp_info_t`,
  `nbp_stats_t`, `nbp_board_status_t`, `nbp_update_info_t`, ...
- A `nebulosa_t` is not thread safe: one thread per connection.
- `nebulosa_close()` stops a stream the connection started.

## Connect

```c
#include "nebulosa.h"
#include <stdio.h>

int main(void)
{
    nebulosa_t *d;
    int e = nebulosa_open_usb(&d, NULL, 0);          /* serial NULL: any; index 0 */
    /* int e = nebulosa_open_tcp(&d, "192.168.1.20", 0, 3000);   port 0: 20034 */
    if (e) {
        fprintf(stderr, "open: %s\n", nebulosa_strerror(e));
        return 1;
    }
    nbp_info_t info;
    if (nebulosa_get_info(d, &info) == 0)
        printf("firmware %u.%u.%u, fpga 0x%08X, %s\n",
               nebulosa_version_major(info.fw_version), nebulosa_version_minor(info.fw_version),
               nebulosa_version_patch(info.fw_version), info.fpga_id, nebulosa_description(d));
    nebulosa_close(d);
    return 0;
}
```

Discovery and USB listing:

```c
nebulosa_net_device_t found[8];
int n = nebulosa_discover(found, 8, 600);           /* devices answering in 600 ms */
for (int i = 0; i < n; i++)
    printf("%s  %s\n", found[i].host, found[i].info.build);

nebulosa_usb_device_t usb[8];
n = nebulosa_list_usb(usb, 8);                      /* NEBULOSA_E_UNSUPPORTED without USB */
```

## Measure

```c
uint32_t rate;
nebulosa_set_range_auto(d, 0, 5);
nebulosa_set_decimation(d, 4, NBP_DECIM_AVERAGE, &rate);   /* 125 kS/s */
nebulosa_start_stream(d, NBP_START_FLUSH);

static nebulosa_sample_t buf[16384];
double sum = 0;
uint64_t count = 0;
while (count < 5ull * rate) {                               /* 5 seconds */
    int n = nebulosa_read_samples(d, buf, 16384, 1000);    /* count, 0 on timeout, < 0 error */
    if (n < 0) { fprintf(stderr, "%s\n", nebulosa_strerror(n)); break; }
    for (int i = 0; i < n; i++)
        sum += buf[i].current;
    count += (uint64_t)n;
}
nebulosa_stop_stream(d);

nebulosa_stream_counters_t c;
nebulosa_stream_counters(d, &c);
printf("mean %.6g A, lost blocks %llu\n", sum / count, (unsigned long long)c.lost_blocks);
```

`nebulosa_sample_t` has `ts`, `step`, `current_code`, `voltage_code`,
`range`, `dirty`, `current` (A), `voltage` (V);
`nebulosa_sample_time(&s)` gives seconds.

Commands are fine while streaming (`nebulosa_get_stats`, range changes, ...):
DATA that arrives meanwhile is queued for the next `nebulosa_read_samples`.

### Raw blocks and offline decoding

```c
nebulosa_block_t blk;
if (nebulosa_read_block(d, &blk, 500) == 1)          /* nwords host-order words */
    printf("block %u, %u words, flags 0x%x\n", blk.seq, blk.nwords, blk.flags);

nebulosa_decoder_t dec;                             /* decode words you recorded */
nebulosa_decoder_init(&dec);
nebulosa_sample_t s;
if (nebulosa_decoder_feed(&dec, word, &s) == NEBULOSA_W_SAMPLE) { /* s.current_code ... */ }
```

Don't mix `read_block` and `read_samples` on one stream.

## Control the board

```c
nbp_board_status_t b;
nebulosa_board_status(d, &b);
printf("V_MAIN %u mV, PD %u mV\n", b.v_main_mv, b.pd_mv);

nebulosa_set_leds(d, 0x15);                         /* manual LEDs */
nebulosa_leds_auto(d);                              /* back to the firmware */
nebulosa_ui_key(d, NBP_KEY_DOWN);
nebulosa_board_set(d, NBP_BSET_APWR_H, 1, &b);      /* NBP_ST_NO_POWER without 12 V */
nebulosa_set_source(d, NBP_SRC_AD7387);

nebulosa_board_test(d, NBP_TEST_ALL, &b);           /* stop streaming first */
if (nebulosa_test_result(&b, NBP_TEST_FLASH_ID) != NBP_TR_PASS) { /* ... */ }
```

## Flash and updates

```c
static void progress(void *ctx, uint32_t done, uint32_t total)
{
    (void)ctx;
    fprintf(stderr, "\r%u / %u", done, total);
}

nbp_update_info_t ui;
int e = nebulosa_update_gateware(d, bit, bit_len, progress, NULL, &ui);  /* .bit or .bin */
if (e > 0)
    fprintf(stderr, "refused: %s (%s)\n", nebulosa_strerror(e), ui.msg);

e = nebulosa_update_firmware(d, nbfw, nbfw_len, progress, NULL, &ui);    /* restarts the device */
```

Lower level: `nebulosa_flash_read` (≤ 256 bytes), `nebulosa_flash_write`
(one 256-byte page), `nebulosa_flash_erase` (4 KB aligned),
`nebulosa_upload_flash` (erase + write + verify), `nebulosa_fpga_config`,
`nebulosa_update`, `nebulosa_crc32`.

## Calibration

```c
nebulosa_calibration_t cal;
nebulosa_calibration_default(&cal);
cal.i_offset[5] = 3e-9;                             /* A */
nebulosa_set_calibration(d, &cal);
```

## Raw commands

```c
uint8_t rsp[512];
size_t len;
int e = nebulosa_request(d, NBP_CMD_GET_RANGE, NULL, 0, rsp, sizeof(rsp), &len, 0);
```

## Examples

`c/examples` builds `nebulosa_info`, `nebulosa_stream_csv`,
`nebulosa_average_current`, `nebulosa_board_leds`:

```sh
./c/build/nebulosa_average_current --usb --seconds 10 --log2n 0
./c/build/nebulosa_stream_csv --host 192.168.1.20 --seconds 5 --log2n 4 --out run.csv
```

Each takes `--host H [--port P]`, `--usb [SERIAL]`, or nothing (discovery).
