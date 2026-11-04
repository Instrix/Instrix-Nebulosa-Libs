//! Average current, voltage and power over a time window, per second.
//!
//!     cargo run --release --example average_current -- --host H --seconds 5
//!     cargo run --release --example average_current --features usb -- --usb --log2n 0
//!
//! Options: `--seconds S` (5), `--log2n N` (0: 2 MSPS; needs a release build).

mod common;

use nebulosa::{msg, StreamOptions};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Acc {
    n: u64,
    i: f64,
    v: f64,
    p: f64,
    i_max: f64,
}

fn main() -> nebulosa::Result<()> {
    let a = common::Args::new();
    let seconds: f64 = a.num("--seconds", 5.0);
    let log2n: u8 = a.num("--log2n", 0);
    let mut dev = a.connect();
    let d = dev.set_decimation(log2n, msg::decim::AVERAGE)?;
    dev.set_range_auto(0, 5)?;
    println!("{} samples/s over {}", d.rate_hz, dev.describe());
    dev.start_stream(StreamOptions::default())?;

    let t0 = Instant::now();
    let mut tick = Instant::now();
    let (mut sec, mut total) = (Acc::default(), Acc::default());
    let mut buf = Vec::with_capacity(1 << 16);
    let result = (|| -> nebulosa::Result<()> {
        while t0.elapsed().as_secs_f64() < seconds {
            buf.clear();
            dev.read_samples(&mut buf, 1 << 16, Duration::from_millis(200))?;
            for s in &buf {
                sec.n += 1;
                sec.i += s.current;
                sec.v += s.voltage;
                sec.p += s.current * s.voltage;
                sec.i_max = sec.i_max.max(s.current);
            }
            if tick.elapsed() >= Duration::from_secs(1) {
                tick = Instant::now();
                if sec.n > 0 {
                    let n = sec.n as f64;
                    println!(
                        "{:>9} samples  I {:>12.6e} A (max {:.3e})  V {:.4} V  P {:.6e} W",
                        sec.n,
                        sec.i / n,
                        sec.i_max,
                        sec.v / n,
                        sec.p / n
                    );
                }
                total.n += sec.n;
                total.i += sec.i;
                total.p += sec.p;
                sec = Acc::default();
            }
        }
        Ok(())
    })();
    dev.stop_stream()?;
    result?;
    total.n += sec.n;
    total.i += sec.i;
    total.p += sec.p;
    let c = dev.stream_counters();
    let s = dev.get_stats()?;
    if total.n > 0 {
        println!(
            "total {} samples: I {:.6e} A, P {:.6e} W",
            total.n,
            total.i / total.n as f64,
            total.p / total.n as f64
        );
    }
    println!(
        "{} DATA blocks, {} lost, {} stall flags, {} seq errors, {} malformed; device stalls {}, FPGA drops {}",
        c.data_blocks, c.lost_blocks, c.stall_flags, c.seq_errors, c.malformed, s.stalls, s.fpga_drops
    );
    Ok(())
}
