# Nebulosa developer libraries

Control the **INSTRIX Nebulosa** power profiler and stream its samples from
your own scripts and programs, over **USB** or **Ethernet**, without the
Companion app.

| Language | Package | Transports | Status |
|----------|---------|------------|--------|
| [Python](guides/python.md) | `nebulosa` (pip) | TCP, discovery, USB (pyusb) | tested on hardware |
| [C](guides/c.md) | `libnebulosa` (CMake) | TCP, discovery, USB (libusb-1.0) | tested on hardware, 2 MSPS lossless |
| [C++](guides/cpp.md) | `nebulosa.hpp` (header-only) | via the C library | tested on hardware |
| [Rust](guides/rust.md) | `nebulosa` crate | TCP, discovery, USB (`usb` feature) | tested on hardware, 2 MSPS lossless |
| [Odin](guides/odin.md) | `nebulosa` package | TCP, discovery, USB (libusb-1.0) | experimental: built in CI only |

Every library has the same model: open a device, send commands, stream
samples.

=== "Python"

    ```python
    import nebulosa

    with nebulosa.Nebulosa.usb() as dev:          # or Nebulosa.tcp("192.168.1.20")
        dev.set_decimation(4)                      # 125 kS/s
        dev.start_stream()
        for s in dev.read_samples(timeout=1.0)[:5]:
            print(s.time_s, s.current, s.voltage, s.range)
        dev.stop_stream()
    ```

=== "C"

    ```c
    nebulosa_t *d;
    nebulosa_open_usb(&d, NULL, 0);                  /* or nebulosa_open_tcp(&d, host, 0, 3000) */
    nebulosa_set_decimation(d, 4, NBP_DECIM_AVERAGE, NULL);
    nebulosa_start_stream(d, NBP_START_FLUSH);
    nebulosa_sample_t s[1024];
    int n = nebulosa_read_samples(d, s, 1024, 1000);
    nebulosa_stop_stream(d);
    nebulosa_close(d);
    ```

=== "C++"

    ```cpp
    auto dev = nebulosa::Device::usb();
    dev.set_decimation(4);
    dev.start_stream();
    std::vector<nebulosa::Sample> s;
    dev.read_samples(s, 1024, std::chrono::milliseconds(1000));
    dev.stop_stream();
    ```

=== "Rust"

    ```rust
    let mut dev = nebulosa::Device::tcp("192.168.1.20")?;
    dev.set_decimation(4, nebulosa::msg::decim::AVERAGE)?;
    dev.start_stream(nebulosa::StreamOptions::default())?;
    let mut s = Vec::new();
    dev.read_samples(&mut s, 1024, std::time::Duration::from_secs(1))?;
    dev.stop_stream()?;
    ```

=== "Odin"

    ```odin
    dev, err := nb.connect_tcp("192.168.1.20")
    if err != nil { return }
    defer nb.close(dev)
    _, _ = nb.set_decimation(dev, 4)
    _ = nb.start_stream(dev)
    buf: [1024]nb.Sample
    n, _ := nb.read_samples(dev, buf[:], time.Second)
    ```

## Where to go next

- **[Getting started](getting-started.md)**: connect the device, USB
  permissions, network discovery, your first script.
- **[Concepts](concepts.md)**: samples, ranges, decimation, timestamps,
  calibration, and how to tell if you lost data.
- **Guides**: one per language, from install to software updates.
- **[Simulator](simulator.md)**: develop and test without hardware.
- **[Protocol reference](API.md)**: the wire format, for writing a binding of
  your own.
