/*
 * nebulosa.c - NBP host library: framing, requests, stream decoding, TCP /
 * UDP / USB transports. See nebulosa.h and docs/API.md.
 */
#if !defined(_WIN32) && !defined(_POSIX_C_SOURCE)
#define _POSIX_C_SOURCE 200809L
#endif
#if defined(__APPLE__) && !defined(_DARWIN_C_SOURCE)
#define _DARWIN_C_SOURCE
#endif

#include "nebulosa.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(__BYTE_ORDER__) && __BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__
#error "libnebulosa copies nbp.h's little-endian records directly: it needs a little-endian host"
#endif

#ifdef _WIN32
#  ifndef WIN32_LEAN_AND_MEAN
#    define WIN32_LEAN_AND_MEAN
#  endif
#  include <winsock2.h>
#  include <ws2tcpip.h>
#  include <windows.h>
typedef SOCKET sock_t;
#  define BAD_SOCK INVALID_SOCKET
#  define sock_close closesocket
#  define poll WSAPoll
#  ifdef _MSC_VER
#    pragma comment(lib, "ws2_32.lib")
#  endif
#else
#  include <arpa/inet.h>
#  include <errno.h>
#  include <fcntl.h>
#  include <netdb.h>
#  include <netinet/in.h>
#  include <netinet/tcp.h>
#  include <poll.h>
#  include <sys/socket.h>
#  include <time.h>
#  include <unistd.h>
typedef int sock_t;
#  define BAD_SOCK (-1)
#  define sock_close close
#endif

#ifdef NEBULOSA_WITH_USB
#  include <libusb.h>
#endif

#define HDR_LEN      8
#define MAX_DATA_LEN (NBP_BLOCK_BYTES - HDR_LEN)    /* 2040: largest payload of any message */
#define MAX_SRC      4                              /* IN endpoints read (USB) */
#define RX_CHUNK     (64 * 1024)
#define USB_QUEUE    8                              /* IN transfers kept queued per endpoint */
#define USB_XFER     (32 * 1024)

/* ---- small helpers ---------------------------------------------------------- */

static uint16_t le16(const uint8_t *p) { return (uint16_t)(p[0] | p[1] << 8); }
static uint32_t le32(const uint8_t *p) { return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24; }
static uint32_t be32(const uint8_t *p) { return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | (uint32_t)p[3]; }
static void     put16(uint8_t *p, uint16_t v) { p[0] = (uint8_t)v; p[1] = (uint8_t)(v >> 8); }
static void     put32(uint8_t *p, uint32_t v) { p[0] = (uint8_t)v; p[1] = (uint8_t)(v >> 8); p[2] = (uint8_t)(v >> 16); p[3] = (uint8_t)(v >> 24); }

static int64_t now_ms(void)
{
#ifdef _WIN32
    return (int64_t)GetTickCount64();
#else
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (int64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
#endif
}

static int net_init(void)
{
#ifdef _WIN32
    static int done;
    if (!done)
    {
        WSADATA w;
        if (WSAStartup(MAKEWORD(2, 2), &w) != 0)
            return NEBULOSA_E_IO;
        done = 1;
    }
#endif
    return NEBULOSA_OK;
}

const char *nebulosa_strerror(int err)
{
    switch (err)
    {
    case NEBULOSA_OK:             return "OK";
    case NEBULOSA_E_IO:           return "I/O error";
    case NEBULOSA_E_TIMEOUT:      return "timeout: no response";
    case NEBULOSA_E_CLOSED:       return "connection closed";
    case NEBULOSA_E_BAD_RESPONSE: return "malformed response";
    case NEBULOSA_E_ARG:          return "bad argument";
    case NEBULOSA_E_NOMEM:        return "out of memory";
    case NEBULOSA_E_NOT_FOUND:    return "device not found";
    case NEBULOSA_E_UNSUPPORTED:  return "not supported (built without USB?)";
    case NEBULOSA_E_VERIFY:       return "flash verify failed";
    case NBP_ST_UNKNOWN:          return "device: UNKNOWN (unknown command)";
    case NBP_ST_BAD_LEN:          return "device: BAD_LEN (payload length wrong)";
    case NBP_ST_BAD_ARG:          return "device: BAD_ARG (argument out of range)";
    case NBP_ST_FPGA:             return "device: FPGA (FPGA not answering)";
    case NBP_ST_BUSY:             return "device: BUSY (not possible now, stream running?)";
    case NBP_ST_HW:               return "device: HW (hardware failed)";
    case NBP_ST_NO_POWER:         return "device: NO_POWER (analog supplies need a 12 V source)";
    case NBP_ST_PROTECTED:        return "device: PROTECTED (flash range in use)";
    default:                      return err > 0 ? "device: unknown status" : "unknown error";
    }
}

uint32_t nebulosa_crc32(const void *data, size_t len)
{
    const uint8_t *p = (const uint8_t *)data;
    uint32_t c = 0xFFFFFFFFu;
    while (len--)
    {
        c ^= *p++;
        for (int k = 0; k < 8; k++)
            c = (c >> 1) ^ (0xEDB88320u & (0u - (c & 1)));
    }
    return ~c;
}

/* ---- calibration ---------------------------------------------------------- */

void nebulosa_calibration_default(nebulosa_calibration_t *c)
{
    static const double fs[NEBULOSA_RANGES] = { 2.0, 0.2, 20e-3, 2e-3, 200e-6, 20e-6, 0, 0 };
    memset(c, 0, sizeof(*c));
    memcpy(c->i_fs, fs, sizeof(fs));
    c->v_scale = 1e-3;
}

/* ---- stream decoder (port of Companion nbp-core stream.rs / neb_stream.c) -- */

void nebulosa_decoder_init(nebulosa_decoder_t *s) { memset(s, 0, sizeof(*s)); }

void nebulosa_decoder_desync(nebulosa_decoder_t *s)
{
    s->synced = 0;
    s->have_prev = 0;
}

int nebulosa_decoder_feed(nebulosa_decoder_t *s, uint32_t w, nebulosa_sample_t *out)
{
    if (w >> 31)
    {
        if (!s->synced)
        {
            s->c.unframed++;
            return NEBULOSA_W_SKIP;
        }
        if (s->hdr_need)
        {
            s->c.malformed++;
            return NEBULOSA_W_ERROR;
        }
        if (out)
        {
            out->ts = s->ts_next;
            out->step = s->step;
            out->current_code = (uint16_t)((w >> 14) & 0x3FFF);
            out->voltage_code = (uint16_t)(w & 0x3FFF);
            out->range = s->range;
            out->dirty = (uint8_t)((w >> 30) & 1);
            out->current = 0;
            out->voltage = 0;
        }
        s->ts_next += s->step;
        s->k++;
        s->c.samples++;
        s->c.dirty += (w >> 30) & 1;
        return NEBULOSA_W_SAMPLE;
    }

    if (w >> 29 == 2)
    {
        switch ((w >> 27) & 3)
        {
        case 0:     /* BLOCK */
        {
            uint16_t seq = (uint16_t)((w >> 11) & 0xFFFF);
            uint8_t range = (uint8_t)((w >> 2) & 7);
            if (s->synced && s->hdr_need)
                s->c.malformed++;               /* the last header never completed */
            if (s->synced)
            {
                s->have_prev = 1;
                s->prev_seq = s->seq;
                s->prev_ts_next = s->ts_next;
            }
            if (s->have_prev && seq != (uint16_t)(s->prev_seq + 1))
                s->c.seq_errors++;
            if (s->synced && range != s->range)
                s->c.range_changes++;
            s->seq = seq;
            s->ovf = (uint8_t)((w >> 10) & 1);
            s->dmode = (uint8_t)((w >> 9) & 1);
            s->log2n = (uint8_t)((w >> 5) & 0xF);
            s->range = range;
            s->src = (uint8_t)((w >> 1) & 1);
            s->step = 1u << s->log2n;
            s->k = 0;
            s->hdr_need = 2;
            s->synced = 1;
            s->c.ovf_blocks += s->ovf;
            return NEBULOSA_W_SKIP;
        }
        case 1:     /* TS_HI */
            if (!s->synced)
                return NEBULOSA_W_SKIP;
            if (s->hdr_need != 2)
            {
                s->c.malformed++;
                return NEBULOSA_W_ERROR;
            }
            s->ts0 = (uint64_t)(w & 0xFFFFFF) << 24;
            s->hdr_need = 1;
            return NEBULOSA_W_SKIP;
        case 2:     /* TS_LO */
            if (!s->synced)
                return NEBULOSA_W_SKIP;
            if (s->hdr_need != 1)
            {
                s->c.malformed++;
                return NEBULOSA_W_ERROR;
            }
            s->ts0 |= w & 0xFFFFFF;
            s->ts_next = s->ts0;
            s->hdr_need = 0;
            s->c.blocks++;
            if (s->have_prev)
            {
                if (s->ts0 >= s->prev_ts_next)
                    s->c.gap_periods += s->ts0 - s->prev_ts_next;
                else
                    s->c.malformed++;           /* overlaps the previous block */
            }
            return NEBULOSA_W_BLOCK;
        default:
            s->c.malformed++;
            return NEBULOSA_W_ERROR;
        }
    }

    if (w == 0)
        s->c.empty++;
    else if (w == 0x20000000u)
        s->c.late++;
    else
    {
        s->c.malformed++;
        return NEBULOSA_W_ERROR;
    }
    return NEBULOSA_W_SKIP;
}

/* ---- the connection ---------------------------------------------------------- */

typedef struct
{
    uint8_t *buf;
    size_t   len, cap;
} rxbuf_t;

typedef struct
{
    uint16_t len;
    uint8_t  p[MAX_DATA_LEN];
} data_msg_t;

enum { T_TCP = 1, T_USB = 2 };

struct nebulosa_s
{
    int      kind;
    sock_t   sock;
#ifdef NEBULOSA_WITH_USB
    libusb_context       *usb;
    libusb_device_handle *uh;
    int                   iface;
    uint8_t               ep_out;
    uint8_t               ep_in[MAX_SRC];
    struct libusb_transfer *xfer[MAX_SRC * USB_QUEUE];
    int                   xfer_active;
    int                   usb_err;          /* set by a transfer callback */
#endif
    int      nsrc;
    rxbuf_t  rx[MAX_SRC];
    uint8_t *chunk;             /* RX_CHUNK read buffer */
    int      closed;
    int      streaming;         /* a STREAM_START was answered OK */

    int      timeout_ms;
    uint16_t seq;
    char     desc[96];

    /* the response being waited for */
    int      waiting;
    uint8_t  want_type;
    uint16_t want_seq;
    int      got;
    uint8_t  got_status;
    uint16_t got_len;
    uint8_t  got_p[MAX_DATA_LEN];

    /* DATA queue: ring of messages */
    data_msg_t *q;
    size_t      q_cap, q_head, q_count;

    /* stream */
    nebulosa_calibration_t cal;
    nebulosa_decoder_t     dec;
    nebulosa_sample_t      pend[NBP_BLOCK_WORDS];
    size_t                 pend_pos, pend_n;
    int                    have_expect;
    uint32_t               expect_seq;
    nebulosa_stream_counters_t cnt;
};

#ifdef NEBULOSA_WITH_USB
static void usb_stop_transfers(nebulosa_t *d);
#endif

static nebulosa_t *conn_new(void)
{
    nebulosa_t *d = (nebulosa_t *)calloc(1, sizeof(*d));
    if (!d)
        return NULL;
    d->chunk = (uint8_t *)malloc(RX_CHUNK);
    if (!d->chunk)
    {
        free(d);
        return NULL;
    }
    d->sock = BAD_SOCK;
    d->timeout_ms = NEBULOSA_DEFAULT_TIMEOUT_MS;
    nebulosa_calibration_default(&d->cal);
    nebulosa_decoder_init(&d->dec);
    return d;
}

void nebulosa_set_timeout(nebulosa_t *d, int timeout_ms) { d->timeout_ms = timeout_ms > 0 ? timeout_ms : NEBULOSA_DEFAULT_TIMEOUT_MS; }
const char *nebulosa_description(const nebulosa_t *d) { return d->desc; }
void nebulosa_set_calibration(nebulosa_t *d, const nebulosa_calibration_t *c) { d->cal = *c; }
void nebulosa_get_calibration(const nebulosa_t *d, nebulosa_calibration_t *c) { *c = d->cal; }

void nebulosa_close(nebulosa_t *d)
{
    if (!d)
        return;
    if (d->streaming && !d->closed)
        nebulosa_request(d, NBP_CMD_STREAM_STOP, NULL, 0, NULL, 0, NULL, 300);
    if (d->sock != BAD_SOCK)
        sock_close(d->sock);
#ifdef NEBULOSA_WITH_USB
    if (d->uh)
    {
        usb_stop_transfers(d);
        libusb_release_interface(d->uh, d->iface);
        libusb_close(d->uh);
    }
    if (d->usb)
        libusb_exit(d->usb);
#endif
    for (int i = 0; i < MAX_SRC; i++)
        free(d->rx[i].buf);
    free(d->q);
    free(d->chunk);
    free(d);
}

/* ---- DATA queue ---------------------------------------------------------- */

static int q_push(nebulosa_t *d, const uint8_t *p, uint16_t len)
{
    if (d->q_count == d->q_cap)
    {
        size_t cap = d->q_cap ? d->q_cap * 2 : 64;
        data_msg_t *n = (data_msg_t *)malloc(cap * sizeof(*n));
        if (!n)
            return NEBULOSA_E_NOMEM;
        for (size_t i = 0; i < d->q_count; i++)
            n[i] = d->q[(d->q_head + i) % d->q_cap];
        free(d->q);
        d->q = n;
        d->q_cap = cap;
        d->q_head = 0;
    }
    data_msg_t *m = &d->q[(d->q_head + d->q_count) % d->q_cap];
    m->len = len;
    memcpy(m->p, p, len);
    d->q_count++;
    return NEBULOSA_OK;
}

static data_msg_t *q_front(nebulosa_t *d) { return d->q_count ? &d->q[d->q_head] : NULL; }

static void q_pop(nebulosa_t *d)
{
    d->q_head = (d->q_head + 1) % d->q_cap;
    d->q_count--;
}

/* ---- framing ------------------------------------------------------------------ */

static int on_frame(nebulosa_t *d, const uint8_t *h, const uint8_t *p)
{
    uint8_t ty = h[2];
    uint16_t seq = le16(h + 4), len = le16(h + 6);
    if (ty == NBP_DATA)
        return q_push(d, p, len);
    if (d->waiting && !d->got && ty == d->want_type && seq == d->want_seq)
    {
        d->got = 1;
        d->got_status = h[3];
        d->got_len = len;
        memcpy(d->got_p, p, len);
    }
    /* anything else: a stale response, dropped */
    return NEBULOSA_OK;
}

/* Append bytes of source src and cut out every whole message. */
static int feed(nebulosa_t *d, int src, const uint8_t *data, size_t n)
{
    rxbuf_t *r = &d->rx[src];
    if (r->len + n > r->cap)
    {
        size_t cap = r->cap ? r->cap : 4096;
        while (cap < r->len + n)
            cap *= 2;
        uint8_t *b = (uint8_t *)realloc(r->buf, cap);
        if (!b)
            return NEBULOSA_E_NOMEM;
        r->buf = b;
        r->cap = cap;
    }
    memcpy(r->buf + r->len, data, n);
    r->len += n;

    size_t pos = 0;
    int err = NEBULOSA_OK;
    while (err == NEBULOSA_OK)
    {
        uint8_t *b = r->buf + pos;
        size_t left = r->len - pos;
        if (left < 2)
            break;
        if (b[0] != 'N' || b[1] != 'B')
        {
            /* garbage: skip to the next "NB", or keep a trailing 'N' */
            size_t i = 1;
            d->cnt.resyncs++;
            while (i + 1 < left && !(b[i] == 'N' && b[i + 1] == 'B'))
                i++;
            if (i + 1 >= left && b[left - 1] != 'N')
                i = left;
            else if (i + 1 >= left)
                i = left - 1;
            pos += i;
            continue;
        }
        if (left < HDR_LEN)
            break;
        uint16_t len = le16(b + 6);
        if (len > MAX_DATA_LEN)
        {
            d->cnt.resyncs++;
            pos++;
            continue;
        }
        if (left < (size_t)HDR_LEN + len)
            break;
        err = on_frame(d, b, b + HDR_LEN);
        pos += HDR_LEN + len;
    }
    memmove(r->buf, r->buf + pos, r->len - pos);
    r->len -= pos;
    return err;
}

/* ---- transport I/O -------------------------------------------------------- */

#ifdef NEBULOSA_WITH_USB
/* USB IN: USB_QUEUE transfers queued on every IN endpoint, so the device can
 * always send (a synchronous read leaves gaps the stream can't afford). The
 * callbacks run inside libusb_handle_events*() (pump), in the caller's
 * thread. user_data: the connection; the endpoint index is in the low bits
 * of the transfer's slot. */
static int feed(nebulosa_t *d, int src, const uint8_t *data, size_t n);

static void LIBUSB_CALL usb_in_done(struct libusb_transfer *t)
{
    nebulosa_t *d = (nebulosa_t *)t->user_data;
    int src = 0;
    while (src < d->nsrc && d->ep_in[src] != t->endpoint)
        src++;
    if (t->status == LIBUSB_TRANSFER_COMPLETED || t->status == LIBUSB_TRANSFER_TIMED_OUT)
    {
        if (t->actual_length > 0)
        {
            int e = feed(d, src, t->buffer, (size_t)t->actual_length);
            if (e && !d->usb_err)
                d->usb_err = e;
        }
        if (!d->closed && libusb_submit_transfer(t) == 0)
            return;
    }
    else if (t->status == LIBUSB_TRANSFER_NO_DEVICE || t->status == LIBUSB_TRANSFER_ERROR
             || t->status == LIBUSB_TRANSFER_STALL)
    {
        d->closed = 1;
        if (!d->usb_err)
            d->usb_err = NEBULOSA_E_CLOSED;
    }
    d->xfer_active--;   /* cancelled, failed, or not resubmitted */
}

static int usb_start_transfers(nebulosa_t *d)
{
    int k = 0;
    for (int i = 0; i < d->nsrc; i++)
        for (int j = 0; j < USB_QUEUE; j++)
        {
            struct libusb_transfer *t = libusb_alloc_transfer(0);
            uint8_t *buf = (uint8_t *)malloc(USB_XFER);
            if (!t || !buf)
            {
                libusb_free_transfer(t);
                free(buf);
                return NEBULOSA_E_NOMEM;
            }
            libusb_fill_bulk_transfer(t, d->uh, d->ep_in[i], buf, USB_XFER, usb_in_done, d, 0);
            t->flags = LIBUSB_TRANSFER_FREE_BUFFER;
            d->xfer[k++] = t;
            if (libusb_submit_transfer(t) != 0)
                return NEBULOSA_E_IO;
            d->xfer_active++;
        }
    return NEBULOSA_OK;
}

static void usb_stop_transfers(nebulosa_t *d)
{
    int closed = d->closed;
    d->closed = 1;      /* the callbacks don't resubmit */
    for (int k = 0; k < MAX_SRC * USB_QUEUE; k++)
        if (d->xfer[k])
            libusb_cancel_transfer(d->xfer[k]);
    int64_t deadline = now_ms() + 1000;
    while (d->xfer_active > 0 && now_ms() < deadline)
    {
        struct timeval tv = { 0, 50000 };
        libusb_handle_events_timeout_completed(d->usb, &tv, NULL);
    }
    if (d->xfer_active <= 0)    /* else leaked rather than freed while in flight */
        for (int k = 0; k < MAX_SRC * USB_QUEUE; k++)
        {
            libusb_free_transfer(d->xfer[k]);
            d->xfer[k] = NULL;
        }
    d->closed = closed;
}
#endif

static int tx_all(nebulosa_t *d, const uint8_t *p, size_t n)
{
    if (d->closed)
        return NEBULOSA_E_CLOSED;
    if (d->kind == T_TCP)
    {
        while (n)
        {
#ifdef _WIN32
            int k = send(d->sock, (const char *)p, (int)n, 0);
#else
            ssize_t k = send(d->sock, p, n, MSG_NOSIGNAL);
#endif
            if (k <= 0)
            {
#ifndef _WIN32
                if (k < 0 && errno == EINTR)
                    continue;
#endif
                d->closed = 1;
                return NEBULOSA_E_IO;
            }
            p += k;
            n -= (size_t)k;
        }
        return NEBULOSA_OK;
    }
#ifdef NEBULOSA_WITH_USB
    if (d->kind == T_USB)
    {
        int done = 0;
        int r = libusb_bulk_transfer(d->uh, d->ep_out, (unsigned char *)p, (int)n, &done, 2000);
        if (r == LIBUSB_ERROR_NO_DEVICE)
        {
            d->closed = 1;
            return NEBULOSA_E_CLOSED;
        }
        return (r == 0 && (size_t)done == n) ? NEBULOSA_OK : NEBULOSA_E_IO;
    }
#endif
    return NEBULOSA_E_UNSUPPORTED;
}

/* Read what arrives within timeout_ms (0: what's there now) and parse it.
 * Returns OK (possibly nothing read) or an error. */
static int pump(nebulosa_t *d, int timeout_ms)
{
    if (d->closed)
        return NEBULOSA_E_CLOSED;
    if (timeout_ms < 0)
        timeout_ms = 0;
    if (d->kind == T_TCP)
    {
        struct pollfd pf;
        pf.fd = d->sock;
        pf.events = POLLIN;
        pf.revents = 0;
        int r = poll(&pf, 1, timeout_ms);
        if (r < 0)
        {
#ifndef _WIN32
            if (errno == EINTR)
                return NEBULOSA_OK;
#endif
            return NEBULOSA_E_IO;
        }
        if (r == 0)
            return NEBULOSA_OK;
#ifdef _WIN32
        int k = recv(d->sock, (char *)d->chunk, RX_CHUNK, 0);
#else
        ssize_t k = recv(d->sock, d->chunk, RX_CHUNK, 0);
#endif
        if (k == 0)
        {
            d->closed = 1;
            return NEBULOSA_E_CLOSED;
        }
        if (k < 0)
        {
#ifndef _WIN32
            if (errno == EINTR || errno == EAGAIN)
                return NEBULOSA_OK;
#endif
            d->closed = 1;
            return NEBULOSA_E_IO;
        }
        return feed(d, 0, d->chunk, (size_t)k);
    }
#ifdef NEBULOSA_WITH_USB
    if (d->kind == T_USB)
    {
        /* the IN transfers are queued all the time (usb_start_transfers):
         * run their callbacks, waiting up to timeout_ms for one */
        struct timeval tv;
        tv.tv_sec = timeout_ms / 1000;
        tv.tv_usec = (timeout_ms % 1000) * 1000;
        int r = libusb_handle_events_timeout_completed(d->usb, &tv, NULL);
        if (d->usb_err)
            return d->usb_err;
        if (r != 0 && r != LIBUSB_ERROR_INTERRUPTED && r != LIBUSB_ERROR_TIMEOUT)
            return NEBULOSA_E_IO;
        return NEBULOSA_OK;
    }
#endif
    return NEBULOSA_E_UNSUPPORTED;
}

/* ---- request ------------------------------------------------------------------ */

int nebulosa_request(nebulosa_t *d, uint8_t cmd, const void *payload, uint16_t len,
                     void *rsp, size_t rsp_max, size_t *rsp_len, int timeout_ms)
{
    uint8_t msg[HDR_LEN + NBP_MAX_PAYLOAD];
    if (!d || len > NBP_MAX_PAYLOAD || (len && !payload))
        return NEBULOSA_E_ARG;
    if (timeout_ms <= 0)
        timeout_ms = d->timeout_ms;
    d->seq++;
    put16(msg, NBP_MAGIC);
    msg[2] = cmd;
    msg[3] = 0;
    put16(msg + 4, d->seq);
    put16(msg + 6, len);
    if (len)
        memcpy(msg + HDR_LEN, payload, len);

    d->waiting = 1;
    d->got = 0;
    d->want_type = NBP_RSP(cmd);
    d->want_seq = d->seq;
    int err = tx_all(d, msg, HDR_LEN + len);
    int64_t deadline = now_ms() + timeout_ms;
    while (!err && !d->got)
    {
        int64_t left = deadline - now_ms();
        if (left <= 0)
        {
            err = NEBULOSA_E_TIMEOUT;
            break;
        }
        err = pump(d, (int)left);
    }
    d->waiting = 0;
    if (err)
        return err;
    if (rsp_len)
        *rsp_len = d->got_len;
    if (rsp && rsp_max)
        memcpy(rsp, d->got_p, d->got_len < rsp_max ? d->got_len : rsp_max);
    return d->got_status;
}

/* A command whose response must be at least `need` bytes (copied to out). */
static int cmd_rec(nebulosa_t *d, uint8_t cmd, const void *p, uint16_t len, void *out, size_t need, int timeout_ms)
{
    uint8_t tmp[MAX_DATA_LEN];
    size_t got = 0;
    int r = nebulosa_request(d, cmd, p, len, tmp, sizeof(tmp), &got, timeout_ms);
    if (r)
        return r;
    if (got < need)
        return NEBULOSA_E_BAD_RESPONSE;
    if (out)
        memcpy(out, tmp, need);
    return NEBULOSA_OK;
}

/* ---- commands ------------------------------------------------------------- */

int nebulosa_ping(nebulosa_t *d, const void *data, uint16_t len, void *echo, size_t echo_max)
{
    size_t got = 0;
    int r = nebulosa_request(d, NBP_CMD_PING, data, len, echo, echo_max, &got, 0);
    if (r == NEBULOSA_OK && got != len)
        return NEBULOSA_E_BAD_RESPONSE;
    return r;
}

int nebulosa_get_info(nebulosa_t *d, nbp_info_t *out) { return cmd_rec(d, NBP_CMD_GET_INFO, NULL, 0, out, sizeof(*out), 0); }
int nebulosa_get_stats(nebulosa_t *d, nbp_stats_t *out) { return cmd_rec(d, NBP_CMD_GET_STATS, NULL, 0, out, sizeof(*out), 0); }

int nebulosa_reg_read(nebulosa_t *d, uint8_t index, uint32_t *value)
{
    uint8_t p[8] = { index }, r[8];
    int e = cmd_rec(d, NBP_CMD_REG_READ, p, 8, r, 8, 0);
    if (!e && value)
        *value = le32(r + 4);
    return e;
}

int nebulosa_reg_write(nebulosa_t *d, uint8_t index, uint32_t value)
{
    uint8_t p[8] = { index };
    put32(p + 4, value);
    return nebulosa_request(d, NBP_CMD_REG_WRITE, p, 8, NULL, 0, NULL, 0);
}

int nebulosa_reset(nebulosa_t *d) { return nebulosa_request(d, NBP_CMD_RESET, NULL, 0, NULL, 0, NULL, 0); }

int nebulosa_set_source(nebulosa_t *d, uint8_t source)
{
    uint8_t p[4] = { source };
    return nebulosa_request(d, NBP_CMD_SET_SOURCE, p, 4, NULL, 0, NULL, 0);
}

int nebulosa_set_range_auto(nebulosa_t *d, uint8_t min, uint8_t max)
{
    uint8_t p[4] = { 1, 0, min, max };
    return nebulosa_request(d, NBP_CMD_SET_RANGE, p, 4, NULL, 0, NULL, 0);
}

int nebulosa_set_range_manual(nebulosa_t *d, uint8_t range)
{
    uint8_t p[4] = { 0, range, 0, 5 };
    return nebulosa_request(d, NBP_CMD_SET_RANGE, p, 4, NULL, 0, NULL, 0);
}

int nebulosa_get_range(nebulosa_t *d, nbp_range_t *out) { return cmd_rec(d, NBP_CMD_GET_RANGE, NULL, 0, out, sizeof(*out), 0); }

int nebulosa_set_range_timing(nebulosa_t *d, uint8_t up_filt, uint8_t down_filt, uint8_t overlap, uint16_t hold)
{
    uint8_t p[8] = { up_filt, down_filt, overlap, 0 };
    put16(p + 4, hold);
    return nebulosa_request(d, NBP_CMD_SET_RANGE_TIMING, p, 8, NULL, 0, NULL, 0);
}

int nebulosa_set_range_force(nebulosa_t *d, int enable, uint8_t value)
{
    uint8_t p[4] = { (uint8_t)(enable != 0), value };
    return nebulosa_request(d, NBP_CMD_SET_RANGE_FORCE, p, 4, NULL, 0, NULL, 0);
}

int nebulosa_set_decimation(nebulosa_t *d, uint8_t log2n, uint8_t mode, uint32_t *rate_hz)
{
    uint8_t p[8] = { log2n, mode }, r[8];
    int e = cmd_rec(d, NBP_CMD_SET_DECIMATION, p, 8, r, 8, 0);
    if (!e && rate_hz)
        *rate_hz = le32(r + 4);
    return e;
}

static int board_cmd(nebulosa_t *d, uint8_t cmd, const void *p, uint16_t len, nbp_board_status_t *out, int timeout_ms)
{
    return cmd_rec(d, cmd, p, len, out, sizeof(nbp_board_status_t), timeout_ms);
}

int nebulosa_board_status(nebulosa_t *d, nbp_board_status_t *out) { return board_cmd(d, NBP_CMD_BOARD_STATUS, NULL, 0, out, 2000); }

int nebulosa_board_set(nebulosa_t *d, uint8_t item, uint32_t value, nbp_board_status_t *out)
{
    uint8_t p[8] = { item };
    put32(p + 4, value);
    return board_cmd(d, NBP_CMD_BOARD_SET, p, 8, out, 2000);
}

int nebulosa_board_test(nebulosa_t *d, uint32_t mask, nbp_board_status_t *out)
{
    uint8_t p[4];
    put32(p, mask);
    return board_cmd(d, NBP_CMD_BOARD_TEST, p, 4, out, 15000);
}

int nebulosa_set_leds(nebulosa_t *d, uint8_t mask) { return nebulosa_board_set(d, NBP_BSET_LEDS, (mask & 0x1Fu) | 0x100u, NULL); }
int nebulosa_leds_auto(nebulosa_t *d) { return nebulosa_board_set(d, NBP_BSET_LEDS, 0, NULL); }
int nebulosa_ui_key(nebulosa_t *d, uint8_t key) { return nebulosa_board_set(d, NBP_BSET_UI_KEY, key, NULL); }

int nebulosa_flash_read(nebulosa_t *d, uint32_t addr, void *buf, uint32_t len)
{
    uint8_t p[8], r[8 + NBP_FLASH_CHUNK];
    size_t got = 0;
    if (len > NBP_FLASH_CHUNK || (len && !buf))
        return NEBULOSA_E_ARG;
    put32(p, addr);
    put32(p + 4, len);
    int e = nebulosa_request(d, NBP_CMD_FLASH_READ, p, 8, r, sizeof(r), &got, 0);
    if (e)
        return e;
    if (got < 8 + (size_t)len)
        return NEBULOSA_E_BAD_RESPONSE;
    memcpy(buf, r + 8, len);
    return NEBULOSA_OK;
}

int nebulosa_flash_write(nebulosa_t *d, uint32_t addr, const void *data, uint32_t len)
{
    uint8_t p[8 + NBP_FLASH_CHUNK];
    if (len > NBP_FLASH_CHUNK || (len && !data))
        return NEBULOSA_E_ARG;
    put32(p, addr);
    put32(p + 4, len);
    memcpy(p + 8, data, len);
    return nebulosa_request(d, NBP_CMD_FLASH_WRITE, p, (uint16_t)(8 + len), NULL, 0, NULL, 0);
}

int nebulosa_flash_erase(nebulosa_t *d, uint32_t addr, uint32_t len)
{
    uint8_t p[8];
    put32(p, addr);
    put32(p + 4, len);
    return nebulosa_request(d, NBP_CMD_FLASH_ERASE, p, 8, NULL, 0, NULL, 5000);
}

int nebulosa_fpga_config(nebulosa_t *d, uint32_t addr, uint32_t len, nbp_board_status_t *out)
{
    uint8_t p[12] = { 0 };
    put32(p, addr);
    put32(p + 4, len);
    return board_cmd(d, NBP_CMD_FPGA_CONFIG, p, 12, out, 10000);
}

int nebulosa_upload_flash(nebulosa_t *d, uint32_t addr, const void *data, uint32_t len,
                          nebulosa_progress_fn progress, void *ctx)
{
    const uint8_t *src = (const uint8_t *)data;
    uint8_t back[NBP_FLASH_CHUNK];
    const uint32_t ERASE = 64 * 1024;
    uint32_t end = addr + ((len + 4095) & ~4095u);
    uint32_t total = (end - addr) + 2 * len, done = 0;
    int e;
    if ((addr & 4095) || (len && !data))
        return NEBULOSA_E_ARG;
    for (uint32_t a = addr; a < end; a += ERASE)
    {
        uint32_t n = end - a < ERASE ? end - a : ERASE;
        if ((e = nebulosa_flash_erase(d, a, n)) != 0)
            return e;
        done += n;
        if (progress)
            progress(ctx, done, total);
    }
    for (uint32_t off = 0; off < len; off += NBP_FLASH_CHUNK)
    {
        uint32_t n = len - off < NBP_FLASH_CHUNK ? len - off : NBP_FLASH_CHUNK;
        int blank = 1;
        for (uint32_t k = 0; k < n && blank; k++)
            blank = src[off + k] == 0xFF;
        if (!blank && (e = nebulosa_flash_write(d, addr + off, src + off, n)) != 0)
            return e;
        done += n;
        if (progress)
            progress(ctx, done, total);
    }
    for (uint32_t off = 0; off < len; off += NBP_FLASH_CHUNK)
    {
        uint32_t n = len - off < NBP_FLASH_CHUNK ? len - off : NBP_FLASH_CHUNK;
        if ((e = nebulosa_flash_read(d, addr + off, back, n)) != 0)
            return e;
        if (memcmp(back, src + off, n) != 0)
            return NEBULOSA_E_VERIFY;
        done += n;
        if (progress)
            progress(ctx, done, total);
    }
    return NEBULOSA_OK;
}

int nebulosa_update(nebulosa_t *d, uint8_t op, uint8_t slot, uint32_t len, uint32_t crc, nbp_update_info_t *out)
{
    uint8_t p[12] = { op, slot };
    uint8_t r[MAX_DATA_LEN];
    size_t got = 0;
    int timeout = op == NBP_UPD_GATEWARE ? 15000 : op == NBP_UPD_FIRMWARE ? 10000 : 5000;
    put32(p + 4, len);
    put32(p + 8, crc);
    int e = nebulosa_request(d, NBP_CMD_UPDATE, p, 12, r, sizeof(r), &got, timeout);
    if (e < 0)
        return e;
    /* a refused UPDATE still carries nbp_update_info_t (its msg says why) */
    if (got >= sizeof(nbp_update_info_t) && out)
        memcpy(out, r, sizeof(*out));
    else if (e == NEBULOSA_OK && got < sizeof(nbp_update_info_t))
        return NEBULOSA_E_BAD_RESPONSE;
    return e;
}

int nebulosa_update_info(nebulosa_t *d, nbp_update_info_t *out) { return nebulosa_update(d, NBP_UPD_INFO, 0, 0, 0, out); }

/* A Xilinx .bit file's raw bitstream (header stripped), or the input when
 * it's a raw .bin; NULL when there's no sync word. */
static const uint8_t *bitstream(const uint8_t *b, uint32_t n, uint32_t *out_len)
{
    static const uint8_t sync[4] = { 0xAA, 0x99, 0x55, 0x66 };
    int found = 0;
    for (uint32_t i = 0; i + 4 <= n && !found; i++)
        found = memcmp(b + i, sync, 4) == 0;
    if (!found)
        return NULL;
    if (n > 13 && b[0] == 0x00 && b[1] == 0x09)
    {
        uint32_t p = 2 + 9 + 2;
        while (p < n)
        {
            uint8_t key = b[p++];
            if (key == 'e')
            {
                if (p + 4 > n)
                    return NULL;
                uint32_t len = be32(b + p);
                p += 4;
                if (len > n - p)
                    return NULL;
                *out_len = len;
                return b + p;
            }
            if (p + 2 > n)
                return NULL;
            p += 2 + ((uint32_t)b[p] << 8 | b[p + 1]);
        }
        return NULL;
    }
    *out_len = n;
    return b;
}

static int install(nebulosa_t *d, uint8_t op, const uint8_t *img, uint32_t len,
                   nebulosa_progress_fn progress, void *ctx, nbp_update_info_t *out)
{
    nbp_update_info_t i;
    int e = nebulosa_update_info(d, &i);
    if (e)
        return e;
    if (len == 0 || len > i.slot_size)
        return NEBULOSA_E_ARG;
    if ((e = nebulosa_upload_flash(d, (uint32_t)i.update_slot * i.slot_size, img, len, progress, ctx)) != 0)
        return e;
    return nebulosa_update(d, op, i.update_slot, len, nebulosa_crc32(img, len), out);
}

int nebulosa_update_gateware(nebulosa_t *d, const void *image, uint32_t len,
                             nebulosa_progress_fn progress, void *ctx, nbp_update_info_t *out)
{
    uint32_t n = 0;
    const uint8_t *b = image ? bitstream((const uint8_t *)image, len, &n) : NULL;
    if (!b)
        return NEBULOSA_E_ARG;  /* not a bitstream */
    return install(d, NBP_UPD_GATEWARE, b, n, progress, ctx, out);
}

int nebulosa_update_firmware(nebulosa_t *d, const void *image, uint32_t len,
                             nebulosa_progress_fn progress, void *ctx, nbp_update_info_t *out)
{
    const uint8_t *b = (const uint8_t *)image;
    /* .nbfw: 64-byte header "NBFW", CRCs, length, board (Companion update.rs) */
    if (!b || len < 64 || le32(b) != 0x5746424Eu || le16(b + 4) != 64
        || nebulosa_crc32(b, 60) != le32(b + 60) || le32(b + 16) != len
        || nebulosa_crc32(b + 64, len - 64) != le32(b + 20) || le32(b + 56) != NBP_BOARD_DIGITAL)
        return NEBULOSA_E_ARG;
    return install(d, NBP_UPD_FIRMWARE, b, len, progress, ctx, out);
}

/* ---- streaming ------------------------------------------------------------ */

int nebulosa_start_stream(nebulosa_t *d, uint32_t flags)
{
    uint8_t p[4];
    put32(p, flags);
    int e = nebulosa_request(d, NBP_CMD_STREAM_START, p, 4, NULL, 0, NULL, 0);
    if (e == NEBULOSA_OK)
        d->streaming = 1;
    return e;
}

int nebulosa_stop_stream(nebulosa_t *d)
{
    int e = nebulosa_request(d, NBP_CMD_STREAM_STOP, NULL, 0, NULL, 0, NULL, 0);
    if (e == NEBULOSA_OK)
        d->streaming = 0;
    return e;
}

/* Header bookkeeping common to samples and raw blocks. Returns 0 to skip
 * the block's words (test block / malformed). */
static int block_begin(nebulosa_t *d, const uint8_t *p, uint16_t len, uint32_t *seq, uint16_t *nwords, uint16_t *flags)
{
    if (len < 12)
        return 0;
    *seq = le32(p);
    *nwords = le16(p + 8);
    *flags = le16(p + 10);
    if (*nwords > (len - 12) / 4)
        *nwords = (uint16_t)((len - 12) / 4);
    if (*flags & NBP_BLK_FIRST)
    {
        uint64_t resyncs = d->cnt.resyncs;
        memset(&d->cnt, 0, sizeof(d->cnt));
        d->cnt.resyncs = resyncs;
        nebulosa_decoder_init(&d->dec);
        d->have_expect = 0;
    }
    d->cnt.data_blocks++;
    if (d->have_expect && *seq != d->expect_seq)
    {
        d->cnt.lost_blocks += (uint32_t)(*seq - d->expect_seq);
        nebulosa_decoder_desync(&d->dec);
    }
    d->have_expect = 1;
    d->expect_seq = *seq + 1;
    if (*flags & NBP_BLK_TEST)
    {
        d->cnt.test_blocks++;
        return 0;
    }
    if (*flags & NBP_BLK_STALL)
    {
        d->cnt.stall_flags++;
        nebulosa_decoder_desync(&d->dec);
    }
    if (*flags & NBP_BLK_GAP)
    {
        d->cnt.gap_flags++;
        nebulosa_decoder_desync(&d->dec);
    }
    return 1;
}

static void decode_block(nebulosa_t *d, const data_msg_t *m)
{
    uint32_t seq;
    uint16_t nwords, flags;
    d->pend_pos = d->pend_n = 0;
    if (!block_begin(d, m->p, m->len, &seq, &nwords, &flags))
        return;
    const uint8_t *w = m->p + 12;
    for (uint16_t i = 0; i < nwords; i++, w += 4)
    {
        nebulosa_sample_t *s = &d->pend[d->pend_n];
        if (nebulosa_decoder_feed(&d->dec, be32(w), s) == NEBULOSA_W_SAMPLE)
        {
            unsigned r = s->range & (NEBULOSA_RANGES - 1);
            s->current = s->current_code * d->cal.i_fs[r] / 16384.0 + d->cal.i_offset[r];
            s->voltage = s->voltage_code * d->cal.v_scale + d->cal.v_offset;
            d->pend_n++;
        }
    }
}

int nebulosa_read_samples(nebulosa_t *d, nebulosa_sample_t *buf, size_t max, int timeout_ms)
{
    size_t n = 0;
    int64_t deadline = now_ms() + (timeout_ms > 0 ? timeout_ms : 0);
    if (!d || (max && !buf))
        return NEBULOSA_E_ARG;
    for (;;)
    {
        while (n < max && d->pend_pos < d->pend_n)
            buf[n++] = d->pend[d->pend_pos++];
        if (n == max)
            break;
        data_msg_t *m = q_front(d);
        if (m)
        {
            decode_block(d, m);
            q_pop(d);
            continue;
        }
        if (n > 0)
            break;
        int64_t left = deadline - now_ms();
        int e = pump(d, left > 0 ? (int)left : 0);
        if (e)
            return e;
        if (left <= 0 && !q_front(d))
            break;
    }
    return (int)(n > (size_t)0x7FFFFFFF ? 0x7FFFFFFF : n);
}

int nebulosa_read_block(nebulosa_t *d, nebulosa_block_t *out, int timeout_ms)
{
    int64_t deadline = now_ms() + (timeout_ms > 0 ? timeout_ms : 0);
    for (;;)
    {
        data_msg_t *m = q_front(d);
        if (m)
        {
            int keep = block_begin(d, m->p, m->len, &out->seq, &out->nwords, &out->flags);
            out->t_us = m->len >= 12 ? le32(m->p + 4) : 0;
            if (!keep && !(out->flags & NBP_BLK_TEST))
                out->nwords = 0;
            for (uint16_t i = 0; i < out->nwords; i++)
                out->words[i] = be32(m->p + 12 + 4 * i);
            q_pop(d);
            return 1;
        }
        int64_t left = deadline - now_ms();
        int e = pump(d, left > 0 ? (int)left : 0);
        if (e)
            return e;
        if (left <= 0 && !q_front(d))
            return 0;
    }
}

void nebulosa_stream_counters(const nebulosa_t *d, nebulosa_stream_counters_t *out)
{
    *out = d->cnt;
    out->decoder = d->dec.c;
    out->queued_blocks = d->q_count;
}

/* ---- TCP / UDP ------------------------------------------------------------ */


static void set_nonblocking(sock_t s, int on)
{
#ifdef _WIN32
    u_long nb = (u_long)on;
    ioctlsocket(s, FIONBIO, &nb);
#else
    int fl = fcntl(s, F_GETFL, 0);
    fcntl(s, F_SETFL, on ? (fl | O_NONBLOCK) : (fl & ~O_NONBLOCK));
#endif
}

/* connect() with a timeout. */
static int connect_timeout(sock_t s, const struct sockaddr *a, socklen_t alen, int timeout_ms)
{
    set_nonblocking(s, 1);
    int r = connect(s, a, alen);
    if (r != 0)
    {
#ifdef _WIN32
        if (WSAGetLastError() != WSAEWOULDBLOCK)
            return NEBULOSA_E_IO;
#else
        if (errno != EINPROGRESS)
            return NEBULOSA_E_IO;
#endif
        struct pollfd pf;
        pf.fd = s;
        pf.events = POLLOUT;
        pf.revents = 0;
        r = poll(&pf, 1, timeout_ms);
        if (r == 0)
            return NEBULOSA_E_TIMEOUT;
        if (r < 0)
            return NEBULOSA_E_IO;
        int so = 0;
        socklen_t sl = sizeof(so);
        getsockopt(s, SOL_SOCKET, SO_ERROR, (char *)&so, &sl);
        if (so != 0)
            return NEBULOSA_E_IO;
    }
    set_nonblocking(s, 0);
    return NEBULOSA_OK;
}

int nebulosa_open_tcp(nebulosa_t **out, const char *host, uint16_t port, int timeout_ms)
{
    struct addrinfo hints, *res = NULL, *ai;
    char ports[8];
    int err;
    if (!out || !host)
        return NEBULOSA_E_ARG;
    *out = NULL;
    if ((err = net_init()) != 0)
        return err;
    if (!port)
        port = NBP_TCP_PORT;
    if (timeout_ms <= 0)
        timeout_ms = 3000;
    snprintf(ports, sizeof(ports), "%u", (unsigned)port);
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    if (getaddrinfo(host, ports, &hints, &res) != 0)
        return NEBULOSA_E_NOT_FOUND;
    err = NEBULOSA_E_NOT_FOUND;
    sock_t s = BAD_SOCK;
    for (ai = res; ai; ai = ai->ai_next)
    {
        s = socket(ai->ai_family, ai->ai_socktype, ai->ai_protocol);
        if (s == BAD_SOCK)
            continue;
        err = connect_timeout(s, ai->ai_addr, (socklen_t)ai->ai_addrlen, timeout_ms);
        if (err == NEBULOSA_OK)
            break;
        sock_close(s);
        s = BAD_SOCK;
    }
    freeaddrinfo(res);
    if (s == BAD_SOCK)
        return err;

    int one = 1;
    setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (const char *)&one, sizeof(one));
    int rcv = 4 << 20;
    setsockopt(s, SOL_SOCKET, SO_RCVBUF, (const char *)&rcv, sizeof(rcv));
#ifdef SO_NOSIGPIPE
    setsockopt(s, SOL_SOCKET, SO_NOSIGPIPE, (const char *)&one, sizeof(one));
#endif
    nebulosa_t *d = conn_new();
    if (!d)
    {
        sock_close(s);
        return NEBULOSA_E_NOMEM;
    }
    d->kind = T_TCP;
    d->sock = s;
    d->nsrc = 1;
    snprintf(d->desc, sizeof(d->desc), "TCP %s:%u", host, (unsigned)port);
    *out = d;
    return NEBULOSA_OK;
}

int nebulosa_discover(nebulosa_net_device_t *out, int max, int timeout_ms)
{
    int err, n = 0;
    uint8_t q[HDR_LEN] = { 'N', 'B', NBP_CMD_GET_INFO, 0, 0, 0, 0, 0 };
    uint8_t buf[1500];
    if ((err = net_init()) != 0)
        return err;
    if (timeout_ms <= 0)
        timeout_ms = 600;
    sock_t s = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    if (s == BAD_SOCK)
        return NEBULOSA_E_IO;
    int one = 1;
    setsockopt(s, SOL_SOCKET, SO_BROADCAST, (const char *)&one, sizeof(one));
    struct sockaddr_in to;
    memset(&to, 0, sizeof(to));
    to.sin_family = AF_INET;
    to.sin_port = htons(NBP_UDP_DISCOVERY_PORT);
    to.sin_addr.s_addr = htonl(INADDR_BROADCAST);
    if (sendto(s, (const char *)q, sizeof(q), 0, (struct sockaddr *)&to, sizeof(to)) < 0)
    {
        sock_close(s);
        return NEBULOSA_E_IO;
    }
    int64_t deadline = now_ms() + timeout_ms;
    for (;;)
    {
        int64_t left = deadline - now_ms();
        if (left <= 0)
            break;
        struct pollfd pf;
        pf.fd = s;
        pf.events = POLLIN;
        pf.revents = 0;
        if (poll(&pf, 1, (int)left) <= 0)
            break;
        struct sockaddr_in from;
        socklen_t fl = sizeof(from);
        int k = (int)recvfrom(s, (char *)buf, sizeof(buf), 0, (struct sockaddr *)&from, &fl);
        if (k < HDR_LEN + (int)sizeof(nbp_info_t) || buf[0] != 'N' || buf[1] != 'B'
            || buf[2] != NBP_RSP(NBP_CMD_GET_INFO) || le16(buf + 6) < sizeof(nbp_info_t))
            continue;
        char host[64];
        inet_ntop(AF_INET, &from.sin_addr, host, sizeof(host));
        int dup = 0;
        for (int i = 0; i < n && !dup; i++)
            dup = strcmp(out[i].host, host) == 0;
        if (dup || n >= max || !out)
            continue;
        memcpy(out[n].host, host, sizeof(host));
        memcpy(&out[n].info, buf + HDR_LEN, sizeof(nbp_info_t));
        n++;
    }
    sock_close(s);
    return n;
}

/* ---- USB (libusb-1.0) ----------------------------------------------------- */

#ifdef NEBULOSA_WITH_USB

static int is_nbp(const struct libusb_device_descriptor *dd)
{
    return dd->idVendor == NBP_USB_VID && (dd->idProduct == NBP_USB_PID_HS || dd->idProduct == NBP_USB_PID_FS);
}

static void usb_string(libusb_device_handle *h, uint8_t idx, char *out, int max)
{
    out[0] = 0;
    if (idx && h && libusb_get_string_descriptor_ascii(h, idx, (unsigned char *)out, max) < 0)
        out[0] = 0;
}

int nebulosa_list_usb(nebulosa_usb_device_t *out, int max)
{
    libusb_context *ctx = NULL;
    libusb_device **list = NULL;
    int n = 0;
    if (libusb_init(&ctx) != 0)
        return NEBULOSA_E_IO;
    ssize_t cnt = libusb_get_device_list(ctx, &list);
    for (ssize_t i = 0; i < cnt && n < max; i++)
    {
        struct libusb_device_descriptor dd;
        if (libusb_get_device_descriptor(list[i], &dd) != 0 || !is_nbp(&dd))
            continue;
        nebulosa_usb_device_t *u = &out[n++];
        memset(u, 0, sizeof(*u));
        u->bus = libusb_get_bus_number(list[i]);
        u->address = libusb_get_device_address(list[i]);
        u->vid = dd.idVendor;
        u->pid = dd.idProduct;
        libusb_device_handle *h = NULL;
        if (libusb_open(list[i], &h) == 0)
        {
            usb_string(h, dd.iSerialNumber, u->serial, sizeof(u->serial));
            usb_string(h, dd.iProduct, u->product, sizeof(u->product));
            libusb_close(h);
        }
    }
    if (list)
        libusb_free_device_list(list, 1);
    libusb_exit(ctx);
    return n;
}

/* Find the NBP interface (class FF / 4E / 01) and its bulk endpoints. */
static int find_iface(libusb_device *dev, nebulosa_t *d)
{
    struct libusb_config_descriptor *cfg = NULL;
    int found = 0;
    if (libusb_get_active_config_descriptor(dev, &cfg) != 0)
        return 0;
    for (int i = 0; i < cfg->bNumInterfaces && !found; i++)
        for (int a = 0; a < cfg->interface[i].num_altsetting && !found; a++)
        {
            const struct libusb_interface_descriptor *id = &cfg->interface[i].altsetting[a];
            if (id->bInterfaceClass != NBP_USB_CLASS || id->bInterfaceSubClass != NBP_USB_SUBCLASS
                || id->bInterfaceProtocol != NBP_USB_PROTOCOL)
                continue;
            d->iface = id->bInterfaceNumber;
            d->nsrc = 0;
            d->ep_out = 0;
            for (int e = 0; e < id->bNumEndpoints; e++)
            {
                const struct libusb_endpoint_descriptor *ep = &id->endpoint[e];
                if ((ep->bmAttributes & 3) != LIBUSB_TRANSFER_TYPE_BULK)
                    continue;
                if (ep->bEndpointAddress & 0x80)
                {
                    if (d->nsrc < MAX_SRC)
                        d->ep_in[d->nsrc++] = ep->bEndpointAddress;
                }
                else if (!d->ep_out)
                    d->ep_out = ep->bEndpointAddress;
            }
            found = d->ep_out && d->nsrc;
        }
    libusb_free_config_descriptor(cfg);
    return found;
}

int nebulosa_open_usb(nebulosa_t **out, const char *serial, int index)
{
    libusb_device **list = NULL;
    int err = NEBULOSA_E_NOT_FOUND, k = 0;
    if (!out)
        return NEBULOSA_E_ARG;
    *out = NULL;
    nebulosa_t *d = conn_new();
    if (!d)
        return NEBULOSA_E_NOMEM;
    if (libusb_init(&d->usb) != 0)
    {
        nebulosa_close(d);
        return NEBULOSA_E_IO;
    }
    ssize_t cnt = libusb_get_device_list(d->usb, &list);
    for (ssize_t i = 0; i < cnt; i++)
    {
        struct libusb_device_descriptor dd;
        libusb_device_handle *h = NULL;
        if (libusb_get_device_descriptor(list[i], &dd) != 0 || !is_nbp(&dd))
            continue;
        if (libusb_open(list[i], &h) != 0)
        {
            err = NEBULOSA_E_IO;    /* permissions? see tools/99-nebulosa.rules */
            continue;
        }
        if (serial)
        {
            char sn[64];
            usb_string(h, dd.iSerialNumber, sn, sizeof(sn));
            if (strcmp(sn, serial) != 0)
            {
                libusb_close(h);
                continue;
            }
        }
        if (k++ != index || !find_iface(list[i], d))
        {
            libusb_close(h);
            continue;
        }
        libusb_set_auto_detach_kernel_driver(h, 1);
        if (libusb_claim_interface(h, d->iface) != 0)
        {
            libusb_close(h);
            err = NEBULOSA_E_IO;
            break;
        }
        d->uh = h;
        d->kind = T_USB;
        if (usb_start_transfers(d) != 0)
        {
            err = NEBULOSA_E_IO;
            break;
        }
        int sp = libusb_get_device_speed(list[i]);
        snprintf(d->desc, sizeof(d->desc), "USB %u:%u (%s)", libusb_get_bus_number(list[i]),
                 libusb_get_device_address(list[i]),
                 sp >= LIBUSB_SPEED_SUPER ? "SuperSpeed" : sp == LIBUSB_SPEED_HIGH ? "High-Speed" : "Full-Speed");
        err = NEBULOSA_OK;
        break;
    }
    if (list)
        libusb_free_device_list(list, 1);
    if (err)
    {
        nebulosa_close(d);
        return err;
    }
    *out = d;
    return NEBULOSA_OK;
}

#else

int nebulosa_list_usb(nebulosa_usb_device_t *out, int max)
{
    (void)out;
    (void)max;
    return NEBULOSA_E_UNSUPPORTED;
}

int nebulosa_open_usb(nebulosa_t **out, const char *serial, int index)
{
    (void)serial;
    (void)index;
    if (out)
        *out = NULL;
    return NEBULOSA_E_UNSUPPORTED;
}

#endif
