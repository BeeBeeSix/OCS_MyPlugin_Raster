//! GeoTIFF georeferencing read straight from the TIFF header tags — no pixel
//! decoding, no dependencies. Handles classic TIFF and BigTIFF, either byte
//! order, and the two ways a GeoTIFF stores its affine transform:
//!
//! * `ModelPixelScaleTag` (33550) + one `ModelTiepointTag` (33922) — the usual one;
//! * `ModelTransformationTag` (34264) — a 4×4 matrix (also rotated grids).
//!
//! `GTRasterTypeGeoKey` (in tag 34735) says whether the tiepoint names the
//! pixel's corner (PixelIsArea, the default) or its centre (PixelIsPoint).
//! The result is expressed as a [`World`] so everything downstream is shared
//! with the world-file path.

use crate::world::World;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

const T_SCALE: u16 = 33550;
const T_TIE: u16 = 33922;
const T_TRANS: u16 = 34264;
const T_GEOKEYS: u16 = 34735;
const KEY_RASTER_TYPE: u16 = 1025;

/// `Ok(Some(w))` georeference found; `Ok(None)` not a GeoTIFF (no tags);
/// `Err` malformed, or a georeference that is not a single affine transform.
pub fn read(path: &Path) -> Result<Option<World>, String> {
    let f = File::open(path).map_err(|e| e.to_string())?;
    from_reader(&mut BufReader::new(f))
}

pub fn from_reader<R: Read + Seek>(r: &mut R) -> Result<Option<World>, String> {
    let io = |e: std::io::Error| e.to_string();
    let mut head = [0u8; 16];
    let n = read_up_to(r, &mut head).map_err(io)?;
    if n < 8 {
        return Ok(None);
    }
    let be = match &head[0..2] {
        b"II" => false,
        b"MM" => true,
        _ => return Ok(None),
    };
    let u16_at = |b: &[u8]| if be { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) };
    let u32_at = |b: &[u8]| {
        let a = [b[0], b[1], b[2], b[3]];
        if be { u32::from_be_bytes(a) } else { u32::from_le_bytes(a) }
    };
    let u64_at = |b: &[u8]| {
        let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
        if be { u64::from_be_bytes(a) } else { u64::from_le_bytes(a) }
    };
    let (big, ifd) = match u16_at(&head[2..4]) {
        42 => (false, u32_at(&head[4..8]) as u64),
        43 if n >= 16 => (true, u64_at(&head[8..16])),
        _ => return Ok(None),
    };

    r.seek(SeekFrom::Start(ifd)).map_err(io)?;
    let (count, esz, inline) = if big {
        let mut b = [0u8; 8];
        r.read_exact(&mut b).map_err(io)?;
        (u64_at(&b), 20usize, 8usize)
    } else {
        let mut b = [0u8; 2];
        r.read_exact(&mut b).map_err(io)?;
        (u16_at(&b) as u64, 12usize, 4usize)
    };
    if count == 0 || count > 4096 {
        return Ok(None);
    }
    let mut buf = vec![0u8; count as usize * esz];
    r.read_exact(&mut buf).map_err(io)?;

    // (tag, type, count, raw 4/8 value bytes)
    let mut found: Vec<(u16, u16, u64, Vec<u8>)> = Vec::new();
    for e in buf.chunks_exact(esz) {
        let tag = u16_at(&e[0..2]);
        if ![T_SCALE, T_TIE, T_TRANS, T_GEOKEYS].contains(&tag) {
            continue;
        }
        let ty = u16_at(&e[2..4]);
        let (cnt, val) = if big { (u64_at(&e[4..12]), e[12..20].to_vec()) } else { (u32_at(&e[4..8]) as u64, e[8..12].to_vec()) };
        found.push((tag, ty, cnt, val));
    }
    if found.is_empty() {
        return Ok(None);
    }

    // The data of one tag, as raw bytes (inline in the entry, or at an offset).
    let mut fetch = |ty: u16, cnt: u64, val: &[u8]| -> Result<Vec<u8>, String> {
        let tsz: u64 = match ty {
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            12 | 16 | 17 | 18 => 8,
            _ => 1,
        };
        let total = tsz.checked_mul(cnt).filter(|t| *t <= 1 << 16).ok_or("tag too large")?;
        if total as usize <= inline {
            return Ok(val[..total as usize].to_vec());
        }
        let off = if big { u64_at(val) } else { u32_at(val) as u64 };
        r.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
        let mut d = vec![0u8; total as usize];
        r.read_exact(&mut d).map_err(|e| e.to_string())?;
        Ok(d)
    };
    let doubles = |d: &[u8]| -> Vec<f64> {
        d.chunks_exact(8)
            .map(|c| {
                let a = [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]];
                if be { f64::from_be_bytes(a) } else { f64::from_le_bytes(a) }
            })
            .collect()
    };

    let (mut scale, mut tie, mut trans, mut raster_type) = (Vec::new(), Vec::new(), Vec::new(), None::<u16>);
    for (tag, ty, cnt, val) in &found {
        let d = fetch(*ty, *cnt, val)?;
        match *tag {
            T_SCALE => scale = doubles(&d),
            T_TIE => tie = doubles(&d),
            T_TRANS => trans = doubles(&d),
            T_GEOKEYS => {
                let k: Vec<u16> = d.chunks_exact(2).map(u16_at).collect();
                // header: version, revision, minor, key count; then 4 shorts per key
                for key in k.chunks_exact(4).skip(1) {
                    if key[0] == KEY_RASTER_TYPE && key[1] == 0 {
                        raster_type = Some(key[3]);
                    }
                }
            }
            _ => {}
        }
    }

    // PixelIsArea (1, default): raster coordinates run to pixel corners.
    // PixelIsPoint (2): they name pixel centres.
    let shift = if raster_type == Some(2) { 0.0 } else { 0.5 };
    let w = if trans.len() == 16 {
        let t = &trans;
        World { a: t[0], b: t[1], d: t[4], e: t[5], c: t[0] * shift + t[1] * shift + t[3], f: t[4] * shift + t[5] * shift + t[7] }
    } else if scale.len() >= 2 && tie.len() == 6 {
        let (sx, sy) = (scale[0], scale[1]);
        World { a: sx, d: 0.0, b: 0.0, e: -sy, c: tie[3] + (shift - tie[0]) * sx, f: tie[4] - (shift - tie[1]) * sy }
    } else if tie.len() > 6 {
        return Err("several tiepoints (GCPs): not a single affine georeference".into());
    } else {
        return Ok(None);
    };
    if ![w.a, w.b, w.c, w.d, w.e, w.f].iter().all(|v| v.is_finite()) || w.a * w.e - w.b * w.d == 0.0 {
        return Err("the GeoTIFF transform is degenerate".into());
    }
    Ok(Some(w))
}

fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a one-IFD TIFF holding only the given (tag, type, bytes) entries.
    fn tiff(be: bool, big: bool, entries: &[(u16, u16, u64, Vec<u8>)]) -> Vec<u8> {
        let w16 = |v: u16| if be { v.to_be_bytes().to_vec() } else { v.to_le_bytes().to_vec() };
        let w32 = |v: u32| if be { v.to_be_bytes().to_vec() } else { v.to_le_bytes().to_vec() };
        let w64 = |v: u64| if be { v.to_be_bytes().to_vec() } else { v.to_le_bytes().to_vec() };
        let (hdr, esz, inline) = if big { (16usize, 20usize, 8usize) } else { (8, 12, 4) };
        let cnt_sz = if big { 8 } else { 2 };
        let data_start = hdr + cnt_sz + entries.len() * esz + if big { 8 } else { 4 };
        let mut out = Vec::new();
        out.extend(if be { b"MM" } else { b"II" });
        if big {
            out.extend(w16(43));
            out.extend(w16(8));
            out.extend(w16(0));
            out.extend(w64(16));
            out.extend(w64(entries.len() as u64));
        } else {
            out.extend(w16(42));
            out.extend(w32(8));
            out.extend(w16(entries.len() as u16));
        }
        let mut data: Vec<u8> = Vec::new();
        for (tag, ty, cnt, bytes) in entries {
            out.extend(w16(*tag));
            out.extend(w16(*ty));
            if big { out.extend(w64(*cnt)) } else { out.extend(w32(*cnt as u32)) }
            if bytes.len() <= inline {
                let mut v = bytes.clone();
                v.resize(inline, 0);
                out.extend(v);
            } else {
                let off = (data_start + data.len()) as u64;
                if big { out.extend(w64(off)) } else { out.extend(w32(off as u32)) }
                data.extend(bytes);
            }
        }
        out.extend(if big { w64(0) } else { w32(0) });
        out.extend(data);
        out
    }

    fn dbl(be: bool, v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| if be { x.to_be_bytes() } else { x.to_le_bytes() }).collect()
    }
    fn sh(be: bool, v: &[u16]) -> Vec<u8> {
        v.iter().flat_map(|x| if be { x.to_be_bytes() } else { x.to_le_bytes() }).collect()
    }
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn scale_tie(be: bool, big: bool, raster_type: Option<u16>, tie_x: f64, tie_y: f64) -> Vec<u8> {
        let mut e = vec![
            (T_SCALE, 12u16, 3u64, dbl(be, &[0.1, 0.1, 0.0])),
            (T_TIE, 12, 6, dbl(be, &[0.0, 0.0, 0.0, tie_x, tie_y, 0.0])),
        ];
        if let Some(rt) = raster_type {
            // version 1.1.0, 1 key: GTRasterTypeGeoKey (inline value)
            e.push((T_GEOKEYS, 3, 8, sh(be, &[1, 1, 0, 1, KEY_RASTER_TYPE, 0, 1, rt])));
        }
        tiff(be, big, &e)
    }

    #[test]
    fn scale_and_tiepoint_pixel_is_area_all_flavours() {
        // same tile as the .pgw sample: corner (636598.1672, 4269610.9415)
        for (be, big) in [(false, false), (true, false), (false, true), (true, true)] {
            let bytes = scale_tie(be, big, None, 636598.1672, 4269610.9415);
            let w = from_reader(&mut Cursor::new(bytes)).unwrap().unwrap();
            assert!(close(w.c, 636598.2172) && close(w.f, 4269610.8915), "{be} {big} {w:?}");
            assert!(close(w.a, 0.1) && close(w.e, -0.1) && w.b == 0.0 && w.d == 0.0);
            // and the placement is the one the .pgw gives
            let p = w.placement(1809, 1476);
            assert!(close(p.ins[0], 636598.1672) && close(p.ins[1], 4269610.9415 - 147.6));
        }
    }

    #[test]
    fn pixel_is_point_means_tiepoint_is_the_centre() {
        let bytes = scale_tie(false, false, Some(2), 636598.2172, 4269610.8915);
        let w = from_reader(&mut Cursor::new(bytes)).unwrap().unwrap();
        assert!(close(w.c, 636598.2172) && close(w.f, 4269610.8915));
        let bytes = scale_tie(false, false, Some(1), 636598.1672, 4269610.9415);
        let w = from_reader(&mut Cursor::new(bytes)).unwrap().unwrap();
        assert!(close(w.c, 636598.2172) && close(w.f, 4269610.8915));
    }

    #[test]
    fn transformation_matrix_with_rotation() {
        let m = [0.0866, 0.05, 0.0, 1000.0, 0.05, -0.0866, 0.0, 2000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let bytes = tiff(false, false, &[(T_TRANS, 12, 16, dbl(false, &m))]);
        let w = from_reader(&mut Cursor::new(bytes)).unwrap().unwrap();
        assert!(close(w.a, 0.0866) && close(w.b, 0.05) && close(w.d, 0.05) && close(w.e, -0.0866));
        // centre of the top-left pixel = matrix applied to (0.5, 0.5)
        assert!(close(w.c, 0.0866 * 0.5 + 0.05 * 0.5 + 1000.0));
        assert!(close(w.f, 0.05 * 0.5 - 0.0866 * 0.5 + 2000.0));
    }

    #[test]
    fn plain_tiff_png_garbage_and_gcps_are_handled() {
        // plain TIFF without geo tags
        let plain = tiff(false, false, &[(256, 3, 1, sh(false, &[10]))]);
        assert_eq!(from_reader(&mut Cursor::new(plain)).unwrap(), None);
        assert_eq!(from_reader(&mut Cursor::new(b"\x89PNG\r\n\x1a\n....".to_vec())).unwrap(), None);
        assert_eq!(from_reader(&mut Cursor::new(vec![1, 2, 3])).unwrap(), None);
        // two tiepoints = GCPs: refuse rather than guess
        let gcp = tiff(false, false, &[(T_SCALE, 12, 3, dbl(false, &[1.0, 1.0, 0.0])), (T_TIE, 12, 12, dbl(false, &[0.0; 12]))]);
        assert!(from_reader(&mut Cursor::new(gcp)).is_err());
    }
}
