//! Message framing (nbp.h `nbp_hdr_t`): the 8-byte header, and a parser that
//! cuts a byte stream into messages, resyncing on the magic after garbage.

use crate::le16;

/// Header magic, bytes `'N' 'B'`.
pub const MAGIC: u16 = 0x424E;
/// Header length in bytes.
pub const HDR_LEN: usize = 8;
/// Largest command / response payload.
pub const MAX_PAYLOAD: usize = 504;
/// A whole DATA message, header included.
pub const BLOCK_BYTES: usize = 2048;
/// Largest `len` the parser accepts (a DATA payload).
pub const MAX_LEN: usize = BLOCK_BYTES - HDR_LEN;

/// One message: header fields and payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Command code, response code (`cmd | 0x40`) or `msg::DATA`.
    pub ty: u8,
    /// Responses: `msg::status::*`; 0 otherwise.
    pub status: u8,
    /// Commands / responses: the request sequence number; DATA: low 16
    /// bits of the block sequence.
    pub seq: u16,
    /// Payload bytes.
    pub payload: Vec<u8>,
}

/// Encodes one message (header + payload) with status 0.
///
/// # Panics
/// If `payload` is longer than [`MAX_PAYLOAD`].
pub fn encode(ty: u8, seq: u16, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= MAX_PAYLOAD, "NBP payload too long");
    let mut v = Vec::with_capacity(HDR_LEN + payload.len());
    v.extend_from_slice(&MAGIC.to_le_bytes());
    v.push(ty);
    v.push(0);
    v.extend_from_slice(&seq.to_le_bytes());
    v.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    v.extend_from_slice(payload);
    v
}

/// Cuts one direction's byte stream into messages. Parse by `len`;
/// transfer boundaries mean nothing. Use one parser per byte stream (per
/// USB IN endpoint).
#[derive(Debug, Default)]
pub struct FrameParser {
    buf: Vec<u8>,
    pos: usize,
    /// Times bytes were discarded to find the next magic.
    pub resyncs: u64,
}

impl FrameParser {
    /// An empty parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends received bytes.
    pub fn push(&mut self, data: &[u8]) {
        if self.pos > 0 && self.pos * 2 >= self.buf.len() {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        self.buf.extend_from_slice(data);
    }

    /// Drops everything buffered.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.pos = 0;
    }

    /// Bytes buffered and not yet returned as a frame.
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// The next complete message, if one is buffered.
    pub fn next_frame(&mut self) -> Option<Frame> {
        loop {
            let b = &self.buf[self.pos..];
            if b.len() < 2 {
                return None;
            }
            if b[0] != b'N' || b[1] != b'B' {
                self.resyncs += 1;
                // skip to the next "NB", or keep a trailing 'N'
                match b[1..].windows(2).position(|w| w == b"NB") {
                    Some(i) => self.pos += 1 + i,
                    None => self.pos += if b[b.len() - 1] == b'N' { b.len() - 1 } else { b.len() },
                }
                continue;
            }
            if b.len() < HDR_LEN {
                return None;
            }
            let len = le16(b, 6) as usize;
            if len > MAX_LEN {
                self.resyncs += 1;
                self.pos += 1;
                continue;
            }
            if b.len() < HDR_LEN + len {
                return None;
            }
            let f = Frame { ty: b[2], status: b[3], seq: le16(b, 4), payload: b[HDR_LEN..HDR_LEN + len].to_vec() };
            self.pos += HDR_LEN + len;
            return Some(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(p: &mut FrameParser) -> Vec<Frame> {
        std::iter::from_fn(|| p.next_frame()).collect()
    }

    #[test]
    fn encode_layout() {
        assert_eq!(encode(0x02, 0x0102, &[]), vec![0x4E, 0x42, 0x02, 0, 0x02, 0x01, 0, 0]);
        assert_eq!(encode(0x01, 0, b"ab")[6..], [2, 0, b'a', b'b']);
    }

    #[test]
    fn roundtrip_split_and_garbage() {
        let a = encode(0x41, 7, b"hello");
        let b = encode(0x42, 8, &[]);
        let mut stream = vec![0xFF, b'N', 0x00];
        stream.extend_from_slice(&a);
        stream.extend_from_slice(&b);
        let mut p = FrameParser::new();
        let mut out = vec![];
        for chunk in stream.chunks(3) {
            p.push(chunk);
            out.extend(all(&mut p));
        }
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].seq, 7);
        assert_eq!(out[0].payload, b"hello");
        assert_eq!(out[1].ty, 0x42);
        assert!(p.resyncs > 0);
    }

    #[test]
    fn byte_by_byte_and_trailing_n() {
        let a = encode(0x43, 1, &[1, 2, 3]);
        let mut p = FrameParser::new();
        // garbage ending in 'N', then the frame: the trailing 'N' must not
        // swallow the real one
        p.push(b"xyzN");
        assert!(p.next_frame().is_none());
        for &x in &a {
            p.push(&[x]);
        }
        let f = all(&mut p);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].payload, [1, 2, 3]);
    }

    #[test]
    fn oversized_len_resyncs() {
        let mut bad = vec![b'N', b'B', 0x41, 0, 0, 0];
        bad.extend_from_slice(&(2041u16).to_le_bytes());
        bad.extend_from_slice(&encode(0x42, 9, b"ok"));
        let mut p = FrameParser::new();
        p.push(&bad);
        let f = all(&mut p);
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].ty, f[0].seq), (0x42, 9));
        assert!(p.resyncs >= 1);
    }

    #[test]
    fn data_frame_max_len() {
        let mut d = vec![b'N', b'B', 0x80, 0, 5, 0];
        d.extend_from_slice(&(2040u16).to_le_bytes());
        d.extend(std::iter::repeat(0xAB).take(2040));
        let mut p = FrameParser::new();
        p.push(&d[..1000]);
        assert!(p.next_frame().is_none());
        p.push(&d[1000..]);
        let f = p.next_frame().unwrap();
        assert_eq!((f.ty, f.payload.len(), p.resyncs), (0x80, 2040, 0));
        assert_eq!(p.buffered(), 0);
    }
}
