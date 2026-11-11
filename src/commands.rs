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

/// The layer chosen in the RT_ATTACH dialog (names may hold spaces, so they
/// do not travel through the command line), and whether to create it.
static PENDING_LAYER: Mutex<Option<(String, bool)>> = Mutex::new(None);

pub fn set_pending_layer(name: String, create: bool) {
    *PENDING_LAYER.lock().unwrap_or_else(|e| e.into_inner()) = Some((name, create));
}

fn take_pending_layer() -> Option<(String, bool)> {
    PENDING_LAYER.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Layer suggested for raster images.
pub const RASTER_LAYER: &str = "RASTER";

/// The drawing's layer names, in table order.
fn layer_names(host: &dyn HostApi) -> Vec<String> {
    host.document().layers.iter().map(|l| l.name.clone()).collect()
}

/// The RGB colour of a layer (white when unknown — ACI 7 on a dark screen).
pub(crate) fn layer_rgb(host: &dyn HostApi, layer: &str) -> [u8; 3] {
    host.document()
        .layers
        .get(layer)
        .and_then(|l| l.color.rgb())
        .map(|(r, g, b)| [r, g, b])
        .unwrap_or([255, 255, 255])
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
        return tr!("nessuno", "none").into();
    }
    let kind = match img.clip_boundary.clip_type {
        ClipType::Rectangular => tr!("rettangolo", "rectangle"),
        ClipType::Polygonal => tr!("poligono", "polygon"),
    };
    match img.clip_boundary.clip_mode {
        ClipMode::Outside => kind.to_string(),
        ClipMode::Inside => tr!(format!("foro ({kind})"), format!("hole ({kind})")),
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
    // Follow a language change made in the CAD since the last command.
    crate::i18n::refresh();
    // Everything after the verb, case and spaces intact (paths) — or, for a
    // bare verb, the arguments a tool kept for it (`tools::deferred`).
    let mut rest = trimmed[verb.len()..].trim();
    let kept = if rest.is_empty() { tools::take_deferred(&verb) } else { None };
    if let Some(k) = kept.as_deref() {
        crate::diag::log(&format!("{verb}: kept arguments {k:?}"));
        rest = k;
    }
    let args: Vec<&str> = rest.split_whitespace().collect();

    match verb.as_str() {
        "RT_HELP" | "RT_" | "RT_HELLO" => help(host),
        "RT_LANG" => lang(host, rest),

        "RT_ATTACH" => {
            let unit = drawing_unit_index(host);
            let layers = layer_names(host);
            let current = match host.system_variable("CLAYER") {
                Some(HostSettingValue::Text(l)) if !l.trim().is_empty() => Some(l),
                _ => None,
            };
            host.start_interactive(Box::new(tools::AttachDialog::new(
                rest.trim_matches('"').to_string(),
                unit,
                layers,
                current,
            )));
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
        "RT_BACK" => {
            let sel = selected_images(host);
            if sel.is_empty() {
                start_pick(host, "RT_BACKAPPLY", tr!("portare dietro", "send behind"));
            } else {
                send_back(host, &sel, true);
            }
        }
        "RT_BACKAPPLY" => {
            if !args.is_empty() {
                let hs = parse_handles(args[0]);
                send_back(host, &hs, true);
            }
        }
        "RT_PURGE" => purge_start(host),
        "RT_PURGEAPPLY" => purge_apply(host),
        "RT_GUIDE" => host.start_interactive(Box::new(tools::GuideDialog::new())),
        "RT_DIAG" => diag_cmd(host, rest),

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
                start_pick(host, "RT_UNCLIPAPPLY", tr!("togliere il ritaglio", "remove the clip from"));
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
                host.push_error(tr!("RT_RESIZE: nel disegno non ci sono immagini raster.", "RT_RESIZE: the drawing has no raster images."));
            } else {
                let pre = single_selected(host);
                host.start_interactive(Box::new(tools::ResizeTool::new(images, pre)));
            }
        }
        "RT_ALIGN" | "RT_ROTATE" => {
            let images = tools::snapshots(host);
            if images.is_empty() {
                host.push_error(&tr!(
                    format!("{verb}: nel disegno non ci sono immagini raster."),
                    format!("{verb}: the drawing has no raster images.")
                ));
            } else {
                let pre = single_selected(host);
                if verb == "RT_ALIGN" {
                    host.start_interactive(Box::new(tools::AlignTool::new(images, pre)));
                } else {
                    host.start_interactive(Box::new(tools::RotateTool::new(images, pre)));
                }
            }
        }
        "RT_ALIGNAPPLY" => {
            if !args.is_empty() {
                align_apply(host, &args);
            }
        }
        "RT_ROTATEAPPLY" => {
            if !args.is_empty() {
                rotate_apply(host, &args);
            }
        }
        "RT_ROTATEDLG" => {
            if !args.is_empty() {
                let h = args.first().and_then(|t| parse_handle(t));
                let (cx, cy) = (num(args.get(1).copied()), num(args.get(2).copied()));
                match (h, cx, cy) {
                    (Some(h), Some(cx), Some(cy)) if fetch(host, h).is_some() => {
                        host.start_interactive(Box::new(tools::RotateDialog::new(h, [cx, cy])))
                    }
                    _ => host.push_error(tr!("Uso: RT_ROTATEDLG <handle> <x> <y>", "Usage: RT_ROTATEDLG <handle> <x> <y>")),
                }
            }
        }
        "RT_SCALEAPPLY" => {
            if !args.is_empty() {
                scale_apply(host, &args);
            }
        }

        "RT_TRANSP" => {
            let images = tools::snapshots(host);
            if images.is_empty() {
                host.push_error(tr!("Nel disegno non ci sono immagini raster.", "The drawing has no raster images."));
            } else {
                let pre = single_selected(host);
                host.start_interactive(Box::new(tools::TranspTool::new(images, pre)));
            }
        }
        "RT_TRANSPPICK" => {
            if !args.is_empty() {
                transp_pick(host, &args);
            }
        }
        "RT_TRANSPAPPLY" => {
            if !args.is_empty() {
                transp_apply(host, &args);
            }
        }
        "RT_TRANSPOFF" => {
            let sel = selected_images(host);
            if sel.is_empty() {
                start_pick(host, "RT_TRANSPOFFAPPLY", tr!("togliere la trasparenza", "remove transparency from"));
            } else {
                transp_off(host, &sel);
            }
        }
        "RT_TRANSPOFFAPPLY" => {
            if let Some(t) = args.first() {
                transp_off(host, &parse_handles(t));
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
                    start_pick(host, "RT_PATHDLG", tr!("cambiare il percorso", "change the path of"));
                } else {
                    start_path_dialog(host, sel);
                }
            } else if sel.is_empty() {
                // Chosen from the ribbon with nothing selected: keep the path
                // and ask which image it is for.
                set_pending_path(rest.to_string());
                start_pick(host, "RT_PATHAPPLY", tr!("collegare al nuovo file", "link to the new file"));
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
                    None => host.push_error(tr!("RT_PATH: nessun file scelto.", "RT_PATH: no file chosen.")),
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
                    None => host.push_error(tr!("RT_FIND: nessuna cartella scelta.", "RT_FIND: no folder chosen.")),
                }
            }
        }

        _ => {
            host.push_error(&tr!(format!("{verb}: comando sconosciuto. Digita RT_HELP per l'elenco."), format!("{verb}: unknown command. Type RT_HELP for the list.")));
        }
    }
    true
}

fn help(host: &mut dyn HostApi) {
    host.push_info(tr!(
        "Raster Tools — immagini GIF/JPG/PNG/BMP/TIFF/WebP.\n\
         RT_GUIDE: guida rapida in una finestra.\n\
         RT_ATTACH: inserisce un'immagine o il PDF di una mappa (a schermo, per larghezza, a DPI con scala, o 1 pixel = 1 unità).\n\
         RT_ALIGN: allinea l'immagine a due punti (sposta, ruota e scala).\n\
         RT_ROTATE: ruota con il mouse (centro, riferimento, nuova direzione) o di un angolo esatto (Invio).\n\
         RT_BACK: porta le immagini dietro al disegno.\n\
         RT_CLIP: ritaglia con un rettangolo (tiene l'interno; ripetuto, restringe).\n\
         RT_CLIPHOLE: nasconde la parte dentro il rettangolo.\n\
         RT_CUT: separa la parte nel rettangolo in una nuova immagine, spostabile.\n\
         RT_CLIPPOLY / RT_HOLEPOLY / RT_CUTPOLY: come sopra, con un poligono disegnato a clic (Invio chiude).\n\
         RT_CLIPPL / RT_HOLEPL / RT_CUTPL: come sopra, con una polilinea chiusa, un cerchio o un'ellisse esistenti.\n\
         RT_UNCLIP: toglie il ritaglio.\n\
         RT_TRANSP: rende trasparente un colore cliccato sull'immagine (crea una copia PNG).\n\
         RT_TRANSPOFF: toglie la trasparenza.\n\
         RT_RESIZE: ridimensiona trascinando un angolo (proporzionale).\n\
         RT_SIZE: larghezza, altezza, fattore di scala o risoluzione DPI.\n\
         RT_PATH: cambia il file su disco dell'immagine.\n\
         RT_FIND: cerca in una cartella le immagini il cui file manca.\n\
         RT_LIST: elenco immagini con percorso, pixel, dimensioni e stato.\n\
         RT_PURGE: sposta in una sottocartella le copie trasparenti non usate.\n\
         RT_LANG IT|EN|AUTO: lingua del plugin (AUTO segue Open CAD Studio).\n\
         RT_DIAG ON|OFF: registro diagnostico.",
        "Raster Tools — GIF/JPG/PNG/BMP/TIFF/WebP images.\n\
         RT_GUIDE: quick guide in a dialog.\n\
         RT_ATTACH: insert an image or a map PDF (on screen, by width, by DPI with scale, or 1 pixel = 1 unit).\n\
         RT_ALIGN: align the image to two points (move, turn and scale).\n\
         RT_ROTATE: rotate by mouse (centre, reference, new direction) or by an exact angle (Enter).\n\
         RT_BACK: send images behind the drawing.\n\
         RT_CLIP: clip with a rectangle (keeps the inside; repeated, it narrows).\n\
         RT_CLIPHOLE: hide the part inside the rectangle.\n\
         RT_CUT: split the part inside the rectangle off into a new, movable image.\n\
         RT_CLIPPOLY / RT_HOLEPOLY / RT_CUTPOLY: as above, with a polygon drawn by clicks (Enter closes it).\n\
         RT_CLIPPL / RT_HOLEPL / RT_CUTPL: as above, with an existing closed polyline, circle or ellipse.\n\
         RT_UNCLIP: remove the clip.\n\
         RT_TRANSP: make a colour clicked on the image transparent (creates a PNG copy).\n\
         RT_TRANSPOFF: turn transparency off.\n\
         RT_RESIZE: resize by dragging a corner (proportional).\n\
         RT_SIZE: width, height, scale factor or DPI resolution.\n\
         RT_PATH: change the image's file on disk.\n\
         RT_FIND: search a folder for images whose file is missing.\n\
         RT_LIST: list images with path, pixels, size and status.\n\
         RT_PURGE: move unused transparent copies to a subfolder.\n\
         RT_LANG IT|EN|AUTO: plugin language (AUTO follows Open CAD Studio).\n\
         RT_DIAG ON|OFF: diagnostic log."
    ));
}

fn lang(host: &mut dyn HostApi, arg: &str) {
    use crate::i18n::{self, Choice, Lang};
    if arg.is_empty() {
        let (c, l) = i18n::status();
        let mode = match c {
            Choice::Auto => tr!("AUTO (segue Open CAD Studio)", "AUTO (follows Open CAD Studio)"),
            Choice::It => "IT",
            Choice::En => "EN",
        };
        let now = if l == Lang::It { "italiano" } else { "English" };
        host.push_info(&tr!(
            format!("RT_LANG: impostazione {mode}, lingua attuale: {now}. Uso: RT_LANG IT | EN | AUTO"),
            format!("RT_LANG: setting {mode}, current language: {now}. Usage: RT_LANG IT | EN | AUTO")
        ));
        return;
    }
    let Some(choice) = i18n::parse_choice(arg) else {
        host.push_error(tr!("Uso: RT_LANG IT | EN | AUTO", "Usage: RT_LANG IT | EN | AUTO"));
        return;
    };
    match i18n::set_choice(choice) {
        Ok(()) => {
            let auto = choice == Choice::Auto;
            host.push_info(tr!(
                if auto {
                    "RT_LANG: AUTO, segue la lingua di Open CAD Studio (ora italiano). Le etichette della barra cambiano al prossimo avvio."
                } else {
                    "RT_LANG: lingua impostata su italiano. Le etichette della barra cambiano al prossimo avvio di Open CAD Studio."
                },
                if auto {
                    "RT_LANG: AUTO, follows Open CAD Studio's language (now English). Ribbon labels change at the next start."
                } else {
                    "RT_LANG: language set to English. Ribbon labels change the next time Open CAD Studio starts."
                }
            ));
        }
        Err(e) => host.push_error(&tr!(
            format!("RT_LANG: impossibile salvare l'impostazione: {e}"),
            format!("RT_LANG: cannot save the setting: {e}")
        )),
    }
}

fn single_selected(host: &dyn HostApi) -> Option<Handle> {
    let sel = selected_images(host);
    (sel.len() == 1).then(|| sel[0])
}

fn start_poly(host: &mut dyn HostApi, kind: tools::ClipKind) {
    let images = tools::snapshots(host);
    if images.is_empty() {
        host.push_error(tr!("Nel disegno non ci sono immagini raster.", "The drawing has no raster images."));
        return;
    }
    let pre = single_selected(host);
    host.start_interactive(Box::new(tools::PolyClipTool::new(images, kind, pre)));
}

fn start_pline(host: &mut dyn HostApi, kind: tools::ClipKind) {
    let images = tools::snapshots(host);
    if images.is_empty() {
        host.push_error(tr!("Nel disegno non ci sono immagini raster.", "The drawing has no raster images."));
        return;
    }
    let shapes = closed_shapes(host);
    if shapes.is_empty() {
        host.push_error(&tr!(
            format!(
                "{}: nel disegno non ci sono polilinee chiuse, cerchi o ellissi da usare come contorno.",
                kind.pline_name()
            ),
            format!("{}: the drawing has no closed polylines, circles or ellipses to use as an outline.", kind.pline_name())
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
        host.push_error(tr!("Nel disegno non ci sono immagini raster.", "The drawing has no raster images."));
        return;
    }
    host.start_interactive(Box::new(tools::PickTool::new(images, verb, what)));
}

fn start_clip(host: &mut dyn HostApi, kind: tools::ClipKind) {
    let images = tools::snapshots(host);
    if images.is_empty() {
        host.push_error(tr!("Nel disegno non ci sono immagini raster.", "The drawing has no raster images."));
        return;
    }
    let pre = single_selected(host);
    host.start_interactive(Box::new(tools::ClipTool::new(images, kind, pre)));
}

fn start_size_dialog(host: &mut dyn HostApi, handles: Vec<Handle>) {
    let first = handles.iter().find_map(|h| fetch(host, *h));
    let Some(first) = first else {
        host.push_error(tr!("RT_SIZE: nessuna immagine raster valida.", "RT_SIZE: no valid raster image."));
        return;
    };
    host.start_interactive(Box::new(tools::SizeDialog::new(handles, &first)));
}

fn start_path_dialog(host: &mut dyn HostApi, handles: Vec<Handle>) {
    let Some(first) = handles.iter().find_map(|h| fetch(host, *h)) else {
        host.push_error(tr!("RT_PATH: nessuna immagine raster valida.", "RT_PATH: no valid raster image."));
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
        host.push_error(tr!("RT_ATTACH: nessun file scelto.", "RT_ATTACH: no file chosen."));
        return;
    };
    if !Path::new(&path).is_file() {
        host.push_error(&tr!(format!("RT_ATTACH: il file \"{path}\" non esiste."), format!("RT_ATTACH: the file \"{path}\" does not exist.")));
        return;
    }
    // A PDF (e.g. a cadastral map extract): its picture becomes an image file.
    let mut pdf_note = String::new();
    let path = if is_pdf(&path) {
        match pdf_to_image(host, Path::new(&path)) {
            Ok((out, dpi)) => {
                let name = file_name_only(&out).to_string();
                pdf_note = match dpi {
                    Some(d) => tr!(
                        format!(" Dal PDF: {name}, risoluzione {} DPI.", fmt_len(d)),
                        format!(" From the PDF: {name}, resolution {} DPI.", fmt_len(d))
                    ),
                    None => tr!(format!(" Dal PDF: {name}."), format!(" From the PDF: {name}.")),
                };
                out
            }
            Err(e) => {
                host.push_error(&tr!(
                    format!("RT_ATTACH: dal PDF \"{}\" non si ricava un'immagine: {e}.", file_name_only(&path)),
                    format!("RT_ATTACH: no picture can be taken from the PDF \"{}\": {e}.", file_name_only(&path))
                ));
                return;
            }
        }
    } else {
        path
    };
    let p = Path::new(&path);
    let (w, h) = match imgsize::read_dims(p) {
        Ok(d) if d.0 > 0 && d.1 > 0 => d,
        _ => {
            host.push_error(&tr!(
                format!(
                    "RT_ATTACH: \"{}\" non è un'immagine GIF, JPG, PNG, BMP, TIFF o WebP leggibile.",
                    file_name_only(&path)
                ),
                format!("RT_ATTACH: \"{}\" is not a readable GIF, JPG, PNG, BMP, TIFF or WebP image.", file_name_only(&path))
            ));
            return;
        }
    };
    let mode = args[0].to_ascii_uppercase();
    let width = num(args.get(1).copied());
    let dpi = num(args.get(2).copied());
    let unit = args.get(3).copied().unwrap_or("MM");
    // map scale 1:N, for DPI mode (a map at 1:2000 printed at 300 DPI)
    let map_scale = num(args.get(4).copied()).filter(|k| *k > 0.0).unwrap_or(1.0);
    let px = match mode.as_str() {
        "SCREEN" => None,
        "PIXEL" => Some(1.0),
        "WIDTH" => match width {
            Some(v) if v > 0.0 => Some(v / w as f64),
            _ => {
                host.push_error(tr!("RT_ATTACH: indica una larghezza maggiore di zero.", "RT_ATTACH: enter a width greater than zero."));
                return;
            }
        },
        "DPI" => match (dpi, geom::units_per_inch(unit)) {
            (Some(d), Some(upi)) if d > 0.0 => Some(upi / d * map_scale),
            _ => {
                host.push_error(tr!("RT_ATTACH: indica un valore DPI maggiore di zero.", "RT_ATTACH: enter a DPI value greater than zero."));
                return;
            }
        },
        _ => {
            host.push_error(tr!("Uso: RT_ATTACHPLACE SCREEN|WIDTH|DPI|PIXEL <larghezza> <dpi> MM|CM|M|IN", "Usage: RT_ATTACHPLACE SCREEN|WIDTH|DPI|PIXEL <width> <dpi> MM|CM|M|IN"));
            return;
        }
    };
    if let Some(px) = px {
        let (name, ww, hh) = (file_name_only(&path), fmt_len(w as f64 * px), fmt_len(h as f64 * px));
        host.push_info(&tr!(
            format!("RT_ATTACH: {name} — {w}×{h} px, {ww} × {hh} unità.{pdf_note}"),
            format!("RT_ATTACH: {name} — {w}×{h} px, {ww} × {hh} units.{pdf_note}")
        ));
    } else if !pdf_note.is_empty() {
        host.push_info(&format!("RT_ATTACH:{pdf_note}"));
    }
    host.start_interactive(Box::new(tools::AttachTool::new(path, w, h, px)));
}

/// `RT_ATTACHAPPLY <x> <y> <z> <pixel_size>` — insert the pending file with
/// its bottom-left corner at x,y,z.
fn attach_apply(host: &mut dyn HostApi, args: &[&str]) {
    let vals: Option<Vec<f64>> = (0..4).map(|i| num(args.get(i).copied())).collect();
    let (Some(v), Some(path)) = (vals, take_pending_path()) else {
        host.push_error(tr!("Uso: RT_ATTACHAPPLY <x> <y> <z> <dimensione_pixel> (dopo la scelta del file)", "Usage: RT_ATTACHAPPLY <x> <y> <z> <pixel_size> (after choosing the file)"));
        return;
    };
    let px = v[3];
    if !valid_factor(px) {
        host.push_error(tr!("RT_ATTACH: dimensione non valida.", "RT_ATTACH: invalid size."));
        return;
    }
    let (w, h) = match imgsize::read_dims(Path::new(&path)) {
        Ok(d) => d,
        Err(e) => {
            host.push_error(&tr!(format!("RT_ATTACH: impossibile leggere \"{path}\": {e}"), format!("RT_ATTACH: cannot read \"{path}\": {e}")));
            return;
        }
    };
    let mut img = RasterImage::new(&path, Vector3::new(v[0], v[1], v[2]), w as f64, h as f64);
    img.u_vector = Vector3::new(px, 0.0, 0.0);
    img.v_vector = Vector3::new(0.0, px, 0.0);
    host.begin_undo_group("RT_ATTACH");
    let mut created = false;
    let mut assigned = false;
    if let Some((name, create)) = take_pending_layer() {
        let exists = host.document().layers.get(&name).is_some();
        if create && !exists {
            let config = ocs_plugin_api::host::LayerConfig {
                name: name.clone(),
                color: Some(codec::types::Color::Index(7)),
                ..Default::default()
            };
            created = host.add_layer(config).is_some();
            if !created {
                host.push_error(&tr!(
                    format!("RT_ATTACH: impossibile creare il layer \"{name}\"; l'immagine va sul layer corrente."),
                    format!("RT_ATTACH: cannot create layer \"{name}\"; the image goes on the current layer.")
                ));
            }
        }
        if exists || created {
            img.common.layer = name;
            assigned = true;
        }
    }
    if !assigned {
        if let Some(HostSettingValue::Text(layer)) = host.system_variable("CLAYER") {
            if !layer.trim().is_empty() {
                img.common.layer = layer;
            }
        }
    }
    let handle = host.add_entity(EntityType::RasterImage(img));
    host.end_undo_group();
    if handle.is_null() {
        let name = file_name_only(&path);
        host.push_error(&tr!(
            format!("RT_ATTACH: il CAD non è riuscito a inserire \"{name}\" (file non leggibile o layer bloccato)."),
            format!("RT_ATTACH: the CAD could not insert \"{name}\" (unreadable file or locked layer).")
        ));
        return;
    }
    host.set_dirty();
    // a scan goes behind the lines already drawn
    let behind = send_back(host, &[handle], false);
    let _ = host.set_selection(&[handle]);
    let (name, ww, hh) = (file_name_only(&path), fmt_len(w as f64 * px), fmt_len(h as f64 * px));
    let layer = host
        .document()
        .get_entity(handle)
        .map(|e| e.common().layer.clone())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "0".into());
    host.push_info(&tr!(
        format!(
            "RT_ATTACH: inserita {name} ({w}×{h} px, {ww} × {hh} unità) sul layer {layer}{new}{back}.",
            new = if created { " (creato)" } else { "" },
            back = if behind { ", dietro al disegno" } else { "" }
        ),
        format!(
            "RT_ATTACH: inserted {name} ({w}×{h} px, {ww} × {hh} units) on layer {layer}{new}{back}.",
            new = if created { " (new)" } else { "" },
            back = if behind { ", behind the drawing" } else { "" }
        )
    ));
}

// ── PDF, draw order, clean-up, diagnostics ──────────────────────────────────

pub fn is_pdf(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Take the picture out of a PDF into an image file next to it:
/// `<name>_pdf.png` (or `.jpg` when the PDF holds a JPEG, copied as is).
/// An existing copy newer than the PDF is reused. Returns the image path and
/// the resolution when the picture fills the page.
fn pdf_to_image(host: &mut dyn HostApi, pdf: &Path) -> Result<(String, Option<f64>), String> {
    let data = std::fs::read(pdf).map_err(|e| e.to_string())?;
    let found = crate::pdf::largest_picture(&data)?;
    let dir = pdf.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = pdf.file_stem().and_then(|s| s.to_str()).unwrap_or("pdf");
    let pdf_time = std::fs::metadata(pdf).and_then(|m| m.modified()).ok();
    let fresh = |p: &Path| {
        let t = std::fs::metadata(p).and_then(|m| m.modified()).ok();
        matches!((t, pdf_time), (Some(t), Some(pt)) if t >= pt)
    };
    match found {
        crate::pdf::Found::Jpeg { data, dpi, .. } => {
            let out = dir.join(format!("{stem}_pdf.jpg"));
            if !fresh(&out) {
                std::fs::write(&out, &data).map_err(|e| e.to_string())?;
            }
            Ok((out.to_string_lossy().into_owned(), dpi))
        }
        crate::pdf::Found::Raw(p) => {
            let out = dir.join(format!("{stem}_pdf.png"));
            if fresh(&out) {
                return Ok((out.to_string_lossy().into_owned(), p.dpi));
            }
            crate::diag::log(&format!("PDF {}: {}x{} dpi {:?}", pdf.display(), p.w, p.h, p.dpi));
            let grey = p.rgba.chunks_exact(4).all(|c| c[0] == c[1] && c[1] == c[2]);
            let saved = if grey {
                let l: Vec<u8> = p.rgba.chunks_exact(4).map(|c| c[0]).collect();
                save_png_as(host, &out, p.w, p.h, &l, image::ExtendedColorType::L8)
            } else {
                save_png(host, &out, p.w, p.h, &p.rgba)
            };
            saved.map(|_| (out.to_string_lossy().into_owned(), p.dpi))
        }
    }
}

/// Put images behind everything else (the host's `DRAWORDER BACK` on them),
/// keeping the selection as it was. `report`: say so on the command line.
fn send_back(host: &mut dyn HostApi, handles: &[Handle], report: bool) -> bool {
    if handles.is_empty() {
        return false;
    }
    let before = host.selection();
    let ok = host.set_selection(handles).is_ok() && host.execute_command("DRAWORDER BACK");
    let _ = host.set_selection(&before);
    if report {
        if ok {
            host.push_info(&tr!(
                format!("RT_BACK: {} immagine/i portate dietro al disegno.", handles.len()),
                format!("RT_BACK: {} image(s) sent behind the drawing.", handles.len())
            ));
        } else {
            host.push_error(tr!(
                "RT_BACK: Open CAD Studio non ha accettato DRAWORDER (forse i comandi da script sono disattivati, SCRIPTCOMMANDS). Seleziona l'immagine e usa DRAWORDER › Dietro.",
                "RT_BACK: Open CAD Studio did not accept DRAWORDER (scripted commands may be off, SCRIPTCOMMANDS). Select the image and use DRAWORDER › Back."
            ));
        }
    }
    ok
}

/// Copies left by RT_TRANSP that no image of this drawing uses, waiting for
/// RT_PURGEAPPLY.
static PURGE_LIST: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// `<stem>_trasp-<n>.png` files in the folders of this drawing's images
/// that none of its images uses.
pub fn unused_copies(host: &dyn HostApi) -> Vec<PathBuf> {
    let dir = dwg_dir(host);
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").replace("\\.\\", "\\").to_lowercase();
    let mut used = std::collections::HashSet::new();
    let mut folders = std::collections::BTreeSet::new();
    for img in all_images(host) {
        if let Some(p) = resolve(&img.file_path, dir.as_deref()) {
            used.insert(norm(&p));
            if let Some(f) = p.parent() {
                folders.insert(f.to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    for f in folders {
        let Ok(rd) = std::fs::read_dir(&f) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let is_copy = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png"))
                && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| crate::pixels::base_stem(s) != s);
            if is_copy && p.is_file() && !used.contains(&norm(&p)) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn purge_start(host: &mut dyn HostApi) {
    let list = unused_copies(host);
    if list.is_empty() {
        host.push_info(tr!(
            "RT_PURGE: nessuna copia trasparente inutilizzata nelle cartelle delle immagini di questo disegno.",
            "RT_PURGE: no unused transparent copies in the folders of this drawing's images."
        ));
        return;
    }
    let names: Vec<String> = list.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    *PURGE_LIST.lock().unwrap_or_else(|e| e.into_inner()) = list;
    host.start_interactive(Box::new(tools::PurgeDialog::new(names)));
}

/// Move the listed copies into a `_trasp-non-usate` subfolder (not deleted:
/// another drawing may still use one).
fn purge_apply(host: &mut dyn HostApi) {
    let list = std::mem::take(&mut *PURGE_LIST.lock().unwrap_or_else(|e| e.into_inner()));
    if list.is_empty() {
        return;
    }
    let (mut moved, mut failed) = (0, Vec::new());
    let mut where_to = std::collections::BTreeSet::new();
    for p in &list {
        let Some(dir) = p.parent() else { continue };
        let bin = dir.join("_trasp-non-usate");
        let Some(name) = p.file_name() else { continue };
        let ok = std::fs::create_dir_all(&bin).is_ok() && std::fs::rename(p, bin.join(name)).is_ok();
        if ok {
            moved += 1;
            where_to.insert(bin.to_string_lossy().into_owned());
        } else {
            failed.push(file_name_only(&p.to_string_lossy()).to_string());
        }
    }
    let folders = where_to.into_iter().collect::<Vec<_>>().join(", ");
    if moved > 0 {
        host.push_info(&tr!(
            format!("RT_PURGE: {moved} copie spostate in {folders}. Controlla che nessun altro disegno le usi, poi puoi cancellare la cartella."),
            format!("RT_PURGE: {moved} copies moved to {folders}. Check that no other drawing uses them, then you can delete the folder.")
        ));
    }
    if !failed.is_empty() {
        host.push_error(&tr!(
            format!("RT_PURGE: non spostate (in uso o protette): {}.", failed.join(", ")),
            format!("RT_PURGE: not moved (in use or protected): {}.", failed.join(", "))
        ));
    }
}

fn diag_cmd(host: &mut dyn HostApi, arg: &str) {
    let path = crate::diag::path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let want = match arg.trim().to_ascii_uppercase().as_str() {
        "" => None,
        "ON" | "SI" | "SÌ" | "1" => Some(true),
        "OFF" | "NO" | "0" => Some(false),
        _ => {
            host.push_error(tr!("Uso: RT_DIAG ON | OFF", "Usage: RT_DIAG ON | OFF"));
            return;
        }
    };
    if let Some(on) = want {
        if let Err(e) = crate::diag::set_enabled(on) {
            host.push_error(&format!("RT_DIAG: {e}"));
            return;
        }
    }
    let on = crate::diag::enabled();
    host.push_info(&if on {
        tr!(format!("RT_DIAG: registro diagnostico ATTIVO, in {path}."), format!("RT_DIAG: diagnostic log ON, in {path}."))
    } else {
        tr!(
            "RT_DIAG: registro diagnostico spento (RT_DIAG ON per attivarlo).".to_string(),
            "RT_DIAG: diagnostic log off (RT_DIAG ON to turn it on).".to_string()
        )
    });
}

// ── RT_TRANSP ───────────────────────────────────────────────────────────────

/// The last picture decoded, so picking a colour and applying it decode the
/// file once: (path, modified time, width, height, RGBA8 pixels).
type Decoded = (PathBuf, Option<std::time::SystemTime>, u32, u32, Vec<u8>);
static DECODED: Mutex<Option<Decoded>> = Mutex::new(None);

/// RGBA8 pixels of the file behind an image entity. `keep`: leave a copy in
/// the cache for the next step (picking keeps it, applying takes it — a scan
/// of 70 megapixels is 280 MB, not worth holding twice).
fn decode_rgba(path: &Path, keep: bool) -> Result<(u32, u32, Vec<u8>), String> {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    {
        let mut cache = DECODED.lock().unwrap_or_else(|e| e.into_inner());
        let hit = cache.as_ref().is_some_and(|(p, t, ..)| p == path && *t == mtime);
        if hit {
            if keep {
                let (_, _, w, h, px) = cache.as_ref().expect("hit");
                return Ok((*w, *h, px.clone()));
            }
            let (_, _, w, h, px) = cache.take().expect("hit");
            return Ok((w, h, px));
        }
        *cache = None;
    }
    let rgba = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = rgba.dimensions();
    let px = rgba.into_raw();
    if keep {
        *DECODED.lock().unwrap_or_else(|e| e.into_inner()) = Some((path.to_path_buf(), mtime, w, h, px.clone()));
    }
    Ok((w, h, px))
}

/// A writer that, while a large PNG is being written, pings the host every
/// few seconds: the host gives up on a plugin call that stays silent for
/// 30 seconds, and any request from the plugin restarts that clock.
struct Heartbeat<'b, 'h, W: std::io::Write> {
    inner: W,
    host: &'b mut (dyn HostApi + 'h),
    last: std::time::Instant,
}

impl<W: std::io::Write> std::io::Write for Heartbeat<'_, '_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.last.elapsed() >= std::time::Duration::from_secs(3) {
            let _ = self.host.selection();
            self.last = std::time::Instant::now();
        }
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Save RGBA8 pixels as a PNG, with fast compression (a 70-megapixel scan
/// takes seconds instead of minutes) and the host kept informed.
fn save_png(host: &mut dyn HostApi, out: &Path, w: u32, h: u32, px: &[u8]) -> Result<(), String> {
    save_png_as(host, out, w, h, px, image::ExtendedColorType::Rgba8)
}

/// [`save_png`] for any colour layout (`L8` for greyscale pictures).
fn save_png_as(
    host: &mut dyn HostApi,
    out: &Path,
    w: u32,
    h: u32,
    px: &[u8],
    colour: image::ExtendedColorType,
) -> Result<(), String> {
    use image::ImageEncoder;
    let file = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut writer = Heartbeat { inner: std::io::BufWriter::with_capacity(1 << 20, file), host, last: std::time::Instant::now() };
    let result = image::codecs::png::PngEncoder::new_with_quality(
        &mut writer,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Up,
    )
    .write_image(px, w, h, colour)
    .map_err(|e| e.to_string())
    .and_then(|_| std::io::Write::flush(&mut writer).map_err(|e| e.to_string()));
    if result.is_err() {
        let _ = std::fs::remove_file(out);
    }
    result
}

/// The image entity and the file it shows, or an error already reported.
fn transp_source(host: &mut dyn HostApi, h: Handle) -> Option<(RasterImage, PathBuf)> {
    let Some(img) = fetch(host, h) else {
        host.push_error(tr!("RT_TRANSP: l'entità non è un'immagine raster.", "RT_TRANSP: the entity is not a raster image."));
        return None;
    };
    match resolve(&img.file_path, dwg_dir(host).as_deref()) {
        Some(p) => Some((img, p)),
        None => {
            let path = img.file_path.clone();
            crate::diag::log(&format!("  file not found: {path:?}"));
            host.push_error(&tr!(
                format!("RT_TRANSP: file non trovato: \"{path}\". Ricollegalo prima con RT_PATH o RT_FIND."),
                format!("RT_TRANSP: file not found: \"{path}\". Relink it first with RT_PATH or RT_FIND.")
            ));
            None
        }
    }
}

fn decode_or_report(host: &mut dyn HostApi, path: &Path, keep: bool) -> Option<(u32, u32, Vec<u8>)> {
    match decode_rgba(path, keep) {
        Ok(d) => Some(d),
        Err(e) => {
            crate::diag::log(&format!("  decode error: {e}"));
            let name = file_name_only(&path.to_string_lossy()).to_string();
            host.push_error(&tr!(
                format!("RT_TRANSP: impossibile leggere \"{name}\": {e}"),
                format!("RT_TRANSP: cannot read \"{name}\": {e}")
            ));
            None
        }
    }
}

/// `RT_TRANSPPICK <handle> <a> <b>` — frame pixel coordinates of the click:
/// read the colour there and open the tolerance dialog.
fn transp_pick(host: &mut dyn HostApi, args: &[&str]) {
    let (Some(h), Some(a), Some(b)) =
        (args.first().and_then(|t| parse_handle(t)), num(args.get(1).copied()), num(args.get(2).copied()))
    else {
        host.push_error(tr!("Uso: RT_TRANSPPICK <handle> <x_px> <y_px>", "Usage: RT_TRANSPPICK <handle> <x_px> <y_px>"));
        return;
    };
    crate::diag::log(&format!("RT_TRANSPPICK {args:?}"));
    let Some((img, path)) = transp_source(host, h) else { return };
    let Some((w, hh, mut px)) = decode_or_report(host, &path, true) else { return };
    crate::diag::log(&format!("  decoded {w}x{hh} from {}", path.display()));
    let Some((x, y)) = crate::pixels::file_pixel(a, b, img.size.x, img.size.y, w, hh) else {
        host.push_error(tr!("RT_TRANSP: il punto è fuori dall'immagine.", "RT_TRANSP: the point is outside the image."));
        return;
    };
    let pic = crate::pixels::Rgba { w, h: hh, px: &mut px };
    let c = pic.get(x, y);
    if c[3] == 0 {
        host.push_error(tr!(
            "RT_TRANSP: in quel punto l'immagine è già trasparente.",
            "RT_TRANSP: the image is already transparent at that point."
        ));
        return;
    }
    let lossy = matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("jpg" | "jpeg" | "jpe" | "webp")
    );
    let layer = if img.common.layer.is_empty() { "0".to_string() } else { img.common.layer.clone() };
    let ink = layer_rgb(host, &layer);
    crate::diag::log(&format!("  pixel ({x},{y}) = {c:?}; layer {layer} {ink:?}; opening the dialog"));
    host.start_interactive(Box::new(tools::TranspDialog::new(h, x, y, [c[0], c[1], c[2]], lossy, layer, ink)));
}

/// `RT_TRANSPAPPLY <handle> <x> <y> <tolerance_%> ALL|AREA [TINT]` — file
/// pixel coordinates of the chosen colour. `TINT`: everything else takes the
/// colour of the image's layer, with soft edges (`pixels::tint_soft`).
fn transp_apply(host: &mut dyn HostApi, args: &[&str]) {
    let usage = tr!(
        "Uso: RT_TRANSPAPPLY <handle> <x> <y> <tolleranza_%> ALL|AREA [TINT]",
        "Usage: RT_TRANSPAPPLY <handle> <x> <y> <tolerance_%> ALL|AREA [TINT]"
    );
    let h = args.first().and_then(|t| parse_handle(t));
    let (x, y, tol) = (num(args.get(1).copied()), num(args.get(2).copied()), num(args.get(3).copied()));
    let (Some(h), Some(x), Some(y), Some(tol)) = (h, x, y, tol) else {
        host.push_error(usage);
        return;
    };
    let area = args.get(4).is_some_and(|m| m.eq_ignore_ascii_case("AREA"));
    let tint = args.get(5).is_some_and(|m| m.eq_ignore_ascii_case("TINT"));
    crate::diag::log(&format!("RT_TRANSPAPPLY {args:?}"));
    let Some((img, path)) = transp_source(host, h) else { return };
    let layer = if img.common.layer.is_empty() { "0".to_string() } else { img.common.layer.clone() };
    let ink = layer_rgb(host, &layer);
    let big = |w: u32, h: u32| w as u64 * h as u64 >= 8_000_000;
    let Some((w, hh, mut px)) = decode_or_report(host, &path, false) else { return };
    if x < 0.0 || y < 0.0 || x >= w as f64 || y >= hh as f64 {
        host.push_error(tr!("RT_TRANSP: il punto è fuori dall'immagine.", "RT_TRANSP: the point is outside the image."));
        return;
    }
    let (x, y) = (x as u32, y as u32);
    let mut pic = crate::pixels::Rgba { w, h: hh, px: &mut px };
    let c = pic.get(x, y);
    let key = [c[0], c[1], c[2]];
    let tol8 = crate::pixels::tol_from_percent(tol);
    let changed = if tint {
        crate::pixels::tint_soft(&mut pic, key, tol8, ink)
    } else {
        crate::pixels::make_transparent(&mut pic, key, tol8, area.then_some((x, y)))
    };
    crate::diag::log(&format!("  {w}x{hh}: {changed} pixels close to {key:?}"));
    if changed == 0 {
        host.push_info(tr!(
            "RT_TRANSP: nessun pixel di quel colore da rendere trasparente.",
            "RT_TRANSP: no pixels of that colour to make transparent."
        ));
        return;
    }
    let out = crate::pixels::transparent_copy_path(&path);
    if big(w, hh) {
        let mpx = w as f64 * hh as f64 / 1e6;
        host.push_info(&tr!(
            format!("RT_TRANSP: salvataggio della copia PNG ({w}×{hh} px, {mpx:.0} megapixel), attendere…"),
            format!("RT_TRANSP: saving the PNG copy ({w}×{hh} px, {mpx:.0} megapixels), please wait…")
        ));
    }
    crate::diag::log(&format!("  saving {}", out.display()));
    let saved = save_png(host, &out, w, hh, &px);
    drop(px);
    crate::diag::log(&format!("  saved: {saved:?}"));
    if let Err(e) = saved {
        let dir = out.parent().map(|d| d.display().to_string()).unwrap_or_default();
        host.push_error(&tr!(
            format!("RT_TRANSP: impossibile salvare la copia trasparente in \"{dir}\": {e}"),
            format!("RT_TRANSP: cannot save the transparent copy in \"{dir}\": {e}")
        ));
        return;
    }
    let out_s = out.to_string_lossy().to_string();
    let mut targets = images_showing(host, &path);
    if !targets.contains(&h) {
        targets.push(h);
    }
    let groups = groups_for(host, Some(&targets));
    let mut done = 0;
    for g in groups {
        done += relink_group(host, g, &out_s, Some((w, hh)), "RT_TRANSP", true);
    }
    crate::diag::log(&format!("  relinked {done} image(s)"));
    let pct = changed as f64 * 100.0 / (w as f64 * hh as f64);
    let (hex, name) = (crate::pixels::hex_color(key), file_name_only(&out_s).to_string());
    if tint {
        let ink_hex = crate::pixels::hex_color(ink);
        host.push_info(&tr!(
            format!(
                "RT_TRANSP: colore {hex} reso trasparente ({changed} pixel, {pct:.1}%), il resto colorato \
                 come il layer {layer} ({ink_hex}), su {done} immagine/i. Copia salvata: {name} \
                 (l'originale non è stato modificato)."
            ),
            format!(
                "RT_TRANSP: colour {hex} made transparent ({changed} pixels, {pct:.1}%), the rest coloured \
                 like layer {layer} ({ink_hex}), on {done} image(s). Copy saved: {name} \
                 (the original is unchanged)."
            )
        ));
        return;
    }
    host.push_info(&tr!(
        format!(
            "RT_TRANSP: colore {hex} reso trasparente ({changed} pixel, {pct:.1}%) su {done} immagine/i. \
             Copia salvata: {name} (l'originale non è stato modificato)."
        ),
        format!(
            "RT_TRANSP: colour {hex} made transparent ({changed} pixels, {pct:.1}%) on {done} image(s). \
             Copy saved: {name} (the original is unchanged)."
        )
    ));
}

/// Every image whose file resolves to `path` — copies inserted several
/// times may each have their own definition, and an opaque copy stacked on
/// the same spot would hide the transparency.
fn images_showing(host: &dyn HostApi, path: &Path) -> Vec<Handle> {
    let dir = dwg_dir(host);
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").replace("\\.\\", "\\").to_lowercase();
    let want = norm(path);
    host.document()
        .entities()
        .filter_map(|e| match e {
            EntityType::RasterImage(i) => Some(i),
            _ => None,
        })
        .filter(|i| resolve(&i.file_path, dir.as_deref()).is_some_and(|p| norm(&p) == want))
        .map(|i| i.common.handle)
        .collect()
}

/// Turn transparency off (the copy keeps its colours, so the picture shows
/// as it was).
fn transp_off(host: &mut dyn HostApi, handles: &[Handle]) {
    let mut edits = Vec::new();
    for h in handles {
        if let Some(mut img) = fetch(host, *h) {
            if img.flags.contains(ImageDisplayFlags::TRANSPARENCY_ON) {
                img.flags &= !ImageDisplayFlags::TRANSPARENCY_ON;
                edits.push(img);
            }
        }
    }
    if edits.is_empty() {
        host.push_info(tr!(
            "RT_TRANSPOFF: nessuna immagine con trasparenza attiva tra quelle scelte.",
            "RT_TRANSPOFF: none of the chosen images has transparency on."
        ));
        return;
    }
    host.begin_undo_group("RT_TRANSPOFF");
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
    host.push_info(&tr!(
        format!("RT_TRANSPOFF: trasparenza tolta da {done} immagine/i."),
        format!("RT_TRANSPOFF: transparency turned off on {done} image(s).")
    ));
}

// ── RT_LIST ─────────────────────────────────────────────────────────────────

fn list(host: &mut dyn HostApi) {
    let images = all_images(host);
    if images.is_empty() {
        host.push_info(tr!("RT_LIST: nel disegno non ci sono immagini raster.", "RT_LIST: the drawing has no raster images."));
        return;
    }
    let dir = dwg_dir(host);
    let mut missing = 0;
    let mut lines = Vec::with_capacity(images.len() * 2 + 1);
    for img in &images {
        let f = frame(img);
        let (w, h) = f.world_size();
        let state = match resolve(&img.file_path, dir.as_deref()) {
            Some(_) => tr!("trovato", "found"),
            None => {
                missing += 1;
                tr!("MANCANTE", "MISSING")
            }
        };
        lines.push(format!(
            "  {}  {}  {}×{} px  {} × {}  pixel {}  {}: {}  [{}]",
            hex(img.common.handle),
            file_name_only(&img.file_path),
            img.size.x as u64,
            img.size.y as u64,
            fmt_len(w),
            fmt_len(h),
            fmt_len(geom::len3(f.u)),
            tr!("ritaglio", "clip"),
            clip_label(img),
            state,
        ));
        lines.push(format!("      {}", img.file_path));
    }
    let (n, body) = (images.len(), lines.join("\n"));
    host.push_output(&tr!(
        format!("RT_LIST: {n} immagini, {missing} con file mancante.\n{body}"),
        format!("RT_LIST: {n} image(s), {missing} with a missing file.\n{body}")
    ));
}

// ── Clip ────────────────────────────────────────────────────────────────────

/// `RT_CLIPAPPLY <handle> KEEP|HOLE|CUT <x1> <y1> <x2> <y2> [z]` — a window
/// drawn on screen (axis-aligned in the drawing).
fn clip_apply(host: &mut dyn HostApi, args: &[&str]) {
    let usage = tr!("Uso: RT_CLIPAPPLY <handle> KEEP|HOLE|CUT <x1> <y1> <x2> <y2>", "Usage: RT_CLIPAPPLY <handle> KEEP|HOLE|CUT <x1> <y1> <x2> <y2>");
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
    let usage = tr!("Uso: RT_CLIPPOLYAPPLY <handle> KEEP|HOLE|CUT <x1> <y1> <x2> <y2> <x3> <y3> ...", "Usage: RT_CLIPPOLYAPPLY <handle> KEEP|HOLE|CUT <x1> <y1> <x2> <y2> <x3> <y3> ...");
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
                host.push_error(tr!("Nessun poligono indicato.", "No polygon given."));
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
        ("KEEP", false) => ("RT_CLIP", tr!("il rettangolo", "the rectangle")),
        ("HOLE", false) => ("RT_CLIPHOLE", tr!("il rettangolo", "the rectangle")),
        ("CUT", false) => ("RT_CUT", tr!("il rettangolo", "the rectangle")),
        ("KEEP", true) => ("RT_CLIPPOLY", tr!("il poligono", "the polygon")),
        ("HOLE", true) => ("RT_HOLEPOLY", tr!("il poligono", "the polygon")),
        ("CUT", true) => ("RT_CUTPOLY", tr!("il poligono", "the polygon")),
        _ => {
            host.push_error(tr!("Modo di ritaglio sconosciuto: usa KEEP, HOLE o CUT.", "Unknown clip mode: use KEEP, HOLE or CUT."));
            return;
        }
    };
    let Some(mut img) = fetch(host, h) else {
        host.push_error(&tr!(format!("{name}: l'entità non è un'immagine raster."), format!("{name}: the entity is not a raster image.")));
        return;
    };
    let f = frame(&img);
    let px: Option<Vec<P2>> = world.iter().map(|&p| f.to_pixel(p)).collect();
    let Some(px) = px else {
        host.push_error(&tr!(format!("{name}: l'immagine ha una cornice degenere."), format!("{name}: the image has a degenerate frame.")));
        return;
    };
    let px = geom::dedup(&px, 1e-9);
    if px.len() >= 4 && !geom::is_simple(&px) {
        host.push_error(&tr!(format!("{name}: {what} si interseca con se stesso; disegnalo senza lati incrociati."), format!("{name}: {what} intersects itself; draw it without crossing sides.")));
        return;
    }
    if px.len() < 3 || geom::signed_area(&px).abs() < geom::MIN_AREA_PX {
        host.push_error(&tr!(format!("{name}: {what} ha area nulla."), format!("{name}: {what} has zero area.")));
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
                host.push_error(&tr!(format!("{name}: {what} non tocca la parte visibile dell'immagine."), format!("{name}: {what} does not touch the visible part of the image.")));
                return;
            };
            if clip_active(&img) && img.clip_boundary.clip_mode == ClipMode::Inside {
                host.push_info(&tr!(format!("{name}: il foro esistente è stato sostituito dal nuovo ritaglio."), format!("{name}: the existing hole was replaced by the new clip.")));
            }
            set_shape(&mut img, &shape, ClipMode::Outside);
            host.begin_undo_group(name);
            let ok = host.update_entity(EntityType::RasterImage(img));
            host.end_undo_group();
            let msg = if nested { tr!("ritaglio ristretto.", "clip narrowed.") } else { tr!("immagine ritagliata.", "image clipped.") };
            report(host, ok, &format!("{name}: {msg}"));
        }
        "HOLE" => {
            let Some(shape) = geom::shape_from_px(f.h, &on_image) else {
                host.push_error(&tr!(format!("{name}: {what} non tocca l'immagine."), format!("{name}: {what} does not touch the image.")));
                return;
            };
            if covers_frame(&f, &on_image) {
                host.push_error(&tr!(format!("{name}: {what} copre tutta l'immagine; per toglierla usa CANCELLA."), format!("{name}: {what} covers the whole image; use ERASE to remove it.")));
                return;
            }
            if clip_active(&img) {
                host.push_info(&tr!(format!("{name}: il ritaglio precedente è stato sostituito dal foro."), format!("{name}: the previous clip was replaced by the hole.")));
            }
            set_shape(&mut img, &shape, ClipMode::Inside);
            host.begin_undo_group(name);
            let ok = host.update_entity(EntityType::RasterImage(img));
            host.end_undo_group();
            report(host, ok, &tr!(format!("{name}: parte interna nascosta."), format!("{name}: inside part hidden.")));
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
                host.push_error(&tr!(format!("{name}: {what} non tocca l'immagine."), format!("{name}: {what} does not touch the image.")));
                return;
            };
            if covers_frame(&f, &on_image) {
                host.push_error(&tr!(format!("{name}: {what} copre tutta l'immagine: non c'è niente da separare."), format!("{name}: {what} covers the whole image: there is nothing to split off.")));
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
                host.push_error(&tr!(format!("{name}: l'immagine non può essere modificata (layer bloccato?)."), format!("{name}: the image cannot be changed (locked layer?).")));
                return;
            }
            if new_handle.is_null() {
                host.push_error(&tr!(format!("{name}: il pezzo non è stato creato."), format!("{name}: the piece was not created.")));
                return;
            }
            host.set_dirty();
            let _ = host.set_selection(&[new_handle]);
            let h = hex(new_handle);
            host.push_info(&tr!(
                format!("{name}: pezzo separato ({h}). È selezionato: usa SPOSTA (MOVE) per spostarlo."),
                format!("{name}: piece split off ({h}). It is selected: use MOVE to move it.")
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
        host.push_error(tr!("L'immagine non può essere modificata (layer bloccato o entità non trovata).", "The image cannot be changed (locked layer or entity not found)."));
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
        host.push_info(tr!("RT_UNCLIP: nessuna immagine ritagliata tra quelle scelte.", "RT_UNCLIP: none of the chosen images is clipped."));
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
    host.push_info(&tr!(format!("RT_UNCLIP: ritaglio tolto da {done} immagine/i."), format!("RT_UNCLIP: clip removed from {done} image(s).")));
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
    let usage = tr!("Uso: RT_SCALEAPPLY <handle> <ancora_px_x> <ancora_px_y> <fattore_x> [fattore_y]", "Usage: RT_SCALEAPPLY <handle> <anchor_px_x> <anchor_px_y> <factor_x> [factor_y]");
    let h = args.first().and_then(|t| parse_handle(t));
    let (a, b, sx) = (num(args.get(1).copied()), num(args.get(2).copied()), num(args.get(3).copied()));
    let (Some(h), Some(a), Some(b), Some(sx)) = (h, a, b, sx) else {
        host.push_error(usage);
        return;
    };
    let sy = num(args.get(4).copied()).unwrap_or(sx);
    if !valid_factor(sx) || !valid_factor(sy) {
        host.push_error(tr!("RT_RESIZE: il fattore di scala deve essere positivo.", "RT_RESIZE: the scale factor must be positive."));
        return;
    }
    let Some(mut img) = fetch(host, h) else {
        host.push_error(tr!("RT_RESIZE: l'entità non è un'immagine raster.", "RT_RESIZE: the entity is not a raster image."));
        return;
    };
    apply_scale(&mut img, [a, b], sx, sy);
    let (w, hh) = frame(&img).world_size();
    host.begin_undo_group("RT_RESIZE");
    let ok = host.update_entity(EntityType::RasterImage(img));
    host.end_undo_group();
    report(host, ok, &tr!(format!("RT_RESIZE: scala {}, nuove dimensioni {} × {}.", fmt_len(sx), fmt_len(w), fmt_len(hh)), format!("RT_RESIZE: scale {}, new size {} × {}.", fmt_len(sx), fmt_len(w), fmt_len(hh))));
}

/// Move / turn / scale an image's frame (clip outlines live in pixel space
/// and follow by themselves). One undo step.
fn transform_image(host: &mut dyn HostApi, mut img: RasterImage, s: &geom::Similarity, label: &str) -> bool {
    let f = s.frame(&frame(&img));
    img.insertion_point = v3(f.ins);
    img.u_vector = v3(f.u);
    img.v_vector = v3(f.v);
    host.begin_undo_group(label);
    let ok = host.update_entity(EntityType::RasterImage(img));
    host.end_undo_group();
    ok
}

fn fmt_angle(deg: f64) -> String {
    let d = if deg > 180.0 { deg - 360.0 } else if deg <= -180.0 { deg + 360.0 } else { deg };
    let d = if d.abs() < 5e-5 { 0.0 } else { d };
    let t = format!("{:.4}", d);
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `RT_ALIGNAPPLY <h> <p1x> <p1y> <q1x> <q1y> [<p2x> <p2y> <q2x> <q2y>]` —
/// p on the image goes to q. One pair moves; two pairs move, turn and scale.
fn align_apply(host: &mut dyn HostApi, args: &[&str]) {
    let usage = tr!(
        "Uso: RT_ALIGNAPPLY <handle> <p1x> <p1y> <q1x> <q1y> [<p2x> <p2y> <q2x> <q2y>]",
        "Usage: RT_ALIGNAPPLY <handle> <p1x> <p1y> <q1x> <q1y> [<p2x> <p2y> <q2x> <q2y>]"
    );
    let h = args.first().and_then(|t| parse_handle(t));
    let n: Vec<f64> = args.iter().skip(1).filter_map(|t| num(Some(t))).collect();
    let (Some(h), true) = (h, n.len() == 4 || n.len() == 8) else {
        host.push_error(usage);
        return;
    };
    let Some(img) = fetch(host, h) else {
        host.push_error(tr!("RT_ALIGN: l'entità non è un'immagine raster.", "RT_ALIGN: the entity is not a raster image."));
        return;
    };
    let sim = if n.len() == 4 {
        Some(geom::Similarity::translation([n[2] - n[0], n[3] - n[1]]))
    } else {
        geom::Similarity::from_pairs([n[0], n[1]], [n[2], n[3]], [n[4], n[5]], [n[6], n[7]])
    };
    let Some(sim) = sim.filter(|s| valid_factor(s.scale())) else {
        host.push_error(tr!(
            "RT_ALIGN: i due punti sull'immagine devono essere distinti (e le destinazioni anche).",
            "RT_ALIGN: the two points on the image must differ (and so must their targets)."
        ));
        return;
    };
    let before = frame(&img).world_size();
    let ok = transform_image(host, img, &sim, "RT_ALIGN");
    let msg = if n.len() == 4 {
        tr!(
            format!("RT_ALIGN: immagine spostata di {}, {}.", fmt_len(n[2] - n[0]), fmt_len(n[3] - n[1])),
            format!("RT_ALIGN: image moved by {}, {}.", fmt_len(n[2] - n[0]), fmt_len(n[3] - n[1]))
        )
    } else {
        let (k, a) = (sim.scale(), fmt_angle(sim.angle_deg()));
        let (w, hh) = (before.0 * k, before.1 * k);
        tr!(
            format!(
                "RT_ALIGN: immagine allineata ai due punti — scala ×{}, rotazione {a}°, nuove dimensioni {} × {}.",
                fmt_len(k),
                fmt_len(w),
                fmt_len(hh)
            ),
            format!(
                "RT_ALIGN: image aligned to the two points — scale ×{}, rotation {a}°, new size {} × {}.",
                fmt_len(k),
                fmt_len(w),
                fmt_len(hh)
            )
        )
    };
    report(host, ok, &msg);
}

/// `RT_ROTATEAPPLY <h> <degrees> <cx> <cy>` — counter-clockwise.
fn rotate_apply(host: &mut dyn HostApi, args: &[&str]) {
    let h = args.first().and_then(|t| parse_handle(t));
    let (a, cx, cy) = (num(args.get(1).copied()), num(args.get(2).copied()), num(args.get(3).copied()));
    let (Some(h), Some(a), Some(cx), Some(cy)) = (h, a, cx, cy) else {
        host.push_error(tr!("Uso: RT_ROTATEAPPLY <handle> <gradi> <x> <y>", "Usage: RT_ROTATEAPPLY <handle> <degrees> <x> <y>"));
        return;
    };
    let Some(img) = fetch(host, h) else {
        host.push_error(tr!("RT_ROTATE: l'entità non è un'immagine raster.", "RT_ROTATE: the entity is not a raster image."));
        return;
    };
    let ok = transform_image(host, img, &geom::Similarity::rotation([cx, cy], a.to_radians()), "RT_ROTATE");
    let a = fmt_angle(a);
    report(host, ok, &tr!(format!("RT_ROTATE: immagine ruotata di {a}°."), format!("RT_ROTATE: image rotated by {a}°.")));
}

/// `RT_SIZEAPPLY <handles> W|H|WH|F|DPI <w> <h> <factor> <dpi> MM|CM|M|IN BL|CTR FULL|VIS`
pub fn size_apply(host: &mut dyn HostApi, args: &[&str]) {
    let usage = tr!("Uso: RT_SIZEAPPLY <handle,...> W|H|WH|F|DPI <larghezza> <altezza> <fattore> <dpi> MM|CM|M|IN BL|CTR FULL|VIS", "Usage: RT_SIZEAPPLY <handle,...> W|H|WH|F|DPI <width> <height> <factor> <dpi> MM|CM|M|IN BL|CTR FULL|VIS");
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
            host.push_error(tr!("RT_SIZE: valori non validi (servono numeri positivi).", "RT_SIZE: invalid values (positive numbers needed)."));
            return;
        }
        let anchor = if centre { [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5] } else { lo };
        apply_scale(&mut img, anchor, sx, sy);
        edits.push(img);
    }
    if edits.is_empty() {
        host.push_error(tr!("RT_SIZE: nessuna immagine raster valida.", "RT_SIZE: no valid raster image."));
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
        let (lw, lh) = (fmt_len(last.0), fmt_len(last.1));
        host.push_info(&tr!(
            format!("RT_SIZE: {done} immagine/i ridimensionata/e (ultima: {lw} × {lh} cornice intera)."),
            format!("RT_SIZE: {done} image(s) resized (last: {lw} × {lh} full frame).")
        ));
    } else {
        host.push_error(tr!("RT_SIZE: nessuna immagine modificata (layer bloccato?).", "RT_SIZE: no image changed (locked layer?)."));
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
        host.push_error(tr!("RT_PATH: nessuna immagine raster scelta.", "RT_PATH: no raster image chosen."));
        return;
    }
    if !Path::new(&new_path).is_file() {
        host.push_error(&tr!(format!("RT_PATH: il file \"{new_path}\" non esiste."), format!("RT_PATH: the file \"{new_path}\" does not exist.")));
        return;
    }
    if !imgsize::is_raster_name(&new_path) {
        host.push_info(tr!("RT_PATH: attenzione, l'estensione non è di un formato raster riconosciuto.", "RT_PATH: warning, the extension is not a recognised raster format."));
    }
    let groups = groups_for(host, Some(targets));
    if groups.is_empty() {
        host.push_error(tr!("RT_PATH: nessuna immagine raster tra quelle scelte.", "RT_PATH: none of the chosen entities is a raster image."));
        return;
    }
    let dims = imgsize::read_dims(Path::new(&new_path)).ok();
    if dims.is_none() {
        host.push_info(tr!("RT_PATH: impossibile leggere le dimensioni in pixel del nuovo file; mantengo quelle attuali.", "RT_PATH: cannot read the new file's pixel size; keeping the current one."));
    }
    let mut total = 0;
    for g in groups {
        total += relink_group(host, g, &new_path, dims, "RT_PATH", false);
    }
    host.push_info(&tr!(format!("RT_PATH: {total} immagine/i collegata/e a \"{new_path}\"."), format!("RT_PATH: {total} image(s) linked to \"{new_path}\".")));
}

/// Point every image of `g` at `new_path`, keeping each one's world width
/// (and pixel aspect) when the new file's pixel count differs, then move the
/// shared definition so the change is saved with the drawing. Returns the
/// number of images updated.
/// `transparent`: also turn the images' transparency on (`RT_TRANSP`).
fn relink_group(
    host: &mut dyn HostApi,
    g: Group,
    new_path: &str,
    dims: Option<(u32, u32)>,
    label: &str,
    transparent: bool,
) -> usize {
    let mut done = 0;
    host.begin_undo_group(label);
    for mut img in g.images {
        img.file_path = new_path.to_string();
        if transparent {
            img.flags |= ImageDisplayFlags::TRANSPARENCY_ON;
        }
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
                host.push_info(&tr!(
                    format!(
                        "RT_PATH: immagine aggiornata a video. Per salvare il nuovo percorso nel file \
                         esegui:  XREF Path {pattern} {new_path}"
                    ),
                    format!(
                        "RT_PATH: image updated on screen. To save the new path in the file \
                         run:  XREF Path {pattern} {new_path}"
                    )
                ));
            }
        }
        None => host.push_info(
            tr!("RT_PATH: l'immagine non ha una definizione nel disegno; il percorso è aggiornato sull'entità.", "RT_PATH: the image has no definition in the drawing; the path is updated on the entity."),
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
        host.push_error(&tr!(format!("RT_FIND: la cartella \"{}\" non esiste.", root.display()), format!("RT_FIND: the folder \"{}\" does not exist.", root.display())));
        return;
    }
    let dir = dwg_dir(host);
    let missing: Vec<Group> = groups_for(host, None)
        .into_iter()
        .filter(|g| resolve(&g.path, dir.as_deref()).is_none())
        .collect();
    if missing.is_empty() {
        host.push_info(tr!("RT_FIND: tutte le immagini del disegno hanno il file al suo posto.", "RT_FIND: every image in the drawing has its file in place."));
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
                fixed += relink_group(host, g, &p, dims, "RT_FIND", false);
            }
            None => not_found.push(file_name_only(&g.path).to_string()),
        }
    }
    let mut msg = tr!(format!("RT_FIND: in \"{}\" ricollegate {fixed} immagine/i.", root.display()), format!("RT_FIND: relinked {fixed} image(s) in \"{}\".", root.display()));
    if !not_found.is_empty() {
        msg.push_str(&tr!(format!(" Non trovati: {}.", not_found.join(", ")), format!(" Not found: {}.", not_found.join(", "))));
    }
    host.push_info(&msg);
}
