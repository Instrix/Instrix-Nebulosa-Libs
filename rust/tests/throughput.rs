//! Decoding throughput through a user-supplied [`Transport`]: a fake link
//! replaying a valid format-5 stream as fast as it is read. In a release
//! build (`cargo test --release`) it must beat 2 MSPS by a wide margin.

use nebulosa::{Device, Transport};
use std::io;
use std::time::{Duration, Instant};

/// Plays DATA frames of a continuous stream; answers nothing (the device
/// is never asked anything here).
struct Replay {
    frames: Vec<u8>,
    pos: usize,
}

fn stream_frames(nblocks: u32) -> Vec<u8> {
    // FPGA blocks of 256 samples, range cycling, cut into 507-word DATA blocks
    let mut words = Vec::new();
    let mut ts = 0u64;
    let mut seq = 0u32;
    while words.len() < nblocks as usize * 507 {
        let range = seq / 10 % 6;
        words.push((2 << 29) | ((seq & 0xFFFF) << 11) | (range << 2) | 2);
        words.push((2 << 29) | (1 << 27) | ((ts >> 24) as u32 & 0xFF_FFFF));
        words.push((2 << 29) | (2 << 27) | (ts as u32 & 0xFF_FFFF));
        for k in 0..256u32 {
            words.push((1 << 31) | (((k * 37) & 0x3FFF) << 14) | 3300);
        }
        ts += 256;
        seq += 1;
    }
    let mut out = Vec::new();
    for (b, chunk) in words.chunks(507).take(nblocks as usize).enumerate() {
        out.extend_from_slice(&[b'N', b'B', 0x80, 0]);
        out.extend_from_slice(&(b as u16).to_le_bytes());
        out.extend_from_slice(&2040u16.to_le_bytes());
        out.extend_from_slice(&(b as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        out.extend_from_slice(&(if b == 0 { 1u16 } else { 0 }).to_le_bytes());
        for w in chunk {
            out.extend_from_slice(&w.to_be_bytes());
        }
        out.resize(out.len() + 4 * (507 - chunk.len()), 0);
    }
    out
}

impl Transport for Replay {
    fn write_all(&mut self, _data: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn read(&mut self, buf: &mut [u8], _timeout: Duration) -> io::Result<Option<(usize, usize)>> {
        if self.pos >= self.frames.len() {
            return Ok(None);
        }
        // odd chunk sizes: frames split across reads
        let n = buf.len().min(self.frames.len() - self.pos).min(60_001);
        buf[..n].copy_from_slice(&self.frames[self.pos..self.pos + n]);
        self.pos += n;
        Ok(Some((0, n)))
    }
}

#[test]
fn decodes_faster_than_2_msps() {
    let nblocks = 8000; // ~4 M samples
    let mut dev = Device::with_transport(Replay { frames: stream_frames(nblocks), pos: 0 });
    let mut buf = Vec::with_capacity(1 << 16);
    let mut n = 0u64;
    let mut last_ts = None;
    let t0 = Instant::now();
    loop {
        buf.clear();
        if dev.read_samples(&mut buf, 1 << 16, Duration::ZERO).unwrap() == 0 {
            break;
        }
        if let Some(t) = last_ts {
            assert_eq!(buf[0].ts, t + 1);
        }
        last_ts = Some(buf.last().unwrap().ts);
        n += buf.len() as u64;
    }
    let dt = t0.elapsed().as_secs_f64();
    let rate = n as f64 / dt;
    let c = dev.stream_counters();
    assert_eq!((c.data_blocks, c.lost_blocks, c.seq_errors, c.malformed, c.gap_periods), (nblocks as u64, 0, 0, 0, 0));
    assert_eq!(c.samples, n);
    assert!(c.range_changes > 0);
    eprintln!("decoded {n} samples in {dt:.3} s: {:.1} MSPS", rate / 1e6);
    if !cfg!(debug_assertions) {
        assert!(rate > 2.0e6 * 4.0, "only {:.2} MSPS", rate / 1e6);
    }
}
