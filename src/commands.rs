//! Command dispatch: the glue between the host and the pure helpers.
//!
//! User-facing verbs (`RT_CLIP`, `RT_SIZE`, …) start an interactive tool or a
//! dialog. Those tools cannot touch the drawing themselves — interactive
//! callbacks get no host access — so when they finish they hand the host a
//! one-line *apply* command (`RT_CLIPAPPLY`, `RT_SIZEAPPLY`, …) that comes
//! back here with full `HostApi` access and performs the edit as one undo
//! step. The apply verbs are scriptable too; see PLUGIN.md.
//!
//! The host's command line probes a typed line's first word on its own before
//! running the whole line, so every apply verb treats a call with no
//! arguments as a silent no-op.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ocs_plugin_api::host::codec;
use ocs_plugin_api::host::{HostApi, HostSettingValue};

use codec::entities::{ClipBoundary, ClipMode, ClipType, ImageDisplayFlags, RasterImage};
use codec::objects::ObjectType;
use codec::{EntityType, Handle, Vector2, Vector3};

use crate::geom::{self, Frame, Shape, P2, P3};
use crate::imgsize;
use crate::tools;

/// A path chosen in a file dialog, waiting for the apply command that uses
/// it. Paths never travel through a command line (it may upper-case them).
static PENDING_PATH: Mutex<Option<String>> = Mutex::new(None);

pub fn set_pending_path(path: String) {
    *PENDING_PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
}

fn take_pending_path() -> Option<String> {
    PENDING_PATH.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// A clip polygon drawn or picked by a tool, waiting for `RT_CLIPPOLYAPPLY`
/// (a polyline with arcs can have hundreds of vertices — too long for a
/// command line).
static PENDING_POLY: Mutex<Option<Vec<P3>>> = Mutex::new(None);

pub fn set_pending_poly(poly: Vec<P3>) {
    *PENDING_POLY.lock().unwrap_or_else(|e| e.into_inner()) = Some(poly);
}

fn take_pending_poly() -> Option<Vec<P3>> {
    PENDING_POLY.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Largest angle between two points of an arc when polylines, circles and
/// ellipses are turned into clip polygons (3°).
const ARC_STEP: f64 = std::f64::consts::PI / 60.0;
const ELLIPSE_STEPS: usize = 120;

fn plan_normal(n: &Vector3) -> bool {
    n.z > 0.999_999
}

/// Every closed outline in the drawing that can serve as a clip boundary —
/// closed polylines (arcs flattened), circles and full ellipses — as world
/// rings (first vertex not repeated). Shapes whose plane is not the XY plane
/// are skipped.
pub fn closed_shapes(host: &dyn HostApi) -> Vec<(Handle, Vec<P3>)> {
    let mut out = Vec::new();
    for e in host.document().entities() {
        let (ring, z): (Vec<P2>, f64) = match e {
            EntityType::LwPolyline(p) if plan_normal(&p.normal) && p.vertices.len() >= 2 => {
                let mut v: Vec<(P2, f64)> =
                    p.vertices.iter().map(|v| ([v.location.x, v.location.y], v.bulge)).collect();
                let explicit = v.len() > 2 && {
                    let (a, b) = (v[0].0, v[v.len() - 1].0);
                    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
                };
                if !(p.is_closed || explicit) {
                    continue;
                }
                if explicit && !p.is_closed {
                    v.pop();
                }
                (geom::flatten_bulged(&v, true, ARC_STEP), p.elevation)
            }
            EntityType::Polyline2D(p) if plan_normal(&p.normal) && p.is_closed() && p.vertices.len() >= 3 => {
                let v: Vec<(P2, f64)> =
                    p.vertices.iter().map(|v| ([v.location.x, v.location.y], v.bulge)).collect();
                (geom::flatten_bulged(&v, true, ARC_STEP), p.elevation)
            }
            EntityType::Polyline(p) if p.is_closed() && p.vertices.len() >= 3 => {
                let z = p.vertices[0].location.z;
                (p.vertices.iter().map(|v| [v.location.x, v.location.y]).collect(), z)
            }
            EntityType::Circle(c) if plan_normal(&c.normal) && c.radius > 0.0 => (
                geom::ellipse_ring([c.center.x, c.center.y], [c.radius, 0.0], 1.0, ELLIPSE_STEPS),
                c.center.z,
            ),
            EntityType::Ellipse(el)
                if plan_normal(&el.normal)
                    && ((el.end_parameter - el.start_parameter).abs() - std::f64::consts::TAU).abs() < 1e-6 =>
            {
                (
                    geom::ellipse_ring(
                        [el.center.x, el.center.y],
                        [el.major_axis.x, el.major_axis.y],
                        el.minor_axis_ratio,
                        ELLIPSE_STEPS,
                    ),
                    el.center.z,
                )
            }
            _ => continue,
        };
        let ring = geom::dedup(&ring, 1e-12);
        if ring.len() >= 3 {
            out.push((e.common().handle, ring.into_iter().map(|p| [p[0], p[1], z]).collect()));
        }
    }
    out
}

// ── Entity helpers ───────────────────────────────────────────────────────────

pub fn p3(v: &Vector3) -> P3 {
    [v.x, v.y, v.z]
}

fn v3(p: P3) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}

/// The image's frame in plugin geometry.
pub fn frame(img: &RasterImage) -> Frame {
    Frame {
        ins: p3(&img.insertion_point),
        u: p3(&img.u_vector),
        v: p3(&img.v_vector),
        w: img.size.x,
        h: img.size.y,
    }
}

/// Whether a clip boundary is in effect (the host draws it only when both
/// the clip state and the display flag are on).
pub fn clip_active(img: &RasterImage) -> bool {
    img.clipping_enabled
        && img.flags.contains(ImageDisplayFlags::USE_CLIPPING_BOUNDARY)
        && match img.clip_boundary.clip_type {
            ClipType::Rectangular => img.clip_boundary.vertices.len() >= 2,
            ClipType::Polygonal => img.clip_boundary.vertices.len() >= 3,
        }
}

fn clip_verts(img: &RasterImage) -> Vec<P2> {
    img.clip_boundary.vertices.iter().map(|v| [v.x, v.y]).collect()
}

/// The part of the image that shows, as a pixel-space polygon. A hole clip
/// (`Inside`) still shows the frame's outline, so its region is the frame.
pub fn visible_px(img: &RasterImage) -> Vec<P2> {
    let f = frame(img);
    if clip_active(img) && img.clip_boundary.clip_mode == ClipMode::Outside {
        let rect = img.clip_boundary.clip_type == ClipType::Rectangular;
        if let Some(region) = geom::region_px_from_clip(f.h, rect, &clip_verts(img)) {
            return region;
        }
    }
    f.full_px()
}

fn set_shape(img: &mut RasterImage, shape: &Shape, mode: ClipMode) {
    let vertices = shape.vertices().into_iter().map(|p| Vector2::new(p[0], p[1])).collect();
    let clip_type = match shape {
        Shape::Rect(..) => ClipType::Rectangular,
        Shape::Poly(_) => ClipType::Polygonal,
    };
    img.clip_boundary = ClipBoundary { clip_type, clip_mode: mode, vertices };
    img.clipping_enabled = true;
    img.flags |= ImageDisplayFlags::USE_CLIPPING_BOUNDARY;
}

fn clip_label(img: &RasterImage) -> String {
    if !clip_active(img) {
        return "nessuno".into();
    }
    let kind = match img.clip_boundary.clip_type {
        ClipType::Rectangular => "rettangolo",
        ClipType::Polygonal => "poligono",
    };
    match img.clip_boundary.clip_mode {
        ClipMode::Outside => kind.to_string(),
        ClipMode::Inside => format!("foro ({kind})"),
    }
}

/// Every raster image in the drawing, in drawing order.
pub fn all_images(host: &dyn HostApi) -> Vec<RasterImage> {
    host.document()
        .entities()
        .filter_map(|e| match e {
            EntityType::RasterImage(img) => Some(img.clone()),
            _ => None,
        })
        .collect()
}

/// Live read of one image by handle.
fn fetch(host: &dyn HostApi, h: Handle) -> Option<RasterImage> {
    let live = host
        .entities_batch(&[h])
        .ok()
        .and_then(|v| v.into_iter().next().flatten());
    let entity = match live {
        Some(e) => Some(e),
        None => host.document().get_entity(h).cloned(),
    };
    match entity {
        Some(EntityType::RasterImage(img)) => Some(img),
        _ => None,
    }
}

/// Handles of the currently selected raster images.
pub fn selected_images(host: &dyn HostApi) -> Vec<Handle> {
    let sel = host.selection();
    if sel.is_empty() {
        return Vec::new();
    }
    let images: HashSet<u64> = all_images(host).iter().map(|i| i.common.handle.value()).collect();
    sel.into_iter().filter(|h| images.contains(&h.value())).collect()
}

/// File-name part of a stored path (either separator, as DWGs from Windows
/// carry backslashes).
pub fn file_name_only(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Where the image file is found: the stored path, the path relative to the
/// drawing's folder, or its file name in the drawing's folder.
pub fn resolve(path: &str, dwg_dir: Option<&Path>) -> Option<PathBuf> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let p = Path::new(path);
    if p.is_file() {
        return Some(p.to_path_buf());
    }
    // Normalise Windows separators for the relative/name probes.
    let norm = path.replace('\\', "/");
    if let Some(dir) = dwg_dir {
        let rel = norm.trim_start_matches("./");
        if !Path::new(&norm).is_absolute() {
            let j = dir.join(rel);
            if j.is_file() {
                return Some(j);
            }
        }
        let j = dir.join(file_name_only(path));
        if j.is_file() {
            return Some(j);
        }
    }
    None
}

fn dwg_dir(host: &dyn HostApi) -> Option<PathBuf> {
    host.document_path(host.tab_id())
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

fn hex(h: Handle) -> String {
    format!("{:X}", h.value())
}

fn parse_handle(tok: &str) -> Option<Handle> {
    let t = tok.trim().trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(t, 16).ok().filter(|v| *v != 0).map(Handle::new)
}

fn parse_handles(tok: &str) -> Vec<Handle> {
    tok.split(',').filter_map(parse_handle).collect()
}

pub fn join_handles(hs: &[Handle]) -> String {
    hs.iter().map(|h| hex(*h)).collect::<Vec<_>>().join(",")
}

fn num(tok: Option<&str>) -> Option<f64> {
    tok.and_then(|t| t.trim().replace(',', ".").parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

fn fmt_len(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}

// ── Dispatch ────────────────────────────────────────────────────────────────

pub fn handle(host: &mut dyn HostApi, cmd: &str) -> bool {
    let trimmed = cmd.trim();
    let verb = trimmed.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
    if !verb.starts_with("RT_") {
        return false;
    }
    // Everything after the verb, case and spaces intact (paths).
    let rest = trimmed[verb.len()..].trim();
    let args: Vec<&str> = rest.split_whitespace().collect();

    match verb.as_str() {
        "RT_HELP" | "RT_" | "RT_HELLO" => help(host),

        "RT_ATTACH" => {
            let unit = drawing_unit_index(host);
            host.start_interactive(Box::new(tools::AttachDialog::new(rest.trim_matches('"').to_string(), unit)));
        }
        "RT_ATTACHPLACE" => {
            if !args.is_empty() {
                attach_place(host, &args);
            }
        }
        "RT_ATTACHAPPLY" => {
            if !args.is_empty() {
                attach_apply(host, &args);
            }
        }
        "RT_LIST" => list(host),

        // The ribbon menus' own ids run their default (window) form.
        "RT_CLIP" | "RT_CLIP_MENU" => start_clip(host, tools::ClipKind::Keep),
        "RT_CLIPHOLE" | "RT_HOLE_MENU" => start_clip(host, tools::ClipKind::Hole),
        "RT_CUT" | "RT_CUT_MENU" => start_clip(host, tools::ClipKind::Cut),
        "RT_CLIPAPPLY" => {
            if !args.is_empty() {
                clip_apply(host, &args);
            }
        }

        "RT_CLIPPOLY" => start_poly(host, tools::ClipKind::Keep),
        "RT_HOLEPOLY" => start_poly(host, tools::ClipKind::Hole),
        "RT_CUTPOLY" => start_poly(host, tools::ClipKind::Cut),
        "RT_CLIPPL" => start_pline(host, tools::ClipKind::Keep),
        "RT_HOLEPL" => start_pline(host, tools::ClipKind::Hole),
        "RT_CUTPL" => start_pline(host, tools::ClipKind::Cut),
        "RT_CLIPPOLYAPPLY" => {
            if !args.is_empty() {
                clip_poly_apply(host, &args);
            }
        }

        "RT_UNCLIP" => {
            let sel = selected_images(host);
            if sel.is_empty() {
                start_pick(host, "RT_UNCLIPAPPLY", "togliere il ritaglio");
            } else {
                unclip(host, &sel);
            }
        }
        "RT_UNCLIPAPPLY" => {
            if let Some(t) = args.first() {
                unclip(host, &parse_handles(t));
            }
        }

        "RT_RESIZE" => {
            let images = tools::snapshots(host);
            if images.is_empty() {
                host.push_error("RT_RESIZE: nel disegno non ci sono immagini raster.");
            } else {
                let pre = single_selected(host);
                host.start_interactive(Box::new(tools::ResizeTool::new(images, pre)));
            }
        }
        "RT_SCALEAPPLY" => {
            if !args.is_empty() {
                scale_apply(host, &args);
            }
        }

        "RT_SIZE" => {
            let sel = selected_images(host);
            if sel.is_empty() {
                start_pick(host, "RT_SIZEDLG", "ridimensionare");
            } else {
                start_size_dialog(host, sel);
            }
        }
        "RT_SIZEDLG" => {
            if let Some(t) = args.first() {
                start_size_dialog(host, parse_handles(t));
            }
        }
        "RT_SIZEAPPLY" => {
            if !args.is_empty() {
                size_apply(host, &args);
            }
        }

        "RT_PATH" => {
            let sel = selected_images(host);
            if rest.is_empty() {
                if sel.is_empty() {
                    start_pick(host, "RT_PATHDLG", "cambiare il percorso");
                } else {
                    start_path_dialog(host, sel);
                }
            } else if sel.is_empty() {
                // Chosen from the ribbon with nothing selected: keep the path
                // and ask which image it is for.
                set_pending_path(rest.to_string());
                start_pick(host, "RT_PATHAPPLY", "collegare al nuovo file");
            } else {
                relink(host, &sel, rest);
            }
        }
        "RT_PATHDLG" => {
            if let Some(t) = args.first() {
                start_path_dialog(host, parse_handles(t));
            }
        }
        "RT_PATHAPPLY" => {
            if let Some(t) = args.first() {
                match take_pending_path() {
                    Some(p) => relink(host, &parse_handles(t), &p),
                    None => host.push_error("RT_PATH: nessun file scelto."),
                }
            }
        }

        "RT_FIND" => {
            if rest.is_empty() {
                host.start_interactive(Box::new(tools::FindDialog::new()));
            } else {
                find_in_folder(host, Path::new(rest), true);
            }
        }
        "RT_FINDAPPLY" => {
            if let Some(mode) = args.first() {
                match take_pending_path() {
                    Some(p) => find_in_folder(host, Path::new(&p), !mode.eq_ignore_ascii_case("TOP")),
                    None => host.push_error("RT_FIND: nessuna cartella scelta."),
                }
            }
        }

        _ => {
            host.push_error(&format!("{verb}: comando sconosciuto. Digita RT_HELP per l'elenco."));
        }
    }
    true
}

fn help(host: &mut dyn HostApi) {
    host.push_info(
        "Raster Tools — immagini GIF/JPG/PNG/BMP/TIFF/WebP.\n\
         RT_ATTACH: inserisce un'immagine nel disegno (a schermo, per larghezza, a DPI o 1 pixel = 1 unità).\n\
         RT_CLIP: ritaglia con un rettangolo (tiene l'interno; ripetuto, restringe).\n\
         RT_CLIPHOLE: nasconde la parte dentro il rettangolo.\n\
         RT_CUT: separa la parte nel rettangolo in una nuova immagine, spostabile.\n\
         RT_CLIPPOLY / RT_HOLEPOLY / RT_CUTPOLY: come sopra, con un poligono disegnato a clic (Invio chiude).\n\
         RT_CLIPPL / RT_HOLEPL / RT_CUTPL: come sopra, con una polilinea chiusa, un cerchio o un'ellisse esistenti.\n\
         RT_UNCLIP: toglie il ritaglio.\n\
         RT_RESIZE: ridimensiona trascinando un angolo (proporzionale).\n\
         RT_SIZE: larghezza, altezza, fattore di scala o risoluzione DPI.\n\
         RT_PATH: cambia il file su disco dell'immagine.\n\
         RT_FIND: cerca in una cartella le immagini il cui file manca.\n\
         RT_LIST: elenco immagini con percorso, pixel, dimensioni e stato.",
    );
}

fn single_selected(host: &dyn HostApi) -> Option<Handle> {
    let sel = selected_images(host);
    (sel.len() == 1).then(|| sel[0])
}

fn start_poly(host: &mut dyn HostApi, kind: tools::ClipKind) {
    let images = tools::snapshots(host);
    if images.is_empty() {
        host.push_error("Nel disegno non ci sono immagini raster.");
        return;
    }
    let pre = single_selected(host);
    host.start_interactive(Box::new(tools::PolyClipTool::new(images, kind, pre)));
}

fn start_pline(host: &mut dyn HostApi, kind: tools::ClipKind) {
    let images = tools::snapshots(host);
    if images.is_empty() {
        host.push_error("Nel disegno non ci sono immagini raster.");
        return;
    }
    let shapes = closed_shapes(host);
    if shapes.is_empty() {
        host.push_error(&format!(
            "{}: nel disegno non ci sono polilinee chiuse, cerchi o ellissi da usare come contorno.",
            kind.pline_name()
        ));
        return;
    }
    // Preselected image + outline: apply straight away.
    let sel = host.selection();
    let img = sel.iter().find(|h| images.iter().any(|s| s.handle == **h)).copied();
    let shape = sel.iter().find_map(|h| shapes.iter().find(|(sh, _)| sh == h));
    if let (Some(img), Some((_, ring))) = (img, shape) {
        set_pending_poly(ring.clone());
        clip_poly_apply(host, &[&hex(img), kind.keyword()]);
        return;
    }
    let pre = single_selected(host);
    host.start_interactive(Box::new(tools::PlineClipTool::new(images, shapes, kind, pre)));
}

fn start_pick(host: &mut dyn HostApi, verb: &'static str, what: &'static str) {
    let images = tools::snapshots(host);
    if images.is_empty() {
        host.push_error("Nel disegno non ci sono immagini raster.");
        return;
    }
    host.start_interactive(Box::new(tools::PickTool::new(images, verb, what)));
}

fn start_clip(host: &mut dyn HostApi, kind: tools::ClipKind) {
    let images = tools::snapshots(host);
    if images.is_empty() {
        host.push_error("Nel disegno non ci sono immagini raster.");
        return;
    }
    let pre = single_selected(host);
    host.start_interactive(Box::new(tools::ClipTool::new(images, kind, pre)));
}

fn start_size_dialog(host: &mut dyn HostApi, handles: Vec<Handle>) {
    let first = handles.iter().find_map(|h| fetch(host, *h));
    let Some(first) = first else {
        host.push_error("RT_SIZE: nessuna immagine raster valida.");
        return;
    };
    host.start_interactive(Box::new(tools::SizeDialog::new(handles, &first)));
}

fn start_path_dialog(host: &mut dyn HostApi, handles: Vec<Handle>) {
    let Some(first) = handles.iter().find_map(|h| fetch(host, *h)) else {
        host.push_error("RT_PATH: nessuna immagine raster valida.");
        return;
    };
    let found = resolve(&first.file_path, dwg_dir(host).as_deref()).is_some();
    host.start_interactive(Box::new(tools::PathDialog::new(handles, first.file_path.clone(), found)));
}

// ── RT_ATTACH ───────────────────────────────────────────────────────────────

/// `INSUNITS` → index into `tools::UNITS` (mm, cm, m, inches); mm when unknown.
fn drawing_unit_index(host: &dyn HostApi) -> usize {
    let code = match host.system_variable("INSUNITS") {
        Some(HostSettingValue::Number(n)) => n as i64,
        Some(HostSettingValue::Text(t)) => t.trim().parse().unwrap_or(0),
        _ => 0,
    };
    match code {
        5 => 1, // centimetres
        6 => 2, // metres
        1 => 3, // inches
        _ => 0, // millimetres
    }
}

/// `RT_ATTACHPLACE SCREEN|WIDTH|DPI|PIXEL <width> <dpi> MM|CM|M|IN` — after
/// the dialog: check the chosen file and start placing it.
fn attach_place(host: &mut dyn HostApi, args: &[&str]) {
    let Some(path) = take_pending_path() else {
        host.push_error("RT_ATTACH: nessun file scelto.");
        return;
    };
    let p = Path::new(&path);
    if !p.is_file() {
        host.push_error(&format!("RT_ATTACH: il file \"{path}\" non esiste."));
        return;
    }
    let (w, h) = match imgsize::read_dims(p) {
        Ok(d) if d.0 > 0 && d.1 > 0 => d,
        _ => {
            host.push_error(&format!(
                "RT_ATTACH: \"{}\" non è un'immagine GIF, JPG, PNG, BMP, TIFF o WebP leggibile.",
                file_name_only(&path)
            ));
            return;
        }
    };
    let mode = args[0].to_ascii_uppercase();
    let width = num(args.get(1).copied());
    let dpi = num(args.get(2).copied());
    let unit = args.get(3).copied().unwrap_or("MM");
    let px = match mode.as_str() {
        "SCREEN" => None,
        "PIXEL" => Some(1.0),
        "WIDTH" => match width {
            Some(v) if v > 0.0 => Some(v / w as f64),
            _ => {
                host.push_error("RT_ATTACH: indica una larghezza maggiore di zero.");
                return;
            }
        },
        "DPI" => match (dpi, geom::units_per_inch(unit)) {
            (Some(d), Some(upi)) if d > 0.0 => Some(upi / d),
            _ => {
                host.push_error("RT_ATTACH: indica un valore DPI maggiore di zero.");
                return;
            }
        },
        _ => {
            host.push_error("Uso: RT_ATTACHPLACE SCREEN|WIDTH|DPI|PIXEL <larghezza> <dpi> MM|CM|M|IN");
            return;
        }
    };
    if let Some(px) = px {
        host.push_info(&format!(
            "RT_ATTACH: {} — {}×{} px, {} × {} unità.",
            file_name_only(&path),
            w,
            h,
            fmt_len(w as f64 * px),
            fmt_len(h as f64 * px)
        ));
    }
    host.start_interactive(Box::new(tools::AttachTool::new(path, w, h, px)));
}

/// `RT_ATTACHAPPLY <x> <y> <z> <pixel_size>` — insert the pending file with
/// its bottom-left corner at x,y,z.
fn attach_apply(host: &mut dyn HostApi, args: &[&str]) {
    let vals: Option<Vec<f64>> = (0..4).map(|i| num(args.get(i).copied())).collect();
    let (Some(v), Some(path)) = (vals, take_pending_path()) else {
        host.push_error("Uso: RT_ATTACHAPPLY <x> <y> <z> <dimensione_pixel> (dopo la scelta del file)");
        return;
    };
    let px = v[3];
    if !valid_factor(px) {
        host.push_error("RT_ATTACH: dimensione non valida.");
        return;
    }
    let (w, h) = match imgsize::read_dims(Path::new(&path)) {
        Ok(d) => d,
        Err(e) => {
            host.push_error(&format!("RT_ATTACH: impossibile leggere \"{path}\": {e}"));
            return;
        }
    };
    let mut img = RasterImage::new(&path, Vector3::new(v[0], v[1], v[2]), w as f64, h as f64);
    img.u_vector = Vector3::new(px, 0.0, 0.0);
    img.v_vector = Vector3::new(0.0, px, 0.0);
    if let Some(HostSettingValue::Text(layer)) = host.system_variable("CLAYER") {
        if !layer.trim().is_empty() {
            img.common.layer = layer;
        }
    }
    host.begin_undo_group("RT_ATTACH");
    let handle = host.add_entity(EntityType::RasterImage(img));
    host.end_undo_group();
    if handle.is_null() {
        host.push_error(&format!(
            "RT_ATTACH: il CAD non è riuscito a inserire \"{}\" (file non leggibile o layer bloccato).",
            file_name_only(&path)
        ));
        return;
    }
    host.set_dirty();
    let _ = host.set_selection(&[handle]);
    host.push_info(&format!(
        "RT_ATTACH: inserita {} ({}×{} px, {} × {} unità).",
        file_name_only(&path),
        w,
        h,
        fmt_len(w as f64 * px),
        fmt_len(h as f64 * px)
    ));
}

// ── RT_LIST ─────────────────────────────────────────────────────────────────

fn list(host: &mut dyn HostApi) {
    let images = all_images(host);
    if images.is_empty() {
        host.push_info("RT_LIST: nel disegno non ci sono immagini raster.");
        return;
    }
    let dir = dwg_dir(host);
    let mut missing = 0;
    let mut lines = Vec::with_capacity(images.len() * 2 + 1);
    for img in &images {
        let f = frame(img);
        let (w, h) = f.world_size();
        let state = match resolve(&img.file_path, dir.as_deref()) {
            Some(_) => "trovato",
            None => {
                missing += 1;
                "MANCANTE"
            }
        };
        lines.push(format!(
            "  {}  {}  {}×{} px  {} × {}  pixel {}  ritaglio: {}  [{}]",
            hex(img.common.handle),
            file_name_only(&img.file_path),
            img.size.x as u64,
            img.size.y as u64,
            fmt_len(w),
            fmt_len(h),
            fmt_len(geom::len3(f.u)),
            clip_label(img),
            state,
        ));
        lines.push(format!("      {}", img.file_path));
    }
    host.push_output(&format!(
        "RT_LIST: {} immagini, {} con file mancante.\n{}",
        images.len(),
        missing,
        lines.join("\n")
    ));
}

// ── Clip ────────────────────────────────────────────────────────────────────

/// `RT_CLIPAPPLY <handle> KEEP|HOLE|CUT <x1> <y1> <x2> <y2> [z]` — a window
/// drawn on screen (axis-aligned in the drawing).
fn clip_apply(host: &mut dyn HostApi, args: &[&str]) {
    let usage = "Uso: RT_CLIPAPPLY <handle> KEEP|HOLE|CUT <x1> <y1> <x2> <y2>";
    let (Some(h), Some(mode)) = (args.first().and_then(|t| parse_handle(t)), args.get(1)) else {
        host.push_error(usage);
        return;
    };
    let nums: Option<Vec<f64>> = (2..6).map(|i| num(args.get(i).copied())).collect();
    let Some(n) = nums else {
        host.push_error(usage);
        return;
    };
    let z = num(args.get(6).copied()).unwrap_or(0.0);
    let (x0, x1) = (n[0].min(n[2]), n[0].max(n[2]));
    let (y0, y1) = (n[1].min(n[3]), n[1].max(n[3]));
    let window = vec![[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]];
    clip_region(host, h, mode, &window, false);
}

/// `RT_CLIPPOLYAPPLY <handle> KEEP|HOLE|CUT [x1 y1 x2 y2 x3 y3 ...]` — a
/// polygon given as coordinates, or the one a tool just drew or picked.
fn clip_poly_apply(host: &mut dyn HostApi, args: &[&str]) {
    let usage = "Uso: RT_CLIPPOLYAPPLY <handle> KEEP|HOLE|CUT <x1> <y1> <x2> <y2> <x3> <y3> ...";
    let (Some(h), Some(mode)) = (args.first().and_then(|t| parse_handle(t)), args.get(1)) else {
        host.push_error(usage);
        return;
    };
    let coords: Option<Vec<f64>> = args[2..].iter().map(|t| num(Some(t))).collect();
    let poly: Vec<P3> = match coords {
        Some(c) if c.len() >= 6 && c.len() % 2 == 0 => c.chunks(2).map(|p| [p[0], p[1], 0.0]).collect(),
        Some(c) if c.is_empty() => match take_pending_poly() {
            Some(p) => p,
            None => {
                host.push_error("Nessun poligono indicato.");
                return;
            }
        },
        _ => {
            host.push_error(usage);
            return;
        }
    };
    clip_region(host, h, mode, &poly, true);
}

/// Clip image `h` with a world polygon: KEEP shows only its inside
/// (narrowing an existing clip), HOLE hides its inside, CUT splits its
/// inside off into a new image entity.
fn clip_region(host: &mut dyn HostApi, h: Handle, mode: &str, world: &[P3], polygon: bool) {
    let mode = mode.to_ascii_uppercase();
    let (name, what) = match (mode.as_str(), polygon) {
        ("KEEP", false) => ("RT_CLIP", "il rettangolo"),
        ("HOLE", false) => ("RT_CLIPHOLE", "il rettangolo"),
        ("CUT", false) => ("RT_CUT", "il rettangolo"),
        ("KEEP", true) => ("RT_CLIPPOLY", "il poligono"),
        ("HOLE", true) => ("RT_HOLEPOLY", "il poligono"),
        ("CUT", true) => ("RT_CUTPOLY", "il poligono"),
        _ => {
            host.push_error("Modo di ritaglio sconosciuto: usa KEEP, HOLE o CUT.");
            return;
        }
    };
    let Some(mut img) = fetch(host, h) else {
        host.push_error(&format!("{name}: l'entità non è un'immagine raster."));
        return;
    };
    let f = frame(&img);
    let px: Option<Vec<P2>> = world.iter().map(|&p| f.to_pixel(p)).collect();
    let Some(px) = px else {
        host.push_error(&format!("{name}: l'immagine ha una cornice degenere."));
        return;
    };
    let px = geom::dedup(&px, 1e-9);
    if px.len() >= 4 && !geom::is_simple(&px) {
        host.push_error(&format!("{name}: {what} si interseca con se stesso; disegnalo senza lati incrociati."));
        return;
    }
    if px.len() < 3 || geom::signed_area(&px).abs() < geom::MIN_AREA_PX {
        host.push_error(&format!("{name}: {what} ha area nulla."));
        return;
    }
    let full = f.full_px();
    // The polygon trimmed to the frame (the frame is convex).
    let on_image = geom::clip_convex(&px, &full);

    match mode.as_str() {
        "KEEP" => {
            let nested = clip_active(&img) && img.clip_boundary.clip_mode == ClipMode::Outside;
            let region = if !nested {
                on_image
            } else {
                let base = visible_px(&img);
                if geom::is_convex(&base) {
                    geom::clip_convex(&px, &base)
                } else if geom::is_convex(&px) {
                    geom::clip_convex(&base, &px)
                } else {
                    host.push_info(&format!(
                        "{name}: ritaglio precedente e nuovo contorno sono entrambi concavi; \
                         il nuovo contorno sostituisce il precedente."
                    ));
                    on_image
                }
            };
            let Some(shape) = geom::shape_from_px(f.h, &region) else {
                host.push_error(&format!("{name}: {what} non tocca la parte visibile dell'immagine."));
                return;
            };
            if clip_active(&img) && img.clip_boundary.clip_mode == ClipMode::Inside {
                host.push_info(&format!("{name}: il foro esistente è stato sostituito dal nuovo ritaglio."));
            }
            set_shape(&mut img, &shape, ClipMode::Outside);
            host.begin_undo_group(name);
            let ok = host.update_entity(EntityType::RasterImage(img));
            host.end_undo_group();
            let msg = if nested { "ritaglio ristretto." } else { "immagine ritagliata." };
            report(host, ok, &format!("{name}: {msg}"));
        }
        "HOLE" => {
            let Some(shape) = geom::shape_from_px(f.h, &on_image) else {
                host.push_error(&format!("{name}: {what} non tocca l'immagine."));
                return;
            };
            if covers_frame(&f, &on_image) {
                host.push_error(&format!("{name}: {what} copre tutta l'immagine; per toglierla usa CANCELLA."));
                return;
            }
            if clip_active(&img) {
                host.push_info(&format!("{name}: il ritaglio precedente è stato sostituito dal foro."));
            }
            set_shape(&mut img, &shape, ClipMode::Inside);
            host.begin_undo_group(name);
            let ok = host.update_entity(EntityType::RasterImage(img));
            host.end_undo_group();
            report(host, ok, &format!("{name}: parte interna nascosta."));
        }
        _ => {
            // CUT
            if clip_active(&img) {
                host.push_error(&format!(
                    "{name}: l'immagine è già ritagliata; togli prima il ritaglio (RT_UNCLIP) \
                     oppure usa RT_CLIP."
                ));
                return;
            }
            let Some(shape) = geom::shape_from_px(f.h, &on_image) else {
                host.push_error(&format!("{name}: {what} non tocca l'immagine."));
                return;
            };
            if covers_frame(&f, &on_image) {
                host.push_error(&format!("{name}: {what} copre tutta l'immagine: non c'è niente da separare."));
                return;
            }
            // The piece: a fresh entity on the same file, clipped to the outline.
            let mut piece = img.clone();
            piece.common.handle = Handle::NULL;
            piece.common.reactors.clear();
            piece.common.xdictionary_handle = None;
            piece.definition_reactor_handle = None;
            set_shape(&mut piece, &shape, ClipMode::Outside);
            // The rest: the original with the outline as a hole.
            set_shape(&mut img, &shape, ClipMode::Inside);

            host.begin_undo_group(name);
            let ok = host.update_entity(EntityType::RasterImage(img));
            let new_handle = if ok { host.add_entity(EntityType::RasterImage(piece)) } else { Handle::NULL };
            host.end_undo_group();
            if !ok {
                host.push_error(&format!("{name}: l'immagine non può essere modificata (layer bloccato?)."));
                return;
            }
            if new_handle.is_null() {
                host.push_error(&format!("{name}: il pezzo non è stato creato."));
                return;
            }
            host.set_dirty();
            let _ = host.set_selection(&[new_handle]);
            host.push_info(&format!(
                "{name}: pezzo separato ({}). È selezionato: usa SPOSTA (MOVE) per spostarlo.",
                hex(new_handle)
            ));
        }
    }
}

fn covers_frame(f: &Frame, region: &[P2]) -> bool {
    geom::signed_area(region).abs() >= f.w * f.h * (1.0 - 1e-9)
}

fn report(host: &mut dyn HostApi, ok: bool, msg: &str) {
    if ok {
        host.set_dirty();
        host.push_info(msg);
    } else {
        host.push_error("L'immagine non può essere modificata (layer bloccato o entità non trovata).");
    }
}

fn unclip(host: &mut dyn HostApi, handles: &[Handle]) {
    let mut edits = Vec::new();
    for h in handles {
        if let Some(mut img) = fetch(host, *h) {
            if clip_active(&img) || img.clipping_enabled {
                img.clear_clip();
                edits.push(img);
            }
        }
    }
    if edits.is_empty() {
        host.push_info("RT_UNCLIP: nessuna immagine ritagliata tra quelle scelte.");
        return;
    }
    host.begin_undo_group("RT_UNCLIP");
    let mut done = 0;
    for img in edits {
        if host.update_entity(EntityType::RasterImage(img)) {
            done += 1;
        }
    }
    host.end_undo_group();
    if done > 0 {
        host.set_dirty();
    }
    host.push_info(&format!("RT_UNCLIP: ritaglio tolto da {done} immagine/i."));
}

// ── Resize ──────────────────────────────────────────────────────────────────

fn apply_scale(img: &mut RasterImage, anchor_px: P2, sx: f64, sy: f64) {
    let (ins, u, v) = frame(img).scaled(anchor_px, sx, sy);
    img.insertion_point = v3(ins);
    img.u_vector = v3(u);
    img.v_vector = v3(v);
}

fn valid_factor(k: f64) -> bool {
    k.is_finite() && k > 1e-9 && k < 1e9
}

/// `RT_SCALEAPPLY <handle> <anchor_a> <anchor_b> <sx> [sy]` — anchor in pixels.
fn scale_apply(host: &mut dyn HostApi, args: &[&str]) {
    let usage = "Uso: RT_SCALEAPPLY <handle> <ancora_px_x> <ancora_px_y> <fattore_x> [fattore_y]";
    let h = args.first().and_then(|t| parse_handle(t));
    let (a, b, sx) = (num(args.get(1).copied()), num(args.get(2).copied()), num(args.get(3).copied()));
    let (Some(h), Some(a), Some(b), Some(sx)) = (h, a, b, sx) else {
        host.push_error(usage);
        return;
    };
    let sy = num(args.get(4).copied()).unwrap_or(sx);
    if !valid_factor(sx) || !valid_factor(sy) {
        host.push_error("RT_RESIZE: il fattore di scala deve essere positivo.");
        return;
    }
    let Some(mut img) = fetch(host, h) else {
        host.push_error("RT_RESIZE: l'entità non è un'immagine raster.");
        return;
    };
    apply_scale(&mut img, [a, b], sx, sy);
    let (w, hh) = frame(&img).world_size();
    host.begin_undo_group("RT_RESIZE");
    let ok = host.update_entity(EntityType::RasterImage(img));
    host.end_undo_group();
    report(host, ok, &format!("RT_RESIZE: scala {}, nuove dimensioni {} × {}.", fmt_len(sx), fmt_len(w), fmt_len(hh)));
}

/// `RT_SIZEAPPLY <handles> W|H|WH|F|DPI <w> <h> <factor> <dpi> MM|CM|M|IN BL|CTR FULL|VIS`
pub fn size_apply(host: &mut dyn HostApi, args: &[&str]) {
    let usage = "Uso: RT_SIZEAPPLY <handle,...> W|H|WH|F|DPI <larghezza> <altezza> <fattore> <dpi> MM|CM|M|IN BL|CTR FULL|VIS";
    if args.len() < 9 {
        host.push_error(usage);
        return;
    }
    let handles = parse_handles(args[0]);
    let mode = args[1].to_ascii_uppercase();
    let (w, h, k, dpi) = (num(Some(args[2])), num(Some(args[3])), num(Some(args[4])), num(Some(args[5])));
    let unit = args[6];
    let centre = args[7].eq_ignore_ascii_case("CTR");
    let visible = args[8].eq_ignore_ascii_case("VIS");

    let mut edits = Vec::new();
    for handle in &handles {
        let Some(mut img) = fetch(host, *handle) else { continue };
        let f = frame(&img);
        let region = if visible { visible_px(&img) } else { f.full_px() };
        let Some((lo, hi)) = geom::bbox(&region) else { continue };
        let (ul, vl) = (geom::len3(f.u), geom::len3(f.v));
        let (bw, bh) = ((hi[0] - lo[0]) * ul, (hi[1] - lo[1]) * vl);
        let (sx, sy) = match mode.as_str() {
            "W" => match w {
                Some(w) if bw > 0.0 => (w / bw, w / bw),
                _ => (f64::NAN, f64::NAN),
            },
            "H" => match h {
                Some(h) if bh > 0.0 => (h / bh, h / bh),
                _ => (f64::NAN, f64::NAN),
            },
            "WH" => match (w, h) {
                (Some(w), Some(h)) if bw > 0.0 && bh > 0.0 => (w / bw, h / bh),
                _ => (f64::NAN, f64::NAN),
            },
            "F" => match k {
                Some(k) => (k, k),
                None => (f64::NAN, f64::NAN),
            },
            "DPI" => match (dpi, geom::units_per_inch(unit)) {
                (Some(d), Some(upi)) if d > 0.0 && ul > 0.0 && vl > 0.0 => {
                    let px = upi / d;
                    (px / ul, px / vl)
                }
                _ => (f64::NAN, f64::NAN),
            },
            _ => {
                host.push_error(usage);
                return;
            }
        };
        if !valid_factor(sx) || !valid_factor(sy) {
            host.push_error("RT_SIZE: valori non validi (servono numeri positivi).");
            return;
        }
        let anchor = if centre { [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5] } else { lo };
        apply_scale(&mut img, anchor, sx, sy);
        edits.push(img);
    }
    if edits.is_empty() {
        host.push_error("RT_SIZE: nessuna immagine raster valida.");
        return;
    }
    host.begin_undo_group("RT_SIZE");
    let mut done = 0;
    let mut last = (0.0, 0.0);
    for img in edits {
        last = frame(&img).world_size();
        if host.update_entity(EntityType::RasterImage(img)) {
            done += 1;
        }
    }
    host.end_undo_group();
    if done > 0 {
        host.set_dirty();
        host.push_info(&format!(
            "RT_SIZE: {done} immagine/i ridimensionata/e (ultima: {} × {} cornice intera).",
            fmt_len(last.0),
            fmt_len(last.1)
        ));
    } else {
        host.push_error("RT_SIZE: nessuna immagine modificata (layer bloccato?).");
    }
}

// ── Relink ──────────────────────────────────────────────────────────────────

/// One image file reference: the definition (when the drawing has one) and
/// every image entity drawn from it.
struct Group {
    def_name: Option<String>,
    images: Vec<RasterImage>,
    path: String,
}

/// Group `images` by the file they show. Images sharing an image definition
/// share its path, so a relink always moves the whole group.
fn groups_for(host: &dyn HostApi, targets: Option<&[Handle]>) -> Vec<Group> {
    let doc = host.document();
    let images: Vec<RasterImage> = doc
        .entities()
        .filter_map(|e| match e {
            EntityType::RasterImage(i) => Some(i.clone()),
            _ => None,
        })
        .collect();
    let key = |img: &RasterImage| match img.definition_handle {
        Some(h) if !h.is_null() => format!("#{}", h.value()),
        _ => format!("@{}", img.file_path.to_lowercase()),
    };
    let wanted: Option<HashSet<String>> = targets.map(|t| {
        let t: HashSet<u64> = t.iter().map(|h| h.value()).collect();
        images.iter().filter(|i| t.contains(&i.common.handle.value())).map(key).collect()
    });
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, Group> = HashMap::new();
    for img in images {
        let k = key(&img);
        if let Some(w) = &wanted {
            if !w.contains(&k) {
                continue;
            }
        }
        let g = map.entry(k.clone()).or_insert_with(|| {
            order.push(k.clone());
            let def_name = img.definition_handle.and_then(|h| match doc.objects.get(&h) {
                Some(ObjectType::ImageDefinition(d)) => Some(d.file_name.clone()),
                _ => None,
            });
            Group { path: def_name.clone().unwrap_or_else(|| img.file_path.clone()), def_name, images: Vec::new() }
        });
        g.images.push(img);
    }
    order.into_iter().filter_map(|k| map.remove(&k)).collect()
}

fn relink(host: &mut dyn HostApi, targets: &[Handle], new_path: &str) {
    let new_path = new_path.trim().trim_matches('"').to_string();
    if targets.is_empty() {
        host.push_error("RT_PATH: nessuna immagine raster scelta.");
        return;
    }
    if !Path::new(&new_path).is_file() {
        host.push_error(&format!("RT_PATH: il file \"{new_path}\" non esiste."));
        return;
    }
    if !imgsize::is_raster_name(&new_path) {
        host.push_info("RT_PATH: attenzione, l'estensione non è di un formato raster riconosciuto.");
    }
    let groups = groups_for(host, Some(targets));
    if groups.is_empty() {
        host.push_error("RT_PATH: nessuna immagine raster tra quelle scelte.");
        return;
    }
    let dims = imgsize::read_dims(Path::new(&new_path)).ok();
    if dims.is_none() {
        host.push_info("RT_PATH: impossibile leggere le dimensioni in pixel del nuovo file; mantengo quelle attuali.");
    }
    let mut total = 0;
    for g in groups {
        total += relink_group(host, g, &new_path, dims);
    }
    host.push_info(&format!("RT_PATH: {total} immagine/i collegata/e a \"{new_path}\"."));
}

/// Point every image of `g` at `new_path`, keeping each one's world width
/// (and pixel aspect) when the new file's pixel count differs, then move the
/// shared definition so the change is saved with the drawing. Returns the
/// number of images updated.
fn relink_group(host: &mut dyn HostApi, g: Group, new_path: &str, dims: Option<(u32, u32)>) -> usize {
    let mut done = 0;
    host.begin_undo_group("RT_PATH");
    for mut img in g.images {
        img.file_path = new_path.to_string();
        if let Some((nw, nh)) = dims {
            let (ow, oh) = (img.size.x, img.size.y);
            let (nw, nh) = (nw as f64, nh as f64);
            if nw > 0.0 && nh > 0.0 && ow > 0.0 && oh > 0.0 && (nw != ow || nh != oh) {
                let k = ow / nw;
                img.u_vector = img.u_vector * k;
                img.v_vector = img.v_vector * k;
                img.size = Vector2::new(nw, nh);
                let verts = geom::rescale_clip(&clip_verts(&img), ow, oh, nw, nh);
                img.clip_boundary.vertices = verts.into_iter().map(|p| Vector2::new(p[0], p[1])).collect();
            }
        }
        if host.update_entity(EntityType::RasterImage(img)) {
            done += 1;
        }
    }
    host.end_undo_group();
    if done > 0 {
        host.set_dirty();
    }

    // The path saved in the DWG lives on the image definition. The host's
    // reference manager rewrites it; it matches definitions by file name,
    // with `?` standing for each space.
    match g.def_name {
        Some(def) => {
            let pattern = file_name_only(&def).replace(' ', "?");
            if pattern.is_empty() || !host.execute_command(&format!("XREF Path {pattern} {new_path}")) {
                host.push_info(&format!(
                    "RT_PATH: immagine aggiornata a video. Per salvare il nuovo percorso nel file \
                     esegui:  XREF Path {pattern} {new_path}"
                ));
            }
        }
        None => host.push_info(
            "RT_PATH: l'immagine non ha una definizione nel disegno; il percorso è aggiornato sull'entità.",
        ),
    }
    done
}

// ── Find moved files ────────────────────────────────────────────────────────

const FIND_MAX_DEPTH: usize = 8;
const FIND_MAX_FILES: usize = 50_000;

/// Raster files under `root`, by lower-case file name (shallowest wins).
fn index_folder(root: &Path, recurse: bool) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut seen = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            seen += 1;
            if seen > FIND_MAX_FILES {
                return out;
            }
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if recurse && depth < FIND_MAX_DEPTH {
                    queue.push_back((path, depth + 1));
                }
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if imgsize::is_raster_name(name) {
                    out.entry(name.to_lowercase()).or_insert(path);
                }
            }
        }
    }
    out
}

fn find_in_folder(host: &mut dyn HostApi, picked: &Path, recurse: bool) {
    let root = if picked.is_dir() { picked.to_path_buf() } else { picked.parent().map(Path::to_path_buf).unwrap_or_default() };
    if !root.is_dir() {
        host.push_error(&format!("RT_FIND: la cartella \"{}\" non esiste.", root.display()));
        return;
    }
    let dir = dwg_dir(host);
    let missing: Vec<Group> = groups_for(host, None)
        .into_iter()
        .filter(|g| resolve(&g.path, dir.as_deref()).is_none())
        .collect();
    if missing.is_empty() {
        host.push_info("RT_FIND: tutte le immagini del disegno hanno il file al suo posto.");
        return;
    }
    let index = index_folder(&root, recurse);
    let (mut fixed, mut not_found) = (0usize, Vec::new());
    for g in missing {
        let name = file_name_only(&g.path).to_lowercase();
        match index.get(&name) {
            Some(found) => {
                let p = found.to_string_lossy().to_string();
                let dims = imgsize::read_dims(found).ok();
                fixed += relink_group(host, g, &p, dims);
            }
            None => not_found.push(file_name_only(&g.path).to_string()),
        }
    }
    let mut msg = format!("RT_FIND: in \"{}\" ricollegate {fixed} immagine/i.", root.display());
    if !not_found.is_empty() {
        msg.push_str(&format!(" Non trovati: {}.", not_found.join(", ")));
    }
    host.push_info(&msg);
}
