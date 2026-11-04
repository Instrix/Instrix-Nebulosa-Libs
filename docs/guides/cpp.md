# C++ guide

`nebulosa.hpp` is a header-only C++17 layer over the [C library](c.md):
an RAII `Device`, exceptions, `std::vector`, `std::chrono`, lambdas. Same
performance as C (2 MSPS over USB).

## Build

```cmake
add_subdirectory(path/to/Instrix-Nebulosa-Libs/cpp nebulosa_cpp)
target_link_libraries(myapp PRIVATE nebulosa::cpp)       # pulls in the C library
```

Or build its examples and tests:

```sh
cmake -S cpp -B cpp/build -DCMAKE_BUILD_TYPE=Release
cmake --build cpp/build && (cd cpp/build && ctest)
```

USB needs libusb-1.0, as for [C](c.md#build).

## Connect

```cpp
#include "nebulosa.hpp"
#include <iostream>

int main() try {
    auto dev = nebulosa::Device::usb();                   // or ::usb("SERIAL")
    // auto dev = nebulosa::Device::tcp("192.168.1.20");
    // auto dev = nebulosa::Device::first_on_network();   // UDP discovery
    auto info = dev.info();
    std::cout << dev.description() << ": firmware "
              << nebulosa::version_string(info.fw_version) << ", built "
              << nebulosa::build_string(info) << "\n";
} catch (const nebulosa::Error &e) {
    std::cerr << e.what() << "\n";
    return 1;
}
```

`Device` is movable, not copyable. Its destructor closes the connection and
stops a stream it started.

```cpp
for (auto &d : nebulosa::discover())      std::cout << d.host << "\n";
for (auto &u : nebulosa::list_usb())      std::cout << u.serial << "\n";
```

## Measure

```cpp
using namespace std::chrono_literals;

uint32_t rate = dev.set_decimation(4);                    // 125 kS/s
dev.set_range_auto();
dev.start_stream();

double sum = 0;
size_t n = 0;
dev.for_each_batch([&](const std::vector<nebulosa::Sample> &batch) {
    for (const auto &s : batch) sum += s.current;
    n += batch.size();
    return n < size_t(rate) * 5;                          // false stops: 5 seconds
});
dev.stop_stream();

auto c = dev.stream_counters();
std::cout << "mean " << sum / n << " A, lost " << c.lost_blocks << "\n";
```

Or pull batches yourself:

```cpp
std::vector<nebulosa::Sample> s;
size_t got = dev.read_samples(s, 8192, 500ms);           // appends; 0 on timeout
double t = nebulosa::time_s(s.front());
```

`dev.read_block(500ms)` returns `std::optional<nebulosa::Block>` with raw
words.

## Control the board

```cpp
auto b = dev.board_status();
std::cout << b.v_main_mv << " mV\n";
dev.set_leds(0b10101);
dev.leds_auto();
dev.ui_key(NBP_KEY_ENTER);
dev.set_source(NBP_SRC_AD7387);
auto r = dev.board_test();                                // stop streaming first
bool ok = nebulosa::test_result(r, NBP_TEST_FPGA_DONE) == NBP_TR_PASS;
```

## Updates

```cpp
std::vector<uint8_t> bit = read_file("nebulosa.bit");
try {
    dev.update_gateware(bit, [](uint32_t done, uint32_t total) {
        std::cerr << "\r" << done * 100 / total << "%";
    });
} catch (const nebulosa::UpdateError &e) {
    std::cerr << "refused: " << e.info().msg << "\n";
}
```

`update_firmware(nbfw)` is the same for `.nbfw` images; the device restarts
afterwards. Lower level: `flash_read`, `flash_write`, `flash_erase`,
`upload_flash`, `fpga_config`, `update`, `update_info`.

## Errors

`nebulosa::Error` (a `std::runtime_error`): `code()` is a `NEBULOSA_E_*`
(< 0) or the device's `NBP_ST_*` (> 0); `is_status()`, `is_timeout()`.
`UpdateError` adds `info()`.

```cpp
try { dev.set_range_manual(9); }
catch (const nebulosa::Error &e) { if (e.code() == NBP_ST_BAD_ARG) { /* ... */ } }
```

Constants (`NBP_SRC_*`, `NBP_BSET_*`, `NBP_TEST_*`, `NBP_KEY_*`, ...) come
from `nbp.h`. `dev.handle()` gives the underlying `nebulosa_t *` for any C
call.

## Examples

`cpp/examples`: `info`, `stream_csv`, `average_current`, built as
`nebulosa_cpp_*`, with the same `--host` / `--usb` arguments as the C ones.
