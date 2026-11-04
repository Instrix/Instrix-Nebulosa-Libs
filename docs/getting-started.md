# Getting started

## 1. Connect the device

The Nebulosa talks NBP (Nebulosa Bulk Protocol) on three links:

| Link | Where | Speed | Notes |
|------|-------|-------|-------|
| USB SuperSpeed / High-Speed | USB 3 Type-A port | full rate (2 MSPS) | VID `1A86`, PID `FE0D` |
| USB Full-Speed | USB-C port | decimated rates only | PID `FE0C`, next to a CDC log port |
| Ethernet (TCP) | RJ45 | depends on the network | port `20034`, discovery on UDP `20035` |

For real measurements, power the analog front end from **12 V**: the barrel
jack, or a USB-PD source that offers 12 V. Without that the device streams
its stub source (test data), which is still fine for developing against.

## 2. USB permissions

=== "Linux"

    Normal users need the udev rule from the repository:

    ```sh
    sudo cp tools/99-nebulosa.rules /etc/udev/rules.d/
    sudo udevadm control --reload && sudo udevadm trigger
    ```

    Unplug and replug the device afterwards. Also install libusb:
    `sudo apt install libusb-1.0-0-dev` (Debian / Ubuntu).

=== "macOS"

    No driver needed. Install libusb: `brew install libusb`.

=== "Windows"

    Bind the **WinUSB** driver to the NBP interface (interface with class
    `FF`, subclass `4E`), for example with [Zadig](https://zadig.akeo.ie/).
    Leave the CDC log interface of the USB-C device alone. Ethernet needs
    no driver.

## 3. Find the device on the network

All libraries can broadcast a discovery request; every Nebulosa on the LAN
answers with its info. The device's IP is also shown on its screen.

```sh
pip install ./python
python3 python/examples/info.py           # no argument: discover
python3 python/examples/info.py --host 192.168.1.20
python3 python/examples/info.py --usb
```

!!! warning "One TCP client at a time"
    The device accepts a single TCP connection and refuses a second one.
    Close the Companion app (or any other script) first. Closing a
    connection stops a stream it started.

## 4. Pick a language

| If you want... | Use |
|----------------|-----|
| quick scripts, notebooks, numpy / pandas analysis | [Python](guides/python.md) |
| an embedded host, a plugin, or bindings for another language | [C](guides/c.md) |
| a desktop tool with RAII and exceptions | [C++](guides/cpp.md) |
| a fast, safe tool or a service | [Rust](guides/rust.md) |
| Odin programs | [Odin](guides/odin.md) |

## 5. Your first measurement

The shape is the same everywhere:

1. **open** the device (`usb()` / `tcp(host)`);
2. **configure**: `set_range_auto()`, `set_decimation(log2n)`;
3. **stream**: `start_stream()`, then call `read_samples()` in a loop;
4. **stop**: `stop_stream()`, then check `stream_counters()` for loss.

Each guide has this as a complete program, and every library ships an
`average_current` example that prints mean, min and max current and the
charge over a time window.
