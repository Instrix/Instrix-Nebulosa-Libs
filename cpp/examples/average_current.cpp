// Mean / min / max current and charge over T seconds:  nebulosa_cpp_average_current [--host H|--usb] [--seconds 10]
#include "connect.hpp"
#include <algorithm>
#include <cstdio>
#include <iostream>
#include <limits>

int main(int argc, char **argv) try {
    double secs = std::stod(arg(argc, argv, "--seconds", "10"));
    auto dev = connect(argc, argv);
    uint32_t rate = dev.set_decimation(static_cast<uint8_t>(std::stoi(arg(argc, argv, "--log2n", "0"))));
    dev.set_range_auto();
    double sum = 0, lo = std::numeric_limits<double>::max(), hi = -lo, t_first = -1, t_last = 0;
    uint64_t n = 0;
    dev.start_stream();
    dev.for_each_batch([&](const std::vector<nebulosa::Sample> &batch) {
        for (const auto &s : batch) {
            sum += s.current;
            lo = std::min(lo, s.current);
            hi = std::max(hi, s.current);
            t_last = nebulosa::time_s(s);
            if (t_first < 0) t_first = t_last;
        }
        n += batch.size();
        return t_first < 0 || t_last - t_first < secs;
    });
    dev.stop_stream();
    double T = t_last - t_first, mean = sum / static_cast<double>(n);
    std::printf("%llu samples over %.3f s (%u S/s)\n", static_cast<unsigned long long>(n), T, rate);
    std::printf("mean %.6g A, min %.6g A, max %.6g A, charge %.6g C\n", mean, lo, hi, mean * T);
} catch (const nebulosa::Error &e) {
    std::cerr << e.what() << "\n";
    return 1;
}
