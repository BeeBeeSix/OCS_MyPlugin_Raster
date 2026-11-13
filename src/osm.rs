//! Base-map import (`RT_OSM`): download web-map tiles (OpenStreetMap by
//! default) for a rectangle given in a projected CRS (any EPSG code the
//! `proj4rs` table knows), reproject them to a north-up raster in that CRS and
//! write it as PNG + world file. Host-free; the network part is isolated in
//! [`http_fetcher`] so everything else is unit-tested with synthetic tiles.
//!
//! Web-map tiles are Web Mercator (EPSG:3857), 256 px, addressed `{z}/{x}/{y}`.
//! The output raster is resampled from them: for every output pixel the
//! drawing coordinate is converted to longitude/latitude and then to a
//! position in the tile mosaic. The conversion is evaluated on a coarse grid
//! (every [`STEP`] pixels) and interpolated between nodes — the warp is smooth
//! — so a 8192² image needs ~260k projections instead of 67 million.

use image::RgbaImage;
use std::f64::consts::PI;
use std::path::PathBuf;
use std::time::Duration;

pub const OSM_URL: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
pub const MAX_ZOOM: u8 = 19;
pub const TILE_PX: u32 = 256;
/// Node spacing of the warp grid, in output pixels.
const STEP: u32 = 16;
/// Largest latitude a Web-Mercator tile covers.
const MAX_LAT: f64 = 85.051_128_78;
pub const ATTRIBUTION: &str = "© OpenStreetMap contributors";

// ── Projection ──────────────────────────────────────────────────────────────

/// Drawing CRS → longitude/latitude (degrees).
pub struct Crs {
    from: proj4rs::Proj,
    wgs84: proj4rs::Proj,
}

impl Crs {
    pub fn new(epsg: u16) -> Result<Crs, String> {
        let from = proj4rs::Proj::from_epsg_code(epsg)
            .map_err(|_| format!("EPSG:{epsg} is not in the projection table"))?;
        if from.is_latlong() {
            return Err(format!("EPSG:{epsg} is geographic (degrees); use a projected CRS such as UTM"));
        }
        let wgs84 = proj4rs::Proj::from_epsg_code(4326).map_err(|e| e.to_string())?;
        Ok(Crs { from, wgs84 })
    }

    /// `(lon, lat)` in degrees, or `None` when the point is outside the CRS's valid area.
    pub fn lonlat(&self, e: f64, n: f64) -> Option<(f64, f64)> {
        let mut p = (e, n, 0.0_f64);
        proj4rs::transform::transform(&self.from, &self.wgs84, &mut p).ok()?;
        let (lon, lat) = (p.0.to_degrees(), p.1.to_degrees());
        (lon.is_finite() && lat.is_finite()).then_some((lon, lat))
    }
}

// ── Web Mercator ────────────────────────────────────────────────────────────

/// Normalised Web-Mercator position (0..1, y down) of a lon/lat in degrees.
pub fn merc_norm(lon: f64, lat: f64) -> (f64, f64) {
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    let x = (lon + 180.0) / 360.0;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0;
    (x, y)
}

/// Inverse of [`merc_norm`]: `(lon, lat)` in degrees.
pub fn merc_inverse(x: f64, y: f64) -> (f64, f64) {
    let lon = x * 360.0 - 180.0;
    let lat = (PI * (1.0 - 2.0 * y)).sinh().atan().to_degrees();
    (lon, lat)
}

/// Ground metres per tile pixel at `lat` for zoom `z`.
pub fn native_res(lat: f64, z: u8) -> f64 {
    156_543.033_928_041 * lat.to_radians().cos() / f64::from(1u32 << z)
}

// ── Planning ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Job {
    /// Area in the drawing CRS.
    pub e0: f64,
    pub n0: f64,
    pub e1: f64,
    pub n1: f64,
    /// Wanted output pixel size (CRS units per pixel); `None` = as detailed as the tile cap allows.
    pub pixel: Option<f64>,
    pub max_tiles: u32,
    /// Largest output side in pixels (the host displays at most 8192²).
    pub max_px: u32,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub zoom: u8,
    /// Inclusive tile range.
    pub tx0: u32,
    pub ty0: u32,
    pub tx1: u32,
    pub ty1: u32,
    pub width: u32,
    pub height: u32,
    pub pixel: f64,
    /// Top-left corner of the output raster (drawing CRS).
    pub e0: f64,
    pub n_top: f64,
    pub notes: Vec<String>,
}

impl Plan {
    pub fn tile_count(&self) -> u32 {
        (self.tx1 - self.tx0 + 1) * (self.ty1 - self.ty0 + 1)
    }
}

/// Mercator extent (normalised) of the job's rectangle, sampled on a 17×17 grid.
fn merc_extent(crs: &Crs, j: &Job) -> Result<((f64, f64, f64, f64), f64), String> {
    let (mut xmin, mut xmax, mut ymin, mut ymax) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for iy in 0..=16 {
        for ix in 0..=16 {
            let e = j.e0 + (j.e1 - j.e0) * f64::from(ix) / 16.0;
            let n = j.n0 + (j.n1 - j.n0) * f64::from(iy) / 16.0;
            let (lon, lat) = crs
                .lonlat(e, n)
                .ok_or("the area is outside the valid range of this CRS — check the EPSG code")?;
            if lat.abs() > MAX_LAT {
                return Err("the area is beyond the latitude range of web-map tiles (±85°)".into());
            }
            let (x, y) = merc_norm(lon, lat);
            xmin = xmin.min(x);
            xmax = xmax.max(x);
            ymin = ymin.min(y);
            ymax = ymax.max(y);
        }
    }
    let (_, lat_c) = crs.lonlat((j.e0 + j.e1) / 2.0, (j.n0 + j.n1) / 2.0).ok_or("centre outside the CRS area")?;
    Ok(((xmin, xmax, ymin, ymax), lat_c))
}

/// Tile range covering `ext` at zoom `z`, with a 2 px margin for interpolation.
fn tile_range(ext: (f64, f64, f64, f64), z: u8) -> (u32, u32, u32, u32) {
    let n = f64::from(1u32 << z);
    let m = 2.0 / (f64::from(TILE_PX) * n);
    let last = (1u32 << z) - 1;
    let t = |v: f64| ((v * n).floor().max(0.0) as u32).min(last);
    (t(ext.0 - m), t(ext.2 - m), t(ext.1 + m), t(ext.3 + m))
}

fn count_at(ext: (f64, f64, f64, f64), z: u8) -> u32 {
    let (x0, y0, x1, y1) = tile_range(ext, z);
    (x1 - x0 + 1) * (y1 - y0 + 1)
}

pub fn plan(crs: &Crs, j: &Job) -> Result<Plan, String> {
    let (de, dn) = (j.e1 - j.e0, j.n1 - j.n0);
    if !(de.is_finite() && dn.is_finite() && de > 0.0 && dn > 0.0) {
        return Err("the rectangle has no area".into());
    }
    let max_tiles = j.max_tiles.clamp(1, 400);
    let (ext, lat_c) = merc_extent(crs, j)?;
    let mut notes = Vec::new();

    // Zoom: as fine as wanted (or as the tile cap allows), never finer than MAX_ZOOM.
    let wanted = match j.pixel {
        Some(p) if p > 0.0 => {
            let z = (156_543.033_928_041 * lat_c.to_radians().cos() / p).log2().ceil();
            z.clamp(0.0, f64::from(MAX_ZOOM)) as u8
        }
        _ => MAX_ZOOM,
    };
    let mut zoom = wanted;
    while zoom > 0 && count_at(ext, zoom) > max_tiles {
        zoom -= 1;
    }
    if zoom < wanted {
        notes.push(format!("detail limited to zoom {zoom} by the {max_tiles}-tile cap (raise it, or pick a smaller area)"));
    }

    let native = native_res(lat_c, zoom);
    let mut pixel = j.pixel.filter(|p| *p > 0.0).map_or(native, |p| p.max(native));
    let side_cap = de.max(dn) / f64::from(j.max_px.max(64));
    if pixel < side_cap {
        notes.push(format!("pixel size raised to {side_cap:.3} to stay within {} px", j.max_px));
        pixel = side_cap;
    }
    let width = (de / pixel).ceil().max(1.0) as u32;
    let height = (dn / pixel).ceil().max(1.0) as u32;
    let (tx0, ty0, tx1, ty1) = tile_range(ext, zoom);
    Ok(Plan { zoom, tx0, ty0, tx1, ty1, width, height, pixel, e0: j.e0, n_top: j.n1, notes })
}

/// World-file text (six lines) for the rendered raster.
pub fn world_file(p: &Plan) -> String {
    format!("{}\n0\n0\n{}\n{}\n{}\n", p.pixel, -p.pixel, p.e0 + p.pixel / 2.0, p.n_top - p.pixel / 2.0)
}

// ── Tiles → mosaic ──────────────────────────────────────────────────────────

pub struct Mosaic {
    /// Global Mercator pixel position of the mosaic's top-left corner.
    pub x0: f64,
    pub y0: f64,
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>, // RGBA
    pub failed: u32,
}

/// Fetch every tile of the plan (sequentially) and assemble them. Tiles that
/// fail stay transparent; it is an error only when none could be fetched.
pub fn build_mosaic<F>(p: &Plan, fetch: &mut F) -> Result<Mosaic, String>
where
    F: FnMut(u8, u32, u32) -> Result<RgbaImage, String>,
{
    let (nx, ny) = (p.tx1 - p.tx0 + 1, p.ty1 - p.ty0 + 1);
    let (w, h) = (nx * TILE_PX, ny * TILE_PX);
    let mut data = vec![0u8; w as usize * h as usize * 4];
    let (mut failed, mut first_err) = (0u32, None);
    for ty in p.ty0..=p.ty1 {
        for tx in p.tx0..=p.tx1 {
            match fetch(p.zoom, tx, ty) {
                Ok(img) if img.width() == TILE_PX && img.height() == TILE_PX => {
                    let (ox, oy) = ((tx - p.tx0) * TILE_PX, (ty - p.ty0) * TILE_PX);
                    for row in 0..TILE_PX {
                        let src = &img.as_raw()[(row * TILE_PX * 4) as usize..((row + 1) * TILE_PX * 4) as usize];
                        let d = (((oy + row) * w + ox) * 4) as usize;
                        data[d..d + (TILE_PX * 4) as usize].copy_from_slice(src);
                    }
                }
                Ok(img) => {
                    failed += 1;
                    first_err.get_or_insert(format!("tile {}/{tx}/{ty} is {}×{} px, expected 256×256", p.zoom, img.width(), img.height()));
                }
                Err(e) => {
                    failed += 1;
                    first_err.get_or_insert(e);
                }
            }
        }
    }
    if failed == nx * ny {
        return Err(first_err.unwrap_or_else(|| "no tiles could be fetched".into()));
    }
    Ok(Mosaic { x0: f64::from(p.tx0 * TILE_PX), y0: f64::from(p.ty0 * TILE_PX), w, h, data, failed })
}

// ── Mosaic → output raster ──────────────────────────────────────────────────

fn sample(m: &Mosaic, x: f64, y: f64) -> [u8; 4] {
    if !(x.is_finite() && y.is_finite()) || x < -1.0 || y < -1.0 || x > f64::from(m.w) || y > f64::from(m.h) {
        return [0, 0, 0, 0];
    }
    let (x, y) = (x.clamp(0.0, f64::from(m.w - 1)), y.clamp(0.0, f64::from(m.h - 1)));
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(m.w as usize - 1), (y0 + 1).min(m.h as usize - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let px = |xx: usize, yy: usize, c: usize| f64::from(m.data[(yy * m.w as usize + xx) * 4 + c]);
    let mut out = [0u8; 4];
    for (c, o) in out.iter_mut().enumerate() {
        let top = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
        let bot = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
        *o = (top * (1.0 - fy) + bot * fy).round() as u8;
    }
    out
}

/// Resample the mosaic into the plan's north-up raster in the drawing CRS (RGBA).
pub fn render(crs: &Crs, p: &Plan, m: &Mosaic) -> Vec<u8> {
    let (w, h) = (p.width as usize, p.height as usize);
    let (ncx, ncy) = (p.width.div_ceil(STEP) as usize, p.height.div_ceil(STEP) as usize);
    let scale = f64::from(1u32 << p.zoom) * f64::from(TILE_PX);
    // Warp-grid nodes: Mercator pixel position of every STEP-th output pixel corner.
    let mut nodes = vec![(f64::NAN, f64::NAN); (ncx + 1) * (ncy + 1)];
    for gy in 0..=ncy {
        for gx in 0..=ncx {
            let e = p.e0 + (gx as f64) * f64::from(STEP) * p.pixel;
            let n = p.n_top - (gy as f64) * f64::from(STEP) * p.pixel;
            if let Some((lon, lat)) = crs.lonlat(e, n) {
                let (x, y) = merc_norm(lon, lat);
                nodes[gy * (ncx + 1) + gx] = (x * scale, y * scale);
            }
        }
    }
    let mut out = vec![0u8; w * h * 4];
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
    let rows_per = h.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        for (ci, chunk) in out.chunks_mut(rows_per * w * 4).enumerate() {
            let nodes = &nodes;
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(w * 4).enumerate() {
                    let j = ci * rows_per + r;
                    let v = (j as f64 + 0.5) / f64::from(STEP);
                    let gy = (v.floor() as usize).min(ncy - 1);
                    let fy = v - gy as f64;
                    for i in 0..w {
                        let u = (i as f64 + 0.5) / f64::from(STEP);
                        let gx = (u.floor() as usize).min(ncx - 1);
                        let fx = u - gx as f64;
                        let n00 = nodes[gy * (ncx + 1) + gx];
                        let n10 = nodes[gy * (ncx + 1) + gx + 1];
                        let n01 = nodes[(gy + 1) * (ncx + 1) + gx];
                        let n11 = nodes[(gy + 1) * (ncx + 1) + gx + 1];
                        let mx = n00.0 * (1.0 - fx) * (1.0 - fy) + n10.0 * fx * (1.0 - fy) + n01.0 * (1.0 - fx) * fy + n11.0 * fx * fy;
                        let my = n00.1 * (1.0 - fx) * (1.0 - fy) + n10.1 * fx * (1.0 - fy) + n01.1 * (1.0 - fx) * fy + n11.1 * fx * fy;
                        // pixel centres sit at +0.5 in Mercator pixel space
                        let px = sample(m, mx - 0.5 - m.x0, my - 0.5 - m.y0);
                        row[i * 4..i * 4 + 4].copy_from_slice(&px);
                    }
                }
            });
        }
    });
    out
}

// ── Network (tile download with an on-disk cache) ───────────────────────────

fn user_agent() -> String {
    format!(
        "OpenCADStudio-RasterPlugin/{} (base-map import; +https://github.com/HakanSeven12/OpenCADStudio)",
        env!("CARGO_PKG_VERSION")
    )
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

/// Cache folder for one tile source; tiles younger than a week are reused.
pub fn cache_dir(template: &str) -> PathBuf {
    std::env::temp_dir().join("ocs_osm_tiles").join(format!("{:016x}", fnv(template)))
}

pub fn tile_url(template: &str, z: u8, x: u32, y: u32) -> String {
    template.replace("{z}", &z.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string()).replace("{s}", "a")
}

pub fn check_template(t: &str) -> Result<(), String> {
    if !(t.starts_with("https://") || t.starts_with("http://")) {
        return Err("the tile URL must start with http:// or https://".into());
    }
    for k in ["{z}", "{x}", "{y}"] {
        if !t.contains(k) {
            return Err(format!("the tile URL needs {k} (e.g. {OSM_URL})"));
        }
    }
    Ok(())
}

/// A tile fetcher for `template`: cache first, then HTTP GET (identifying
/// User-Agent, 20 s timeout), one tile at a time.
pub fn http_fetcher(template: &str) -> impl FnMut(u8, u32, u32) -> Result<RgbaImage, String> {
    let template = template.to_string();
    let dir = cache_dir(&template);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(user_agent())
        .build()
        .into();
    move |z, x, y| {
        let file = dir.join(z.to_string()).join(x.to_string()).join(format!("{y}.png"));
        let fresh = std::fs::metadata(&file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(7 * 24 * 3600));
        if fresh {
            // Decode by content: JPEG/WebP tiles are cached under a .png name too.
            if let Some(img) = std::fs::read(&file).ok().and_then(|b| image::load_from_memory(&b).ok()) {
                return Ok(img.to_rgba8());
            }
        }
        let url = tile_url(&template, z, x, y);
        let mut resp = agent.get(&url).call().map_err(|e| format!("{url}: {e}"))?;
        let bytes = resp.body_mut().read_to_vec().map_err(|e| format!("{url}: {e}"))?;
        let img = image::load_from_memory(&bytes).map_err(|e| format!("{url}: {e}"))?.to_rgba8();
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&file, &bytes);
        Ok(img)
    }
}

/// True for the default OpenStreetMap tile server.
pub fn is_osm(template: &str) -> bool {
    host_of(template).ends_with("openstreetmap.org")
}

fn host_of(template: &str) -> String {
    template.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("").to_ascii_lowercase()
}

/// Visible credit text for a tile source.
pub fn credit(template: &str) -> String {
    if is_osm(template) {
        ATTRIBUTION.to_string()
    } else {
        format!("Map tiles: {} - check the source's licence", host_of(template))
    }
}

/// Layer name for a tile source.
pub fn layer_name(template: &str) -> &'static str {
    if is_osm(template) { "OSM" } else { "BASEMAP" }
}

/// File-name stem: `<osm|map>_epsg<code>_z<zoom>_<E>_<N>`.
pub fn file_stem(template: &str, epsg: u16, p: &Plan) -> String {
    let pre = if is_osm(template) { "osm" } else { "map" };
    format!("{pre}_epsg{epsg}_z{}_{}_{}", p.zoom, p.e0.round() as i64, (p.n_top - f64::from(p.height) * p.pixel).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn cache_reads_by_content_and_source_naming() {
        let tmpl = "https://example.org/{z}/{x}/{y}";
        let f = cache_dir(tmpl).join("3").join("1").join("2.png");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let img = image::RgbImage::from_pixel(4, 4, image::Rgb([9, 8, 7]));
        img.save_with_format(&f, image::ImageFormat::Jpeg).unwrap();
        let mut fetch = http_fetcher(tmpl);
        assert_eq!(fetch(3, 1, 2).unwrap().dimensions(), (4, 4));
        let _ = std::fs::remove_dir_all(cache_dir(tmpl));
        assert!(is_osm(OSM_URL) && !is_osm(tmpl));
        assert_eq!(layer_name(tmpl), "BASEMAP");
        assert!(credit(tmpl).contains("example.org"));
        assert_eq!(credit(OSM_URL), ATTRIBUTION);
    }

    #[test]
    fn mercator_roundtrip_and_known_tile() {
        let (x, y) = merc_norm(-121.4321, 38.5645);
        let (lon, lat) = merc_inverse(x, y);
        assert!(close(lon, -121.4321, 1e-9) && close(lat, 38.5645, 1e-9));
        // slippy-map tile numbers of two known places at zoom 10 (OSM wiki formula)
        let tile = |lon: f64, lat: f64| {
            let (x, y) = merc_norm(lon, lat);
            ((x * 1024.0) as u32, (y * 1024.0) as u32)
        };
        assert_eq!(tile(-122.4194, 37.7749), (163, 395)); // San Francisco
        assert_eq!(tile(-121.4321, 38.5645), (166, 392)); // Sacramento
        assert_eq!(tile(0.0, 0.0), (512, 512));
        assert!(close(native_res(0.0, 0), 156_543.03, 0.05));
        assert!(close(native_res(0.0, 1), 78_271.52, 0.05));
    }

    #[test]
    fn crs_utm10_and_other_epsg() {
        let c = Crs::new(32610).unwrap();
        let (lon, lat) = c.lonlat(636598.2172, 4269610.8915).unwrap();
        assert!(close(lon, -121.4321, 1e-3) && close(lat, 38.5645, 1e-3), "{lon} {lat}");
        // New Zealand Transverse Mercator, Wellington
        let nz = Crs::new(2193).unwrap();
        let (lon, lat) = nz.lonlat(1_748_000.0, 5_428_000.0).unwrap();
        assert!(close(lon, 174.78, 0.1) && close(lat, -41.29, 0.1), "{lon} {lat}");
        assert!(Crs::new(4326).is_err(), "geographic CRS must be refused");
        assert!(Crs::new(1).is_err());
    }

    fn job(pixel: Option<f64>, max_tiles: u32) -> Job {
        // 1 km square in UTM 10N
        Job { e0: 636_000.0, n0: 4_269_000.0, e1: 637_000.0, n1: 4_270_000.0, pixel, max_tiles, max_px: 8192 }
    }

    #[test]
    fn plan_picks_zoom_from_pixel_size_and_tile_cap() {
        let crs = Crs::new(32610).unwrap();
        // 1 m/px at 38.6°N needs zoom 17 (native 0.93 m/px); 1 km ≈ 1000 px ≈ 4-5 tiles a side
        let p = plan(&crs, &job(Some(1.0), 64)).unwrap();
        assert_eq!(p.zoom, 17);
        assert!(close(p.pixel, 1.0, 1e-9) && p.width == 1000 && p.height == 1000);
        assert!(p.tile_count() <= 64 && p.tile_count() >= 16);
        // automatic = finest zoom within the cap
        let a = plan(&crs, &job(None, 64)).unwrap();
        assert!(a.tile_count() <= 64 && a.zoom >= 17 && a.zoom <= 18, "{}", a.zoom);
        assert!(close(a.pixel, native_res(38.58, a.zoom), 0.02));
        // a tiny cap limits detail and says so
        let l = plan(&crs, &job(Some(0.2), 4)).unwrap();
        assert!(l.tile_count() <= 4 && l.zoom < 19 && !l.notes.is_empty());
        assert!(l.pixel >= native_res(38.58, l.zoom) - 1e-6);
        // output side cap
        let big = Job { e1: 636_000.0 + 50_000.0, n1: 4_269_000.0 + 50_000.0, ..job(Some(1.0), 400) };
        let b = plan(&crs, &big).unwrap();
        assert!(b.width <= 8192 && b.height <= 8192);
        assert!(plan(&crs, &Job { e1: 636_000.0, ..job(None, 8) }).is_err());
    }

    #[test]
    fn world_file_anchors_pixel_centres() {
        let crs = Crs::new(32610).unwrap();
        let p = plan(&crs, &job(Some(1.0), 64)).unwrap();
        let w = crate::world::parse(&world_file(&p)).unwrap();
        let pl = w.placement(p.width, p.height);
        assert!(close(pl.ins[0], 636_000.0, 1e-6));
        assert!(close(pl.ins[1], 4_270_000.0 - f64::from(p.height) * p.pixel, 1e-6));
    }

    /// The whole warp, end to end, against a synthetic world map: tiles are
    /// painted from a known function of (lon, lat); the UTM raster must match
    /// that function evaluated directly at every checked pixel.
    #[test]
    fn warp_matches_the_source_map_for_utm_and_nztm() {
        for (epsg, e0, n0) in [(32610u16, 636_000.0, 4_269_000.0), (2193, 1_748_000.0, 5_428_000.0)] {
            let crs = Crs::new(epsg).unwrap();
            let j = Job { e0, n0, e1: e0 + 800.0, n1: n0 + 600.0, pixel: Some(1.0), max_tiles: 64, max_px: 8192 };
            let p = plan(&crs, &j).unwrap();
            let (lon_c, lat_c) = crs.lonlat(e0 + 400.0, n0 + 300.0).unwrap();
            // smooth gradient: ±100 levels across ~0.01° — fine enough to expose any 1 px error
            let f = |lon: f64, lat: f64| {
                let r = 128.0 + (lon - lon_c) * 100.0 / 0.006;
                let g = 128.0 + (lat - lat_c) * 100.0 / 0.004;
                [r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, 77u8, 255u8]
            };
            let mut fetch = |z: u8, tx: u32, ty: u32| -> Result<RgbaImage, String> {
                let mut img = RgbaImage::new(TILE_PX, TILE_PX);
                let s = f64::from(1u32 << z) * f64::from(TILE_PX);
                for py in 0..TILE_PX {
                    for px in 0..TILE_PX {
                        let (lon, lat) = merc_inverse((f64::from(tx * TILE_PX + px) + 0.5) / s, (f64::from(ty * TILE_PX + py) + 0.5) / s);
                        img.put_pixel(px, py, image::Rgba(f(lon, lat)));
                    }
                }
                Ok(img)
            };
            let m = build_mosaic(&p, &mut fetch).unwrap();
            assert_eq!(m.failed, 0);
            let out = render(&crs, &p, &m);
            assert_eq!(out.len(), (p.width * p.height * 4) as usize);
            let mut worst = 0i32;
            for &(i, jx) in &[(5u32, 5u32), (400, 300), (790, 590), (10, 580), (780, 12), (123, 456), (601, 77)] {
                let e = p.e0 + (f64::from(i) + 0.5) * p.pixel;
                let n = p.n_top - (f64::from(jx) + 0.5) * p.pixel;
                let (lon, lat) = crs.lonlat(e, n).unwrap();
                let want = f(lon, lat);
                let got = &out[((jx * p.width + i) * 4) as usize..][..4];
                for c in 0..3 {
                    worst = worst.max((i32::from(got[c]) - i32::from(want[c])).abs());
                }
                assert_eq!(got[3], 255, "pixel ({i},{jx}) not covered");
            }
            assert!(worst <= 3, "EPSG:{epsg}: worst channel error {worst}");
        }
    }

    #[test]
    fn missing_tiles_stay_transparent_and_total_failure_is_an_error() {
        let crs = Crs::new(32610).unwrap();
        let p = plan(&crs, &job(Some(1.0), 64)).unwrap();
        let mut calls = 0;
        let mut half = |_: u8, _: u32, _: u32| -> Result<RgbaImage, String> {
            calls += 1;
            if calls % 2 == 0 { Err("boom".into()) } else { Ok(RgbaImage::from_pixel(256, 256, image::Rgba([9, 9, 9, 255]))) }
        };
        let m = build_mosaic(&p, &mut half).unwrap();
        assert!(m.failed > 0 && m.failed < p.tile_count());
        let mut none = |_: u8, _: u32, _: u32| -> Result<RgbaImage, String> { Err("offline".into()) };
        assert_eq!(build_mosaic(&p, &mut none).err().as_deref(), Some("offline"));
    }

    #[test]
    fn url_template_checks() {
        assert!(check_template(OSM_URL).is_ok());
        assert!(check_template("ftp://x/{z}/{x}/{y}").is_err());
        assert!(check_template("https://x/{z}/{x}.png").is_err());
        assert_eq!(tile_url(OSM_URL, 17, 1, 2), "https://tile.openstreetmap.org/17/1/2.png");
    }

    #[test]
    fn http_fetch_and_cache_against_a_local_server() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let png = {
            let mut v = Vec::new();
            let img = RgbaImage::from_pixel(256, 256, image::Rgba([10, 20, 30, 255]));
            image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).unwrap();
            v
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = png.clone();
        let server = std::thread::spawn(move || {
            // serve exactly one request; a second fetch must come from the cache
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let head = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            s.write_all(head.as_bytes()).unwrap();
            s.write_all(&body).unwrap();
            req
        });
        let tpl = format!("http://127.0.0.1:{port}/{{z}}/{{x}}/{{y}}.png?t={}", std::process::id());
        let _ = std::fs::remove_dir_all(cache_dir(&tpl));
        let mut fetch = http_fetcher(&tpl);
        let a = fetch(5, 6, 7).unwrap();
        assert_eq!(a.get_pixel(3, 3).0, [10, 20, 30, 255]);
        let req = server.join().unwrap();
        assert!(req.starts_with("GET /5/6/7.png"), "{req}");
        assert!(req.to_lowercase().contains("user-agent: opencadstudio-rasterplugin"), "{req}");
        // server is gone: this only works from the cache
        let b = fetch(5, 6, 7).unwrap();
        assert_eq!(b.get_pixel(0, 0).0, [10, 20, 30, 255]);
        let _ = std::fs::remove_dir_all(cache_dir(&tpl));
    }
}
