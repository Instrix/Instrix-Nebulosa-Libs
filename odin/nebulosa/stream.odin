package nebulosa

// FPGA sample stream, format 5 (FPGA ID 0x4E42000B on). A port of the
// Companion's crates/nbp-core/src/stream.rs (itself a port of the
// firmware's neb_stream.c); keep them in step.
//
// Every FIFO word (host order, after the big-endian read) is one of, by
// bits 31:29:
//   sample  1 d 0   [30] DIRTY  [27:14] current  [13:0] voltage (14-bit codes)
//   header  0 1 0   [28:27] type, then:
//     BLOCK (0)  [26:11] block seq  [10] OVF  [9] decim mode  [8:5] log2 N
//                [4:2] range  [1] source (0 stub, 1 ADC)
//     TS_HI (1)  [23:0] timestamp[47:24]
//     TS_LO (2)  [23:0] timestamp[23:0]
//   empty   0x00000000           late   0x20000000
// A block is BLOCK, TS_HI, TS_LO (empty words may fall between), then
// 1..256 samples; it ends where the next BLOCK begins. Sample k of a block
// was taken at timestamp + k * 2^log2N, in 500 ns ADC periods.

LATE_WORD :: u32(0x2000_0000)

@(private)
HDR_BLOCK :: 0
@(private)
HDR_TS_HI :: 1
@(private)
HDR_TS_LO :: 2

// A decoded sample, codes only.
Raw_Sample :: struct {
	ts:      u64, // ADC sample periods (500 ns) since FPGA reset
	step:    u32, // 2^log2N: the timestamp distance to the next sample in the block
	current: u16, // 14-bit code, from the chain of `range`
	voltage: u16, // 14-bit code
	range:   u8,
	dirty:   bool,
}

// What one word was.
Word_Kind :: enum u8 {
	Skip,   // empty, LATE, or a header word (state updated)
	Sample, // the returned Raw_Sample is valid
	Block,  // a block header just completed
	Error,  // malformed: a word out of place (counted)
}

Decoder_Counters :: struct {
	samples:       u64,
	dirty:         u64,
	blocks:        u64,
	empty:         u64,
	late:          u64,
	ovf_blocks:    u64,
	range_changes: u64,
	seq_errors:    u64,
	malformed:     u64,
	unframed:      u64,
	gap_periods:   u64,
}

Stream_Decoder :: struct {
	ts0:          u64,
	ts_next:      u64,
	step:         u32,
	seq:          u16,
	k:            u16,
	range:        u8,
	dmode:        u8,
	log2n:        u8,
	src:          u8,
	ovf:          bool,
	hdr_need:     u8,
	synced:       bool,
	have_prev:    bool,
	prev_seq:     u16,
	prev_ts_next: u64,
	counters:     Decoder_Counters,
}

decoder_reset :: proc(d: ^Stream_Decoder) {
	d^ = {}
}

// Words were lost upstream: drop samples until the next BLOCK header and
// don't count that header's sequence jump again.
decoder_desync :: proc(d: ^Stream_Decoder) {
	d.synced = false
	d.have_prev = false
}

decoder_feed :: proc(d: ^Stream_Decoder, w: u32) -> (kind: Word_Kind, s: Raw_Sample) {
	if w >> 31 != 0 {
		if !d.synced {
			d.counters.unframed += 1
			return .Skip, {}
		}
		if d.hdr_need != 0 {
			d.counters.malformed += 1
			return .Error, {}
		}
		s = Raw_Sample {
			ts      = d.ts_next,
			step    = d.step,
			current = u16((w >> 14) & 0x3FFF),
			voltage = u16(w & 0x3FFF),
			range   = d.range,
			dirty   = (w >> 30) & 1 != 0,
		}
		d.ts_next += u64(d.step)
		d.k += 1 // wraps
		d.counters.samples += 1
		if s.dirty {
			d.counters.dirty += 1
		}
		return .Sample, s
	}

	if w >> 29 == 2 {
		switch (w >> 27) & 3 {
		case HDR_BLOCK:
			if d.synced && d.hdr_need != 0 {
				d.counters.malformed += 1 // the last header never completed
			}
			if d.synced {
				d.have_prev = true
				d.prev_seq = d.seq
				d.prev_ts_next = d.ts_next
			}
			seq := u16((w >> 11) & 0xFFFF)
			rng := u8((w >> 2) & 7)
			if d.have_prev && seq != d.prev_seq + 1 {
				d.counters.seq_errors += 1
			}
			if d.synced && rng != d.range {
				d.counters.range_changes += 1
			}
			d.seq = seq
			d.ovf = (w >> 10) & 1 != 0
			d.dmode = u8((w >> 9) & 1)
			d.log2n = u8((w >> 5) & 0xF)
			d.range = rng
			d.src = u8((w >> 1) & 1)
			d.step = u32(1) << d.log2n
			d.k = 0
			d.hdr_need = 2
			d.synced = true
			if d.ovf {
				d.counters.ovf_blocks += 1
			}
			return .Skip, {}
		case HDR_TS_HI:
			if !d.synced {
				return .Skip, {}
			}
			if d.hdr_need != 2 {
				d.counters.malformed += 1
				return .Error, {}
			}
			d.ts0 = u64(w & 0xFF_FFFF) << 24
			d.hdr_need = 1
			return .Skip, {}
		case HDR_TS_LO:
			if !d.synced {
				return .Skip, {}
			}
			if d.hdr_need != 1 {
				d.counters.malformed += 1
				return .Error, {}
			}
			d.ts0 |= u64(w & 0xFF_FFFF)
			d.ts_next = d.ts0
			d.hdr_need = 0
			d.counters.blocks += 1
			if d.have_prev {
				if d.ts0 >= d.prev_ts_next {
					d.counters.gap_periods += d.ts0 - d.prev_ts_next
				} else {
					d.counters.malformed += 1 // overlaps the previous block
				}
			}
			return .Block, {}
		case:
			d.counters.malformed += 1
			return .Error, {}
		}
	}

	switch w {
	case 0:
		d.counters.empty += 1
	case LATE_WORD:
		d.counters.late += 1
	case:
		d.counters.malformed += 1
		return .Error, {}
	}
	return .Skip, {}
}

// ---- calibration and samples --------------------------------------------

// Code -> unit conversion. Index by range (0 = 2 A .. 5 = 20 uA; 6, 7 unused).
Calibration :: struct {
	i_fs:     [8]f64, // full scale current of each range, A
	i_offset: [8]f64, // A
	v_scale:  f64, // V per code
	v_offset: f64, // V
}

default_calibration :: proc() -> Calibration {
	return Calibration {
		i_fs = {2.0, 0.2, 20e-3, 2e-3, 200e-6, 20e-6, 0, 0},
		i_offset = {},
		v_scale = 1e-3,
		v_offset = 0,
	}
}

// A decoded, calibrated sample.
Sample :: struct {
	ts:           u64, // ADC periods of 500 ns since FPGA reset (sample_time_s)
	step:         u32, // 2^log2N: ts distance to the next sample
	current:      f64, // A
	voltage:      f64, // V
	current_code: u16, // 14-bit
	voltage_code: u16, // 14-bit
	range:        u8, // 0 = highest current range (2 A) .. 5 (20 uA)
	dirty:        bool, // may be unreliable (range switching)
}

calibrate :: proc(s: Raw_Sample, cal: ^Calibration) -> Sample {
	r := int(s.range & 7)
	return Sample {
		ts = s.ts,
		step = s.step,
		current = f64(s.current) * cal.i_fs[r] / 16384 + cal.i_offset[r],
		voltage = f64(s.voltage) * cal.v_scale + cal.v_offset,
		current_code = s.current,
		voltage_code = s.voltage,
		range = s.range,
		dirty = s.dirty,
	}
}

// Seconds since FPGA reset.
sample_time_s :: proc(s: Sample) -> f64 {
	return f64(s.ts) * TS_PERIOD_S
}
