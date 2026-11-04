package nebulosa

// The commands of docs/API.md section 4, one procedure each.

// Echoes `data` (any payload up to MAX_PAYLOAD). The echo is allocated
// with `allocator`.
ping :: proc(dev: ^Device, data: []u8, allocator := context.allocator) -> (echo: []u8, err: Error) {
	resp: []u8
	resp, err = request(dev, .Ping, data)
	if err != nil {
		return nil, err
	}
	echo = make([]u8, len(resp), allocator)
	copy(echo, resp)
	return echo, nil
}

// GET_INFO. Also refreshes Device.info.
get_info :: proc(dev: ^Device) -> (info: Info, err: Error) {
	resp: []u8
	resp, err = request(dev, .Get_Info)
	if err != nil {
		return
	}
	ok: bool
	info, ok = parse_info(resp)
	if !ok {
		return {}, General_Error.Short_Payload
	}
	dev.info = info
	return info, nil
}

// An FPGA register.
reg_read :: proc(dev: ^Device, index: u8) -> (value: u32, err: Error) {
	p: [8]u8
	p[0] = index
	resp: []u8
	resp, err = request(dev, .Reg_Read, p[:])
	if err != nil {
		return
	}
	if len(resp) < 8 {
		return 0, General_Error.Short_Payload
	}
	return le32(resp, 4), nil
}

reg_write :: proc(dev: ^Device, index: u8, value: u32) -> Error {
	p: [8]u8
	p[0] = index
	put_le32(p[:], 4, value)
	_, err := request(dev, .Reg_Write, p[:])
	return err
}

// The device restarts ~50 ms later: the connection drops. Close `dev`
// and connect again.
reset :: proc(dev: ^Device) -> Error {
	_, err := request(dev, .Reset)
	dev.streaming = false
	return err
}

// The sample source: 0 stub random, 1 stub pattern, 2 AD7387.
set_source :: proc(dev: ^Device, src: Source) -> Error {
	p: [4]u8
	p[0] = u8(src)
	_, err := request(dev, .Set_Source, p[:])
	return err
}

// Automatic ranging between min_range and max_range (0 = 2 A .. 5 = 20 uA).
set_range_auto :: proc(dev: ^Device, min_range: u8 = 0, max_range: u8 = 5) -> Error {
	p := [4]u8{1, 0, min_range, max_range}
	_, err := request(dev, .Set_Range, p[:])
	return err
}

// A fixed range (0 = 2 A .. 5 = 20 uA).
set_range_manual :: proc(dev: ^Device, range: u8) -> Error {
	p := [4]u8{0, range, 0, 5}
	_, err := request(dev, .Set_Range, p[:])
	return err
}

get_range :: proc(dev: ^Device) -> (r: Range_State, err: Error) {
	resp: []u8
	resp, err = request(dev, .Get_Range)
	if err != nil {
		return
	}
	ok: bool
	r, ok = parse_range_state(resp)
	if !ok {
		return {}, General_Error.Short_Payload
	}
	return r, nil
}

// Range switch timing, in FPGA clock cycles (register 41, CLK_HZ).
set_range_timing :: proc(dev: ^Device, up_filt: u8, down_filt: u8, overlap: u8, hold: u16) -> Error {
	p: [8]u8
	p[0] = up_filt
	p[1] = down_filt
	p[2] = overlap
	put_le16(p[:], 4, hold)
	_, err := request(dev, .Set_Range_Timing, p[:])
	return err
}

// Forces the range comparators from `value` ({down_c, up_c, down_b, up_b,
// down_a, up_a}); enable = false gives them back to the hardware.
set_range_force :: proc(dev: ^Device, enable: bool, value: u8) -> Error {
	p: [4]u8
	p[0] = enable ? 1 : 0
	p[1] = value
	_, err := request(dev, .Set_Range_Force, p[:])
	return err
}

// N = 2^log2n (0..8) conversions per sample. Returns what was applied.
set_decimation :: proc(dev: ^Device, log2n: u8, mode: Decim_Mode = .Average) -> (d: Decim, err: Error) {
	p: [8]u8
	p[0] = log2n
	p[1] = u8(mode)
	resp: []u8
	resp, err = request(dev, .Set_Decimation, p[:])
	if err != nil {
		return
	}
	ok: bool
	d, ok = parse_decim(resp)
	if !ok {
		return {}, General_Error.Short_Payload
	}
	return d, nil
}

get_stats :: proc(dev: ^Device) -> (s: Stats, err: Error) {
	resp: []u8
	resp, err = request(dev, .Get_Stats)
	if err != nil {
		return
	}
	ok: bool
	s, ok = parse_stats(resp)
	if !ok {
		return {}, General_Error.Short_Payload
	}
	return s, nil
}

// ---- board ------------------------------------------------------------------

@(private)
board_request :: proc(dev: ^Device, cmd: Cmd, payload: []u8, timeout := DEFAULT_TIMEOUT) -> (b: Board_Status, err: Error) {
	resp: []u8
	resp, err = request(dev, cmd, payload, timeout)
	if err != nil {
		return
	}
	ok: bool
	b, ok = parse_board_status(resp)
	if !ok {
		return {}, General_Error.Short_Payload
	}
	return b, nil
}

board_status :: proc(dev: ^Device) -> (b: Board_Status, err: Error) {
	return board_request(dev, .Board_Status, nil)
}

// One board setting; returns the status after the change.
board_set :: proc(dev: ^Device, item: Board_Set_Item, value: u32) -> (b: Board_Status, err: Error) {
	p: [8]u8
	p[0] = u8(item)
	put_le32(p[:], 4, value)
	return board_request(dev, .Board_Set, p[:])
}

// Runs the board self-tests in `mask` (bits of Board_Test); results in
// test_results / test_detail (board_test_result).
board_test :: proc(dev: ^Device, mask: u32 = TEST_ALL) -> (b: Board_Status, err: Error) {
	p: [4]u8
	put_le32(p[:], 0, mask & TEST_ALL)
	return board_request(dev, .Board_Test, p[:], BOARD_TEST_TIMEOUT)
}

// LEDs 0..4 (bit n = LED n), under manual control.
set_leds :: proc(dev: ^Device, mask: u8) -> (b: Board_Status, err: Error) {
	return board_set(dev, .Leds, u32(mask & 0x1F) | 0x100)
}

// The LEDs back to the firmware's status pattern.
leds_auto :: proc(dev: ^Device) -> (b: Board_Status, err: Error) {
	return board_set(dev, .Leds, 0)
}

// A key for the on-board UI, as if from its button.
ui_key :: proc(dev: ^Device, key: Ui_Key) -> (b: Board_Status, err: Error) {
	return board_set(dev, .Ui_Key, u32(key))
}

// Configures the FPGA from a raw bitstream at `addr` in the device flash.
fpga_config :: proc(dev: ^Device, addr: u32, length: u32) -> (b: Board_Status, err: Error) {
	p: [12]u8
	put_le32(p[:], 0, addr)
	put_le32(p[:], 4, length)
	return board_request(dev, .Fpga_Config, p[:], FPGA_CONFIG_TIMEOUT)
}

// ---- flash ------------------------------------------------------------------

// Reads len(out) <= FLASH_PAGE bytes at `addr` into `out`.
flash_read_into :: proc(dev: ^Device, addr: u32, out: []u8) -> Error {
	if len(out) == 0 || len(out) > FLASH_PAGE {
		return General_Error.Invalid_Argument
	}
	p: [8]u8
	put_le32(p[:], 0, addr)
	put_le32(p[:], 4, u32(len(out)))
	resp, err := request(dev, .Flash_Read, p[:])
	if err != nil {
		return err
	}
	if len(resp) < 8 + len(out) {
		return General_Error.Short_Payload
	}
	copy(out, resp[8:8 + len(out)])
	return nil
}

// Reads `length` <= FLASH_PAGE bytes at `addr`, allocated with `allocator`.
flash_read :: proc(dev: ^Device, addr: u32, length: int, allocator := context.allocator) -> (data: []u8, err: Error) {
	if length <= 0 || length > FLASH_PAGE {
		return nil, General_Error.Invalid_Argument
	}
	data = make([]u8, length, allocator)
	err = flash_read_into(dev, addr, data)
	if err != nil {
		delete(data, allocator)
		return nil, err
	}
	return data, nil
}

// Programs `data` at `addr`, within one 256-byte page (erase first).
flash_write :: proc(dev: ^Device, addr: u32, data: []u8) -> Error {
	if len(data) == 0 || len(data) > FLASH_PAGE || int(addr % FLASH_PAGE) + len(data) > FLASH_PAGE {
		return General_Error.Invalid_Argument
	}
	p: [8 + FLASH_PAGE]u8
	put_le32(p[:], 0, addr)
	put_le32(p[:], 4, u32(len(data)))
	copy(p[8:], data)
	_, err := request(dev, .Flash_Write, p[:8 + len(data)])
	return err
}

// Erases [addr, addr + length), both multiples of FLASH_SECTOR (4 KB).
flash_erase :: proc(dev: ^Device, addr: u32, length: u32) -> Error {
	if addr % FLASH_SECTOR != 0 || length % FLASH_SECTOR != 0 {
		return General_Error.Invalid_Argument
	}
	p: [8]u8
	put_le32(p[:], 0, addr)
	put_le32(p[:], 4, length)
	_, err := request(dev, .Flash_Erase, p[:], FLASH_ERASE_TIMEOUT)
	return err
}

// Writes `data` to the flash at `addr` (a multiple of 4 KB): erases the 4 KB
// sectors it covers, writes 256-byte pages, then (verify) reads it all back.
upload_flash :: proc(dev: ^Device, addr: u32, data: []u8, verify := true) -> Error {
	if addr % FLASH_SECTOR != 0 {
		return General_Error.Invalid_Argument
	}
	if len(data) == 0 {
		return nil
	}
	end := addr + u32(len(data))
	for a := addr; a < end; a += FLASH_SECTOR {
		if err := flash_erase(dev, a, FLASH_SECTOR); err != nil {
			return err
		}
	}
	off := 0
	for off < len(data) {
		a := addr + u32(off)
		chunk := min(FLASH_PAGE - int(a % FLASH_PAGE), len(data) - off)
		if err := flash_write(dev, a, data[off:off + chunk]); err != nil {
			return err
		}
		off += chunk
	}
	if verify {
		buf: [FLASH_PAGE]u8
		off = 0
		for off < len(data) {
			chunk := min(FLASH_PAGE, len(data) - off)
			if err := flash_read_into(dev, addr + u32(off), buf[:chunk]); err != nil {
				return err
			}
			for k in 0 ..< chunk {
				if buf[k] != data[off + k] {
					return General_Error.Verify_Failed
				}
			}
			off += chunk
		}
	}
	return nil
}

// ---- software update ------------------------------------------------------

// NBP_CMD_UPDATE: op on `slot` (image of `length` bytes, zlib CRC-32 `crc`).
// Gateware: tested, then made active (else the old one comes back and the
// status is HW). Firmware: checked, installed by the bootloader at a
// restart ~0.5 s after the response.
update :: proc(dev: ^Device, op: Update_Op, slot: u8 = 0, length: u32 = 0, crc: u32 = 0) -> (u: Update_Info, err: Error) {
	p: [12]u8
	p[0] = u8(op)
	p[1] = slot
	put_le32(p[:], 4, length)
	put_le32(p[:], 8, crc)
	resp: []u8
	resp, err = request(dev, .Update, p[:], op == .Info ? DEFAULT_TIMEOUT : UPDATE_TIMEOUT)
	if err != nil {
		return
	}
	ok: bool
	u, ok = parse_update_info(resp)
	if !ok {
		return {}, General_Error.Short_Payload
	}
	return u, nil
}

// Where the update slot is, what the slots hold, the running firmware.
update_info :: proc(dev: ^Device) -> (u: Update_Info, err: Error) {
	return update(dev, .Info)
}

// ---- helpers ----------------------------------------------------------------

// zlib's CRC-32.
crc32 :: proc(data: []u8) -> u32 {
	c := u32(0xFFFF_FFFF)
	for b in data {
		c ~= u32(b)
		for _ in 0 ..< 8 {
			c = (c >> 1) ~ (0xEDB8_8320 & (0 - (c & 1))) // poly when the low bit is set
		}
	}
	return ~c
}
