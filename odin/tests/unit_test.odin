package nebulosa_tests

// Unit tests: framing, the stream decoder, record layouts, CRC.
//
//     odin test odin/tests -collection:nebulosa=odin

import "core:testing"
import nb "nebulosa:nebulosa"

@(private = "file")
w16 :: proc(b: []u8, off: int, v: u16) {
	b[off] = u8(v)
	b[off + 1] = u8(v >> 8)
}

@(private = "file")
w32 :: proc(b: []u8, off: int, v: u32) {
	b[off] = u8(v)
	b[off + 1] = u8(v >> 8)
	b[off + 2] = u8(v >> 16)
	b[off + 3] = u8(v >> 24)
}

// ---- framing ----------------------------------------------------------------

@(test)
frame_roundtrip_split_and_garbage :: proc(t: ^testing.T) {
	hello := "hello"
	a: [64]u8
	na, oka := nb.frame_encode(a[:], 0x41, 7, transmute([]u8)hello)
	testing.expect(t, oka)
	testing.expect_value(t, na, 13)
	testing.expect_value(t, a[0], u8('N'))
	testing.expect_value(t, a[1], u8('B'))
	b: [8]u8
	nb_, okb := nb.frame_encode(b[:], 0x42, 8, nil)
	testing.expect(t, okb)

	stream := make([dynamic]u8)
	defer delete(stream)
	append(&stream, 0xFF, 'N', 0x00) // garbage, with a false 'N'
	append(&stream, ..a[:na])
	append(&stream, ..b[:nb_])

	p: nb.Frame_Parser
	nb.parser_init(&p)
	defer nb.parser_destroy(&p)

	count := 0
	for i := 0; i < len(stream); i += 3 {
		end := min(i + 3, len(stream))
		nb.parser_push(&p, stream[i:end])
		for {
			f, ok := nb.parser_next(&p)
			if !ok {
				break
			}
			switch count {
			case 0:
				testing.expect_value(t, f.ty, u8(0x41))
				testing.expect_value(t, f.seq, u16(7))
				testing.expect_value(t, string(f.payload), "hello")
			case 1:
				testing.expect_value(t, f.ty, u8(0x42))
				testing.expect_value(t, f.seq, u16(8))
				testing.expect_value(t, len(f.payload), 0)
			}
			count += 1
		}
	}
	testing.expect_value(t, count, 2)
	testing.expect(t, p.resyncs > 0)
}

@(test)
frame_rejects_oversized_len :: proc(t: ^testing.T) {
	// a header claiming 0xFFFF bytes, then a real frame
	bad := [8]u8{'N', 'B', 0x41, 0, 1, 0, 0xFF, 0xFF}
	good: [16]u8
	n, _ := nb.frame_encode(good[:], 0x43, 9, []u8{1, 2, 3})

	p: nb.Frame_Parser
	nb.parser_init(&p)
	defer nb.parser_destroy(&p)
	nb.parser_push(&p, bad[:])
	nb.parser_push(&p, good[:n])
	f, ok := nb.parser_next(&p)
	testing.expect(t, ok)
	testing.expect_value(t, f.ty, u8(0x43))
	testing.expect_value(t, f.seq, u16(9))
	testing.expect_value(t, len(f.payload), 3)
	testing.expect(t, p.resyncs >= 1)
	_, more := nb.parser_next(&p)
	testing.expect(t, !more)
}

@(test)
frame_trailing_n_kept :: proc(t: ^testing.T) {
	p: nb.Frame_Parser
	nb.parser_init(&p)
	defer nb.parser_destroy(&p)
	// garbage ending in 'N', then "B" + rest of a header in the next push
	nb.parser_push(&p, []u8{0x00, 0x11, 'N'})
	_, ok := nb.parser_next(&p)
	testing.expect(t, !ok)
	nb.parser_push(&p, []u8{'B', 0x45, 0, 3, 0, 0, 0})
	f, ok2 := nb.parser_next(&p)
	testing.expect(t, ok2)
	testing.expect_value(t, f.ty, u8(0x45))
	testing.expect_value(t, f.seq, u16(3))
}

@(test)
frame_encode_limits :: proc(t: ^testing.T) {
	small: [4]u8
	_, ok := nb.frame_encode(small[:], 1, 0, nil)
	testing.expect(t, !ok)
	big := make([]u8, nb.MAX_FRAME_LEN + 1)
	defer delete(big)
	out := make([]u8, nb.MAX_FRAME_LEN + 16)
	defer delete(out)
	_, ok2 := nb.frame_encode(out, 1, 0, big)
	testing.expect(t, !ok2)
}

// ---- stream decoder (mirrors stream.rs `decodes_blocks`) ----------------------

@(private = "file")
Test_Sample :: struct {
	i, u: u16,
	d:    bool,
}

@(private = "file")
block_words :: proc(out: ^[dynamic]u32, seq: u16, rng: u8, log2n: u8, ts: u64, samples: []Test_Sample) {
	append(out, (u32(2) << 29) | (u32(seq) << 11) | (u32(log2n) << 5) | (u32(rng) << 2) | 2)
	append(out, (u32(2) << 29) | (u32(1) << 27) | (u32(ts >> 24) & 0xFF_FFFF))
	append(out, u32(0)) // empty words may fall between header words
	append(out, (u32(2) << 29) | (u32(2) << 27) | (u32(ts) & 0xFF_FFFF))
	for s in samples {
		dbit: u32 = 0
		if s.d {
			dbit = 1
		}
		append(out, (u32(1) << 31) | (dbit << 30) | (u32(s.i) << 14) | u32(s.u))
	}
}

@(test)
decoder_decodes_blocks :: proc(t: ^testing.T) {
	TS :: u64(0x12_3456_789A)
	words := make([dynamic]u32)
	defer delete(words)
	block_words(&words, 10, 3, 1, TS, []Test_Sample{{100, 200, false}, {101, 201, true}})
	block_words(&words, 11, 4, 1, TS + 4, []Test_Sample{{5, 6, false}})

	d: nb.Stream_Decoder
	got: [8]nb.Raw_Sample
	n := 0
	for w in words {
		kind, s := nb.decoder_feed(&d, w)
		if kind == .Sample && n < len(got) {
			got[n] = s
			n += 1
		}
	}
	testing.expect_value(t, n, 3)
	testing.expect_value(t, got[0].ts, TS)
	testing.expect_value(t, got[0].step, u32(2))
	testing.expect_value(t, got[0].current, u16(100))
	testing.expect_value(t, got[0].voltage, u16(200))
	testing.expect_value(t, got[1].ts, TS + 2)
	testing.expect(t, got[1].dirty)
	testing.expect_value(t, got[2].range, u8(4))
	testing.expect_value(t, got[2].current, u16(5))
	testing.expect_value(t, got[2].voltage, u16(6))
	testing.expect_value(t, d.counters.seq_errors, u64(0))
	testing.expect_value(t, d.counters.range_changes, u64(1))
	testing.expect_value(t, d.counters.gap_periods, u64(0))
	testing.expect_value(t, d.counters.malformed, u64(0))
	testing.expect_value(t, d.counters.blocks, u64(2))
	testing.expect_value(t, d.counters.samples, u64(3))
	testing.expect_value(t, d.counters.dirty, u64(1))
	testing.expect_value(t, d.counters.empty, u64(2))
}

@(test)
decoder_counts_errors :: proc(t: ^testing.T) {
	words := make([dynamic]u32)
	defer delete(words)
	append(&words, u32(0x8000_0001)) // a sample before any header: unframed
	block_words(&words, 1, 0, 0, 100, []Test_Sample{{1, 1, false}})
	block_words(&words, 3, 0, 0, 110, []Test_Sample{{1, 1, false}}) // seq jump, 9-period gap
	append(&words, nb.LATE_WORD)
	append(&words, u32(0x1234_5678)) // not a valid word: malformed

	d: nb.Stream_Decoder
	for w in words {
		_, _ = nb.decoder_feed(&d, w)
	}
	testing.expect_value(t, d.counters.unframed, u64(1))
	testing.expect_value(t, d.counters.seq_errors, u64(1))
	testing.expect_value(t, d.counters.gap_periods, u64(9))
	testing.expect_value(t, d.counters.late, u64(1))
	testing.expect_value(t, d.counters.malformed, u64(1))

	// after desync the next block's seq jump isn't counted again
	nb.decoder_desync(&d)
	clear(&words)
	block_words(&words, 40, 0, 0, 500, []Test_Sample{{1, 1, false}})
	for w in words {
		_, _ = nb.decoder_feed(&d, w)
	}
	testing.expect_value(t, d.counters.seq_errors, u64(1))
}

@(test)
calibration_defaults :: proc(t: ^testing.T) {
	cal := nb.default_calibration()
	s := nb.calibrate(nb.Raw_Sample{current = 8192, voltage = 3300, range = 0}, &cal)
	testing.expect(t, abs(s.current - 1.0) < 1e-12)
	testing.expect(t, abs(s.voltage - 3.3) < 1e-12)
	s5 := nb.calibrate(nb.Raw_Sample{current = 16384 / 4, range = 5}, &cal)
	testing.expect(t, abs(s5.current - 5e-6) < 1e-15)
}

// ---- records ----------------------------------------------------------------

@(test)
info_layout :: proc(t: ^testing.T) {
	p: [nb.INFO_LEN + 4]u8 // longer than the record: accepted
	w32(p[:], 0, 1)
	w32(p[:], 4, (1 << 16) | (4 << 8) | 2)
	w32(p[:], 8, 0x4E42000B)
	w32(p[:], 12, 2_000_000)
	w32(p[:], 16, 3)
	w32(p[:], 20, 1460)
	w32(p[:], 24, 507)
	w32(p[:], 28, 0b111111)
	copy(p[32:], "build 1")
	p[64] = 0x02
	p[69] = 0x01
	p[72] = 192
	p[73] = 168
	p[74] = 1
	p[75] = 50
	info, ok := nb.parse_info(p[:])
	testing.expect(t, ok)
	testing.expect_value(t, info.proto_version, u32(1))
	maj, mnr, pat := nb.version_parts(info.fw_version)
	testing.expect_value(t, maj, u32(1))
	testing.expect_value(t, mnr, u32(4))
	testing.expect_value(t, pat, u32(2))
	testing.expect_value(t, info.fpga_id, u32(0x4E42000B))
	testing.expect_value(t, info.base_rate_hz, u32(2_000_000))
	testing.expect_value(t, info.link, nb.Link.TCP)
	testing.expect_value(t, info.block_words, u32(507))
	testing.expect(t, .Eth in info.caps && .Board in info.caps && .Usb_SS in info.caps)
	testing.expect_value(t, nb.info_build(&info), "build 1")
	testing.expect_value(t, info.mac[0], u8(0x02))
	testing.expect_value(t, info.mac[5], u8(0x01))
	testing.expect(t, info.ip == [4]u8{192, 168, 1, 50})
	testing.expect(t, nb.info_is_format5(info))
	_, short := nb.parse_info(p[:nb.INFO_LEN - 1])
	testing.expect(t, !short)
}

@(test)
board_status_layout :: proc(t: ^testing.T) {
	p: [nb.BOARD_STATUS_LEN]u8
	w32(p[:], 0, nb.BOARD_DIGITAL)
	w32(p[:], 16, (1 << 20) | (1 << 6) | (1 << 21)) // APWR_OK, APWR_H, USB_SS
	w16(p[:], 24, 12_050)
	w16(p[:], 28, 11_900)
	p[32] = 0x15
	p[38] = 3
	w32(p[:], 48, 0x4E42000B)
	w32(p[:], 80, 1 | (2 << 2) | (3 << 12)) // HSE pass, LSE fail, SD skip
	w32(p[:], 84, 150_000_000)
	w32(p[:], 84 + 4 * 11, 12_000)
	p[132] = 4
	p[135] = 2
	w16(p[:], 136, 12_000)
	w16(p[:], 140, 15_000)
	p[142] = 2
	w32(p[:], 144, (100 << 10) | 300) // fixed 5 V 3 A
	w32(p[:], 148, (240 << 10) | 225) // fixed 12 V 2.25 A
	w32(p[:], 172, 5)
	w32(p[:], 180, 7)
	b, ok := nb.parse_board_status(p[:])
	testing.expect(t, ok)
	testing.expect_value(t, b.board_id, nb.BOARD_DIGITAL)
	testing.expect(t, .Apwr_Ok in b.flags && .Apwr_H in b.flags && .Usb_SS in b.flags)
	testing.expect(t, .Apwr_L not_in b.flags)
	testing.expect_value(t, b.v_bj_mv, u16(12_050))
	testing.expect_value(t, b.v_main_mv, u16(11_900))
	testing.expect_value(t, b.leds, u8(0x15))
	testing.expect_value(t, b.sd_type, u8(3))
	testing.expect_value(t, b.fpga_id, u32(0x4E42000B))
	testing.expect_value(t, nb.board_test_result(b, .HSE), nb.Test_Result.Pass)
	testing.expect_value(t, nb.board_test_result(b, .LSE), nb.Test_Result.Fail)
	testing.expect_value(t, nb.board_test_result(b, .SD), nb.Test_Result.Skip)
	testing.expect_value(t, nb.board_test_result(b, .RTC), nb.Test_Result.None)
	testing.expect_value(t, b.test_detail[0], u32(150_000_000))
	testing.expect_value(t, b.test_detail[11], u32(12_000))
	testing.expect_value(t, b.pd_state, u8(4))
	testing.expect_value(t, b.pd_npdo, u8(2))
	testing.expect_value(t, b.pd_mv, u16(12_000))
	testing.expect_value(t, b.pd_target_mv, u16(15_000))
	testing.expect_value(t, b.pd_pdo_sel, u8(2))
	testing.expect_value(t, b.pd_contracts, u32(5))
	testing.expect_value(t, b.pd_errors, u32(7))
	pdo0 := nb.board_pdo(b, 0)
	pdo1 := nb.board_pdo(b, 1)
	testing.expect_value(t, pdo0.mv, u32(5000))
	testing.expect_value(t, pdo0.ma, u32(3000))
	testing.expect_value(t, pdo1.mv, u32(12_000))
	testing.expect_value(t, pdo1.ma, u32(2250))
	_, short := nb.parse_board_status(p[:nb.BOARD_STATUS_LEN - 1])
	testing.expect(t, !short)
}

@(test)
update_info_layout :: proc(t: ^testing.T) {
	p: [nb.UPDATE_INFO_LEN]u8
	w32(p[:], 0, 0x0002_0000)
	copy(p[16:], "Nov 3")
	w32(p[:], 40, 0x10000)
	p[44] = 0
	p[45] = 1
	p[47] = 6
	w32(p[:], 48, 448 * 1024)
	p[52] = 1
	w32(p[:], 56, 340_000)
	w32(p[:], 60, 0xCAFEF00D)
	w32(p[:], 64, 0x4E42000B)
	p[68] = 2
	w32(p[:], 84, 3)
	copy(p[88:], "done")
	u, ok := nb.parse_update_info(p[:])
	testing.expect(t, ok)
	testing.expect_value(t, u.fw_version, u32(0x0002_0000))
	testing.expect_value(t, nb.update_info_date(&u), "Nov 3")
	testing.expect_value(t, u.boot_version, u32(0x10000))
	testing.expect_value(t, u.update_slot, u8(1))
	testing.expect_value(t, u.result, nb.Status.HW)
	testing.expect_value(t, u.slot_size, u32(448 * 1024))
	testing.expect_value(t, u.slots[0].kind, nb.Update_Slot_Kind.Gateware)
	testing.expect_value(t, u.slots[0].length, u32(340_000))
	testing.expect_value(t, u.slots[0].crc, u32(0xCAFEF00D))
	testing.expect_value(t, u.slots[0].id, u32(0x4E42000B))
	testing.expect_value(t, u.slots[1].kind, nb.Update_Slot_Kind.Firmware)
	testing.expect_value(t, u.installs, u32(3))
	testing.expect_value(t, nb.update_info_msg(&u), "done")
	_, short := nb.parse_update_info(p[:nb.UPDATE_INFO_LEN - 1])
	testing.expect(t, !short)
}

@(test)
small_records_layout :: proc(t: ^testing.T) {
	p: [nb.STATS_LEN]u8
	for k in 0 ..< 16 {
		w32(p[:], 4 * k, u32(100 + k))
	}
	w32(p[:], 4, 3)
	s, ok := nb.parse_stats(p[:])
	testing.expect(t, ok)
	testing.expect_value(t, s.uptime_ms, u32(100))
	testing.expect_value(t, s.stream_link, nb.Link.TCP)
	testing.expect_value(t, s.blocks, u32(102))
	testing.expect_value(t, s.adc_ctrl, u32(109))
	testing.expect_value(t, s.eth_ip, u32(111))

	r, okr := nb.parse_range_state(p[:16])
	testing.expect(t, okr)
	testing.expect_value(t, r.count, u32(3))
	testing.expect_value(t, r.timing, u32(103))

	dp := [8]u8{4, 1, 0, 0, 0x48, 0xE8, 0x01, 0x00} // 125000
	d, okd := nb.parse_decim(dp[:])
	testing.expect(t, okd)
	testing.expect_value(t, d.log2n, u8(4))
	testing.expect_value(t, d.mode, nb.Decim_Mode.Subsample)
	testing.expect_value(t, d.rate_hz, u32(125_000))
	_, shortd := nb.parse_decim(dp[:7])
	testing.expect(t, !shortd)
}

@(test)
crc_is_zlibs :: proc(t: ^testing.T) {
	s := "123456789"
	testing.expect_value(t, nb.crc32(transmute([]u8)s), u32(0xCBF4_3926))
	testing.expect_value(t, nb.crc32(nil), u32(0))
}

@(test)
errors_carry_status :: proc(t: ^testing.T) {
	err: nb.Error = nb.Status.Bad_Arg
	st, ok := err.(nb.Status)
	testing.expect(t, ok)
	testing.expect_value(t, st, nb.Status.Bad_Arg)
	testing.expect_value(t, nb.error_string(err), "BAD_ARG")
	terr: nb.Error = nb.General_Error.Timeout
	testing.expect(t, nb.is_timeout(terr))
	testing.expect_value(t, nb.error_string(nil), "ok")
}
