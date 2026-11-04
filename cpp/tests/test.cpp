// C++ wrapper tests against tools/nebulosa_sim.py (NEBULOSA_SIM_PORT).
#include "nebulosa.hpp"
#include <cstdio>
#include <cstdlib>
#include <iostream>

static int fails;
#define CHECK(c) do { if (!(c)) { std::fprintf(stderr, "%s:%d: CHECK failed: %s\n", __FILE__, __LINE__, #c); fails++; } } while (0)

int main() {
    using namespace nebulosa;
    CHECK(crc32("123456789", 9) == 0xCBF43926u);
    CHECK(default_calibration().i_fs[1] == 0.2);
    const char *port = std::getenv("NEBULOSA_SIM_PORT");
    if (!port) {
        std::puts("NEBULOSA_SIM_PORT not set: skipped");
        return fails != 0;
    }
    try {
        auto dev = Device::tcp("127.0.0.1", static_cast<uint16_t>(std::atoi(port)));
        auto i = dev.info();
        CHECK(i.fpga_id == 0x4E42000Bu);
        CHECK(build_string(i).rfind("sim", 0) == 0);
        CHECK((dev.ping({1, 2, 3}) == std::vector<uint8_t>{1, 2, 3}));
        dev.reg_write(7, 99);
        CHECK(dev.reg_read(7) == 99);
        try {
            dev.reg_read(250);
            CHECK(false);
        } catch (const Error &e) {
            CHECK(e.is_status() && e.code() == NBP_ST_BAD_ARG);
        }
        dev.set_leds(0x3);
        CHECK(dev.board_status().leds == 0x3);
        dev.leds_auto();
        CHECK(test_result(dev.board_test(), NBP_TEST_LCD) == NBP_TR_PASS);

        std::vector<uint8_t> img(3000);
        for (size_t k = 0; k < img.size(); k++) img[k] = static_cast<uint8_t>(k ^ 0x5A);
        uint32_t last = 0;
        dev.upload_flash(0x20000, img, [&](uint32_t done, uint32_t) { last = done; });
        CHECK(last > 0);
        CHECK(dev.flash_read(0x20000, 3000) == img);
        CHECK(dev.update_info().slot_size == 448u * 1024);

        CHECK(dev.set_decimation(3) == 250000);
        dev.start_stream();
        std::vector<Sample> s;
        while (s.size() < 100000) dev.read_samples(s, 8192);
        dev.stats();   // a command while DATA flows
        dev.stop_stream();
        bool ordered = true;
        for (size_t k = 1; k < s.size(); k++) ordered &= s[k].ts > s[k - 1].ts;
        CHECK(ordered);
        auto c = dev.stream_counters();
        CHECK(c.lost_blocks == 0 && c.decoder.malformed == 0 && c.decoder.seq_errors == 0);
        std::printf("cpp sim: %zu samples, %llu blocks\n", s.size(), static_cast<unsigned long long>(c.data_blocks));
    } catch (const Error &e) {
        std::cerr << "error: " << e.what() << "\n";
        fails++;
    }
    std::puts(fails ? "FAILED" : "all passed");
    return fails != 0;
}
