//! Pixel size of a raster file read from its header — no decoding, no
//! dependencies. Covers the formats Open CAD Studio displays: PNG, JPEG,
//! GIF, BMP, TIFF and WebP.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// Extensions offered in file pickers (lower case, no dot).
pub const EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "jpe", "gif", "bmp", "dib", "tif", "tiff", "webp",
];

/// Whether a path names a raster file this plugin understands.
pub fn is_raster_name(name: &str) -> bool {
    name.rsplit_once('.')
        .map(|(_, ext)| EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// `(width, height)` in pixels of the image at `path`.
pub fn read_dims(path: &Path) -> io::Result<(u32, u32)> {
    let mut r = BufReader::new(File::open(path)?);
    dims_from_reader(&mut r)
}

/// `(width, height)` from any seekable byte source.
pub fn dims_from_reader<R: Read + Seek>(r: &mut R) -> io::Result<(u32, u32)> {
    let mut head = [0u8; 32];
    let n = read_up_to(r, &mut head)?;
    let head = &head[..n];
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        if n < 24 {
            return Err(bad("PNG troncato"));
        }
        return Ok((be32(&head[16..20]), be32(&head[20..24])));
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        if n < 10 {
            return Err(bad("GIF troncato"));
        }
        return Ok((le16(&head[6..8]) as u32, le16(&head[8..10]) as u32));
    }
    if head.starts_with(b"BM") {
        if n < 26 {
            return Err(bad("BMP troncato"));
        }
        let header_size = le32(&head[14..18]);
        if header_size == 12 {
            // OS/2 BITMAPCOREHEADER: 16-bit width/height.
            return Ok((le16(&head[18..20]) as u32, le16(&head[20..22]) as u32));
        }
        let w = le32(&head[18..22]) as i32;
        let h = le32(&head[22..26]) as i32;
        return Ok((w.unsigned_abs(), h.unsigned_abs()));
    }
    if head.starts_with(b"\xff\xd8") {
        return jpeg_dims(r);
    }
    if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        return tiff_dims(r, head[0] == b'I');
    }
    if n >= 30 && head.starts_with(b"RIFF") && &head[8..12] == b"WEBP" {
        return webp_dims(head);
    }
    Err(bad("formato immagine non riconosciuto"))
}

fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Walk JPEG segments to the first SOFn frame header.
fn jpeg_dims<R: Read + Seek>(r: &mut R) -> io::Result<(u32, u32)> {
    r.seek(SeekFrom::Start(2))?;
    let mut b = [0u8; 1];
    loop {
        // Find the 0xFF marker prefix (skip fill bytes).
        r.read_exact(&mut b)?;
        if b[0] != 0xFF {
            continue;
        }
        let mut marker = 0xFFu8;
        while marker == 0xFF {
            r.read_exact(&mut b)?;
            marker = b[0];
        }
        match marker {
            // Standalone markers: no length.
            0x01 | 0xD0..=0xD7 => continue,
            0xD9 | 0xDA => return Err(bad("JPEG senza intestazione di frame")),
            _ => {}
        }
        let mut len = [0u8; 2];
        r.read_exact(&mut len)?;
        let len = be16(&len) as i64;
        if len < 2 {
            return Err(bad("JPEG: segmento non valido"));
        }
        let is_sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let mut sof = [0u8; 5];
            r.read_exact(&mut sof)?;
            let h = be16(&sof[1..3]) as u32;
            let w = be16(&sof[3..5]) as u32;
            return Ok((w, h));
        }
        r.seek(SeekFrom::Current(len - 2))?;
    }
}

/// First IFD's ImageWidth (256) / ImageLength (257).
fn tiff_dims<R: Read + Seek>(r: &mut R, little: bool) -> io::Result<(u32, u32)> {
    let u16_ = |b: &[u8]| if little { le16(b) } else { be16(b) };
    let u32_ = |b: &[u8]| if little { le32(b) } else { be32(b) };
    r.seek(SeekFrom::Start(4))?;
    let mut off = [0u8; 4];
    r.read_exact(&mut off)?;
    r.seek(SeekFrom::Start(u32_(&off) as u64))?;
    let mut cnt = [0u8; 2];
    r.read_exact(&mut cnt)?;
    let (mut w, mut h) = (None, None);
    for _ in 0..u16_(&cnt) {
        let mut e = [0u8; 12];
        r.read_exact(&mut e)?;
        let tag = u16_(&e[0..2]);
        let ty = u16_(&e[2..4]);
        let val = match ty {
            3 => u16_(&e[8..10]) as u32, // SHORT, left-justified in the value field
            4 => u32_(&e[8..12]),        // LONG
            _ => continue,
        };
        match tag {
            256 => w = Some(val),
            257 => h = Some(val),
            _ => {}
        }
        if let (Some(w), Some(h)) = (w, h) {
            return Ok((w, h));
        }
    }
    Err(bad("TIFF senza dimensioni"))
}

fn webp_dims(head: &[u8]) -> io::Result<(u32, u32)> {
    let chunk = &head[12..16];
    let d = &head[20..];
    match chunk {
        b"VP8 " => {
            // 3-byte frame tag, 3-byte start code, then 14-bit w/h.
            if d.len() < 10 || d[3..6] != [0x9d, 0x01, 0x2a] {
                return Err(bad("WebP VP8 non valido"));
            }
            Ok(((le16(&d[6..8]) & 0x3fff) as u32, (le16(&d[8..10]) & 0x3fff) as u32))
        }
        b"VP8L" => {
            if d.len() < 5 || d[0] != 0x2f {
                return Err(bad("WebP VP8L non valido"));
            }
            let bits = le32(&d[1..5]);
            Ok(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => {
            if d.len() < 10 {
                return Err(bad("WebP VP8X troncato"));
            }
            let w = 1 + (d[4] as u32 | (d[5] as u32) << 8 | (d[6] as u32) << 16);
            let h = 1 + (d[7] as u32 | (d[8] as u32) << 8 | (d[9] as u32) << 16);
            Ok((w, h))
        }
        _ => Err(bad("WebP: blocco sconosciuto")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn dims(bytes: Vec<u8>) -> (u32, u32) {
        dims_from_reader(&mut Cursor::new(bytes)).unwrap()
    }

    #[test]
    fn png() {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend_from_slice(&640u32.to_be_bytes());
        b.extend_from_slice(&480u32.to_be_bytes());
        b.extend_from_slice(&[8, 2, 0, 0, 0]);
        assert_eq!(dims(b), (640, 480));
    }

    #[test]
    fn gif() {
        let mut b = b"GIF89a".to_vec();
        b.extend_from_slice(&300u16.to_le_bytes());
        b.extend_from_slice(&200u16.to_le_bytes());
        b.extend_from_slice(&[0; 8]);
        assert_eq!(dims(b), (300, 200));
    }

    #[test]
    fn bmp_top_down() {
        let mut b = b"BM".to_vec();
        b.extend_from_slice(&[0; 12]);
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&1024i32.to_le_bytes());
        b.extend_from_slice(&(-768i32).to_le_bytes());
        b.extend_from_slice(&[0; 8]);
        assert_eq!(dims(b), (1024, 768));
    }

    #[test]
    fn jpeg_after_app_segment() {
        let mut b = vec![0xFF, 0xD8];
        // APP1 with 6 bytes of payload.
        b.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x08, 1, 2, 3, 4, 5, 6]);
        // SOF2 (progressive): len, precision, height, width.
        b.extend_from_slice(&[0xFF, 0xC2, 0x00, 0x11, 8]);
        b.extend_from_slice(&1080u16.to_be_bytes());
        b.extend_from_slice(&1920u16.to_be_bytes());
        b.extend_from_slice(&[0; 20]);
        assert_eq!(dims(b), (1920, 1080));
    }

    #[test]
    fn jpeg_dht_is_not_a_frame() {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xC4, 0x00, 0x04, 9, 9];
        b.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 8]);
        b.extend_from_slice(&50u16.to_be_bytes());
        b.extend_from_slice(&70u16.to_be_bytes());
        b.extend_from_slice(&[0; 20]);
        assert_eq!(dims(b), (70, 50));
    }

    #[test]
    fn tiff_little_and_big_endian() {
        // II: IFD at 8, two entries: width LONG 5000, height SHORT 3000.
        let mut b = b"II*\0".to_vec();
        b.extend_from_slice(&8u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&256u16.to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&5000u32.to_le_bytes());
        b.extend_from_slice(&257u16.to_le_bytes());
        b.extend_from_slice(&3u16.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&3000u16.to_le_bytes());
        b.extend_from_slice(&[0, 0]);
        assert_eq!(dims(b), (5000, 3000));

        let mut b = b"MM\0*".to_vec();
        b.extend_from_slice(&8u32.to_be_bytes());
        b.extend_from_slice(&2u16.to_be_bytes());
        b.extend_from_slice(&256u16.to_be_bytes());
        b.extend_from_slice(&3u16.to_be_bytes());
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(&800u16.to_be_bytes());
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&257u16.to_be_bytes());
        b.extend_from_slice(&3u16.to_be_bytes());
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(&600u16.to_be_bytes());
        b.extend_from_slice(&[0, 0]);
        assert_eq!(dims(b), (800, 600));
    }

    #[test]
    fn webp_variants() {
        let riff = |chunk: &[u8], data: &[u8]| {
            let mut b = b"RIFF\0\0\0\0WEBP".to_vec();
            b.extend_from_slice(chunk);
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(data);
            b.extend_from_slice(&[0; 16]);
            b
        };
        let mut vp8 = vec![0, 0, 0, 0x9d, 0x01, 0x2a];
        vp8.extend_from_slice(&400u16.to_le_bytes());
        vp8.extend_from_slice(&300u16.to_le_bytes());
        assert_eq!(dims(riff(b"VP8 ", &vp8)), (400, 300));

        let bits: u32 = (640 - 1) | ((480 - 1) << 14);
        let mut vp8l = vec![0x2f];
        vp8l.extend_from_slice(&bits.to_le_bytes());
        assert_eq!(dims(riff(b"VP8L", &vp8l)), (640, 480));

        let mut vp8x = vec![0; 4];
        vp8x.extend_from_slice(&[(1999 & 0xff) as u8, (1999 >> 8) as u8, 0]);
        vp8x.extend_from_slice(&[(999 & 0xff) as u8, (999 >> 8) as u8, 0]);
        assert_eq!(dims(riff(b"VP8X", &vp8x)), (2000, 1000));
    }

    #[test]
    fn names() {
        assert!(is_raster_name("C:\\foto\\Pianta.JPG"));
        assert!(is_raster_name("a.webp"));
        assert!(!is_raster_name("disegno.dwg"));
        assert!(!is_raster_name("senza_estensione"));
    }

    #[test]
    fn unknown_format_errors() {
        assert!(dims_from_reader(&mut Cursor::new(b"hello world, not an image".to_vec())).is_err());
    }
}
