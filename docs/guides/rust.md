# Rust guide

The `nebulosa` crate is a blocking API, one `Device` per connection, with
**no dependencies** by default (std TCP / UDP). The `usb` feature adds USB
through [rusb](https://crates.io/crates/rusb) (libusb-1.0). A release build
keeps up with 2 MSPS easily (measured on hardware: 0 lost blocks; the
decoder alone does ~60 MSPS).

## Add it

```toml
[dependencies]
nebulosa = { git = "https://github.com/Instrix/Instrix-Nebulosa-Libs" }                      # TCP only
nebulosa = { git = "https://github.com/Instrix/Instrix-Nebulosa-Libs", features = ["usb"] }  # + USB
```

(or `path = ".../Instrix-Nebulosa-Libs/rust"` for a local clone). The `usb`
feature needs libusb-1.0 found through pkg-config (`libusb-1.0-0-dev` on
Debian / Ubuntu). Minimum Rust: 1.74.

API docs: [Rust API reference](../api/rust/nebulosa/index.html).

## Connect

```rust
use nebulosa::Device;

fn main() -> nebulosa::Result<()> {
    let mut dev = Device::tcp("192.168.1.20")?;                 // "host" or "host:port"
    // let mut dev = Device::usb(nebulosa::UsbSelector::Any)?;  // feature "usb"
    let info = dev.get_info()?;
    println!("{}: firmware {} on {}", dev.describe(), info.fw_version_string(), info.link_name());
    Ok(())
}
```

```rust
for d in nebulosa::discover(std::time::Duration::from_millis(600))? {
    println!("{} {}", d.addr, d.info.fw_version_string());
    let dev = Device::tcp(&d.addr.to_string())?;
}
```

`UsbSelector::Any`, `::Serial(String)`, or by bus / address;
`nebulosa::list_usb()` lists devices. Dropping a `Device` stops a stream it
started.

## Measure

```rust
use std::time::{Duration, Instant};
use nebulosa::{msg, Device, StreamOptions};

fn main() -> nebulosa::Result<()> {
    let mut dev = Device::tcp("192.168.1.20")?;
    dev.set_range_auto(0, 5)?;
    let rate = dev.set_decimation(4, msg::decim::AVERAGE)?.rate_hz;   // 125 kS/s
    dev.start_stream(StreamOptions::default())?;                      // flush: true

    let (mut sum, mut n) = (0.0, 0u64);
    let mut buf = Vec::with_capacity(65536);
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(5) {
        buf.clear();
        dev.read_samples(&mut buf, 65536, Duration::from_millis(200))?;
        for s in &buf {
            sum += s.current;                  // also s.voltage, s.range, s.dirty, s.time_s()
        }
        n += buf.len() as u64;
    }
    dev.stop_stream()?;

    let c = dev.stream_counters();
    println!("{rate} S/s: mean {:.6e} A over {n} samples, {} lost blocks", sum / n as f64, c.lost_blocks);
    Ok(())
}
```

Or as an iterator: `for s in dev.samples(Duration::from_millis(500)) { let s = s?; ... }`
(ends after the timeout passes with no data).

`read_blocks(&mut Vec<Block>, max, timeout)` gives raw blocks (host-order
words) for format-4 FPGAs (`!info.is_format5()`) or your own decoding.

## Control the board

```rust
use nebulosa::msg::{board, source};

let b = dev.board_status()?;
println!("V_MAIN {} mV, flags ok: {}", b.v_main_mv, b.has_flags(board::flag::APWR_OK));
dev.set_leds(0b10101)?;
dev.leds_auto()?;
dev.ui_key(board::key::DOWN)?;
dev.board_set(board::set::APWR_H, 1)?;           // Status NO_POWER without 12 V
dev.set_source(source::AD7387)?;
let r = dev.board_test(board::test::ALL)?;       // stop streaming first
println!("{:?}", r.test_result(board::test::FPGA_DONE));
```

## Updates

```rust
let bit = std::fs::read("nebulosa.bit")?;
match dev.update_gateware_with_progress(&bit, |p| eprint!("\r{:?} {}/{}", p.stage, p.done, p.total)) {
    Ok(info) => println!("active slot {}", info.active),
    Err(nebulosa::Error::Update { status, info }) => eprintln!("refused ({status}): {}", info.msg),
    Err(e) => return Err(e),
}
```

`update_firmware(&nbfw)` checks the `.nbfw` image first
(`update::firmware_check`); the device restarts ~0.5 s after answering.
Lower level: `flash_read`, `flash_write`, `flash_erase`, `upload_flash`,
`fpga_config`, `update`, `update_info`, `update_revert`.

## Errors

`nebulosa::Error`: `Io`, `Timeout { cmd }`, `Status { cmd, status }` (its
`Display` names the status, e.g. `BAD_ARG`), `Update { status, info }`,
`BadResponse`, `Closed`, `Invalid`, `Verify { addr, expected, got }`.

```rust
match dev.set_range_manual(9) {
    Err(nebulosa::Error::Status { status, .. }) if status == nebulosa::msg::status::BAD_ARG => {}
    other => other?,
}
```

## Tuning

- `dev.timeouts_mut().default = Duration::from_secs(3);` (also `board_test`,
  `fpga_config`, `flash_erase`, ...).
- `dev.set_calibration(Calibration { i_fs, i_offset, v_scale, v_offset })`.
- Custom links: implement `nebulosa::Transport` and use
  `Device::with_transport(t)`.
- Raw commands: `dev.request(cmd, &payload, timeout)` / `dev.transact(...)`.

## Examples and tests

```sh
cd rust
cargo run --example info -- --host 192.168.1.20
cargo run --release --features usb --example average_current -- --usb --log2n 0
cargo run --release --example stream_csv -- --host H --seconds 2 --log2n 4 --out x.csv
cargo test                                         # starts the simulator itself
cargo test --release --features usb --test hardware -- --ignored --test-threads 1
```
