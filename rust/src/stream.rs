//! FPGA sample stream, format 5 (FPGA ID 0x4E42000B on): a port of the
//! firmware's `neb_stream.c` (`spec/neb_stream.h`) and the Companion's
//! decoder; keep them in step.
//!
//! Every FIFO word (host order, after byte-swapping) is one of, by bits 31:29:
//!
//! ```text
//! sample  1 d 0   [30] DIRTY  [27:14] current  [13:0] voltage (14-bit codes)
//! header  0 1 0   [28:27] type, then:
//!   BLOCK (0)  [26:11] block seq  [10] OVF  [9] decim mode  [8:5] log2 N
//!              [4:2] range  [1] source (0 stub, 1 ADC)
//!   TS_HI (1)  [23:0] timestamp[47:24]
//!   TS_LO (2)  [23:0] timestamp[23:0]
//! empty   0x00000000           late   0x20000000
//! ```
//!
//! A block is BLOCK, TS_HI, TS_LO (empty words may fall between), then
//! 1..256 samples; it ends where the next BLOCK begins. Sample k of a block
//! was taken at timestamp + k * 2^log2N, in 500 ns ADC periods.

use crate::calibration::Calibration;

/// The LATE word: a QSPI read missed its deadline.
pub const LATE_WORD: u32 = 0x2000_0000;
/// ADC sample period: timestamps count these.
pub const TS_PERIOD_S: f64 = 500e-9;

const HDR_BLOCK: u32 = 0;
const HDR_TS_HI: u32 = 1;
const HDR_TS_LO: u32 = 2;

/// A decoded sample, codes only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawSample {
    /// ADC sample periods (500 ns) since FPGA reset.
    pub ts: u64,
    /// 2^log2N: the timestamp distance to the next sample in the block.
    pub step: u32,
    /// 14-bit code, from the chain of `range`.
    pub current: u16,
    /// 14-bit code.
    pub voltage: u16,
    /// 0 = highest current range (2 A) .. 5 (20 uA).
    pub range: u8,
    /// May be unreliable (range switching).
    pub dirty: bool,
}

/// A decoded, calibrated sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// ADC sample periods (500 ns) since FPGA reset.
    pub ts: u64,
    /// 2^log2N: the timestamp distance to the next sample.
    pub step: u32,
    /// 14-bit current code.
    pub current_code: u16,
    /// 14-bit voltage code.
    pub voltage_code: u16,
    /// 0 = highest current range (2 A) .. 5 (20 uA).
    pub range: u8,
    /// May be unreliable (range switching).
    pub dirty: bool,
    /// Amps.
    pub current: f64,
    /// Volts.
    pub voltage: f64,
}

impl Sample {
    /// Seconds since FPGA reset (`ts * 500 ns`).
    #[inline]
    pub fn time_s(&self) -> f64 {
        self.ts as f64 * TS_PERIOD_S
    }
}

impl RawSample {
    /// Converts the codes with `cal`.
    #[inline]
    pub fn calibrate(&self, cal: &Calibration) -> Sample {
        Sample {
            ts: self.ts,
            step: self.step,
            current_code: self.current,
            voltage_code: self.voltage,
            range: self.range,
            dirty: self.dirty,
            current: cal.current(self.current, self.range),
            voltage: cal.voltage(self.voltage),
        }
    }
}

/// What one word was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// Empty, LATE, or a header word (state updated).
    Skip,
    /// A sample.
    Sample(RawSample),
    /// A block header just completed.
    Block,
    /// Malformed: a word out of place (counted).
    Error,
}

/// The decoder's counters (`neb_stream_t`), since it was created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecoderCounters {
    /// Samples decoded.
    pub samples: u64,
    /// Of them, dirty.
    pub dirty: u64,
    /// Block headers completed.
    pub blocks: u64,
    /// Empty words.
    pub empty: u64,
    /// LATE words.
    pub late: u64,
    /// Blocks carrying OVF (samples dropped before them).
    pub ovf_blocks: u64,
    /// Blocks whose range differs from the previous one.
    pub range_changes: u64,
    /// A block's sequence number wasn't the last + 1.
    pub seq_errors: u64,
    /// Words out of place.
    pub malformed: u64,
    /// Samples skipped before the first header.
    pub unframed: u64,
    /// Sample periods between blocks without samples.
    pub gap_periods: u64,
}

/// The format-5 word decoder: feed it words one at a time, in order;
/// blocks may span DATA blocks.
#[derive(Debug, Default, Clone)]
pub struct StreamDecoder {
    ts0: u64,
    ts_next: u64,
    step: u32,
    seq: u16,
    k: u16,
    /// Range of the current block.
    pub range: u8,
    /// Decimation mode of the current block.
    pub dmode: u8,
    /// log2 N of the current block.
    pub log2n: u8,
    /// Source of the current block (0 stub, 1 ADC).
    pub src: u8,
    ovf: bool,
    hdr_need: u8,
    synced: bool,
    have_prev: bool,
    prev_seq: u16,
    prev_ts_next: u64,
    /// Counters.
    pub c: DecoderCounters,
}

impl StreamDecoder {
    /// A decoder waiting for its first BLOCK header.
    pub fn new() -> Self {
        Self::default()
    }

    /// Words were lost upstream: drop samples until the next BLOCK header and
    /// don't count that header's sequence jump again.
    pub fn desync(&mut self) {
        self.synced = false;
        self.have_prev = false;
    }

    /// Decodes one word (host order).
    pub fn feed(&mut self, w: u32) -> Word {
        if w >> 31 != 0 {
            if !self.synced {
                self.c.unframed += 1;
                return Word::Skip;
            }
            if self.hdr_need != 0 {
                self.c.malformed += 1;
                return Word::Error;
            }
            let s = RawSample {
                ts: self.ts_next,
                step: self.step,
                current: ((w >> 14) & 0x3FFF) as u16,
                voltage: (w & 0x3FFF) as u16,
                range: self.range,
                dirty: (w >> 30) & 1 != 0,
            };
            self.ts_next += self.step as u64;
            self.k = self.k.wrapping_add(1);
            self.c.samples += 1;
            self.c.dirty += s.dirty as u64;
            return Word::Sample(s);
        }

        if w >> 29 == 2 {
            match (w >> 27) & 3 {
                HDR_BLOCK => {
                    if self.synced && self.hdr_need != 0 {
                        self.c.malformed += 1; // the last header never completed
                    }
                    if self.synced {
                        self.have_prev = true;
                        self.prev_seq = self.seq;
                        self.prev_ts_next = self.ts_next;
                    }
                    let seq = ((w >> 11) & 0xFFFF) as u16;
                    let range = ((w >> 2) & 7) as u8;
                    if self.have_prev && seq != self.prev_seq.wrapping_add(1) {
                        self.c.seq_errors += 1;
                    }
                    if self.synced && range != self.range {
                        self.c.range_changes += 1;
                    }
                    self.seq = seq;
                    self.ovf = (w >> 10) & 1 != 0;
                    self.dmode = ((w >> 9) & 1) as u8;
                    self.log2n = ((w >> 5) & 0xF) as u8;
                    self.range = range;
                    self.src = ((w >> 1) & 1) as u8;
                    self.step = 1 << self.log2n;
                    self.k = 0;
                    self.hdr_need = 2;
                    self.synced = true;
                    self.c.ovf_blocks += self.ovf as u64;
                    return Word::Skip;
                }
                HDR_TS_HI => {
                    if !self.synced {
                        return Word::Skip;
                    }
                    if self.hdr_need != 2 {
                        self.c.malformed += 1;
                        return Word::Error;
                    }
                    self.ts0 = ((w & 0xFF_FFFF) as u64) << 24;
                    self.hdr_need = 1;
                    return Word::Skip;
                }
                HDR_TS_LO => {
                    if !self.synced {
                        return Word::Skip;
                    }
                    if self.hdr_need != 1 {
                        self.c.malformed += 1;
                        return Word::Error;
                    }
                    self.ts0 |= (w & 0xFF_FFFF) as u64;
                    self.ts_next = self.ts0;
                    self.hdr_need = 0;
                    self.c.blocks += 1;
                    if self.have_prev {
                        if self.ts0 >= self.prev_ts_next {
                            self.c.gap_periods += self.ts0 - self.prev_ts_next;
                        } else {
                            self.c.malformed += 1; // overlaps the previous block
                        }
                    }
                    return Word::Block;
                }
                _ => {
                    self.c.malformed += 1;
                    return Word::Error;
                }
            }
        }

        match w {
            0 => self.c.empty += 1,
            LATE_WORD => self.c.late += 1,
            _ => {
                self.c.malformed += 1;
                return Word::Error;
            }
        }
        Word::Skip
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn block_words(seq: u16, range: u8, log2n: u8, ts: u64, samples: &[(u16, u16, bool)]) -> Vec<u32> {
        let mut v = vec![
            (2 << 29) | ((seq as u32) << 11) | ((log2n as u32) << 5) | ((range as u32) << 2) | 2,
            (2 << 29) | (1 << 27) | ((ts >> 24) as u32 & 0xFF_FFFF),
            0, // empty words may fall between header words
            (2 << 29) | (2 << 27) | (ts as u32 & 0xFF_FFFF),
        ];
        for &(i, u, d) in samples {
            v.push((1 << 31) | ((d as u32) << 30) | ((i as u32) << 14) | u as u32);
        }
        v
    }

    fn samples(d: &mut StreamDecoder, w: &[u32]) -> Vec<RawSample> {
        w.iter()
            .filter_map(|&w| match d.feed(w) {
                Word::Sample(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn decodes_blocks() {
        let mut d = StreamDecoder::new();
        let mut w = block_words(10, 3, 1, 0x0012_3456_789A, &[(100, 200, false), (101, 201, true)]);
        w.extend(block_words(11, 4, 1, 0x0012_3456_789A + 4, &[(5, 6, false)]));
        let s = samples(&mut d, &w);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].ts, 0x0012_3456_789A);
        assert_eq!(s[1].ts, 0x0012_3456_789A + 2);
        assert_eq!(s[0].step, 2);
        assert!(s[1].dirty);
        assert_eq!((s[2].range, s[2].current, s[2].voltage), (4, 5, 6));
        assert_eq!(d.c.seq_errors, 0);
        assert_eq!(d.c.range_changes, 1);
        assert_eq!(d.c.gap_periods, 0);
        assert_eq!(d.c.malformed, 0);
        assert_eq!((d.c.blocks, d.c.samples, d.c.dirty, d.c.empty), (2, 3, 1, 2));
    }

    #[test]
    fn counts_errors_gaps_and_unframed() {
        let mut d = StreamDecoder::new();
        // samples before any header are unframed
        let mut w = vec![(1u32 << 31) | 5, LATE_WORD];
        w.extend(block_words(1, 0, 0, 100, &[(1, 1, false)]));
        // seq jumps 1 -> 3, ts leaves a 9-period gap
        w.extend(block_words(3, 0, 0, 110, &[(2, 2, false)]));
        // garbage word
        w.push(0x1234_5678);
        let s = samples(&mut d, &w);
        assert_eq!(s.len(), 2);
        assert_eq!((d.c.unframed, d.c.late, d.c.seq_errors, d.c.gap_periods, d.c.malformed), (1, 1, 1, 9, 1));

        // desync: the next block's jump is not counted
        d.desync();
        let s = samples(&mut d, &block_words(40, 0, 0, 500, &[(3, 3, false)]));
        assert_eq!(s.len(), 1);
        assert_eq!(d.c.seq_errors, 1);

        // a sample inside an incomplete header is malformed
        let mut w = block_words(41, 0, 0, 600, &[]);
        w.truncate(2);
        w.push((1 << 31) | 7);
        samples(&mut d, &w);
        assert_eq!(d.c.malformed, 2);
    }

    #[test]
    fn calibrated_sample() {
        let r = RawSample { ts: 2_000_000, step: 1, current: 8192, voltage: 3300, range: 1, dirty: false };
        let s = r.calibrate(&Calibration::default());
        assert!((s.current - 0.1).abs() < 1e-12 && (s.voltage - 3.3).abs() < 1e-12);
        assert!((s.time_s() - 1.0).abs() < 1e-12);
        assert_eq!((s.current_code, s.voltage_code, s.range), (8192, 3300, 1));
    }
}
