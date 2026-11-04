package nebulosa

import "core:time"

// Stream counters: the decoder's plus the link's (docs/API.md section 5).
Stream_Counters :: struct {
	using decoder: Decoder_Counters,
	data_blocks:   u64, // DATA blocks received
	lost_blocks:   u64, // by nbp_block_t.seq jumps
	stall_flags:   u64, // blocks flagged STALL (the device ring was full)
	gap_flags:     u64, // blocks flagged GAP (QSPI words lost)
	test_blocks:   u64, // link test blocks
	resyncs:       u64, // frame parser resyncs (garbage on the link)
}

// One raw DATA block. `words` (host order) points into the device: valid
// until the next read on it.
Data_Block :: struct {
	seq:   u32,
	t_us:  u32,
	flags: Block_Flags,
	words: []u32,
}

@(private)
total_resyncs :: proc(dev: ^Device) -> u64 {
	n: u64
	for i in 0 ..< MAX_IN_ENDPOINTS {
		n += dev.parsers[i].resyncs
	}
	return n
}

// Forgets everything an earlier stream left: queued and buffered DATA,
// the current block, the decoder and the counters.
@(private)
stream_reset :: proc(dev: ^Device) {
	for qb in dev.queue[dev.queue_head:] {
		append(&dev.free_blocks, qb.buf)
	}
	clear(&dev.queue)
	dev.queue_head = 0
	for i in 0 ..< dev.n_parsers {
		for {
			_, ok := parser_next(&dev.parsers[i])
			if !ok {
				break
			}
		}
	}
	dev.blk_nwords = 0
	dev.blk_pos = 0
	dev.blk_decode = false
	decoder_reset(&dev.decoder)
	dev.counters = {}
	dev.have_expected = false
	dev.resync_base = total_resyncs(dev)
}

// Per-block accounting (docs/API.md section 5) and the words to host order.
@(private)
load_block :: proc(dev: ^Device, p: []u8) {
	dev.blk_pos = 0
	dev.blk_nwords = 0
	dev.blk_decode = false
	if len(p) < BLOCK_HDR_LEN {
		return
	}
	seq := le32(p, 0)
	flags := transmute(Block_Flags)le16(p, 10)
	nwords := min(int(le16(p, 8)), (len(p) - BLOCK_HDR_LEN) / 4, BLOCK_WORDS)

	// 1. a new stream: reset the decoder and counters
	if .First in flags {
		decoder_reset(&dev.decoder)
		dev.counters = {}
		dev.have_expected = false
		dev.resync_base = total_resyncs(dev)
	}
	dev.counters.data_blocks += 1
	// 2. blocks lost on the way
	if dev.have_expected && seq != dev.expected_seq {
		dev.counters.lost_blocks += u64(seq - dev.expected_seq) // u32 wrapping
		decoder_desync(&dev.decoder)
	}
	dev.expected_seq = seq + 1
	dev.have_expected = true

	dev.blk_seq = seq
	dev.blk_t_us = le32(p, 4)
	dev.blk_flags = flags
	for k in 0 ..< nwords {
		dev.words[k] = be32(p, BLOCK_HDR_LEN + 4 * k)
	}
	dev.blk_nwords = nwords

	// 3. a link test block: counted, its words not decoded
	if .Test in flags {
		dev.counters.test_blocks += 1
		return
	}
	// 4. words lost before this block
	if .Stall in flags {
		dev.counters.stall_flags += 1
		decoder_desync(&dev.decoder)
	}
	if .Gap in flags {
		dev.counters.gap_flags += 1
		decoder_desync(&dev.decoder)
	}
	// 5. feed (read_samples)
	dev.blk_decode = true
}

// The next DATA block already received (the queue, then the parser
// buffers) loaded; false when none is buffered. Other frames found here
// (stale responses) are discarded.
@(private)
take_block :: proc(dev: ^Device) -> bool {
	if dev.queue_head < len(dev.queue) {
		qb := dev.queue[dev.queue_head]
		dev.queue_head += 1
		load_block(dev, qb.buf[:qb.n])
		append(&dev.free_blocks, qb.buf)
		if dev.queue_head == len(dev.queue) {
			clear(&dev.queue)
			dev.queue_head = 0
		}
		return true
	}
	for i in 0 ..< dev.n_parsers {
		for {
			f, ok := parser_next(&dev.parsers[i])
			if !ok {
				break
			}
			if f.ty == DATA_TYPE {
				load_block(dev, f.payload)
				return true
			}
		}
	}
	return false
}

// Starts the sample stream on this link: DATA follows. flush: discard what
// the FPGA FIFO holds first. link_test: no FPGA, link test blocks as fast
// as the link takes them (read them with read_block).
start_stream :: proc(dev: ^Device, flush := true, link_test := false) -> Error {
	flags: u32
	if flush {
		flags |= START_FLUSH
	}
	if link_test {
		flags |= START_LINK_TEST
	}
	stream_reset(dev)
	p: [4]u8
	put_le32(p[:], 0, flags)
	_, err := request(dev, .Stream_Start, p[:])
	if err == nil {
		dev.streaming = true
	}
	return err
}

stop_stream :: proc(dev: ^Device) -> Error {
	_, err := request(dev, .Stream_Stop)
	if err == nil {
		dev.streaming = false
	}
	return err
}

// Decoded samples into `buf`: whatever is available, from the queued DATA
// first, then the link, waiting up to `timeout` for the first one. n = 0
// with err = nil: nothing arrived in time. Format 5 FPGAs only
// (.Unsupported_Format otherwise: use read_block).
read_samples :: proc(dev: ^Device, buf: []Sample, timeout: time.Duration = DEFAULT_TIMEOUT) -> (n: int, err: Error) {
	if !info_is_format5(dev.info) {
		return 0, General_Error.Unsupported_Format
	}
	start := time.tick_now()
	for n < len(buf) {
		if dev.blk_decode && dev.blk_pos < dev.blk_nwords {
			w := dev.words[dev.blk_pos]
			dev.blk_pos += 1
			kind, s := decoder_feed(&dev.decoder, w)
			if kind == .Sample {
				buf[n] = calibrate(s, &dev.calibration)
				n += 1
			}
			continue
		}
		if take_block(dev) {
			continue
		}
		if n > 0 {
			return n, nil
		}
		remaining := timeout - time.tick_since(start)
		if remaining <= 0 {
			return 0, nil
		}
		err = pump(dev, remaining)
		if err != nil {
			return n, err
		}
	}
	return n, nil
}

// The next DATA block, undecoded (format 4 FPGAs, link tests, your own
// decoder). The per-block accounting still runs; .Timeout when none came.
// Don't mix with read_samples on one stream.
read_block :: proc(dev: ^Device, timeout: time.Duration = DEFAULT_TIMEOUT) -> (blk: Data_Block, err: Error) {
	start := time.tick_now()
	for {
		if take_block(dev) {
			dev.blk_pos = dev.blk_nwords // consumed raw
			dev.blk_decode = false
			return Data_Block {
					seq = dev.blk_seq,
					t_us = dev.blk_t_us,
					flags = dev.blk_flags,
					words = dev.words[:dev.blk_nwords],
				},
				nil
		}
		remaining := timeout - time.tick_since(start)
		if remaining <= 0 {
			return {}, General_Error.Timeout
		}
		err = pump(dev, remaining)
		if err != nil {
			return {}, err
		}
	}
	return {}, General_Error.Timeout // not reached
}

// The decoder counters plus the link's, since the stream started.
stream_counters :: proc(dev: ^Device) -> Stream_Counters {
	c := dev.counters
	c.decoder = dev.decoder.counters
	c.resyncs = total_resyncs(dev) - dev.resync_base
	return c
}
