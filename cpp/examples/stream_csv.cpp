// Stream for N seconds to CSV:  nebulosa_cpp_stream_csv [--host H|--usb] [--seconds 5] [--log2n 4] [--out f.csv]
#include "connect.hpp"
#include <cstdio>
#include <iostream>

int main(int argc, char **argv) try {
    double secs = std::stod(arg(argc, argv, "--seconds", "5"));
    auto log2n = static_cast<uint8_t>(std::stoi(arg(argc, argv, "--log2n", "4")));
    std::string path = arg(argc, argv, "--out");
    FILE *f = path.empty() ? stdout : std::fopen(path.c_str(), "w");
    if (!f) { std::perror(path.c_str()); return 1; }

    auto dev = connect(argc, argv);
    uint32_t rate = dev.set_decimation(log2n);
    auto want = static_cast<uint64_t>(secs * rate);
    uint64_t n = 0;
    double t0 = -1;
    std::fprintf(f, "time_s,current_a,voltage_v,range,dirty\n");
    dev.start_stream();
    dev.for_each_batch([&](const std::vector<nebulosa::Sample> &batch) {
        for (const auto &s : batch) {
            if (n++ >= want) return false;
            double t = nebulosa::time_s(s);
            if (t0 < 0) t0 = t;
            std::fprintf(f, "%.7f,%.9g,%.6f,%u,%u\n", t - t0, s.current, s.voltage, s.range, s.dirty);
        }
        return n < want;
    });
    dev.stop_stream();
    std::cerr << want << " samples at " << rate << " S/s, lost blocks " << dev.stream_counters().lost_blocks << "\n";
    if (f != stdout) std::fclose(f);
} catch (const nebulosa::Error &e) {
    std::cerr << e.what() << "\n";
    return 1;
}
