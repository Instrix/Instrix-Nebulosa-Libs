// nebulosa.hpp - C++17 interface to the INSTRIX Nebulosa power profiler.
//
// Header-only wrapper over libnebulosa (c/): RAII connection, exceptions,
// std::vector / std::string / std::chrono. Same model as every binding
// (docs/API.md).
//
//   auto dev = nebulosa::Device::tcp("192.168.1.20");      // or Device::usb()
//   std::cout << dev.info().build << "\n";
//   dev.set_decimation(4);
//   dev.start_stream();
//   std::vector<nebulosa::Sample> s;
//   dev.read_samples(s, 10000, std::chrono::milliseconds(500));
//   dev.stop_stream();
#ifndef NEBULOSA_HPP
#define NEBULOSA_HPP

#include "nebulosa.h"

#include <algorithm>
#include <chrono>
#include <cstring>
#include <functional>
#include <optional>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace nebulosa {

using Info         = nbp_info_t;
using Stats        = nbp_stats_t;
using RangeState   = nbp_range_t;
using BoardStatus  = nbp_board_status_t;
using UpdateInfo   = nbp_update_info_t;
using Sample       = nebulosa_sample_t;
using Block        = nebulosa_block_t;
using Calibration  = nebulosa_calibration_t;
using StreamCounters = nebulosa_stream_counters_t;
using Ms           = std::chrono::milliseconds;

/// A failed call: `code()` is a NEBULOSA_E_* (< 0) or an NBP_ST_* the device answered (> 0).
class Error : public std::runtime_error {
public:
    Error(int code, const std::string &what)
        : std::runtime_error(what + ": " + nebulosa_strerror(code)), code_(code) {}
    int code() const noexcept { return code_; }
    /// The device refused the command (NBP_ST_*), as opposed to an I/O problem.
    bool is_status() const noexcept { return code_ > 0; }
    bool is_timeout() const noexcept { return code_ == NEBULOSA_E_TIMEOUT; }
private:
    int code_;
};

/// An UPDATE the device refused: info().msg says why.
class UpdateError : public Error {
public:
    UpdateError(int code, const UpdateInfo &i) : Error(code, "update: " + std::string(i.msg, strnlen(i.msg, sizeof(i.msg)))), info_(i) {}
    const UpdateInfo &info() const noexcept { return info_; }
private:
    UpdateInfo info_;
};

namespace detail {
inline void check(int e, const char *what) {
    if (e != NEBULOSA_OK)
        throw Error(e, what);
}
inline int ms(Ms t) { return static_cast<int>(t.count()); }
inline std::string cstr(const char *p, size_t max) { return std::string(p, strnlen(p, max)); }
}  // namespace detail

inline std::string version_string(uint32_t v) {
    return std::to_string(v >> 16) + "." + std::to_string((v >> 8) & 0xFF) + "." + std::to_string(v & 0xFF);
}
inline std::string build_string(const Info &i) { return detail::cstr(i.build, sizeof(i.build)); }
inline std::string ip_string(uint32_t ip_net_order) {
    const auto *b = reinterpret_cast<const uint8_t *>(&ip_net_order);
    return std::to_string(b[0]) + "." + std::to_string(b[1]) + "." + std::to_string(b[2]) + "." + std::to_string(b[3]);
}
inline uint32_t test_result(const BoardStatus &b, int test) { return NBP_TEST_RESULT(b.test_results, test); }
inline double time_s(const Sample &s) { return nebulosa_sample_time(&s); }
inline uint32_t crc32(const void *p, size_t n) { return nebulosa_crc32(p, n); }
inline Calibration default_calibration() { Calibration c; nebulosa_calibration_default(&c); return c; }

struct NetDevice {
    std::string host;   ///< connect here
    Info info;
};

/// UDP broadcast discovery.
inline std::vector<NetDevice> discover(Ms timeout = Ms(600), int max = 32) {
    std::vector<nebulosa_net_device_t> buf(static_cast<size_t>(max));
    int n = nebulosa_discover(buf.data(), max, detail::ms(timeout));
    if (n < 0)
        throw Error(n, "discover");
    std::vector<NetDevice> out;
    for (int i = 0; i < n; i++)
        out.push_back({buf[static_cast<size_t>(i)].host, buf[static_cast<size_t>(i)].info});
    return out;
}

using UsbDevice = nebulosa_usb_device_t;

/// NBP USB devices (empty when built without USB).
inline std::vector<UsbDevice> list_usb(int max = 32) {
    std::vector<UsbDevice> buf(static_cast<size_t>(max));
    int n = nebulosa_list_usb(buf.data(), max);
    if (n == NEBULOSA_E_UNSUPPORTED)
        return {};
    if (n < 0)
        throw Error(n, "list_usb");
    buf.resize(static_cast<size_t>(n));
    return buf;
}

using Progress = std::function<void(uint32_t done, uint32_t total)>;

/// One connection to a device. Movable, not copyable; closes on destruction
/// (and stops a stream it started).
class Device {
public:
    static Device tcp(const std::string &host, uint16_t port = NBP_TCP_PORT, Ms timeout = Ms(3000)) {
        nebulosa_t *d = nullptr;
        detail::check(nebulosa_open_tcp(&d, host.c_str(), port, detail::ms(timeout)), "connect");
        return Device(d);
    }
    /// The index-th NBP USB device (with that serial, if given).
    static Device usb(const std::optional<std::string> &serial = std::nullopt, int index = 0) {
        nebulosa_t *d = nullptr;
        detail::check(nebulosa_open_usb(&d, serial ? serial->c_str() : nullptr, index), "open USB");
        return Device(d);
    }
    /// The first device UDP discovery finds.
    static Device first_on_network(Ms timeout = Ms(800)) {
        auto found = discover(timeout);
        if (found.empty())
            throw Error(NEBULOSA_E_NOT_FOUND, "discover");
        return tcp(found.front().host);
    }

    Device(Device &&o) noexcept : d_(std::exchange(o.d_, nullptr)) {}
    Device &operator=(Device &&o) noexcept {
        if (this != &o) {
            close();
            d_ = std::exchange(o.d_, nullptr);
        }
        return *this;
    }
    Device(const Device &) = delete;
    Device &operator=(const Device &) = delete;
    ~Device() { close(); }

    void close() noexcept {
        if (d_)
            nebulosa_close(d_);
        d_ = nullptr;
    }
    nebulosa_t *handle() const noexcept { return d_; }
    std::string description() const { return nebulosa_description(d_); }
    void set_timeout(Ms t) { nebulosa_set_timeout(d_, detail::ms(t)); }

    /// Raw command: the response payload (throws on a status != OK).
    std::vector<uint8_t> request(uint8_t cmd, const std::vector<uint8_t> &payload = {}, Ms timeout = Ms(0)) {
        std::vector<uint8_t> rsp(NBP_BLOCK_BYTES);
        size_t n = 0;
        detail::check(nebulosa_request(d_, cmd, payload.data(), static_cast<uint16_t>(payload.size()), rsp.data(),
                                       rsp.size(), &n, detail::ms(timeout)),
                      "request");
        rsp.resize(n);
        return rsp;
    }

    // ---- commands -----------------------------------------------------------
    std::vector<uint8_t> ping(const std::vector<uint8_t> &data = {}) {
        std::vector<uint8_t> echo(data.size());
        detail::check(nebulosa_ping(d_, data.data(), static_cast<uint16_t>(data.size()), echo.data(), echo.size()), "ping");
        return echo;
    }
    Info info() { Info i; detail::check(nebulosa_get_info(d_, &i), "get_info"); return i; }
    Stats stats() { Stats s; detail::check(nebulosa_get_stats(d_, &s), "get_stats"); return s; }
    uint32_t reg_read(uint8_t index) { uint32_t v; detail::check(nebulosa_reg_read(d_, index, &v), "reg_read"); return v; }
    void reg_write(uint8_t index, uint32_t value) { detail::check(nebulosa_reg_write(d_, index, value), "reg_write"); }
    void reset() { detail::check(nebulosa_reset(d_), "reset"); }

    void set_source(uint8_t src) { detail::check(nebulosa_set_source(d_, src), "set_source"); }
    void set_range_auto(uint8_t min = 0, uint8_t max = 5) { detail::check(nebulosa_set_range_auto(d_, min, max), "set_range"); }
    void set_range_manual(uint8_t range) { detail::check(nebulosa_set_range_manual(d_, range), "set_range"); }
    RangeState range() { RangeState r; detail::check(nebulosa_get_range(d_, &r), "get_range"); return r; }
    void set_range_timing(uint8_t up_filt, uint8_t down_filt, uint8_t overlap, uint16_t hold) {
        detail::check(nebulosa_set_range_timing(d_, up_filt, down_filt, overlap, hold), "set_range_timing");
    }
    void set_range_force(bool enable, uint8_t value = 0) { detail::check(nebulosa_set_range_force(d_, enable, value), "set_range_force"); }
    /// Returns the sample rate applied (Hz).
    uint32_t set_decimation(uint8_t log2n, uint8_t mode = NBP_DECIM_AVERAGE) {
        uint32_t r;
        detail::check(nebulosa_set_decimation(d_, log2n, mode, &r), "set_decimation");
        return r;
    }

    BoardStatus board_status() { BoardStatus b; detail::check(nebulosa_board_status(d_, &b), "board_status"); return b; }
    BoardStatus board_set(uint8_t item, uint32_t value) {
        BoardStatus b;
        detail::check(nebulosa_board_set(d_, item, value, &b), "board_set");
        return b;
    }
    BoardStatus board_test(uint32_t mask = NBP_TEST_ALL) {
        BoardStatus b;
        detail::check(nebulosa_board_test(d_, mask, &b), "board_test");
        return b;
    }
    void set_leds(uint8_t mask) { detail::check(nebulosa_set_leds(d_, mask), "set_leds"); }
    void leds_auto() { detail::check(nebulosa_leds_auto(d_), "leds_auto"); }
    void ui_key(uint8_t key) { detail::check(nebulosa_ui_key(d_, key), "ui_key"); }

    std::vector<uint8_t> flash_read(uint32_t addr, uint32_t len) {
        std::vector<uint8_t> out;
        out.reserve(len);
        uint8_t chunk[NBP_FLASH_CHUNK];
        for (uint32_t off = 0; off < len; off += NBP_FLASH_CHUNK) {
            uint32_t n = std::min<uint32_t>(NBP_FLASH_CHUNK, len - off);
            detail::check(nebulosa_flash_read(d_, addr + off, chunk, n), "flash_read");
            out.insert(out.end(), chunk, chunk + n);
        }
        return out;
    }
    void flash_write(uint32_t addr, const std::vector<uint8_t> &page) {
        detail::check(nebulosa_flash_write(d_, addr, page.data(), static_cast<uint32_t>(page.size())), "flash_write");
    }
    void flash_erase(uint32_t addr, uint32_t len) { detail::check(nebulosa_flash_erase(d_, addr, len), "flash_erase"); }
    BoardStatus fpga_config(uint32_t addr, uint32_t len) {
        BoardStatus b;
        detail::check(nebulosa_fpga_config(d_, addr, len, &b), "fpga_config");
        return b;
    }
    /// Erase, write (blank pages skipped), read back.
    void upload_flash(uint32_t addr, const std::vector<uint8_t> &data, Progress progress = {}) {
        detail::check(nebulosa_upload_flash(d_, addr, data.data(), static_cast<uint32_t>(data.size()),
                                            progress ? &Device::progress_tramp : nullptr, &progress),
                      "upload_flash");
    }

    UpdateInfo update(uint8_t op, uint8_t slot = 0, uint32_t len = 0, uint32_t crc = 0) {
        UpdateInfo i{};
        int e = nebulosa_update(d_, op, slot, len, crc, &i);
        if (e > 0)
            throw UpdateError(e, i);
        detail::check(e, "update");
        return i;
    }
    UpdateInfo update_info() { return update(NBP_UPD_INFO); }
    /// A .bit or .bin gateware: uploaded, tested on the FPGA, made active (else the old one stays).
    UpdateInfo update_gateware(const std::vector<uint8_t> &image, Progress progress = {}) {
        return install(&nebulosa_update_gateware, image, progress, "update_gateware");
    }
    /// An .nbfw firmware image: checked, staged; the device restarts to install it.
    UpdateInfo update_firmware(const std::vector<uint8_t> &image, Progress progress = {}) {
        return install(&nebulosa_update_firmware, image, progress, "update_firmware");
    }

    // ---- calibration ----------------------------------------------------------
    void set_calibration(const Calibration &c) { nebulosa_set_calibration(d_, &c); }
    Calibration calibration() const { Calibration c; nebulosa_get_calibration(d_, &c); return c; }

    // ---- streaming ------------------------------------------------------------
    void start_stream(bool flush = true, bool link_test = false) {
        uint32_t f = (flush ? NBP_START_FLUSH : 0u) | (link_test ? NBP_START_LINK_TEST : 0u);
        detail::check(nebulosa_start_stream(d_, f), "start_stream");
    }
    void stop_stream() { detail::check(nebulosa_stop_stream(d_), "stop_stream"); }

    /// Appends up to `max` samples to `out`, waiting up to `timeout` for the
    /// first. Returns how many were appended (0 on timeout).
    size_t read_samples(std::vector<Sample> &out, size_t max, Ms timeout = Ms(500)) {
        size_t base = out.size();
        out.resize(base + max);
        int n = nebulosa_read_samples(d_, out.data() + base, max, detail::ms(timeout));
        if (n < 0) {
            out.resize(base);
            throw Error(n, "read_samples");
        }
        out.resize(base + static_cast<size_t>(n));
        return static_cast<size_t>(n);
    }

    /// Calls `fn(samples)` with each batch until it returns false.
    template <class Fn>
    void for_each_batch(Fn &&fn, size_t batch = 16384, Ms timeout = Ms(500)) {
        std::vector<Sample> buf;
        buf.reserve(batch);
        for (;;) {
            buf.clear();
            read_samples(buf, batch, timeout);
            if (!fn(static_cast<const std::vector<Sample> &>(buf)))
                break;
        }
    }

    /// One raw DATA block (format-4 FPGAs, or your own decoding); nullopt on timeout.
    std::optional<Block> read_block(Ms timeout = Ms(500)) {
        Block b;
        int r = nebulosa_read_block(d_, &b, detail::ms(timeout));
        if (r < 0)
            throw Error(r, "read_block");
        if (r == 0)
            return std::nullopt;
        return b;
    }

    StreamCounters stream_counters() const { StreamCounters c; nebulosa_stream_counters(d_, &c); return c; }

private:
    explicit Device(nebulosa_t *d) : d_(d) {}

    static void progress_tramp(void *ctx, uint32_t done, uint32_t total) { (*static_cast<Progress *>(ctx))(done, total); }

    using InstallFn = int (*)(nebulosa_t *, const void *, uint32_t, nebulosa_progress_fn, void *, nbp_update_info_t *);
    UpdateInfo install(InstallFn fn, const std::vector<uint8_t> &img, Progress &progress, const char *what) {
        UpdateInfo i{};
        int e = fn(d_, img.data(), static_cast<uint32_t>(img.size()), progress ? &Device::progress_tramp : nullptr,
                   &progress, &i);
        if (e > 0 && i.msg[0])
            throw UpdateError(e, i);
        detail::check(e, what);
        return i;
    }

    nebulosa_t *d_ = nullptr;
};

}  // namespace nebulosa

#endif
