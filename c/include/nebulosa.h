/*
 * nebulosa.h - C library for the INSTRIX Nebulosa power profiler.
 *
 * Talks NBP (Nebulosa Bulk Protocol, nbp.h) to the device over TCP or USB
 * (libusb-1.0, when built with NEBULOSA_WITH_USB), without the Companion
 * app. See docs/API.md for the model shared by every language binding.
 *
 * Errors: functions return NEBULOSA_OK (0), a negative NEBULOSA_E_* code
 * (library / transport), or a positive NBP_ST_* code (the device refused
 * the command). nebulosa_strerror() names both.
 *
 * Threads: a nebulosa_t is not thread safe; use it from one thread (or lock).
 * Records (nbp_info_t, nbp_board_status_t, ...) are nbp.h's packed structs,
 * filled from the little-endian wire bytes; this library needs a
 * little-endian host (x86, ARM, RISC-V: all of them in practice).
 */
#ifndef NEBULOSA_H
#define NEBULOSA_H

#include <stddef.h>
#include <stdint.h>
/* nbp.h is the firmware's header, verbatim: GCC / Clang syntax for packing.
 * MSVC gets the same layout through #pragma pack. */
#if defined(_MSC_VER) && !defined(__clang__)
#  pragma pack(push, 1)
#  define __attribute__(x)
#  include "nbp.h"
#  undef __attribute__
#  pragma pack(pop)
#else
#  include "nbp.h"
#endif

#ifdef __cplusplus
extern "C" {
#endif

#if defined(_WIN32) && defined(NEBULOSA_SHARED)
#  ifdef NEBULOSA_BUILD
#    define NEBULOSA_API __declspec(dllexport)
#  else
#    define NEBULOSA_API __declspec(dllimport)
#  endif
#else
#  define NEBULOSA_API
#endif

#define NEBULOSA_VERSION_MAJOR 0
#define NEBULOSA_VERSION_MINOR 1
#define NEBULOSA_VERSION_PATCH 0

/* ---- errors -------------------------------------------------------------- */

enum
{
    NEBULOSA_OK             = 0,
    NEBULOSA_E_IO           = -1,   /* socket / USB transfer error */
    NEBULOSA_E_TIMEOUT      = -2,   /* no response in time */
    NEBULOSA_E_CLOSED       = -3,   /* the device closed the connection */
    NEBULOSA_E_BAD_RESPONSE = -4,   /* response too short / malformed */
    NEBULOSA_E_ARG          = -5,   /* bad argument */
    NEBULOSA_E_NOMEM        = -6,
    NEBULOSA_E_NOT_FOUND    = -7,   /* no such host / USB device */
    NEBULOSA_E_UNSUPPORTED  = -8,   /* built without USB, ... */
    NEBULOSA_E_VERIFY       = -9,   /* flash read-back differs */
};

/* Name of a NEBULOSA_E_* or NBP_ST_* code. */
NEBULOSA_API const char *nebulosa_strerror(int err);

/* ---- connection ---------------------------------------------------------- */

typedef struct nebulosa_s nebulosa_t;

#define NEBULOSA_DEFAULT_TIMEOUT_MS 1500

/* Connect over TCP (port 0 = NBP_TCP_PORT). One client at a time: close the
 * Companion first. */
NEBULOSA_API int  nebulosa_open_tcp(nebulosa_t **out, const char *host, uint16_t port, int timeout_ms);

/* Open the index-th NBP USB device whose serial matches (serial NULL: any).
 * Needs NEBULOSA_WITH_USB, else NEBULOSA_E_UNSUPPORTED. */
NEBULOSA_API int  nebulosa_open_usb(nebulosa_t **out, const char *serial, int index);

NEBULOSA_API void nebulosa_close(nebulosa_t *d);

/* Default timeout for commands that don't take one (ms). */
NEBULOSA_API void nebulosa_set_timeout(nebulosa_t *d, int timeout_ms);

/* A short text, e.g. "TCP 192.168.1.20:20034" or "USB 3:7 (High-Speed)". */
NEBULOSA_API const char *nebulosa_description(const nebulosa_t *d);

/* ---- discovery ----------------------------------------------------------- */

typedef struct
{
    char       host[64];        /* where the answer came from: connect here */
    nbp_info_t info;
} nebulosa_net_device_t;

/* UDP broadcast discovery (timeout_ms <= 0: 600). Returns the number of
 * devices written to out (at most max), or a negative error. */
NEBULOSA_API int nebulosa_discover(nebulosa_net_device_t *out, int max, int timeout_ms);

typedef struct
{
    uint8_t  bus, address;
    uint16_t vid, pid;          /* NBP_USB_PID_HS / NBP_USB_PID_FS */
    char     serial[64];
    char     product[64];
} nebulosa_usb_device_t;

/* NBP USB devices. Returns the count written (at most max) or an error. */
NEBULOSA_API int nebulosa_list_usb(nebulosa_usb_device_t *out, int max);

/* ---- raw request --------------------------------------------------------- */

/* One command and its response payload. rsp may be NULL (rsp_max 0) when
 * the payload isn't wanted. *rsp_len (may be NULL) gets the full response
 * payload length, even if larger than rsp_max (the rest is cut). DATA that
 * arrives meanwhile is queued for nebulosa_read_samples(). timeout_ms <= 0:
 * the default. */
NEBULOSA_API int nebulosa_request(nebulosa_t *d, uint8_t cmd, const void *payload, uint16_t len,
                                  void *rsp, size_t rsp_max, size_t *rsp_len, int timeout_ms);

/* ---- commands ------------------------------------------------------------ */

NEBULOSA_API int nebulosa_ping(nebulosa_t *d, const void *data, uint16_t len, void *echo, size_t echo_max);
NEBULOSA_API int nebulosa_get_info(nebulosa_t *d, nbp_info_t *out);
NEBULOSA_API int nebulosa_get_stats(nebulosa_t *d, nbp_stats_t *out);
NEBULOSA_API int nebulosa_reg_read(nebulosa_t *d, uint8_t index, uint32_t *value);
NEBULOSA_API int nebulosa_reg_write(nebulosa_t *d, uint8_t index, uint32_t value);
NEBULOSA_API int nebulosa_reset(nebulosa_t *d);

NEBULOSA_API int nebulosa_set_source(nebulosa_t *d, uint8_t source);            /* NBP_SRC_* */
NEBULOSA_API int nebulosa_set_range_auto(nebulosa_t *d, uint8_t min, uint8_t max);
NEBULOSA_API int nebulosa_set_range_manual(nebulosa_t *d, uint8_t range);
NEBULOSA_API int nebulosa_get_range(nebulosa_t *d, nbp_range_t *out);
NEBULOSA_API int nebulosa_set_range_timing(nebulosa_t *d, uint8_t up_filt, uint8_t down_filt,
                                           uint8_t overlap, uint16_t hold);
NEBULOSA_API int nebulosa_set_range_force(nebulosa_t *d, int enable, uint8_t value);
/* log2n 0..8, mode NBP_DECIM_*; *rate_hz (may be NULL): the rate applied */
NEBULOSA_API int nebulosa_set_decimation(nebulosa_t *d, uint8_t log2n, uint8_t mode, uint32_t *rate_hz);

/* board (NBP_CAP_BOARD); out may be NULL */
NEBULOSA_API int nebulosa_board_status(nebulosa_t *d, nbp_board_status_t *out);
NEBULOSA_API int nebulosa_board_set(nebulosa_t *d, uint8_t item, uint32_t value, nbp_board_status_t *out);
NEBULOSA_API int nebulosa_board_test(nebulosa_t *d, uint32_t mask, nbp_board_status_t *out);
NEBULOSA_API int nebulosa_set_leds(nebulosa_t *d, uint8_t mask);   /* manual LEDs */
NEBULOSA_API int nebulosa_leds_auto(nebulosa_t *d);                /* back to the status pattern */
NEBULOSA_API int nebulosa_ui_key(nebulosa_t *d, uint8_t key);      /* NBP_KEY_* */

/* SPI flash. read: len <= NBP_FLASH_CHUNK; write: within one 256-byte page;
 * erase: 4 KB aligned. */
NEBULOSA_API int nebulosa_flash_read(nebulosa_t *d, uint32_t addr, void *buf, uint32_t len);
NEBULOSA_API int nebulosa_flash_write(nebulosa_t *d, uint32_t addr, const void *data, uint32_t len);
NEBULOSA_API int nebulosa_flash_erase(nebulosa_t *d, uint32_t addr, uint32_t len);
NEBULOSA_API int nebulosa_fpga_config(nebulosa_t *d, uint32_t addr, uint32_t len, nbp_board_status_t *out);

/* Erase (whole 4 KB sectors), program and read back len bytes at addr
 * (4 KB aligned). progress (may be NULL) gets bytes done / total. */
typedef void (*nebulosa_progress_fn)(void *ctx, uint32_t done, uint32_t total);
NEBULOSA_API int nebulosa_upload_flash(nebulosa_t *d, uint32_t addr, const void *data, uint32_t len,
                                       nebulosa_progress_fn progress, void *ctx);

/* software update (nbp.h): op NBP_UPD_*; out may be NULL */
NEBULOSA_API int nebulosa_update(nebulosa_t *d, uint8_t op, uint8_t slot, uint32_t len, uint32_t crc,
                                 nbp_update_info_t *out);
NEBULOSA_API int nebulosa_update_info(nebulosa_t *d, nbp_update_info_t *out);
/* The whole flow: upload into the update slot, then NBP_UPD_GATEWARE /
 * NBP_UPD_FIRMWARE. A firmware update restarts the device afterwards. */
NEBULOSA_API int nebulosa_update_gateware(nebulosa_t *d, const void *image, uint32_t len,
                                          nebulosa_progress_fn progress, void *ctx, nbp_update_info_t *out);
NEBULOSA_API int nebulosa_update_firmware(nebulosa_t *d, const void *image, uint32_t len,
                                          nebulosa_progress_fn progress, void *ctx, nbp_update_info_t *out);

/* zlib CRC-32 */
NEBULOSA_API uint32_t nebulosa_crc32(const void *data, size_t len);

/* ---- calibration --------------------------------------------------------- */

#define NEBULOSA_RANGES 8

typedef struct
{
    double i_fs[NEBULOSA_RANGES];       /* full scale per current range, A (0 = highest) */
    double i_offset[NEBULOSA_RANGES];   /* A, added after scaling */
    double v_scale;                     /* V per code */
    double v_offset;                    /* V */
} nebulosa_calibration_t;

/* The firmware's own scaling: 2 A, 200 mA, 20 mA, 2 mA, 200 uA, 20 uA; 1 mV/code. */
NEBULOSA_API void nebulosa_calibration_default(nebulosa_calibration_t *c);
NEBULOSA_API void nebulosa_set_calibration(nebulosa_t *d, const nebulosa_calibration_t *c);
NEBULOSA_API void nebulosa_get_calibration(const nebulosa_t *d, nebulosa_calibration_t *c);

/* ---- stream decoder (FPGA format 5, neb_stream.h) ------------------------- */

typedef struct
{
    uint64_t ts;            /* ADC periods (500 ns) since FPGA reset */
    uint32_t step;          /* 2^log2N: ts distance to the next sample */
    uint16_t current_code;  /* 14-bit */
    uint16_t voltage_code;  /* 14-bit */
    uint8_t  range;         /* 0 = highest current range */
    uint8_t  dirty;         /* may be unreliable (range switching) */
    double   current;       /* A (calibrated) */
    double   voltage;       /* V (calibrated) */
} nebulosa_sample_t;

#define NEBULOSA_TS_PERIOD_S 500e-9
static inline double nebulosa_sample_time(const nebulosa_sample_t *s) { return (double)s->ts * NEBULOSA_TS_PERIOD_S; }

typedef struct
{
    uint64_t samples, dirty, blocks, empty, late, ovf_blocks, range_changes,
             seq_errors, malformed, unframed, gap_periods;
} nebulosa_decoder_counters_t;

typedef struct
{
    uint64_t ts0, ts_next, prev_ts_next;
    uint32_t step;
    uint16_t seq, k, prev_seq;
    uint8_t  range, dmode, log2n, src, ovf, hdr_need, synced, have_prev;
    nebulosa_decoder_counters_t c;
} nebulosa_decoder_t;

enum { NEBULOSA_W_SKIP = 0, NEBULOSA_W_SAMPLE = 1, NEBULOSA_W_BLOCK = 2, NEBULOSA_W_ERROR = -1 };

/* Standalone: feed host-order words (e.g. from files you recorded). */
NEBULOSA_API void nebulosa_decoder_init(nebulosa_decoder_t *s);
NEBULOSA_API void nebulosa_decoder_desync(nebulosa_decoder_t *s);
/* NEBULOSA_W_*; on NEBULOSA_W_SAMPLE *out has the sample (current/voltage
 * left 0: apply a calibration yourself) */
NEBULOSA_API int  nebulosa_decoder_feed(nebulosa_decoder_t *s, uint32_t w, nebulosa_sample_t *out);

/* ---- streaming ----------------------------------------------------------- */

#define NEBULOSA_START_FLUSH     NBP_START_FLUSH
#define NEBULOSA_START_LINK_TEST NBP_START_LINK_TEST

/* flags NBP_START_* (NBP_START_FLUSH is the usual choice) */
NEBULOSA_API int nebulosa_start_stream(nebulosa_t *d, uint32_t flags);
NEBULOSA_API int nebulosa_stop_stream(nebulosa_t *d);

/* Up to max decoded samples: whatever is there, waiting up to timeout_ms
 * (0: don't wait) for the first. Returns the count (0 on timeout) or an
 * error. */
NEBULOSA_API int nebulosa_read_samples(nebulosa_t *d, nebulosa_sample_t *buf, size_t max, int timeout_ms);

/* One raw DATA block instead (for format-4 FPGAs, or your own decoding):
 * header fields and nwords host-order words. Returns 1 for a block, 0 on
 * timeout, or an error. Don't mix with read_samples on the same stream. */
typedef struct
{
    uint32_t seq, t_us;
    uint16_t nwords, flags;     /* NBP_BLK_* */
    uint32_t words[NBP_BLOCK_WORDS];
} nebulosa_block_t;
NEBULOSA_API int nebulosa_read_block(nebulosa_t *d, nebulosa_block_t *out, int timeout_ms);

typedef struct
{
    nebulosa_decoder_counters_t decoder;
    uint64_t data_blocks, lost_blocks, stall_flags, gap_flags, test_blocks, resyncs;
    uint64_t queued_blocks;     /* DATA blocks waiting in the library */
} nebulosa_stream_counters_t;

NEBULOSA_API void nebulosa_stream_counters(const nebulosa_t *d, nebulosa_stream_counters_t *out);

/* ---- small helpers --------------------------------------------------------- */

static inline uint32_t nebulosa_test_result(const nbp_board_status_t *b, int t) { return NBP_TEST_RESULT(b->test_results, t); }
static inline uint32_t nebulosa_version_major(uint32_t v) { return v >> 16; }
static inline uint32_t nebulosa_version_minor(uint32_t v) { return (v >> 8) & 0xFF; }
static inline uint32_t nebulosa_version_patch(uint32_t v) { return v & 0xFF; }

#ifdef __cplusplus
}
#endif

#endif
