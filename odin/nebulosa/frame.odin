package nebulosa

import "core:encoding/endian"

// ---- byte helpers (explicit little / big endian, no struct casting) -------
// Callers check the length first; a short slice reads as 0.

@(private)
le16 :: proc(b: []u8, off: int) -> u16 {
	v, _ := endian.get_u16(b[off:], .Little)
	return v
}

@(private)
le32 :: proc(b: []u8, off: int) -> u32 {
	v, _ := endian.get_u32(b[off:], .Little)
	return v
}

@(private)
be32 :: proc(b: []u8, off: int) -> u32 {
	v, _ := endian.get_u32(b[off:], .Big)
	return v
}

@(private)
put_le16 :: proc(b: []u8, off: int, v: u16) {
	_ = endian.put_u16(b[off:], .Little, v)
}

@(private)
put_le32 :: proc(b: []u8, off: int, v: u32) {
	_ = endian.put_u32(b[off:], .Little, v)
}

// The bytes up to the first NUL (all of `b` without one), as a string
// that points into `b`.
cstring_view :: proc(b: []u8) -> string {
	for c, i in b {
		if c == 0 {
			return string(b[:i])
		}
	}
	return string(b)
}

// ---- frames -----------------------------------------------------------------

// One message. `payload` points into the parser's buffer: it is valid until
// the next `parser_push` on that parser (copy it to keep it).
Frame :: struct {
	ty:      u8, // command code, response (cmd | 0x40) or DATA_TYPE
	status:  u8,
	seq:     u16,
	payload: []u8,
}

// Writes header + payload into `buf`. Returns the bytes written; ok = false
// when the payload is above MAX_FRAME_LEN or `buf` is too small.
frame_encode :: proc(buf: []u8, ty: u8, seq: u16, payload: []u8, status: u8 = 0) -> (n: int, ok: bool) {
	if len(payload) > MAX_FRAME_LEN || len(buf) < HDR_LEN + len(payload) {
		return 0, false
	}
	put_le16(buf, 0, MAGIC)
	buf[2] = ty
	buf[3] = status
	put_le16(buf, 4, seq)
	put_le16(buf, 6, u16(len(payload)))
	copy(buf[HDR_LEN:], payload)
	return HDR_LEN + len(payload), true
}

// Cuts one direction's byte stream into messages. Parse by `len`; transfer
// boundaries mean nothing. Resyncs on the magic when it finds garbage.
Frame_Parser :: struct {
	buf:     [dynamic]u8,
	pos:     int,
	resyncs: u64, // times bytes were discarded to find the next magic
}

parser_init :: proc(p: ^Frame_Parser, allocator := context.allocator) {
	p.buf = make([dynamic]u8, 0, 4 * BLOCK_BYTES, allocator)
	p.pos = 0
	p.resyncs = 0
}

parser_destroy :: proc(p: ^Frame_Parser) {
	delete(p.buf)
	p.buf = nil
	p.pos = 0
}

parser_clear :: proc(p: ^Frame_Parser) {
	clear(&p.buf)
	p.pos = 0
}

// Adds received bytes. Invalidates the payloads of frames returned before.
parser_push :: proc(p: ^Frame_Parser, data: []u8) {
	if p.pos > 0 && p.pos * 2 >= len(p.buf) {
		rest := len(p.buf) - p.pos
		if rest > 0 {
			copy(p.buf[:rest], p.buf[p.pos:]) // memmove semantics
		}
		resize(&p.buf, rest)
		p.pos = 0
	}
	append(&p.buf, ..data)
}

@(private)
find_nb :: proc(b: []u8) -> int {
	for i := 0; i + 1 < len(b); i += 1 {
		if b[i] == 'N' && b[i + 1] == 'B' {
			return i
		}
	}
	return -1
}

// The next complete message, if one is buffered.
parser_next :: proc(p: ^Frame_Parser) -> (f: Frame, ok: bool) {
	for {
		b := p.buf[p.pos:]
		if len(b) < 2 {
			return
		}
		if b[0] != 'N' || b[1] != 'B' {
			p.resyncs += 1
			// skip to the next "NB", or keep a trailing 'N'
			i := find_nb(b[1:])
			if i >= 0 {
				p.pos += 1 + i
			} else if b[len(b) - 1] == 'N' {
				p.pos += len(b) - 1
			} else {
				p.pos += len(b)
			}
			continue
		}
		if len(b) < HDR_LEN {
			return
		}
		n := int(le16(b, 6))
		if n > MAX_FRAME_LEN {
			p.resyncs += 1
			p.pos += 1
			continue
		}
		if len(b) < HDR_LEN + n {
			return
		}
		f = Frame {
			ty      = b[2],
			status  = b[3],
			seq     = le16(b, 4),
			payload = b[HDR_LEN:HDR_LEN + n],
		}
		p.pos += HDR_LEN + n
		return f, true
	}
}
