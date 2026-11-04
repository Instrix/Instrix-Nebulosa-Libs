//! Software update helpers: CRC-32, firmware image (`.nbfw`) checks, and
//! gateware file (`.bit` / `.bin`) to raw bitstream. The steps themselves
//! are [`crate::Device::update_gateware`] / [`crate::Device::update_firmware`].

use crate::le32;
use crate::msg::{version_string, BOARD_DIGITAL};

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static CRC_TABLE: [u32; 256] = crc_table();

/// zlib's CRC-32 of `data` (what the device checks images with).
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// `.nbfw` magic, bytes `"NBFW"`.
pub const NBFW_MAGIC: u32 = 0x5746_424E;
/// `.nbfw` header size.
pub const NBFW_HEADER: usize = 64;

/// Checks a firmware image (`.nbfw`: a 64-byte header with its own CRC,
/// then the body). Returns its version (`major << 16 | minor << 8 | patch`)
/// or what is wrong with it.
pub fn firmware_check(img: &[u8]) -> Result<u32, String> {
    if img.len() < NBFW_HEADER || le32(img, 0) != NBFW_MAGIC {
        return Err("not an .nbfw firmware image (no NBFW header)".into());
    }
    if u16::from_le_bytes([img[4], img[5]]) as usize != NBFW_HEADER {
        return Err("unknown header size".into());
    }
    if crc32(&img[..60]) != le32(img, 60) {
        return Err("header CRC wrong".into());
    }
    if le32(img, 16) as usize != img.len() {
        return Err("the length in the header doesn't match the file".into());
    }
    if crc32(&img[NBFW_HEADER..]) != le32(img, 20) {
        return Err("image CRC wrong: the file is damaged".into());
    }
    if le32(img, 56) != BOARD_DIGITAL {
        return Err("an image for another board".into());
    }
    Ok(le32(img, 24))
}

/// [`firmware_check`]'s version as text, for messages.
pub fn firmware_version_string(img: &[u8]) -> Option<String> {
    firmware_check(img).ok().map(version_string)
}

/// The Xilinx sync word.
pub const SYNC_WORD: [u8; 4] = [0xAA, 0x99, 0x55, 0x66];

/// A gateware file to the raw bitstream the FPGA loads: a `.bit`'s `e`
/// field, or a `.bin` as it is. Fails without a sync word (`AA995566`).
pub fn bitstream(d: &[u8]) -> Result<Vec<u8>, String> {
    if !d.windows(4).any(|w| w == SYNC_WORD) {
        return Err("no sync word (0xAA995566): not a bitstream".into());
    }
    // .bit: fields 'a'..'d' (16-bit lengths), then 'e' with a 32-bit length
    if d.len() > 13 && d[0] == 0x00 && d[1] == 0x09 {
        let mut p = 2 + 9 + 2;
        while p < d.len() {
            let key = d[p];
            p += 1;
            if key == b'e' {
                if p + 4 > d.len() {
                    break;
                }
                let n = u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]) as usize;
                p += 4;
                if p + n > d.len() {
                    break;
                }
                return Ok(d[p..p + n].to_vec());
            }
            if p + 2 > d.len() {
                break;
            }
            p += 2 + ((d[p] as usize) << 8 | d[p + 1] as usize);
        }
        return Err("malformed .bit header".into());
    }
    Ok(d.to_vec())
}

/// Builds a valid `.nbfw` image around `body` (for tests and tools).
#[doc(hidden)]
pub fn make_nbfw(body: &[u8], version: u32, board: u32) -> Vec<u8> {
    let mut h = vec![0u8; NBFW_HEADER];
    h[0..4].copy_from_slice(&NBFW_MAGIC.to_le_bytes());
    h[4..6].copy_from_slice(&(NBFW_HEADER as u16).to_le_bytes());
    h[16..20].copy_from_slice(&((NBFW_HEADER + body.len()) as u32).to_le_bytes());
    h[20..24].copy_from_slice(&crc32(body).to_le_bytes());
    h[24..28].copy_from_slice(&version.to_le_bytes());
    h[56..60].copy_from_slice(&board.to_le_bytes());
    let c = crc32(&h[..60]);
    h[60..64].copy_from_slice(&c.to_le_bytes());
    h.extend_from_slice(body);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_is_zlibs() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn firmware_images() {
        let ok = make_nbfw(&[1, 2, 3, 4], 0x0002_0300, BOARD_DIGITAL);
        assert_eq!(firmware_check(&ok), Ok(0x0002_0300));
        assert_eq!(firmware_version_string(&ok).as_deref(), Some("2.3.0"));
        let mut damaged = ok.clone();
        *damaged.last_mut().unwrap() ^= 1;
        assert!(firmware_check(&damaged).unwrap_err().contains("damaged"));
        assert!(firmware_check(&make_nbfw(&[1], 1, 0x1234)).unwrap_err().contains("another board"));
        assert!(firmware_check(b"hello").unwrap_err().contains("NBFW"));
    }

    #[test]
    fn bit_and_bin() {
        let raw = [0xFF, 0xFF, 0xAA, 0x99, 0x55, 0x66, 0x30, 0x01];
        assert_eq!(bitstream(&raw).unwrap(), raw);
        let mut bit = vec![0x00, 0x09, 0x0f, 0xf0, 0x0f, 0xf0, 0x0f, 0xf0, 0x0f, 0xf0, 0x00, 0x00, 0x01];
        bit.extend_from_slice(&[b'a', 0, 1, b'x', b'b', 0, 1, b'p', b'e']);
        bit.extend_from_slice(&(raw.len() as u32).to_be_bytes());
        bit.extend_from_slice(&raw);
        assert_eq!(bitstream(&bit).unwrap(), raw);
        assert!(bitstream(&[0, 1, 2, 3]).is_err());
    }
}
