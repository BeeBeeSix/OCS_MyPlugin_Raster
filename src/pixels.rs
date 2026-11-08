//! Pixel-level work for "make a colour transparent", on plain RGBA8 buffers.
//!
//! Decoding and encoding files happen in `commands.rs` (with the `image`
//! crate); everything here is host-free and unit-tested on its own.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// An RGBA8 picture: `w × h` pixels, row 0 at the top, 4 bytes per pixel.
pub struct Rgba<'a> {
    pub w: u32,
    pub h: u32,
    pub px: &'a mut [u8],
}

impl Rgba<'_> {
    fn idx(&self, x: u32, y: u32) -> usize {
        (y as usize * self.w as usize + x as usize) * 4
    }

    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let i = self.idx(x, y);
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }
}

/// The file pixel under a point of the image frame.
///
/// `a`, `b` are frame pixel coordinates (origin at the insertion corner,
/// bottom-left, `b` upwards) for a frame of `fw × fh` pixels; the file is
/// `w × h` pixels with row 0 at the top. The two sizes normally match, but a
/// frame stretched to another pixel count still maps proportionally.
pub fn file_pixel(a: f64, b: f64, fw: f64, fh: f64, w: u32, h: u32) -> Option<(u32, u32)> {
    if !(a.is_finite() && b.is_finite()) || fw <= 0.0 || fh <= 0.0 || w == 0 || h == 0 {
        return None;
    }
    let (u, v) = (a / fw, b / fh);
    if !(-1e-9..=1.0 + 1e-9).contains(&u) || !(-1e-9..=1.0 + 1e-9).contains(&v) {
        return None;
    }
    let x = ((u * w as f64).floor() as i64).clamp(0, w as i64 - 1) as u32;
    let row_from_bottom = ((v * h as f64).floor() as i64).clamp(0, h as i64 - 1) as u32;
    Some((x, h - 1 - row_from_bottom))
}

/// Whether `c` is within `tol` (0–255, largest channel difference) of `key`.
pub fn close(c: [u8; 4], key: [u8; 3], tol: u8) -> bool {
    c[0].abs_diff(key[0]) <= tol && c[1].abs_diff(key[1]) <= tol && c[2].abs_diff(key[2]) <= tol
}

/// Tolerance in percent (0–100) → channel difference (0–255).
pub fn tol_from_percent(p: f64) -> u8 {
    (p.clamp(0.0, 100.0) * 255.0 / 100.0).round() as u8
}

/// Make transparent every pixel whose colour is within `tol` of `key`:
/// all of them, or — `contiguous` — only those connected (4-neighbours) to
/// the seed pixel. Already transparent pixels stay transparent and do not
/// carry the fill across. Colour channels are kept, only alpha changes, so
/// turning transparency off shows the picture as it was. Returns how many
/// pixels changed.
pub fn make_transparent(img: &mut Rgba, key: [u8; 3], tol: u8, contiguous: Option<(u32, u32)>) -> usize {
    let mut changed = 0;
    match contiguous {
        None => {
            for p in img.px.chunks_exact_mut(4) {
                if p[3] != 0 && close([p[0], p[1], p[2], p[3]], key, tol) {
                    p[3] = 0;
                    changed += 1;
                }
            }
        }
        Some((sx, sy)) => {
            if sx >= img.w || sy >= img.h {
                return 0;
            }
            let (w, h) = (img.w as usize, img.h as usize);
            let mut seen = vec![false; w * h];
            let mut queue = VecDeque::from([(sx, sy)]);
            seen[sy as usize * w + sx as usize] = true;
            while let Some((x, y)) = queue.pop_front() {
                let c = img.get(x, y);
                if c[3] == 0 || !close(c, key, tol) {
                    continue;
                }
                let i = img.idx(x, y);
                img.px[i + 3] = 0;
                changed += 1;
                let (x, y) = (x as usize, y as usize);
                let mut push = |nx: usize, ny: usize| {
                    let k = ny * w + nx;
                    if !seen[k] {
                        seen[k] = true;
                        queue.push_back((nx as u32, ny as u32));
                    }
                };
                if x > 0 {
                    push(x - 1, y);
                }
                if x + 1 < w {
                    push(x + 1, y);
                }
                if y > 0 {
                    push(x, y - 1);
                }
                if y + 1 < h {
                    push(x, y + 1);
                }
            }
        }
    }
    changed
}

/// `#RRGGBB`.
pub fn hex_color(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// The file stem without a previous `_trasp-N` suffix this plugin added.
pub fn base_stem(stem: &str) -> &str {
    if let Some(i) = stem.rfind("_trasp-") {
        let tail = &stem[i + "_trasp-".len()..];
        if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
            return &stem[..i];
        }
    }
    stem
}

/// A new, unused PNG path next to `source`: `<stem>_trasp-<n>.png`.
///
/// A fresh name every time, because the host caches decoded images per
/// path — rewriting the same file would leave the old picture on screen.
pub fn transparent_copy_path(source: &Path) -> PathBuf {
    let dir = source.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("immagine");
    let base = base_stem(stem);
    (1u32..)
        .map(|n| dir.join(format!("{base}_trasp-{n}.png")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&f(x, y));
            }
        }
        v
    }

    #[test]
    fn frame_point_to_file_pixel() {
        // 4 x 2 frame, same-size file: bottom-left frame pixel → file row 1.
        assert_eq!(file_pixel(0.2, 0.3, 4.0, 2.0, 4, 2), Some((0, 1)));
        assert_eq!(file_pixel(3.9, 1.9, 4.0, 2.0, 4, 2), Some((3, 0)));
        // the far edge belongs to the last pixel
        assert_eq!(file_pixel(4.0, 2.0, 4.0, 2.0, 4, 2), Some((3, 0)));
        // frame stretched over a file with twice the pixels
        assert_eq!(file_pixel(1.5, 0.5, 4.0, 2.0, 8, 4), Some((3, 2)));
        assert_eq!(file_pixel(-1.0, 0.5, 4.0, 2.0, 4, 2), None);
        assert_eq!(file_pixel(1.0, 2.5, 4.0, 2.0, 4, 2), None);
    }

    #[test]
    fn tolerance() {
        assert!(close([250, 250, 248, 255], [255, 255, 255], 7));
        assert!(!close([250, 250, 247, 255], [255, 255, 255], 7));
        assert_eq!(tol_from_percent(0.0), 0);
        assert_eq!(tol_from_percent(10.0), 26);
        assert_eq!(tol_from_percent(150.0), 255);
    }

    #[test]
    fn global_replace_keeps_colours() {
        // white background with a 2x2 dark square in the middle of a 6x6
        let mut buf = picture(6, 6, |x, y| {
            if (2..4).contains(&x) && (2..4).contains(&y) { [10, 10, 10, 255] } else { [252, 253, 255, 255] }
        });
        let mut img = Rgba { w: 6, h: 6, px: &mut buf };
        let n = make_transparent(&mut img, [255, 255, 255], 5, None);
        assert_eq!(n, 32);
        assert_eq!(img.get(0, 0), [252, 253, 255, 0]); // colour kept, alpha 0
        assert_eq!(img.get(2, 2)[3], 255);
        // running again changes nothing
        assert_eq!(make_transparent(&mut img, [255, 255, 255], 5, None), 0);
    }

    #[test]
    fn contiguous_fill_stops_at_borders() {
        // 7x5: white outside, a black ring, white inside the ring
        let mut buf = picture(7, 5, |x, y| {
            let ring = (1..=5).contains(&x) && (1..=3).contains(&y) && !((2..=4).contains(&x) && y == 2);
            if ring { [0, 0, 0, 255] } else { [255, 255, 255, 255] }
        });
        let mut img = Rgba { w: 7, h: 5, px: &mut buf };
        // click outside: the 3 white pixels inside the ring stay opaque
        let n = make_transparent(&mut img, [255, 255, 255], 0, Some((0, 0)));
        assert_eq!(n, 7 * 5 - 15);
        assert_eq!(img.get(3, 2)[3], 255);
        assert_eq!(img.get(0, 4)[3], 0);
        // click inside: only those 3
        let n = make_transparent(&mut img, [255, 255, 255], 0, Some((3, 2)));
        assert_eq!(n, 3);
        // seed on a non-matching pixel does nothing
        assert_eq!(make_transparent(&mut img, [255, 255, 255], 0, Some((1, 1))), 0);
    }

    #[test]
    fn names() {
        assert_eq!(base_stem("pianta"), "pianta");
        assert_eq!(base_stem("pianta_trasp-3"), "pianta");
        assert_eq!(base_stem("pianta_trasp-x"), "pianta_trasp-x");
        assert_eq!(hex_color([255, 8, 171]), "#FF08AB");
        let dir = std::env::temp_dir().join(format!("rt-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("foto.jpg");
        assert_eq!(transparent_copy_path(&src), dir.join("foto_trasp-1.png"));
        std::fs::write(dir.join("foto_trasp-1.png"), b"x").unwrap();
        assert_eq!(transparent_copy_path(&dir.join("foto_trasp-1.png")), dir.join("foto_trasp-2.png"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
