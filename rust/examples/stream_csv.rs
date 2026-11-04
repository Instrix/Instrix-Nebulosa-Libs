//! Streams samples to CSV (stdout or `--out FILE`).
//!
//!     cargo run --release --example stream_csv -- --host H --seconds 2 --log2n 4 --out x.csv
//!
//! Options: `--seconds S` (1), `--log2n N` (4: 125 kSPS), `--range R`
//! (fixed range 0..5; default auto), `--out FILE`.

mod common;

use nebulosa::{msg, StreamOptions};
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = common::Args::new();
    let seconds: f64 = a.num("--seconds", 1.0);
    let log2n: u8 = a.num("--log2n", 4);
    let mut dev = a.connect();
    let out: Box<dyn Write> = match a.value("--out") {
        Some(p) => Box::new(std::fs::File::create(p)?),
        None => Box::new(std::io::stdout().lock()),
    };
    let mut out = BufWriter::with_capacity(1 << 20, out);

    let d = dev.set_decimation(log2n, msg::decim::AVERAGE)?;
    match a.value("--range") {
        Some(r) => dev.set_range_manual(r.parse()?)?,
        None => dev.set_range_auto(0, 5)?,
    }
    eprintln!("streaming {seconds} s at {} samples/s", d.rate_hz);
    writeln!(out, "time_s,current_a,voltage_v,range,dirty")?;
    dev.start_stream(StreamOptions::default())?;
    let t0 = Instant::now();
    let mut buf = Vec::with_capacity(1 << 16);
    let mut n = 0u64;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        while t0.elapsed().as_secs_f64() < seconds {
            buf.clear();
            dev.read_samples(&mut buf, 1 << 16, Duration::from_millis(200))?;
            for s in &buf {
                writeln!(out, "{:.7},{:.6e},{:.4},{},{}", s.time_s(), s.current, s.voltage, s.range, s.dirty as u8)?;
            }
            n += buf.len() as u64;
        }
        Ok(())
    })();
    dev.stop_stream()?;
    out.flush()?;
    result?;
    let c = dev.stream_counters();
    eprintln!(
        "{n} samples, {} DATA blocks, {} lost, {} seq errors, {} malformed, {} range changes",
        c.data_blocks, c.lost_blocks, c.seq_errors, c.malformed, c.range_changes
    );
    Ok(())
}
