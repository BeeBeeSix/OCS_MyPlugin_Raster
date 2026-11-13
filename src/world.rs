//! World files (`.pgw`, `.tfw`, `.jgw`, `.gfw`, `.bpw`, `.wld` …): the six-line
//! text file that georeferences a raster. Host-free and unit-tested.
//!
//! ```text
//! A   pixel size in x (map units per column)
//! D   rotation term (y shift per column)
//! B   rotation term (x shift per row)
//! E   pixel size in y (negative for a north-up image)
//! C   x of the CENTRE of the top-left pixel
//! F   y of the CENTRE of the top-left pixel
//! ```
//!
//! `X = A·col + B·row + C`, `Y = D·col + E·row + F`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct World {
    pub a: f64,
    pub d: f64,
    pub b: f64,
    pub e: f64,
    pub c: f64,
    pub f: f64,
}

/// Where an image goes: bottom-left corner of its pixel grid, and the vectors
/// for one pixel to the right (`u`) and one pixel up (`v`) — the way a DWG
/// raster image is described.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub ins: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
}

/// Read the first six numbers of a world file (comma decimals, CRLF and a BOM
/// are tolerated; extra lines are ignored).
pub fn parse(text: &str) -> Result<World, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut v: Vec<f64> = Vec::with_capacity(6);
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if v.len() == 6 {
            break;
        }
        match line.replace(',', ".").parse::<f64>() {
            Ok(n) if n.is_finite() => v.push(n),
            _ => return Err(format!("line {} is not a number: \"{line}\"", v.len() + 1)),
        }
    }
    if v.len() < 6 {
        return Err(format!("{} number(s) found, 6 expected", v.len()));
    }
    let w = World { a: v[0], d: v[1], b: v[2], e: v[3], c: v[4], f: v[5] };
    if w.a * w.e - w.b * w.d == 0.0 {
        return Err("the pixel vectors are degenerate (zero area)".into());
    }
    Ok(w)
}

impl World {
    /// Placement of a `w`×`h` pixel image. The world file anchors the CENTRE
    /// of the top-left pixel, so the grid's corner is half a pixel further out.
    pub fn placement(&self, _w: u32, h: u32) -> Placement {
        let r = h as f64 - 0.5; // bottom edge, in pixel-centre units
        Placement {
            ins: [
                self.c - 0.5 * self.a + self.b * r,
                self.f - 0.5 * self.d + self.e * r,
                0.0,
            ],
            u: [self.a, self.d, 0.0],
            v: [-self.b, -self.e, 0.0],
        }
    }

    /// True when the pixel grid is mirrored (a normal north-up image has
    /// `A·E − B·D < 0`).
    pub fn mirrored(&self) -> bool {
        self.a * self.e - self.b * self.d > 0.0
    }
}

/// World-file extensions for an image extension, most specific first:
/// first+last letter + `w` (`png`→`pgw`, `tif`→`tfw`), extension + `w`
/// (`tiffw`), then the generic `wld`.
pub fn candidate_exts(image_ext: &str) -> Vec<String> {
    let e = image_ext.to_ascii_lowercase();
    let mut out = Vec::new();
    let chars: Vec<char> = e.chars().collect();
    if chars.len() >= 2 {
        out.push(format!("{}{}w", chars[0], chars[chars.len() - 1]));
    }
    if !e.is_empty() {
        out.push(format!("{e}w"));
    }
    out.push("wld".into());
    out.dedup();
    out
}

/// The world file beside `image` (same folder, same name ignoring case).
pub fn find(image: &Path) -> Option<PathBuf> {
    let dir = image.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stem = image.file_stem()?.to_string_lossy().to_lowercase();
    let ext = image.extension()?.to_string_lossy().to_string();
    let entries: Vec<(PathBuf, String, String)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let p = e.path();
            let s = p.file_stem()?.to_string_lossy().to_lowercase();
            let x = p.extension()?.to_string_lossy().to_lowercase();
            Some((p, s, x))
        })
        .collect();
    candidate_exts(&ext)
        .into_iter()
        .find_map(|c| entries.iter().find(|(_, s, x)| *s == stem && *x == c).map(|(p, _, _)| p.clone()))
}

/// Georeference of `image`: for a TIFF, the GeoTIFF tags in its own header
/// (returned path = the image itself); otherwise — or when the TIFF carries
/// none — the world file beside it.
pub fn load(image: &Path) -> Result<(PathBuf, World), String> {
    let mut geo_err = None;
    let is_tiff = image
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff"));
    if is_tiff {
        match crate::geotiff::read(image) {
            Ok(Some(w)) => return Ok((image.to_path_buf(), w)),
            Ok(None) => {}
            Err(e) => geo_err = Some(e),
        }
    }
    let p = find(image).ok_or_else(|| {
        let ext = image.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        let names: Vec<String> = candidate_exts(&ext).iter().map(|e| format!(".{e}")).collect();
        let geo = match (&geo_err, is_tiff) {
            (Some(e), _) => format!("; GeoTIFF header: {e}"),
            (None, true) => " and no GeoTIFF tags in the file".to_string(),
            _ => String::new(),
        };
        format!("no world file ({}) beside the image{geo}", names.join(" / "))
    })?;
    let text = std::fs::read(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
    let w = parse(&String::from_utf8_lossy(&text)).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((p, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "0.10000000\n0.00000000\n0.00000000\n-0.10000000\n636598.2172\n4269610.8915\n";

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn sample_pgw_places_the_1809x1476_tile() {
        let w = parse(SAMPLE).unwrap();
        let p = w.placement(1809, 1476);
        // bottom-left corner: half a pixel left of C, and (H - 0.5) pixels below F
        assert!(close(p.ins[0], 636598.1672), "{:?}", p.ins);
        assert!(close(p.ins[1], 4269610.8915 + 0.05 - 147.6), "{:?}", p.ins);
        assert_eq!(p.u, [0.1, 0.0, 0.0]);
        assert_eq!(p.v, [-0.0, 0.1, 0.0]);
        // top-left corner = ins + h*v must be half a pixel outside (C, F)
        let top_y = p.ins[1] + 1476.0 * p.v[1];
        assert!(close(top_y, 4269610.8915 + 0.05));
        assert!(!w.mirrored());
    }

    #[test]
    fn matches_the_old_autolisp_formula() {
        // OpenTIF.lsp: Y = Yorient - V/2 + rows*V (V negative), X = Xorient - H/2
        let w = parse(SAMPLE).unwrap();
        let p = w.placement(1809, 1476);
        let lisp_x = 636598.2172 - 0.1 / 2.0;
        let lisp_y = (4269610.8915 - (-0.1) / 2.0) + 1476.0 * (-0.1);
        assert!(close(p.ins[0], lisp_x) && close(p.ins[1], lisp_y));
    }

    #[test]
    fn rotation_terms_are_carried() {
        // 30 degree rotated grid, 2 m pixels
        let (c, s) = (30f64.to_radians().cos() * 2.0, 30f64.to_radians().sin() * 2.0);
        let txt = format!("{c}\n{s}\n{s}\n{}\n1000\n2000\n", -c);
        let w = parse(&txt).unwrap();
        let p = w.placement(10, 20);
        assert_eq!(p.u, [c, s, 0.0]);
        assert!(close(p.v[0], -s) && close(p.v[1], c));
        // the pixel centre (0,0) must come back as (C, F)
        let cx = p.ins[0] + 0.5 * p.u[0] + (20.0 - 0.5) * p.v[0];
        let cy = p.ins[1] + 0.5 * p.u[1] + (20.0 - 0.5) * p.v[1];
        assert!(close(cx, 1000.0) && close(cy, 2000.0), "{cx} {cy}");
    }

    #[test]
    fn parse_is_forgiving_and_strict_where_needed() {
        assert!(parse("1,5\r\n0\r\n0\r\n-1,5\r\n10\r\n20\r\n").is_ok());
        assert!(parse("\u{feff}1\n0\n0\n-1\n10\n20\nextra\n").is_ok());
        assert!(parse("1\n0\n0\n-1\n10\n").is_err());
        assert!(parse("1\n0\nabc\n-1\n10\n20\n").is_err());
        assert!(parse("0\n0\n0\n0\n10\n20\n").is_err());
    }

    #[test]
    fn extensions_and_lookup() {
        assert_eq!(candidate_exts("PNG"), vec!["pgw", "pngw", "wld"]);
        assert_eq!(candidate_exts("tif"), vec!["tfw", "tifw", "wld"]);
        assert_eq!(candidate_exts("jpeg")[0], "jgw");
        let dir = std::env::temp_dir().join(format!("rt_world_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("Tile_R02C03.PNG");
        std::fs::write(&img, b"x").unwrap();
        assert!(find(&img).is_none());
        std::fs::write(dir.join("tile_r02c03.PGW"), SAMPLE).unwrap();
        let (p, w) = load(&img).unwrap();
        assert!(p.ends_with("tile_r02c03.PGW"));
        assert_eq!(w.c, 636598.2172);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
