package nebulosa_tests

// Integration test against tools/nebulosa_sim.py. Skipped (passes with a
// log line) unless NEBULOSA_SIM_PORT is set:
//
//     python3 tools/nebulosa_sim.py --port 21000 --no-udp &
//     NEBULOSA_SIM_PORT=21000 odin test odin/tests -collection:nebulosa=odin

import "core:log"
import "core:os"
import "core:strconv"
import "core:testing"
import "core:time"
import nb "nebulosa:nebulosa"

@(test)
sim_integration :: proc(t: ^testing.T) {
	port_s := os.get_env("NEBULOSA_SIM_PORT", context.temp_allocator)
	if port_s == "" {
		log.info("NEBULOSA_SIM_PORT not set: integration test skipped")
		return
	}
	port, port_ok := strconv.parse_int(port_s)
	if !testing.expectf(t, port_ok, "bad NEBULOSA_SIM_PORT %q", port_s) {
		return
	}

	dev, err := nb.connect_tcp("127.0.0.1", port)
	if !testing.expectf(t, err == nil, "connect: %v", err) {
		return
	}
	defer nb.close(dev)

	// GET_INFO (done by connect)
	testing.expect_value(t, dev.info.proto_version, u32(1))
	testing.expect_value(t, dev.info.fpga_id, nb.FPGA_ID_FORMAT5)
	testing.expect_value(t, dev.info.link, nb.Link.TCP)
	testing.expect_value(t, dev.info.block_words, u32(nb.BLOCK_WORDS))

	// ping
	msg := [5]u8{1, 2, 3, 4, 5}
	echo, perr := nb.ping(dev, msg[:])
	testing.expect(t, perr == nil)
	testing.expect_value(t, len(echo), 5)
	if len(echo) == 5 {
		testing.expect_value(t, echo[4], u8(5))
	}
	delete(echo)

	// registers
	id, rerr := nb.reg_read(dev, 0)
	testing.expect(t, rerr == nil)
	testing.expect_value(t, id, nb.FPGA_ID_FORMAT5)
	testing.expect(t, nb.reg_write(dev, 10, 0xABCD) == nil)
	v10, _ := nb.reg_read(dev, 10)
	testing.expect_value(t, v10, u32(0xABCD))

	// a status error carries its code
	serr := nb.set_source(dev, nb.Source(7))
	st, is_status := serr.(nb.Status)
	testing.expect(t, is_status)
	testing.expect_value(t, st, nb.Status.Bad_Arg)
	testing.expect(t, nb.set_source(dev, .AD7387) == nil)

	// range, decimation
	testing.expect(t, nb.set_range_auto(dev) == nil)
	_, gerr := nb.get_range(dev)
	testing.expect(t, gerr == nil)
	testing.expect(t, nb.set_range_timing(dev, 4, 4, 2, 100) == nil)
	testing.expect(t, nb.set_range_force(dev, false, 0) == nil)
	d, derr := nb.set_decimation(dev, 4)
	testing.expect(t, derr == nil)
	testing.expect_value(t, d.log2n, u8(4))
	testing.expect_value(t, d.rate_hz, u32(125_000))

	// board
	bs, berr := nb.board_status(dev)
	testing.expect(t, berr == nil)
	testing.expect_value(t, bs.board_id, nb.BOARD_DIGITAL)
	bl, lerr := nb.set_leds(dev, 0x05)
	testing.expect(t, lerr == nil)
	testing.expect_value(t, bl.leds, u8(0x05))
	testing.expect_value(t, bl.led_manual, u8(1))
	_, _ = nb.leds_auto(dev)
	bt, terr := nb.board_test(dev)
	testing.expect(t, terr == nil)
	testing.expect_value(t, nb.board_test_result(bt, .HSE), nb.Test_Result.Pass)

	// flash: upload into the update slot, then UPDATE checks its CRC
	ui, uerr := nb.update_info(dev)
	testing.expect(t, uerr == nil)
	img := make([]u8, 1000)
	defer delete(img)
	for k in 0 ..< len(img) {
		img[k] = u8(k * 7 + 3)
	}
	slot_addr := u32(ui.update_slot) * ui.slot_size
	ferr := nb.upload_flash(dev, slot_addr, img)
	testing.expectf(t, ferr == nil, "upload_flash: %v", ferr)
	back, rberr := nb.flash_read(dev, slot_addr + 250, 10)
	testing.expect(t, rberr == nil)
	if len(back) == 10 {
		testing.expect_value(t, back[0], img[250])
	}
	delete(back)
	ur, urerr := nb.update(dev, .Gateware, ui.update_slot, u32(len(img)), nb.crc32(img))
	testing.expect(t, urerr == nil)
	testing.expect_value(t, ur.result, nb.Status.OK)
	// a write crossing a 256-byte page is refused locally
	werr := nb.flash_write(dev, 0x10F0, img[:32])
	wge, _ := werr.(nb.General_Error)
	testing.expect_value(t, wge, nb.General_Error.Invalid_Argument)

	// streaming
	testing.expect(t, nb.start_stream(dev) == nil)
	buf := make([]nb.Sample, 1024)
	defer delete(buf)
	total := 0
	have_prev := false
	prev: nb.Sample
	ts_ok := true
	asked_stats := false
	start := time.tick_now()
	for total < 20_000 && time.tick_since(start) < 10 * time.Second {
		n, rerr2 := nb.read_samples(dev, buf, 500 * time.Millisecond)
		if !testing.expectf(t, rerr2 == nil, "read_samples: %v", rerr2) {
			break
		}
		for s in buf[:n] {
			if have_prev && s.ts != prev.ts + u64(prev.step) {
				ts_ok = false
			}
			if s.current < 0 || s.current > 2.0 || s.voltage < 2.5 || s.voltage > 4.0 {
				ts_ok = false
			}
			prev = s
			have_prev = true
		}
		total += n
		if total > 5000 && !asked_stats {
			// a request mid-stream: its DATA must be queued, not dropped
			stats, sterr := nb.get_stats(dev)
			testing.expect(t, sterr == nil)
			testing.expect_value(t, stats.stream_link, nb.Link.TCP)
			asked_stats = true
		}
	}
	testing.expectf(t, total >= 20_000, "only %d samples", total)
	testing.expect(t, ts_ok)
	c := nb.stream_counters(dev)
	testing.expect_value(t, c.lost_blocks, u64(0))
	testing.expect_value(t, c.malformed, u64(0))
	testing.expect_value(t, c.seq_errors, u64(0))
	testing.expect_value(t, c.gap_periods, u64(0))
	testing.expect_value(t, c.resyncs, u64(0))
	testing.expect(t, c.data_blocks > 0 && c.blocks > 0)
	testing.expect(t, nb.stop_stream(dev) == nil)

	// a request after the stream still works
	_, ierr := nb.get_info(dev)
	testing.expect(t, ierr == nil)
}
