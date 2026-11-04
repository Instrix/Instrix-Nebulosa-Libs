/*
 * Nebulosa FPGA stream, format 5 (FPGA ID 0x4E42000B): blocks of samples.
 * Gateware rtl/block_framer.v, docs/qspi-driver-manual.md "Byte layouts".
 *
 * Every FIFO word (after neb_word(): host order) is one of, by bits 31:29:
 *   sample  1 d 0   [30] DIRTY  [27:14] current  [13:0] voltage (14-bit codes)
 *   header  0 1 0   [28:27] type, then:
 *     BLOCK (0)  [26:11] block sequence number (+1 per block)  [10] OVF
 *                [9] decimation mode (0 average, 1 subsample)  [8:5] log2 N
 *                [4:2] range  [1] source (0 stub, 1 ADC)
 *     TS_HI (1)  [23:0] timestamp[47:24]
 *     TS_LO (2)  [23:0] timestamp[23:0]
 *   empty   0x00000000 (the FIFO was empty)    late: 0x20000000 (LATE)
 * A block is BLOCK, TS_HI, TS_LO (empty words may fall in between), then
 * 1..256 samples; it ends where the next BLOCK begins. The timestamp is the
 * block's first sample's index in ADC sample periods (500 ns) since FPGA
 * reset; sample k of the block was taken at timestamp + k * 2^log2N. Every
 * sample shares its block's range (the current comes from that range's
 * chain: 0/1 A, 2/3 B, 4/5 C), source and decimation.
 *
 * neb_stream_feed() takes one word at a time and keeps the position, so a
 * block can span bursts (and NBP blocks).
 */
#ifndef NEB_STREAM_H
#define NEB_STREAM_H

#include <stdint.h>

#define NEB_S_SAMPLE            (1u << 31)
#define NEB_S_DIRTY             (1u << 30)
#define NEB_S_LATE_WORD         0x20000000u
#define NEB_S_HDR_TAG           2u              /* bits 31:29 of a header word */
#define NEB_S_BLOCK_MAX         256u

enum { NEB_HDR_BLOCK = 0, NEB_HDR_TS_HI = 1, NEB_HDR_TS_LO = 2 };

static inline int      neb_s_is_sample(uint32_t w)  { return (int)(w >> 31); }
static inline int      neb_s_is_header(uint32_t w)  { return (w >> 29) == NEB_S_HDR_TAG; }
static inline uint8_t  neb_s_hdr_type(uint32_t w)   { return (uint8_t)((w >> 27) & 3); }
static inline uint16_t neb_s_current(uint32_t w)    { return (uint16_t)((w >> 14) & 0x3FFF); }
static inline uint16_t neb_s_voltage(uint32_t w)    { return (uint16_t)(w & 0x3FFF); }
static inline uint8_t  neb_s_dirty(uint32_t w)      { return (uint8_t)((w >> 30) & 1); }
/* BLOCK fields */
static inline uint16_t neb_s_seq(uint32_t w)        { return (uint16_t)((w >> 11) & 0xFFFF); }
static inline uint8_t  neb_s_ovf(uint32_t w)        { return (uint8_t)((w >> 10) & 1); }
static inline uint8_t  neb_s_dmode(uint32_t w)      { return (uint8_t)((w >> 9) & 1); }
static inline uint8_t  neb_s_log2n(uint32_t w)      { return (uint8_t)((w >> 5) & 0xF); }
static inline uint8_t  neb_s_range(uint32_t w)      { return (uint8_t)((w >> 2) & 7); }
static inline uint8_t  neb_s_src(uint32_t w)        { return (uint8_t)((w >> 1) & 1); }

/* what one word was */
typedef enum
{
    NEB_SW_SKIP   = 0,      /* empty, LATE, or a header word (state updated) */
    NEB_SW_SAMPLE = 1,      /* *out holds a sample */
    NEB_SW_BLOCK  = 2,      /* a block's header is complete: block fields valid */
    NEB_SW_ERROR  = -1,     /* malformed: a word out of place (counted) */
} neb_sw_t;

typedef struct
{
    uint64_t ts;            /* timestamp, ADC sample periods (500 ns) since FPGA reset */
    uint16_t current;       /* 14-bit code, the chain of `range` */
    uint16_t voltage;       /* 14-bit code */
    uint8_t  range;
    uint8_t  dirty;
} neb_sample_t;

typedef struct
{
    /* the current block */
    uint64_t ts0;           /* timestamp of its first sample */
    uint64_t ts_next;       /* timestamp of its next sample */
    uint32_t step;          /* 2^log2N */
    uint16_t seq;
    uint16_t k;             /* samples so far in it */
    uint8_t  range, dmode, log2n, src, ovf;
    uint8_t  hdr_need;      /* header words still due: 2 TS_HI, 1 TS_LO, 0 in samples */
    uint8_t  synced;        /* a complete header seen */
    uint8_t  have_prev;     /* a previous block exists (ts_next, seq valid) */
    uint16_t prev_seq;
    uint64_t prev_ts_next;  /* where the previous block left off */

    /* counters, since neb_stream_init() */
    uint32_t samples;
    uint32_t dirty;
    uint32_t blocks;
    uint32_t empty;
    uint32_t late;
    uint32_t ovf_blocks;    /* blocks carrying OVF (samples dropped before them: DROP_COUNT) */
    uint32_t range_changes;
    uint32_t seq_errors;    /* a block's sequence number isn't the last + 1: words lost or repeated */
    uint32_t malformed;     /* words out of place (a sample before a complete header, ...) */
    uint32_t unframed;      /* samples skipped before the first header */
    uint64_t gap_periods;   /* sample periods between blocks without samples (HOLD, ADC frames
                               lost, OVF); with OVF they are the drops */
} neb_stream_t;

void     neb_stream_init(neb_stream_t *s);
neb_sw_t neb_stream_feed(neb_stream_t *s, uint32_t w, neb_sample_t *out);

#endif
