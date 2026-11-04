# nebulosa.hpp (C++17)

Header-only C++ wrapper over [libnebulosa](../c): RAII `Device`, exceptions
(`nebulosa::Error`, `UpdateError`), `std::vector`, `std::chrono`, lambdas.
Tested against the simulator and real hardware (2 MSPS over USB).

```sh
cmake -S cpp -B cpp/build -DCMAKE_BUILD_TYPE=Release
cmake --build cpp/build && (cd cpp/build && ctest)
```

In your project: `add_subdirectory(path/to/Instrix-Nebulosa-Libs/cpp)` then
`target_link_libraries(app PRIVATE nebulosa::cpp)`.

```cpp
#include "nebulosa.hpp"

auto dev = nebulosa::Device::usb();                 // ::tcp("192.168.1.20"), ::first_on_network()
std::cout << nebulosa::version_string(dev.info().fw_version) << "\n";
uint32_t rate = dev.set_decimation(4);
dev.set_range_auto();
dev.start_stream();
double sum = 0; size_t n = 0;
dev.for_each_batch([&](const std::vector<nebulosa::Sample> &batch) {
    for (auto &s : batch) sum += s.current;
    n += batch.size();
    return n < rate * 5;                             // 5 seconds
});
dev.stop_stream();
std::cout << "mean " << sum / n << " A\n";
```

`Error::code()` is a `NEBULOSA_E_*` (< 0) or the device's `NBP_ST_*` (> 0,
`is_status()`). Constants (`NBP_SRC_*`, `NBP_BSET_*`, `NBP_TEST_*`, ...) come
from `spec/nbp.h`. Examples: `cpp/examples` (`info`, `stream_csv`,
`average_current`).
