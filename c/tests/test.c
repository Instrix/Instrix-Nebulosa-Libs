/* libnebulosa tests: unit tests always; against the simulator when
 * NEBULOSA_SIM_PORT is set (tests/run_with_sim.py does that). */
#include "nebulosa.h"

#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "%s:%d: CHECK failed: %s\n", __FILE__, __LINE__, #c); fails++; } } while (0)
#define OK(e) do { int _e = (e); if (_e) { fprintf(stderr, "%s:%d: %s -> %d %s\n", __FILE__, __LINE__, #e, _e, nebulosa_strerror(_e)); fails++; } } while (0)

static size_t block_words(uint32_t *v, uint16_t seq, uint8_t range, uint8_t log2n, uint64_t ts,
                          const uint16_t (*s)[3], size_t ns)
{
    size_t n = 0;
    v[n++] = (2u << 29) | ((uint32_t)seq << 11) | ((uint32_t)log2n << 5) | ((uint32_t)range << 2) | 2;
    v[n++] = (2u << 29) | (1u << 27) | (uint32_t)((ts >> 24) & 0xFFFFFF);
    v[n++] = 0;
    v[n++] = (2u << 29) | (2u << 27) | (uint32_t)(ts & 0xFFFFFF);
    for (size_t i = 0; i < ns; i++)
        v[n++] = (1u << 31) | ((uint32_t)s[i][2] << 30) | ((uint32_t)s[i][0] << 14) | s[i][1];
    return n;
}

static void unit(void)
{
    /* layouts */
    CHECK(sizeof(nbp_info_t) == 76);
    CHECK(sizeof(nbp_stats_t) == 64);
    CHECK(sizeof(nbp_board_status_t) == 200);
    CHECK(sizeof(nbp_update_info_t) == 136);
    CHECK(offsetof(nbp_board_status_t, pd_pdo) == 144);
    CHECK(offsetof(nbp_update_info_t, msg) == 88);

    CHECK(nebulosa_crc32("123456789", 9) == 0xCBF43926u);

    /* decoder: Companion stream.rs `decodes_blocks` */
    uint32_t w[64];
    const uint16_t a[2][3] = { { 100, 200, 0 }, { 101, 201, 1 } };
    const uint16_t b[1][3] = { { 5, 6, 0 } };
    size_t n = block_words(w, 10, 3, 1, 0x123456789AULL, a, 2);
    n += block_words(w + n, 11, 4, 1, 0x123456789AULL + 4, b, 1);
    nebulosa_decoder_t d;
    nebulosa_sample_t s[4];
    int k = 0;
    nebulosa_decoder_init(&d);
    for (size_t i = 0; i < n; i++)
        if (nebulosa_decoder_feed(&d, w[i], &s[k]) == NEBULOSA_W_SAMPLE)
            k++;
    CHECK(k == 3);
    CHECK(s[0].ts == 0x123456789AULL);
    CHECK(s[1].ts == 0x123456789AULL + 2);
    CHECK(s[1].dirty);
    CHECK(s[2].range == 4 && s[2].current_code == 5 && s[2].voltage_code == 6);
    CHECK(d.c.seq_errors == 0 && d.c.range_changes == 1 && d.c.gap_periods == 0 && d.c.malformed == 0);

    nebulosa_calibration_t c;
    nebulosa_calibration_default(&c);
    CHECK(c.i_fs[0] == 2.0 && c.i_fs[5] == 20e-6 && c.v_scale == 1e-3);
}

static void sim(const char *port)
{
    nebulosa_t *d = NULL;
    OK(nebulosa_open_tcp(&d, "127.0.0.1", (uint16_t)atoi(port), 2000));
    if (!d)
        return;

    nbp_info_t info;
    OK(nebulosa_get_info(d, &info));
    CHECK(info.proto_version == NBP_VERSION);
    CHECK(info.block_words == NBP_BLOCK_WORDS);
    CHECK(info.fpga_id == 0x4E42000Bu);

    uint8_t echo[16];
    OK(nebulosa_ping(d, "hello", 5, echo, sizeof(echo)));
    CHECK(memcmp(echo, "hello", 5) == 0);

    uint32_t v = 0;
    OK(nebulosa_reg_write(d, 5, 0x1234));
    OK(nebulosa_reg_read(d, 5, &v));
    CHECK(v == 0x1234);
    CHECK(nebulosa_reg_read(d, 200, &v) == NBP_ST_BAD_ARG);

    nbp_stats_t st;
    OK(nebulosa_get_stats(d, &st));
    OK(nebulosa_set_source(d, NBP_SRC_AD7387));
    OK(nebulosa_set_range_manual(d, 2));
    OK(nebulosa_set_range_auto(d, 0, 5));
    nbp_range_t rg;
    OK(nebulosa_get_range(d, &rg));
    OK(nebulosa_set_range_timing(d, 4, 4, 2, 100));
    OK(nebulosa_set_range_force(d, 0, 0));

    nbp_board_status_t bs;
    OK(nebulosa_board_status(d, &bs));
    CHECK(bs.board_id == NBP_BOARD_DIGITAL);
    OK(nebulosa_set_leds(d, 0x15));
    OK(nebulosa_board_status(d, &bs));
    CHECK(bs.leds == 0x15 && bs.led_manual == 1);
    OK(nebulosa_leds_auto(d));
    OK(nebulosa_board_test(d, NBP_TEST_ALL, &bs));
    CHECK(nebulosa_test_result(&bs, NBP_TEST_HSE) == NBP_TR_PASS);
    OK(nebulosa_fpga_config(d, 0, 1000, NULL));

    /* flash */
    uint8_t img[5000], back[256];
    for (size_t i = 0; i < sizeof(img); i++)
        img[i] = (uint8_t)(i * 7);
    memset(img + 256, 0xFF, 256);      /* a blank page: skipped */
    OK(nebulosa_upload_flash(d, 0x10000, img, sizeof(img), NULL, NULL));
    OK(nebulosa_flash_read(d, 0x10000 + 4096, back, 200));
    CHECK(memcmp(back, img + 4096, 200) == 0);

    nbp_update_info_t ui;
    OK(nebulosa_update_info(d, &ui));
    CHECK(ui.slot_size == 448 * 1024);

    /* stream */
    uint32_t rate = 0;
    OK(nebulosa_set_decimation(d, 2, NBP_DECIM_AVERAGE, &rate));
    CHECK(rate == 500000);
    OK(nebulosa_start_stream(d, NBP_START_FLUSH));
    static nebulosa_sample_t buf[4096];
    uint64_t total = 0, last_ts = 0, bad_order = 0;
    unsigned ranges = 0;
    for (int it = 0; it < 2000 && total < 200000; it++)
    {
        int k = nebulosa_read_samples(d, buf, 4096, 500);
        if (k < 0)
        {
            OK(k);
            break;
        }
        for (int i = 0; i < k; i++)
        {
            if (total && buf[i].ts <= last_ts)
                bad_order++;
            last_ts = buf[i].ts;
            ranges |= 1u << buf[i].range;
            total++;
        }
        if (it == 10)
            OK(nebulosa_get_stats(d, &st));     /* a command while DATA flows */
    }
    OK(nebulosa_stop_stream(d));
    nebulosa_stream_counters_t sc;
    nebulosa_stream_counters(d, &sc);
    printf("sim stream: %llu samples, ranges 0x%02x, blocks %llu, lost %llu, seq_err %llu, malformed %llu\n",
           (unsigned long long)total, ranges, (unsigned long long)sc.data_blocks, (unsigned long long)sc.lost_blocks,
           (unsigned long long)sc.decoder.seq_errors, (unsigned long long)sc.decoder.malformed);
    CHECK(total >= 200000);
    CHECK(bad_order == 0);
    CHECK(sc.lost_blocks == 0 && sc.decoder.seq_errors == 0 && sc.decoder.malformed == 0);
    CHECK(buf[0].step == 4);
    nebulosa_close(d);
}

int main(void)
{
    unit();
    const char *port = getenv("NEBULOSA_SIM_PORT");
    if (port)
        sim(port);
    else
        printf("NEBULOSA_SIM_PORT not set: simulator tests skipped\n");
    printf(fails ? "FAILED (%d)\n" : "all passed\n", fails);
    return fails != 0;
}
