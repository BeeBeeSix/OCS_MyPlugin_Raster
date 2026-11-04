//! Interactive tools: picking an image, drawing the clip window, dragging a
//! corner, and the dialogs. Interactive callbacks get no host access, so each
//! tool works on a snapshot taken when it starts and ends by handing the host
//! an apply command (see `commands.rs`).

use std::collections::HashMap;

use ocs_plugin_api::dialog::{DialogControl, DialogDef, DialogResponse, DialogValue};
use ocs_plugin_api::host::codec;
use ocs_plugin_api::host::{CommandStep, HostApi, InteractiveCommand, PreviewWire};

use codec::entities::RasterImage;
use codec::Handle;

use crate::commands::{self, frame, join_handles, visible_px};
use crate::geom::{self, Frame, P2, P3};
use crate::imgsize::EXTENSIONS;

const CYAN: [f32; 4] = [0.43, 0.72, 0.93, 1.0];
const GREEN: [f32; 4] = [0.35, 0.85, 0.45, 1.0];
const RED: [f32; 4] = [0.95, 0.35, 0.30, 1.0];
const ORANGE: [f32; 4] = [1.0, 0.65, 0.2, 1.0];
const GREY: [f32; 4] = [0.6, 0.6, 0.62, 0.8];

/// What a tool needs to know about one image.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub handle: Handle,
    pub frame: Frame,
    /// Visible part, pixel space.
    pub visible: Vec<P2>,
    pub name: String,
}

impl Snapshot {
    fn of(img: &RasterImage) -> Self {
        Snapshot {
            handle: img.common.handle,
            frame: frame(img),
            visible: visible_px(img),
            name: commands::file_name_only(&img.file_path).to_string(),
        }
    }

    fn outline(&self) -> Vec<P3> {
        ring(self.frame.corners().to_vec())
    }

    fn visible_outline(&self) -> Vec<P3> {
        ring(self.visible.iter().map(|&p| self.frame.to_world(p)).collect())
    }

    /// World corners of the visible part's bounding box, in pixel and world
    /// coordinates, counter-clockwise from bottom-left.
    fn visible_corners(&self) -> ([P2; 4], [P3; 4]) {
        let (lo, hi) = geom::bbox(&self.visible).unwrap_or(([0.0, 0.0], [self.frame.w, self.frame.h]));
        let px = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
        (px, px.map(|p| self.frame.to_world(p)))
    }
}

fn ring(mut pts: Vec<P3>) -> Vec<P3> {
    if let Some(&f) = pts.first() {
        pts.push(f);
    }
    pts
}

/// Snapshot every raster image in the drawing.
pub fn snapshots(host: &dyn HostApi) -> Vec<Snapshot> {
    commands::all_images(host).iter().map(Snapshot::of).collect()
}

/// Index of the topmost image under a world point.
fn hit(images: &[Snapshot], pt: P3) -> Option<usize> {
    images.iter().rposition(|s| s.frame.contains(pt))
}

fn index_of(images: &[Snapshot], h: Handle) -> Option<usize> {
    images.iter().position(|s| s.handle == h)
}

fn with_msg(msg: &Option<String>, prompt: String) -> String {
    match msg {
        Some(m) => format!("{m}  |  {prompt}"),
        None => prompt,
    }
}

// ── Pick one image, then dispatch `<verb> <handle>` ────────────────────────

pub struct PickTool {
    images: Vec<Snapshot>,
    verb: &'static str,
    what: &'static str,
    msg: Option<String>,
}

impl PickTool {
    pub fn new(images: Vec<Snapshot>, verb: &'static str, what: &'static str) -> Self {
        PickTool { images, verb, what, msg: None }
    }

    fn take(&mut self, idx: Option<usize>) -> CommandStep {
        match idx {
            Some(i) => CommandStep::Dispatch(format!("{} {:X}", self.verb, self.images[i].handle.value())),
            None => {
                self.msg = Some("non è un'immagine raster".into());
                CommandStep::NeedPoint
            }
        }
    }
}

impl InteractiveCommand for PickTool {
    fn prompt(&self) -> String {
        with_msg(&self.msg, format!("Seleziona l'immagine da {} (Invio per annullare)", self.what))
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let i = hit(&self.images, pt);
        self.take(i)
    }
    fn needs_object_pick(&self) -> bool {
        true
    }
    fn on_object_pick(&mut self, handle: Handle, pt: [f64; 3]) -> CommandStep {
        let i = index_of(&self.images, handle).or_else(|| hit(&self.images, pt));
        self.take(i)
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        hit(&self.images, pt)
            .map(|i| vec![PreviewWire::with_color(self.images[i].outline(), CYAN)])
            .unwrap_or_default()
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }
}

// ── Rectangular clip / hole / cut ──────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    /// Keep what is inside the window.
    Keep,
    /// Hide what is inside the window.
    Hole,
    /// Split the window off into a new image entity.
    Cut,
}

impl ClipKind {
    pub fn keyword(self) -> &'static str {
        match self {
            ClipKind::Keep => "KEEP",
            ClipKind::Hole => "HOLE",
            ClipKind::Cut => "CUT",
        }
    }
    fn name(self) -> &'static str {
        match self {
            ClipKind::Keep => "RT_CLIP",
            ClipKind::Hole => "RT_CLIPHOLE",
            ClipKind::Cut => "RT_CUT",
        }
    }
    fn poly_name(self) -> &'static str {
        match self {
            ClipKind::Keep => "RT_CLIPPOLY",
            ClipKind::Hole => "RT_HOLEPOLY",
            ClipKind::Cut => "RT_CUTPOLY",
        }
    }
    pub fn pline_name(self) -> &'static str {
        match self {
            ClipKind::Keep => "RT_CLIPPL",
            ClipKind::Hole => "RT_HOLEPL",
            ClipKind::Cut => "RT_CUTPL",
        }
    }
    fn color(self) -> [f32; 4] {
        match self {
            ClipKind::Keep => GREEN,
            ClipKind::Hole => RED,
            ClipKind::Cut => ORANGE,
        }
    }
}

pub struct ClipTool {
    images: Vec<Snapshot>,
    kind: ClipKind,
    target: Option<usize>,
    first: Option<P3>,
    msg: Option<String>,
}

impl ClipTool {
    pub fn new(images: Vec<Snapshot>, kind: ClipKind, preselected: Option<Handle>) -> Self {
        let target = preselected.and_then(|h| index_of(&images, h));
        ClipTool { images, kind, target, first: None, msg: None }
    }

    fn choose(&mut self, idx: Option<usize>) -> CommandStep {
        match idx {
            Some(i) => {
                self.target = Some(i);
                self.msg = None;
            }
            None => self.msg = Some("non è un'immagine raster".into()),
        }
        CommandStep::NeedPoint
    }
}

impl InteractiveCommand for ClipTool {
    fn prompt(&self) -> String {
        let p = match (self.target, self.first) {
            (None, _) => format!("{}: seleziona l'immagine", self.kind.name()),
            (Some(_), None) => format!("{}: primo angolo del rettangolo", self.kind.name()),
            (Some(_), Some(_)) => format!("{}: angolo opposto", self.kind.name()),
        };
        with_msg(&self.msg, p)
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let Some(t) = self.target else {
            let i = hit(&self.images, pt);
            return self.choose(i);
        };
        let Some(p1) = self.first else {
            self.first = Some(pt);
            return CommandStep::NeedPoint;
        };
        if (p1[0] - pt[0]).abs() < 1e-12 || (p1[1] - pt[1]).abs() < 1e-12 {
            self.msg = Some("il rettangolo ha area nulla".into());
            return CommandStep::NeedPoint;
        }
        CommandStep::Dispatch(format!(
            "RT_CLIPAPPLY {:X} {} {} {} {} {} {}",
            self.images[t].handle.value(),
            self.kind.keyword(),
            p1[0],
            p1[1],
            pt[0],
            pt[1],
            p1[2]
        ))
    }

    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }

    fn needs_object_pick(&self) -> bool {
        self.target.is_none()
    }

    fn on_object_pick(&mut self, handle: Handle, pt: [f64; 3]) -> CommandStep {
        let i = index_of(&self.images, handle).or_else(|| hit(&self.images, pt));
        self.choose(i)
    }

    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        match (self.target, self.first) {
            (None, _) => hit(&self.images, pt)
                .map(|i| vec![PreviewWire::with_color(self.images[i].outline(), CYAN)])
                .unwrap_or_default(),
            (Some(t), None) => vec![
                PreviewWire::with_color(self.images[t].outline(), GREY),
                PreviewWire::with_color(self.images[t].visible_outline(), CYAN),
            ],
            (Some(t), Some(p1)) => {
                let z = p1[2];
                let rect = vec![
                    [p1[0], p1[1], z],
                    [pt[0], p1[1], z],
                    [pt[0], pt[1], z],
                    [p1[0], pt[1], z],
                    [p1[0], p1[1], z],
                ];
                vec![
                    PreviewWire::with_color(self.images[t].outline(), GREY),
                    PreviewWire::with_color(rect, self.kind.color()),
                ]
            }
        }
    }
}

// ── Polygon drawn point by point ───────────────────────────────────────────

/// Pick an image, then click the polygon's vertices; Enter (or a click on
/// the first vertex) closes it.
pub struct PolyClipTool {
    images: Vec<Snapshot>,
    kind: ClipKind,
    target: Option<usize>,
    pts: Vec<P3>,
    msg: Option<String>,
}

impl PolyClipTool {
    pub fn new(images: Vec<Snapshot>, kind: ClipKind, preselected: Option<Handle>) -> Self {
        let target = preselected.and_then(|h| index_of(&images, h));
        PolyClipTool { images, kind, target, pts: Vec::new(), msg: None }
    }

    /// A click this close to the first vertex closes the polygon.
    fn close_tolerance(&self) -> f64 {
        let t = self.target.unwrap_or(0);
        let (w, h) = self.images[t].frame.world_size();
        (w.hypot(h) * 0.015).max(1e-9)
    }

    fn finish(&mut self) -> CommandStep {
        if self.pts.len() < 3 {
            self.msg = Some("servono almeno 3 vertici".into());
            return CommandStep::NeedPoint;
        }
        let t = self.target.expect("target chosen before vertices");
        commands::set_pending_poly(std::mem::take(&mut self.pts));
        CommandStep::Dispatch(format!(
            "RT_CLIPPOLYAPPLY {:X} {}",
            self.images[t].handle.value(),
            self.kind.keyword()
        ))
    }

    fn choose(&mut self, idx: Option<usize>) -> CommandStep {
        match idx {
            Some(i) => {
                self.target = Some(i);
                self.msg = None;
            }
            None => self.msg = Some("non è un'immagine raster".into()),
        }
        CommandStep::NeedPoint
    }
}

impl InteractiveCommand for PolyClipTool {
    fn prompt(&self) -> String {
        let name = self.kind.poly_name();
        let p = match (self.target, self.pts.len()) {
            (None, _) => format!("{name}: seleziona l'immagine"),
            (Some(_), 0) => format!("{name}: primo vertice del poligono"),
            (Some(_), n) if n < 3 => format!("{name}: vertice {} (Esc per annullare)", n + 1),
            (Some(_), n) => format!("{name}: vertice {} — Invio o clic sul primo vertice per chiudere", n + 1),
        };
        with_msg(&self.msg, p)
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        if self.target.is_none() {
            let i = hit(&self.images, pt);
            return self.choose(i);
        }
        if self.pts.len() >= 3 {
            let f = self.pts[0];
            if (f[0] - pt[0]).hypot(f[1] - pt[1]) <= self.close_tolerance() {
                return self.finish();
            }
        }
        if let Some(l) = self.pts.last() {
            if (l[0] - pt[0]).abs() < 1e-12 && (l[1] - pt[1]).abs() < 1e-12 {
                return CommandStep::NeedPoint; // double click on the same spot
            }
        }
        self.msg = None;
        self.pts.push(pt);
        CommandStep::NeedPoint
    }

    fn on_enter(&mut self) -> CommandStep {
        if self.target.is_none() || self.pts.is_empty() {
            return CommandStep::Cancel;
        }
        self.finish()
    }

    fn needs_object_pick(&self) -> bool {
        self.target.is_none()
    }

    fn on_object_pick(&mut self, handle: Handle, pt: [f64; 3]) -> CommandStep {
        let i = index_of(&self.images, handle).or_else(|| hit(&self.images, pt));
        self.choose(i)
    }

    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let Some(t) = self.target else {
            return hit(&self.images, pt)
                .map(|i| vec![PreviewWire::with_color(self.images[i].outline(), CYAN)])
                .unwrap_or_default();
        };
        let mut wires = vec![PreviewWire::with_color(self.images[t].outline(), GREY)];
        if self.pts.is_empty() {
            return wires;
        }
        let mut path = self.pts.clone();
        path.push(pt);
        if self.pts.len() >= 2 {
            path.push(self.pts[0]);
        }
        wires.push(PreviewWire::with_color(path, self.kind.color()));
        if self.pts.len() >= 3 {
            wires.push(PreviewWire::circle(self.pts[0], self.close_tolerance(), Some(self.kind.color())));
        }
        wires
    }
}

// ── Existing closed polyline / circle / ellipse ────────────────────────────

/// Pick an image, then a closed outline already in the drawing (or click
/// inside one).
pub struct PlineClipTool {
    images: Vec<Snapshot>,
    shapes: Vec<(Handle, Vec<P3>)>,
    kind: ClipKind,
    target: Option<usize>,
    msg: Option<String>,
}

impl PlineClipTool {
    pub fn new(images: Vec<Snapshot>, shapes: Vec<(Handle, Vec<P3>)>, kind: ClipKind, preselected: Option<Handle>) -> Self {
        let target = preselected.and_then(|h| index_of(&images, h));
        PlineClipTool { images, shapes, kind, target, msg: None }
    }

    /// The smallest outline containing a point.
    fn shape_at(&self, pt: P3) -> Option<usize> {
        self.shapes
            .iter()
            .enumerate()
            .filter(|(_, (_, ring))| {
                let r: Vec<P2> = ring.iter().map(|p| [p[0], p[1]]).collect();
                geom::point_in_polygon(&r, [pt[0], pt[1]])
            })
            .min_by(|a, b| {
                let area = |ring: &Vec<P3>| {
                    let r: Vec<P2> = ring.iter().map(|p| [p[0], p[1]]).collect();
                    geom::signed_area(&r).abs()
                };
                area(&a.1 .1).total_cmp(&area(&b.1 .1))
            })
            .map(|(i, _)| i)
    }

    fn take_shape(&mut self, idx: Option<usize>) -> CommandStep {
        let (Some(t), Some(i)) = (self.target, idx) else {
            self.msg = Some("non è una polilinea chiusa, un cerchio o un'ellisse".into());
            return CommandStep::NeedPoint;
        };
        commands::set_pending_poly(self.shapes[i].1.clone());
        CommandStep::Dispatch(format!(
            "RT_CLIPPOLYAPPLY {:X} {}",
            self.images[t].handle.value(),
            self.kind.keyword()
        ))
    }

    fn ring(&self, i: usize) -> Vec<P3> {
        ring(self.shapes[i].1.clone())
    }
}

impl InteractiveCommand for PlineClipTool {
    fn prompt(&self) -> String {
        let name = self.kind.pline_name();
        let p = match self.target {
            None => format!("{name}: seleziona l'immagine"),
            Some(_) => format!("{name}: seleziona la polilinea chiusa (o clicca al suo interno)"),
        };
        with_msg(&self.msg, p)
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        match self.target {
            None => {
                match hit(&self.images, pt) {
                    Some(i) => {
                        self.target = Some(i);
                        self.msg = None;
                    }
                    None => self.msg = Some("non è un'immagine raster".into()),
                }
                CommandStep::NeedPoint
            }
            Some(_) => {
                let i = self.shape_at(pt);
                self.take_shape(i)
            }
        }
    }

    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }

    fn needs_object_pick(&self) -> bool {
        true
    }

    fn on_object_pick(&mut self, handle: Handle, pt: [f64; 3]) -> CommandStep {
        match self.target {
            None => {
                match index_of(&self.images, handle).or_else(|| hit(&self.images, pt)) {
                    Some(i) => {
                        self.target = Some(i);
                        self.msg = None;
                    }
                    None => self.msg = Some("non è un'immagine raster".into()),
                }
                CommandStep::NeedPoint
            }
            Some(_) => {
                let i = self.shapes.iter().position(|(h, _)| *h == handle).or_else(|| self.shape_at(pt));
                self.take_shape(i)
            }
        }
    }

    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        match self.target {
            None => hit(&self.images, pt)
                .map(|i| vec![PreviewWire::with_color(self.images[i].outline(), CYAN)])
                .unwrap_or_default(),
            Some(t) => {
                let mut w = vec![PreviewWire::with_color(self.images[t].outline(), GREY)];
                if let Some(i) = self.shape_at(pt) {
                    w.push(PreviewWire::with_color(self.ring(i), self.kind.color()));
                }
                w
            }
        }
    }
}

// ── Proportional resize by dragging a corner ───────────────────────────────

pub struct ResizeTool {
    images: Vec<Snapshot>,
    target: Option<usize>,
    /// Fixed corner (pixel, world) and the dragged opposite corner (world).
    anchor: Option<(P2, P3, P3)>,
    msg: Option<String>,
}

impl ResizeTool {
    pub fn new(images: Vec<Snapshot>, preselected: Option<Handle>) -> Self {
        let target = preselected.and_then(|h| index_of(&images, h));
        ResizeTool { images, target, anchor: None, msg: None }
    }

    fn nearest_corner(&self, t: usize, pt: P3) -> (usize, [P2; 4], [P3; 4]) {
        let (px, w) = self.images[t].visible_corners();
        (geom::nearest(&w, pt), px, w)
    }
}

impl InteractiveCommand for ResizeTool {
    fn prompt(&self) -> String {
        let p = match (self.target, self.anchor) {
            (None, _) => "RT_RESIZE: seleziona l'immagine".to_string(),
            (Some(_), None) => "RT_RESIZE: clicca vicino all'angolo che deve restare fermo".to_string(),
            (Some(_), Some(_)) => "RT_RESIZE: porta l'angolo opposto alla nuova posizione".to_string(),
        };
        with_msg(&self.msg, p)
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let Some(t) = self.target else {
            match hit(&self.images, pt) {
                Some(i) => {
                    self.target = Some(i);
                    self.msg = None;
                }
                None => self.msg = Some("non è un'immagine raster".into()),
            }
            return CommandStep::NeedPoint;
        };
        let Some((anchor_px, anchor_w, opp_w)) = self.anchor else {
            let (i, px, w) = self.nearest_corner(t, pt);
            self.anchor = Some((px[i], w[i], w[(i + 2) % 4]));
            return CommandStep::NeedPoint;
        };
        let k = geom::drag_factor(anchor_w, opp_w, pt);
        if !(k.is_finite() && k > 1e-6) {
            self.msg = Some("porta l'angolo dalla parte dell'immagine".into());
            return CommandStep::NeedPoint;
        }
        CommandStep::Dispatch(format!(
            "RT_SCALEAPPLY {:X} {} {} {} {}",
            self.images[t].handle.value(),
            anchor_px[0],
            anchor_px[1],
            k,
            k
        ))
    }

    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }

    fn needs_object_pick(&self) -> bool {
        self.target.is_none()
    }

    fn on_object_pick(&mut self, handle: Handle, pt: [f64; 3]) -> CommandStep {
        match index_of(&self.images, handle).or_else(|| hit(&self.images, pt)) {
            Some(i) => {
                self.target = Some(i);
                self.msg = None;
            }
            None => self.msg = Some("non è un'immagine raster".into()),
        }
        CommandStep::NeedPoint
    }

    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let Some(t) = self.target else {
            return hit(&self.images, pt)
                .map(|i| vec![PreviewWire::with_color(self.images[i].outline(), CYAN)])
                .unwrap_or_default();
        };
        let snap = &self.images[t];
        match self.anchor {
            None => {
                let (i, _, w) = self.nearest_corner(t, pt);
                let (fw, fh) = snap.frame.world_size();
                let r = (fw.hypot(fh) * 0.02).max(1e-9);
                vec![
                    PreviewWire::with_color(snap.visible_outline(), CYAN),
                    PreviewWire::circle(w[i], r, Some(ORANGE)),
                ]
            }
            Some((anchor_px, anchor_w, opp_w)) => {
                let k = geom::drag_factor(anchor_w, opp_w, pt);
                if !(k.is_finite() && k > 1e-6) {
                    return vec![PreviewWire::with_color(snap.visible_outline(), GREY)];
                }
                let (ins, u, v) = snap.frame.scaled(anchor_px, k, k);
                let scaled = Frame { ins, u, v, ..snap.frame };
                let vis = ring(snap.visible.iter().map(|&p| scaled.to_world(p)).collect());
                vec![
                    PreviewWire::with_color(snap.visible_outline(), GREY),
                    PreviewWire::with_color(vis, ORANGE),
                    PreviewWire::line(anchor_w, pt, Some(GREY)),
                ]
            }
        }
    }
}

// ── Dialog helpers ──────────────────────────────────────────────────────────

fn number(id: &str, label: &str, default: f64) -> DialogControl {
    DialogControl::Number {
        id: id.into(),
        label: label.into(),
        default,
        min: Some(0.0),
        max: None,
        step: None,
    }
}

fn choice(id: &str, label: &str, options: &[&str], default_index: usize) -> DialogControl {
    DialogControl::Choice {
        id: id.into(),
        label: label.into(),
        options: options.iter().map(|s| s.to_string()).collect(),
        default_index,
    }
}

fn info(text: String) -> DialogControl {
    DialogControl::InfoText { text, align_h: None }
}

fn get_num(m: &HashMap<String, DialogValue>, id: &str) -> Option<f64> {
    match m.get(id)? {
        DialogValue::Number(v) => Some(*v),
        DialogValue::Text(s) => s.trim().replace(',', ".").parse().ok(),
        _ => None,
    }
}

fn get_choice(m: &HashMap<String, DialogValue>, id: &str) -> usize {
    match m.get(id) {
        Some(DialogValue::Choice { index, .. }) => *index,
        Some(DialogValue::Number(v)) => *v as usize,
        _ => 0,
    }
}

fn get_bool(m: &HashMap<String, DialogValue>, id: &str, default: bool) -> bool {
    match m.get(id) {
        Some(DialogValue::Bool(b)) => *b,
        _ => default,
    }
}

fn get_path(m: &HashMap<String, DialogValue>, id: &str) -> Option<String> {
    match m.get(id)? {
        DialogValue::FilePath(s) | DialogValue::Text(s) => {
            let s = s.trim().trim_matches('"').to_string();
            (!s.is_empty()).then_some(s)
        }
        _ => None,
    }
}

// ── RT_SIZE dialog ──────────────────────────────────────────────────────────

const SIZE_MODES: &[(&str, &str)] = &[
    ("W", "Larghezza (proporzionale)"),
    ("H", "Altezza (proporzionale)"),
    ("WH", "Larghezza e altezza (libere)"),
    ("F", "Fattore di scala"),
    ("DPI", "Risoluzione di stampa (DPI)"),
];
const UNITS: &[(&str, &str)] = &[("MM", "millimetri"), ("CM", "centimetri"), ("M", "metri"), ("IN", "pollici")];
const ANCHORS: &[(&str, &str)] = &[("BL", "Angolo in basso a sinistra"), ("CTR", "Centro")];

pub struct SizeDialog {
    handles: Vec<Handle>,
    def: DialogDef,
    done: bool,
}

impl SizeDialog {
    pub fn new(handles: Vec<Handle>, first: &RasterImage) -> Self {
        let f = frame(first);
        let (w, h) = f.world_size();
        let clipped = commands::clip_active(first);
        let header = format!(
            "{} immagine/i. {}: {} × {} px, cornice {:.4} × {:.4} unità di disegno.",
            handles.len(),
            commands::file_name_only(&first.file_path),
            first.size.x as u64,
            first.size.y as u64,
            w,
            h
        );
        let mut def = DialogDef::new("Dimensioni immagine raster")
            .with_control(info(header))
            .with_control(choice("mode", "Modo", &SIZE_MODES.iter().map(|m| m.1).collect::<Vec<_>>(), 0))
            .with_control(number("width", "Larghezza", w))
            .with_control(number("height", "Altezza", h))
            .with_control(number("factor", "Fattore di scala", 1.0))
            .with_control(number("dpi", "DPI", 300.0))
            .with_control(choice("unit", "Unità del disegno (per DPI)", &UNITS.iter().map(|u| u.1).collect::<Vec<_>>(), 0))
            .with_control(choice("anchor", "Punto fisso", &ANCHORS.iter().map(|a| a.1).collect::<Vec<_>>(), 0))
            .with_control(DialogControl::Checkbox {
                id: "visible".into(),
                label: "Misure riferite alla parte visibile (ritagliata)".into(),
                default: clipped,
            })
            .submit_label("Applica")
            .cancel_label("Annulla");
        def = def.width(460.0);
        SizeDialog { handles, def, done: false }
    }
}

impl InteractiveCommand for SizeDialog {
    fn prompt(&self) -> String {
        "RT_SIZE: imposta le dimensioni nella finestra".into()
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> CommandStep {
        CommandStep::NeedPoint
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }
    fn modal_dialog(&self) -> Option<DialogDef> {
        (!self.done).then(|| self.def.clone())
    }
    fn on_dialog_response(&mut self, response: DialogResponse) -> CommandStep {
        self.done = true;
        let DialogResponse::Submitted(m) = response else {
            return CommandStep::Cancel;
        };
        let mode = SIZE_MODES[get_choice(&m, "mode").min(SIZE_MODES.len() - 1)].0;
        let unit = UNITS[get_choice(&m, "unit").min(UNITS.len() - 1)].0;
        let anchor = ANCHORS[get_choice(&m, "anchor").min(ANCHORS.len() - 1)].0;
        let vis = if get_bool(&m, "visible", false) { "VIS" } else { "FULL" };
        let n = |id| get_num(&m, id).unwrap_or(0.0);
        CommandStep::Dispatch(format!(
            "RT_SIZEAPPLY {} {} {} {} {} {} {} {} {}",
            join_handles(&self.handles),
            mode,
            n("width"),
            n("height"),
            n("factor"),
            n("dpi"),
            unit,
            anchor,
            vis
        ))
    }
}

// ── RT_PATH dialog ──────────────────────────────────────────────────────────

pub struct PathDialog {
    handles: Vec<Handle>,
    def: DialogDef,
    done: bool,
}

impl PathDialog {
    pub fn new(handles: Vec<Handle>, current: String, found: bool) -> Self {
        let state = if found { "file trovato" } else { "FILE MANCANTE" };
        let def = DialogDef::new("Percorso immagine raster")
            .with_control(info(format!("Percorso attuale ({state}):\n{current}")))
            .with_control(info(
                "Tutte le immagini che usano lo stesso file verranno ricollegate. \
                 La larghezza nel disegno resta la stessa."
                    .into(),
            ))
            .with_control(DialogControl::FilePicker {
                id: "file".into(),
                label: "Nuovo file".into(),
                default_path: current,
                filter_name: "Immagini raster".into(),
                extensions: EXTENSIONS.iter().map(|e| e.to_string()).collect(),
                save: false,
            })
            .submit_label("Ricollega")
            .cancel_label("Annulla")
            .width(560.0);
        PathDialog { handles, def, done: false }
    }
}

impl InteractiveCommand for PathDialog {
    fn prompt(&self) -> String {
        "RT_PATH: scegli il nuovo file nella finestra".into()
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> CommandStep {
        CommandStep::NeedPoint
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }
    fn modal_dialog(&self) -> Option<DialogDef> {
        (!self.done).then(|| self.def.clone())
    }
    fn on_dialog_response(&mut self, response: DialogResponse) -> CommandStep {
        self.done = true;
        let DialogResponse::Submitted(m) = response else {
            return CommandStep::Cancel;
        };
        match get_path(&m, "file") {
            Some(p) => {
                commands::set_pending_path(p);
                CommandStep::Dispatch(format!("RT_PATHAPPLY {}", join_handles(&self.handles)))
            }
            None => CommandStep::Cancel,
        }
    }
}

// ── RT_FIND dialog ──────────────────────────────────────────────────────────

pub struct FindDialog {
    def: DialogDef,
    done: bool,
}

impl FindDialog {
    pub fn new() -> Self {
        let def = DialogDef::new("Cerca le immagini mancanti")
            .with_control(info(
                "Scegli un file qualsiasi nella cartella in cui si trovano ora le immagini. \
                 Ogni immagine il cui file manca viene ricollegata al file con lo stesso nome."
                    .into(),
            ))
            .with_control(DialogControl::FilePicker {
                id: "file".into(),
                label: "Un file nella cartella".into(),
                default_path: String::new(),
                filter_name: "Immagini raster".into(),
                extensions: EXTENSIONS.iter().map(|e| e.to_string()).collect(),
                save: false,
            })
            .with_control(DialogControl::Checkbox {
                id: "sub".into(),
                label: "Cerca anche nelle sottocartelle".into(),
                default: true,
            })
            .submit_label("Cerca")
            .cancel_label("Annulla")
            .width(520.0);
        FindDialog { def, done: false }
    }
}

impl Default for FindDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractiveCommand for FindDialog {
    fn prompt(&self) -> String {
        "RT_FIND: scegli la cartella nella finestra".into()
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> CommandStep {
        CommandStep::NeedPoint
    }
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }
    fn modal_dialog(&self) -> Option<DialogDef> {
        (!self.done).then(|| self.def.clone())
    }
    fn on_dialog_response(&mut self, response: DialogResponse) -> CommandStep {
        self.done = true;
        let DialogResponse::Submitted(m) = response else {
            return CommandStep::Cancel;
        };
        match get_path(&m, "file") {
            Some(p) => {
                commands::set_pending_path(p);
                let mode = if get_bool(&m, "sub", true) { "SUB" } else { "TOP" };
                CommandStep::Dispatch(format!("RT_FINDAPPLY {mode}"))
            }
            None => CommandStep::Cancel,
        }
    }
}
