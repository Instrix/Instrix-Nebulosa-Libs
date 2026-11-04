# nebulosa (Rust)

A Rust library for the INSTRIX Nebulosa power profiler. It talks to the
device directly over TCP or USB, so you can script it without the Companion
app. It implements the common contract in [`../docs/API.md`](../docs/API.md).
The wire format is [`../spec/nbp.h`](../spec/nbp.h) (NBP) and
[`../spec/neb_stream.h`](../spec/neb_stream.h) (FPGA stream format 5).

- Blocking API, one `Device` per connection. Every NBP command is a method
  that returns `Result<T, nebulosa::Error>`.
- The sample stream comes out decoded and calibrated (`Sample`: timestamp,
  range, codes, amps, volts). It keeps up with 2 MSPS (8 MB/s) easily in a
  release build; the bundled replay benchmark decodes about 60 MSPS.
- DATA that arrives while a command waits for its response is queued, never
  dropped.
- Helpers: `crc32`, `set_leds` / `leds_auto`, `ui_key`, `upload_flash`
  (erase / write / verify), `update_gateware` (.bit or .bin) and
  `update_firmware` (.nbfw).
- UDP discovery, and USB device listing (with the `usb` feature).

## Features

| feature | what | dependencies |
|---------|------|--------------|
| *(default)* | TCP and UDP discovery, std only, blocking | none |
| `usb` | USB transport, `Device::usb`, `list_usb` | [`rusb`](https://crates.io/crates/rusb) 0.9 (libusb-1.0) |

```toml
[dependencies]
nebulosa = { path = "…/Instrix-Nebulosa-Libs/rust" }              # TCP only
nebulosa = { path = "…/Instrix-Nebulosa-Libs/rust", features = ["usb"] }
```

The `usb` feature uses **rusb** and links to the system libusb-1.0, found
through pkg-config. On Debian or Ubuntu that is `libusb-1.0-0-dev`. The
library claims only the NBP vendor interface (class FF / 4E / 01) and leaves
the CDC log interface of the Full-Speed device alone. Each bulk IN endpoint
gets its own reader thread with a 64 KB read always outstanding. Each thread
has its own frame parser and feeds one channel. Keep a read outstanding on
every endpoint: if you read the endpoints one after another, a wait on the
idle response endpoint starves the DATA endpoint and the device stalls.

## Usage

```rust
use std::time::Duration;
use nebulosa::{msg, Device, StreamOptions};

fn main() -> nebulosa::Result<()> {
    let mut dev = Device::tcp("192.168.1.50")?;          // port 20034 by default
    // let mut dev = Device::usb(nebulosa::UsbSelector::Any)?;   // feature "usb"
    let info = dev.get_info()?;
    println!("firmware {} on {}", info.fw_version_string(), info.link_name());

    dev.set_decimation(4, msg::decim::AVERAGE)?;           // 2 MSPS / 16
    dev.set_range_auto(0, 5)?;
    dev.start_stream(StreamOptions::default())?;
    let mut buf = Vec::new();
    let t0 = std::time::Instant::now();
    while t0.elapsed() < Duration::from_secs(2) {
        buf.clear();
        dev.read_samples(&mut buf, 65536, Duration::from_millis(200))?;
        for s in &buf {
            // s.ts, s.step, s.range, s.dirty, s.current_code, s.voltage_code
            let _ = (s.time_s(), s.current, s.voltage);
        }
    }
    dev.stop_stream()?;
    let c = dev.stream_counters();
    println!("{} samples, {} lost blocks", c.samples, c.lost_blocks);
    Ok(())
}
```

- `dev.samples(timeout)` is an iterator of `Result<Sample>`. It ends when no
  sample arrives within `timeout`.
- `read_blocks` returns raw DATA blocks, with the FPGA words converted to
  host order. Use it for FPGAs older than `0x4E42000B` (stream format 4,
  which is not decoded; check `Info::is_format5()`) and for link tests.
- Replace the default calibration with `set_calibration(Calibration { .. })`.
  The default full scales are 2 A, 200 mA, 20 mA, 2 mA, 200 uA and 20 uA,
  at 1 mV per voltage code.
- To run NBP over anything else, implement `Transport` (`write_all`, plus a
  `read` with a timeout that returns `(source, n)`) and pass it to
  `Device::with_transport`.
- `request` and `transact` send raw commands.
- `timeouts_mut()` changes the response timeouts. The defaults are 1.5 s,
  `board_test` 15 s, `fpga_config` 10 s, `flash_erase` 5 s, UPDATE gateware
  15 s, UPDATE firmware 10 s, and other UPDATE ops 30 s.

Errors are `Io`, `Timeout { cmd }`, `Status { cmd, status }` (its Display
gives the status name, such as `BAD_ARG`), `Update { status, info }` (with
the device's `info.msg`), `BadResponse`, `Closed`, `Invalid` (an argument
the protocol cannot carry, or an image that fails its checks) and `Verify`.

### Software update

`update_gateware(file)` accepts a `.bit` or a raw `.bin`. It must contain
the sync word `AA995566`. `update_firmware(image)` takes an `.nbfw` and
checks it first with `update::firmware_check`. Both run these steps:

1. Read UPDATE INFO.
2. Erase, write and verify the update slot at `update_slot * slot_size`.
3. Send UPDATE GATEWARE or UPDATE FIRMWARE with the length and CRC-32.

A gateware is test-loaded and made active only if it works; otherwise the
device keeps the old one. After a firmware update the device restarts about
0.5 s after it answers, so reconnect and check `update_info()`.
`_with_progress` variants report the erase, write and verify progress.

## Connection notes

- **TCP takes one client at a time.** The device refuses a second
  connection, so close the Companion (or another script) first. Closing the
  connection stops a stream it started. `TCP_NODELAY` is set.
- **USB on Linux needs permissions.** Install
  [`../tools/99-nebulosa.rules`](../tools/99-nebulosa.rules):

  ```sh
  sudo cp tools/99-nebulosa.rules /etc/udev/rules.d/
  sudo udevadm control --reload && sudo udevadm trigger
  ```

  Without it, opening the device fails with a permission error (the message
  points at the rules file), and `list_usb()` shows no strings.
- Discovery: `nebulosa::discover(Duration::from_millis(600))` broadcasts to
  UDP port 20035. `discover_at(addr, timeout)` sends to one address.

## Examples

```sh
cargo run --example info -- --host 192.168.1.50
cargo run --example info --features usb -- --usb          # or --discover
cargo run --release --example stream_csv -- --host H --seconds 2 --log2n 4 --out x.csv
cargo run --release --example average_current --features usb -- --usb --log2n 0
cargo run --example board_leds -- --host H
```

## Tests

```sh
cargo test                 # unit tests + tests/sim.rs (spawns python3 ../tools/nebulosa_sim.py)
cargo test --release --test throughput -- --nocapture       # decode rate
cargo test --release --features usb --test hardware -- --ignored --test-threads 1   # a real device
```

`tests/sim.rs` starts its own simulator on a free port for each test. Only
`python3` is needed. The hardware tests are harmless: info, LEDs, and a
3-second stream at 2 MSPS that checks for 0 lost blocks and no stalls. They
never erase or write flash and never run an update.
