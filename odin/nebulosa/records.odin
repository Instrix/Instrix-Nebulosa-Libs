package nebulosa

// Response records (docs/API.md section 4). Each parse_* proc reads the
// fields at their documented offsets, little-endian. A payload longer than
// the record is accepted (newer firmware may append); a shorter one gives
// ok = false.

INFO_LEN :: 76
STATS_LEN :: 64
RANGE_STATE_LEN :: 16
DECIM_LEN :: 8
BOARD_STATUS_LEN :: 200
UPDATE_INFO_LEN :: 136

// nbp_info_t
Info :: struct {
	proto_version: u32,
	fw_version:    u32, // major << 16 | minor << 8 | patch
	fpga_id:       u32,
	base_rate_hz:  u32,
	link:          Link, // the link this response came on
	link_unit:     u32,
	block_words:   u32,
	caps:          Caps,
	build:         [32]u8, // NUL-terminated: info_build()
	mac:           [6]u8, // all zero without Ethernet
	ip:            [4]u8, // a.b.c.d = ip[0..3]; all zero without one
}

parse_info :: proc(p: []u8) -> (info: Info, ok: bool) {
	if len(p) < INFO_LEN {
		return
	}
	info.proto_version = le32(p, 0)
	info.fw_version = le32(p, 4)
	info.fpga_id = le32(p, 8)
	info.base_rate_hz = le32(p, 12)
	info.link = Link(le32(p, 16))
	info.link_unit = le32(p, 20)
	info.block_words = le32(p, 24)
	info.caps = transmute(Caps)le32(p, 28)
	copy(info.build[:], p[32:64])
	copy(info.mac[:], p[64:70])
	copy(info.ip[:], p[72:76])
	return info, true
}

// The build string (points into `info`).
info_build :: proc(info: ^Info) -> string {
	return cstring_view(info.build[:])
}

// fw_version split into major, minor, patch.
version_parts :: proc(v: u32) -> (major, minor, patch: u32) {
	return v >> 16, (v >> 8) & 0xFF, v & 0xFF
}

// Stream format 5 (decoded by read_samples)?
info_is_format5 :: proc(info: Info) -> bool {
	return info.fpga_id >= FPGA_ID_FORMAT5
}

// nbp_stats_t
Stats :: struct {
	uptime_ms:    u32,
	stream_link:  Link, // None when stopped
	blocks:       u32,
	words:        u32,
	stalls:       u32,
	fpga_drops:   u32,
	fifo_level:   u32,
	range_status: u32,
	decim:        u32,
	adc_ctrl:     u32,
	eth:          Eth_Flags,
	eth_ip:       u32, // network byte order as read little-endian: a = eth_ip & 0xFF
	reserved:     [4]u32,
}

parse_stats :: proc(p: []u8) -> (s: Stats, ok: bool) {
	if len(p) < STATS_LEN {
		return
	}
	s.uptime_ms = le32(p, 0)
	s.stream_link = Link(le32(p, 4))
	s.blocks = le32(p, 8)
	s.words = le32(p, 12)
	s.stalls = le32(p, 16)
	s.fpga_drops = le32(p, 20)
	s.fifo_level = le32(p, 24)
	s.range_status = le32(p, 28)
	s.decim = le32(p, 32)
	s.adc_ctrl = le32(p, 36)
	s.eth = transmute(Eth_Flags)le32(p, 40)
	s.eth_ip = le32(p, 44)
	for k in 0 ..< 4 {
		s.reserved[k] = le32(p, 48 + 4 * k)
	}
	return s, true
}

// nbp_range_t
Range_State :: struct {
	status: u32, // FPGA RANGE_STATUS
	count:  u32, // FPGA RANGE_COUNT
	ctrl:   u32, // FPGA RANGE_CTRL
	timing: u32, // FPGA RANGE_TIMING
}

parse_range_state :: proc(p: []u8) -> (r: Range_State, ok: bool) {
	if len(p) < RANGE_STATE_LEN {
		return
	}
	r.status = le32(p, 0)
	r.count = le32(p, 4)
	r.ctrl = le32(p, 8)
	r.timing = le32(p, 12)
	return r, true
}

// nbp_decim_t, as applied
Decim :: struct {
	log2n:   u8,
	mode:    Decim_Mode,
	rate_hz: u32, // conversions/s after decimation
}

parse_decim :: proc(p: []u8) -> (d: Decim, ok: bool) {
	if len(p) < DECIM_LEN {
		return
	}
	d.log2n = p[0]
	d.mode = Decim_Mode(p[1])
	d.rate_hz = le32(p, 4)
	return d, true
}

// nbp_board_status_t
Board_Status :: struct {
	board_id:       u32,
	uptime_ms:      u32,
	core_hz:        u32,
	hclk_hz:        u32,
	flags:          Board_Flags,
	rtc_s:          u32,
	v_bj_mv:        u16,
	v_vbus_mv:      u16,
	v_main_mv:      u16,
	leds:           u8,
	led_manual:     u8,
	pwr_sel:        u8, // 0 auto, 1 barrel jack only, 2 USB only
	cc:             u8, // CC1 bits 1:0, CC2 bits 3:2
	fpga_mode:      u8, // 0 unknown, 1 master, 2 slave
	lcd_mode:       u8, // 0 off, 1 status UI, 2 pattern
	sd_type:        u8,
	fpga_cfg_ms:    u32,
	fpga_cfg_bytes: u32,
	fpga_id:        u32,
	flash_jedec:    u32,
	flash_bytes:    u32,
	sd_blocks:      u32,
	sd_cid:         [4]u32,
	test_results:   u32, // 2 bits per Board_Test: board_test_result()
	test_detail:    [TEST_COUNT]u32,
	pd_state:       u8,
	pd_cc_line:     u8,
	pd_spec_rev:    u8,
	pd_npdo:        u8,
	pd_mv:          u16,
	pd_ma:          u16,
	pd_target_mv:   u16,
	pd_pdo_sel:     u8,
	pd_pdo:         [7]u32,
	pd_contracts:   u32,
	pd_hard_resets: u32,
	pd_errors:      u32,
}

parse_board_status :: proc(p: []u8) -> (b: Board_Status, ok: bool) {
	if len(p) < BOARD_STATUS_LEN {
		return
	}
	b.board_id = le32(p, 0)
	b.uptime_ms = le32(p, 4)
	b.core_hz = le32(p, 8)
	b.hclk_hz = le32(p, 12)
	b.flags = transmute(Board_Flags)le32(p, 16)
	b.rtc_s = le32(p, 20)
	b.v_bj_mv = le16(p, 24)
	b.v_vbus_mv = le16(p, 26)
	b.v_main_mv = le16(p, 28)
	b.leds = p[32]
	b.led_manual = p[33]
	b.pwr_sel = p[34]
	b.cc = p[35]
	b.fpga_mode = p[36]
	b.lcd_mode = p[37]
	b.sd_type = p[38]
	b.fpga_cfg_ms = le32(p, 40)
	b.fpga_cfg_bytes = le32(p, 44)
	b.fpga_id = le32(p, 48)
	b.flash_jedec = le32(p, 52)
	b.flash_bytes = le32(p, 56)
	b.sd_blocks = le32(p, 60)
	for k in 0 ..< 4 {
		b.sd_cid[k] = le32(p, 64 + 4 * k)
	}
	b.test_results = le32(p, 80)
	for k in 0 ..< TEST_COUNT {
		b.test_detail[k] = le32(p, 84 + 4 * k)
	}
	b.pd_state = p[132]
	b.pd_cc_line = p[133]
	b.pd_spec_rev = p[134]
	b.pd_npdo = p[135]
	b.pd_mv = le16(p, 136)
	b.pd_ma = le16(p, 138)
	b.pd_target_mv = le16(p, 140)
	b.pd_pdo_sel = p[142]
	for k in 0 ..< 7 {
		b.pd_pdo[k] = le32(p, 144 + 4 * k)
	}
	b.pd_contracts = le32(p, 172)
	b.pd_hard_resets = le32(p, 176)
	b.pd_errors = le32(p, 180)
	return b, true
}

// The last result of board test `t`.
board_test_result :: proc(b: Board_Status, t: Board_Test) -> Test_Result {
	return Test_Result(u8((b.test_results >> (2 * u32(t))) & 3))
}

// A USB PD source capability; mv = ma = 0 for non-fixed PDOs.
Pdo :: struct {
	mv:    u32,
	ma:    u32,
	fixed: bool,
}

// PDO k (0 .. pd_npdo-1, at most 6) decoded.
board_pdo :: proc(b: Board_Status, k: int) -> Pdo {
	if k < 0 || k >= 7 {
		return {}
	}
	w := b.pd_pdo[k]
	if w >> 30 != 0 {
		return Pdo{fixed = false}
	}
	return Pdo{mv = ((w >> 10) & 0x3FF) * 50, ma = (w & 0x3FF) * 10, fixed = true}
}

// One update slot of nbp_update_info_t.
Update_Slot :: struct {
	kind:   Update_Slot_Kind,
	length: u32,
	crc:    u32,
	id:     u32, // gateware: the IP core's ID; firmware: its version
}

// nbp_update_info_t
Update_Info :: struct {
	fw_version:   u32,
	fw_build_id:  u32,
	fw_len:       u32,
	fw_crc:       u32,
	fw_date:      [24]u8, // update_info_date()
	boot_version: u32,
	active:       u8,
	update_slot:  u8,
	fw_pending:   u8,
	result:       Status, // of the operation
	slot_size:    u32,
	slots:        [2]Update_Slot,
	installs:     u32,
	msg:          [48]u8, // update_info_msg()
}

parse_update_info :: proc(p: []u8) -> (u: Update_Info, ok: bool) {
	if len(p) < UPDATE_INFO_LEN {
		return
	}
	u.fw_version = le32(p, 0)
	u.fw_build_id = le32(p, 4)
	u.fw_len = le32(p, 8)
	u.fw_crc = le32(p, 12)
	copy(u.fw_date[:], p[16:40])
	u.boot_version = le32(p, 40)
	u.active = p[44]
	u.update_slot = p[45]
	u.fw_pending = p[46]
	u.result = Status(p[47])
	u.slot_size = le32(p, 48)
	for k in 0 ..< 2 {
		o := 52 + 16 * k
		u.slots[k] = Update_Slot {
			kind   = Update_Slot_Kind(p[o]),
			length = le32(p, o + 4),
			crc    = le32(p, o + 8),
			id     = le32(p, o + 12),
		}
	}
	u.installs = le32(p, 84)
	copy(u.msg[:], p[88:136])
	return u, true
}

update_info_date :: proc(u: ^Update_Info) -> string {
	return cstring_view(u.fw_date[:])
}

update_info_msg :: proc(u: ^Update_Info) -> string {
	return cstring_view(u.msg[:])
}
