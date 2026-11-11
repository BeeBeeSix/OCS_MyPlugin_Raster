//! Interactive tools: picking an image, drawing the clip window, dragging a
//! corner, and the dialogs. Interactive callbacks get no host access, so each
//! tool works on a snapshot taken when it starts and ends by handing the host
//! an apply command (see `commands.rs`).

use std::collections::HashMap;
use std::sync::Mutex;

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

/// Arguments of a command handed to the host as a bare verb (see
/// [`deferred`]).
static DEFERRED: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Hand `verb args` to the host as the bare `verb`, keeping `args` here.
///
/// Needed when the command opens a dialog or a new interactive step: the
/// host runs a multi-word line by dispatching it and then feeding the words
/// after the verb — and a final Enter — to whatever command is active, which
/// is the step just opened. That Enter ended the step at once (the dialog
/// still showed, but its button then did nothing). A single word is
/// dispatched with nothing fed after it.
pub fn deferred(verb: &str, args: String) -> CommandStep {
    *DEFERRED.lock().unwrap_or_else(|e| e.into_inner()) = Some((verb.to_string(), args));
    CommandStep::Dispatch(verb.to_string())
}

/// The arguments [`deferred`] kept for `verb`, once.
pub fn take_deferred(verb: &str) -> Option<String> {
    let mut slot = DEFERRED.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().is_some_and(|(v, _)| v.eq_ignore_ascii_case(verb)) {
        return slot.take().map(|(_, a)| a);
    }
    None
}

impl PickTool {
    pub fn new(images: Vec<Snapshot>, verb: &'static str, what: &'static str) -> Self {
        PickTool { images, verb, what, msg: None }
    }

    fn take(&mut self, idx: Option<usize>) -> CommandStep {
        match idx {
            Some(i) => deferred(self.verb, format!("{:X}", self.images[i].handle.value())),
            None => {
                self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into());
                CommandStep::NeedPoint
            }
        }
    }
}

impl InteractiveCommand for PickTool {
    fn prompt(&self) -> String {
        with_msg(&self.msg, tr!(format!("Seleziona l'immagine da {} (Invio per annullare)", self.what), format!("Select the image to {} (Enter to cancel)", self.what)))
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
            None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
        }
        CommandStep::NeedPoint
    }
}

impl InteractiveCommand for ClipTool {
    fn prompt(&self) -> String {
        let p = match (self.target, self.first) {
            (None, _) => tr!(format!("{}: seleziona l'immagine", self.kind.name()), format!("{}: select the image", self.kind.name())),
            (Some(_), None) => tr!(format!("{}: primo angolo del rettangolo", self.kind.name()), format!("{}: first corner of the rectangle", self.kind.name())),
            (Some(_), Some(_)) => tr!(format!("{}: angolo opposto", self.kind.name()), format!("{}: opposite corner", self.kind.name())),
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
            self.msg = Some(tr!("il rettangolo ha area nulla", "the rectangle has zero area").into());
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
            self.msg = Some(tr!("servono almeno 3 vertici", "at least 3 vertices are needed").into());
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
            None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
        }
        CommandStep::NeedPoint
    }
}

impl InteractiveCommand for PolyClipTool {
    fn prompt(&self) -> String {
        let name = self.kind.poly_name();
        let p = match (self.target, self.pts.len()) {
            (None, _) => tr!(format!("{name}: seleziona l'immagine"), format!("{name}: select the image")),
            (Some(_), 0) => tr!(format!("{name}: primo vertice del poligono"), format!("{name}: first vertex of the polygon")),
            (Some(_), n) if n < 3 => tr!(format!("{name}: vertice {} (Esc per annullare)", n + 1), format!("{name}: vertex {} (Esc to cancel)", n + 1)),
            (Some(_), n) => tr!(
                format!("{name}: vertice {} — Invio o clic sul primo vertice per chiudere", n + 1),
                format!("{name}: vertex {} — Enter or click the first vertex to close", n + 1)
            ),
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
            self.msg = Some(tr!("non è una polilinea chiusa, un cerchio o un'ellisse", "not a closed polyline, circle or ellipse").into());
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
            None => tr!(format!("{name}: seleziona l'immagine"), format!("{name}: select the image")),
            Some(_) => tr!(
                format!("{name}: seleziona la polilinea chiusa (o clicca al suo interno)"),
                format!("{name}: select the closed polyline (or click inside it)")
            ),
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
                    None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
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
                    None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
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
            (None, _) => tr!("RT_RESIZE: seleziona l'immagine", "RT_RESIZE: select the image").to_string(),
            (Some(_), None) => tr!(
                "RT_RESIZE: clicca vicino all'angolo che deve restare fermo",
                "RT_RESIZE: click near the corner that must stay fixed"
            )
            .to_string(),
            (Some(_), Some(_)) => tr!(
                "RT_RESIZE: porta l'angolo opposto alla nuova posizione",
                "RT_RESIZE: move the opposite corner to its new position"
            )
            .to_string(),
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
                None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
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
            self.msg = Some(tr!("porta l'angolo dalla parte dell'immagine", "move the corner towards the image").into());
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
            None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
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

/// Choice lists: (code sent to the apply command, Italian label, English label).
type Options = &'static [(&'static str, &'static str, &'static str)];

fn labels(opts: Options) -> Vec<&'static str> {
    opts.iter().map(|o| tr!(o.1, o.2)).collect()
}

const SIZE_MODES: Options = &[
    ("W", "Larghezza (proporzionale)", "Width (proportional)"),
    ("H", "Altezza (proporzionale)", "Height (proportional)"),
    ("WH", "Larghezza e altezza (libere)", "Width and height (free)"),
    ("F", "Fattore di scala", "Scale factor"),
    ("DPI", "Risoluzione di stampa (DPI)", "Print resolution (DPI)"),
];
pub const UNITS: Options = &[
    ("MM", "millimetri", "millimetres"),
    ("CM", "centimetri", "centimetres"),
    ("M", "metri", "metres"),
    ("IN", "pollici", "inches"),
];
const ANCHORS: Options = &[("BL", "Angolo in basso a sinistra", "Bottom-left corner"), ("CTR", "Centro", "Centre")];

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
        let (n, name) = (handles.len(), commands::file_name_only(&first.file_path));
        let (px_w, px_h) = (first.size.x as u64, first.size.y as u64);
        let header = tr!(
            format!("{n} immagine/i. {name}: {px_w} × {px_h} px, cornice {w:.4} × {h:.4} unità di disegno."),
            format!("{n} image(s). {name}: {px_w} × {px_h} px, frame {w:.4} × {h:.4} drawing units.")
        );
        let mut def = DialogDef::new(tr!("Dimensioni immagine raster", "Raster image size"))
            .with_control(info(header))
            .with_control(choice("mode", tr!("Modo", "Mode"), &labels(SIZE_MODES), 0))
            .with_control(number("width", tr!("Larghezza", "Width"), w))
            .with_control(number("height", tr!("Altezza", "Height"), h))
            .with_control(number("factor", tr!("Fattore di scala", "Scale factor"), 1.0))
            .with_control(number("dpi", "DPI", 300.0))
            .with_control(choice("unit", tr!("Unità del disegno (per DPI)", "Drawing units (for DPI)"), &labels(UNITS), 0))
            .with_control(choice("anchor", tr!("Punto fisso", "Fixed point"), &labels(ANCHORS), 0))
            .with_control(DialogControl::Checkbox {
                id: "visible".into(),
                label: tr!("Misure riferite alla parte visibile (ritagliata)", "Sizes refer to the visible (clipped) part").into(),
                default: clipped,
            })
            .submit_label(tr!("Applica", "Apply"))
            .cancel_label(tr!("Annulla", "Cancel"));
        def = def.width(460.0);
        SizeDialog { handles, def, done: false }
    }
}

impl InteractiveCommand for SizeDialog {
    fn prompt(&self) -> String {
        tr!("RT_SIZE: imposta le dimensioni nella finestra", "RT_SIZE: set the size in the dialog").into()
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
        let state = if found { tr!("file trovato", "file found") } else { tr!("FILE MANCANTE", "FILE MISSING") };
        let def = DialogDef::new(tr!("Percorso immagine raster", "Raster image path"))
            .with_control(info(tr!(
                format!("Percorso attuale ({state}):\n{current}"),
                format!("Current path ({state}):\n{current}")
            )))
            .with_control(info(
                tr!(
                    "Tutte le immagini che usano lo stesso file verranno ricollegate. \
                     La larghezza nel disegno resta la stessa.",
                    "Every image using the same file will be relinked. \
                     The width in the drawing stays the same."
                )
                .into(),
            ))
            .with_control(DialogControl::FilePicker {
                id: "file".into(),
                label: tr!("Nuovo file", "New file").into(),
                default_path: current,
                filter_name: tr!("Immagini raster", "Raster images").into(),
                extensions: EXTENSIONS.iter().map(|e| e.to_string()).collect(),
                save: false,
            })
            .submit_label(tr!("Ricollega", "Relink"))
            .cancel_label(tr!("Annulla", "Cancel"))
            .width(560.0);
        PathDialog { handles, def, done: false }
    }
}

impl InteractiveCommand for PathDialog {
    fn prompt(&self) -> String {
        tr!("RT_PATH: scegli il nuovo file nella finestra", "RT_PATH: choose the new file in the dialog").into()
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
        let def = DialogDef::new(tr!("Cerca le immagini mancanti", "Find missing images"))
            .with_control(info(
                tr!(
                    "Scegli un file qualsiasi nella cartella in cui si trovano ora le immagini. \
                     Ogni immagine il cui file manca viene ricollegata al file con lo stesso nome.",
                    "Choose any file in the folder where the images are now. \
                     Every image whose file is missing is relinked to the file with the same name."
                )
                .into(),
            ))
            .with_control(DialogControl::FilePicker {
                id: "file".into(),
                label: tr!("Un file nella cartella", "A file in the folder").into(),
                default_path: String::new(),
                filter_name: tr!("Immagini raster", "Raster images").into(),
                extensions: EXTENSIONS.iter().map(|e| e.to_string()).collect(),
                save: false,
            })
            .with_control(DialogControl::Checkbox {
                id: "sub".into(),
                label: tr!("Cerca anche nelle sottocartelle", "Search subfolders too").into(),
                default: true,
            })
            .submit_label(tr!("Cerca", "Search"))
            .cancel_label(tr!("Annulla", "Cancel"))
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
        tr!("RT_FIND: scegli la cartella nella finestra", "RT_FIND: choose the folder in the dialog").into()
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

// ── RT_ATTACH: insert a new image ──────────────────────────────────────────

const ATTACH_MODES: Options = &[
    ("SCREEN", "Indica sullo schermo (punto e angolo opposto)", "Specify on screen (point and opposite corner)"),
    ("WIDTH", "Larghezza nel disegno", "Width in the drawing"),
    ("DPI", "Risoluzione di stampa (DPI)", "Print resolution (DPI)"),
    ("PIXEL", "1 pixel = 1 unità di disegno", "1 pixel = 1 drawing unit"),
];

pub struct AttachDialog {
    def: DialogDef,
    done: bool,
    /// The layer list shown: (name, needs creating).
    layers: Vec<(String, bool)>,
}

/// Layer choices for a new image: RASTER first (marked new when the drawing
/// has none), then the drawing's other layers. The default is RASTER.
pub fn attach_layer_options(existing: &[String]) -> Vec<(String, bool)> {
    let raster = commands::RASTER_LAYER;
    let found = existing.iter().find(|n| n.eq_ignore_ascii_case(raster));
    let mut out = vec![match found {
        Some(n) => (n.clone(), false),
        None => (raster.to_string(), true),
    }];
    out.extend(existing.iter().filter(|n| !n.eq_ignore_ascii_case(raster)).map(|n| (n.clone(), false)));
    out
}

impl AttachDialog {
    /// `unit_index` preselects the drawing unit (index into `UNITS`);
    /// `layers` are the drawing's layers, `current` the current one.
    pub fn new(default_path: String, unit_index: usize, layers: Vec<String>, current: Option<String>) -> Self {
        let layers = attach_layer_options(&layers);
        let layer_labels: Vec<String> = layers
            .iter()
            .map(|(n, new)| {
                let mut l = n.clone();
                if *new {
                    l.push_str(tr!(" (nuovo, bianco)", " (new, white)"));
                }
                if current.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(n)) {
                    l.push_str(tr!(" — corrente", " — current"));
                }
                l
            })
            .collect();
        let def = DialogDef::new(tr!("Inserisci immagine raster", "Insert raster image"))
            .with_control(DialogControl::FilePicker {
                id: "file".into(),
                label: tr!("File immagine (anche PDF di una mappa)", "Image file (or a map PDF)").into(),
                default_path,
                filter_name: tr!("Immagini raster e PDF", "Raster images and PDF").into(),
                extensions: EXTENSIONS.iter().map(|e| e.to_string()).chain(["pdf".to_string()]).collect(),
                save: false,
            })
            .with_control(choice("mode", tr!("Dimensione", "Size"), &labels(ATTACH_MODES), 0))
            .with_control(number("width", tr!("Larghezza (unità di disegno)", "Width (drawing units)"), 100.0))
            .with_control(number("dpi", "DPI", 300.0))
            .with_control(DialogControl::Number {
                id: "mapscale".into(),
                label: tr!("Scala della mappa 1: (per DPI; 1 = carta)", "Map scale 1: (for DPI; 1 = paper)").into(),
                default: 1.0,
                min: Some(1.0),
                max: None,
                step: Some(500.0),
            })
            .with_control(choice(
                "unit",
                tr!("Unità del disegno (per DPI)", "Drawing units (for DPI)"),
                &labels(UNITS),
                unit_index.min(UNITS.len() - 1),
            ))
            .with_control(DialogControl::Choice {
                id: "layer".into(),
                label: tr!("Layer", "Layer").into(),
                options: layer_labels,
                default_index: 0,
            })
            .with_control(info(
                tr!(
                    "L'immagine va sul layer scelto (RASTER viene creato se manca) e dietro al \
                     disegno, con l'angolo in basso a sinistra nel punto indicato. Il file resta \
                     collegato. Con un PDF (es. estratto di mappa) l'immagine contenuta viene \
                     salvata accanto come <nome>_pdf.png. Mappa catastale da PDF a 300 DPI, \
                     scala 1:2000, unità metri: dimensioni reali.",
                    "The image goes on the chosen layer (RASTER is created if missing) and behind \
                     the drawing, with its bottom-left corner at the given point. The file stays \
                     linked. With a PDF (e.g. a map extract) its picture is saved next to it as \
                     <name>_pdf.png. A map PDF at 300 DPI, scale 1:2000, metres: real size."
                )
                .into(),
            ))
            .submit_label(tr!("Inserisci", "Insert"))
            .cancel_label(tr!("Annulla", "Cancel"))
            .width(540.0);
        AttachDialog { def, done: false, layers }
    }
}

impl InteractiveCommand for AttachDialog {
    fn prompt(&self) -> String {
        tr!("RT_ATTACH: scegli il file e la dimensione nella finestra", "RT_ATTACH: choose the file and size in the dialog").into()
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
        let Some(path) = get_path(&m, "file") else {
            return CommandStep::Cancel;
        };
        commands::set_pending_path(path);
        if let Some((name, create)) = self.layers.get(get_choice(&m, "layer")) {
            commands::set_pending_layer(name.clone(), *create);
        }
        let mode = ATTACH_MODES[get_choice(&m, "mode").min(ATTACH_MODES.len() - 1)].0;
        let unit = UNITS[get_choice(&m, "unit").min(UNITS.len() - 1)].0;
        deferred("RT_ATTACHPLACE", format!(
            "{} {} {} {} {}",
            mode,
            get_num(&m, "width").unwrap_or(0.0),
            get_num(&m, "dpi").unwrap_or(0.0),
            unit,
            get_num(&m, "mapscale").filter(|k| *k > 0.0).unwrap_or(1.0)
        ))
    }
}

/// Place a new image: click the insertion corner, then (when the size is
/// not fixed yet) the opposite corner — proportional. Enter at the second
/// step uses 1 pixel = 1 unit.
pub struct AttachTool {
    path: String,
    w: f64,
    h: f64,
    /// Size of one pixel in drawing units, when already decided.
    px: Option<f64>,
    ins: Option<P3>,
    msg: Option<String>,
}

impl AttachTool {
    pub fn new(path: String, w: u32, h: u32, px: Option<f64>) -> Self {
        AttachTool { path, w: w as f64, h: h as f64, px, ins: None, msg: None }
    }

    fn frame_ring(&self, corner: P3, px: f64) -> Vec<P3> {
        let (ww, hh, z) = (self.w * px, self.h * px, corner[2]);
        vec![
            corner,
            [corner[0] + ww, corner[1], z],
            [corner[0] + ww, corner[1] + hh, z],
            [corner[0], corner[1] + hh, z],
            corner,
        ]
    }

    fn finish(&mut self, corner: P3, px: f64) -> CommandStep {
        commands::set_pending_path(self.path.clone());
        CommandStep::Dispatch(format!("RT_ATTACHAPPLY {} {} {} {}", corner[0], corner[1], corner[2], px))
    }
}

impl InteractiveCommand for AttachTool {
    fn prompt(&self) -> String {
        let name = commands::file_name_only(&self.path);
        let p = match (self.ins, self.px) {
            (None, _) => tr!(
                format!("RT_ATTACH {name}: punto di inserimento (angolo in basso a sinistra)"),
                format!("RT_ATTACH {name}: insertion point (bottom-left corner)")
            ),
            (Some(_), _) => tr!(
                format!("RT_ATTACH {name}: angolo opposto (Invio = 1 pixel per unità)"),
                format!("RT_ATTACH {name}: opposite corner (Enter = 1 pixel per unit)")
            ),
        };
        with_msg(&self.msg, p)
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        if let Some(px) = self.px {
            return self.finish(pt, px);
        }
        match self.ins {
            None => {
                self.ins = Some(pt);
                CommandStep::NeedPoint
            }
            Some(a) => match geom::fit_proportional(a, pt, self.w, self.h) {
                Some((corner, px)) => self.finish(corner, px),
                None => {
                    self.msg = Some(tr!("l'angolo opposto coincide con il primo", "the opposite corner is the same as the first").into());
                    CommandStep::NeedPoint
                }
            },
        }
    }

    fn on_enter(&mut self) -> CommandStep {
        match self.ins {
            Some(a) if self.px.is_none() => self.finish(a, 1.0),
            _ => CommandStep::Cancel,
        }
    }

    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        if let Some(px) = self.px {
            return vec![PreviewWire::with_color(self.frame_ring(pt, px), CYAN)];
        }
        match self.ins {
            None => Vec::new(),
            Some(a) => match geom::fit_proportional(a, pt, self.w, self.h) {
                Some((corner, px)) => vec![
                    PreviewWire::with_color(self.frame_ring(corner, px), CYAN),
                    PreviewWire::line(a, pt, Some(GREY)),
                ],
                None => Vec::new(),
            },
        }
    }
}

// ── RT_TRANSP: pick the image, then the colour ─────────────────────────────

pub struct TranspTool {
    images: Vec<Snapshot>,
    target: Option<usize>,
    msg: Option<String>,
}

impl TranspTool {
    pub fn new(images: Vec<Snapshot>, preselected: Option<Handle>) -> Self {
        let target = preselected.and_then(|h| index_of(&images, h));
        TranspTool { images, target, msg: None }
    }

    fn choose(&mut self, idx: Option<usize>) -> CommandStep {
        match idx {
            Some(i) => {
                self.target = Some(i);
                self.msg = None;
            }
            None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
        }
        CommandStep::NeedPoint
    }
}

impl InteractiveCommand for TranspTool {
    fn prompt(&self) -> String {
        let p = match self.target {
            None => tr!("RT_TRANSP: seleziona l'immagine", "RT_TRANSP: select the image").to_string(),
            Some(_) => tr!(
                "RT_TRANSP: clicca sull'immagine il colore da rendere trasparente",
                "RT_TRANSP: click the colour to make transparent on the image"
            )
            .to_string(),
        };
        with_msg(&self.msg, p)
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let Some(t) = self.target else {
            let i = hit(&self.images, pt);
            return self.choose(i);
        };
        let snap = &self.images[t];
        let Some(p) = snap.frame.to_pixel(pt) else {
            self.msg = Some(tr!("l'immagine ha una cornice degenere", "the image has a degenerate frame").into());
            return CommandStep::NeedPoint;
        };
        // Only the part that shows can be clicked.
        if !snap.frame.contains(pt) || !geom::point_in_polygon(&snap.visible, p) {
            self.msg = Some(tr!("clicca dentro la parte visibile dell'immagine", "click inside the visible part of the image").into());
            return CommandStep::NeedPoint;
        }
        crate::diag::log(&format!("TranspTool: click on {:X} at pixel ({}, {})", snap.handle.value(), p[0], p[1]));
        deferred("RT_TRANSPPICK", format!("{:X} {} {}", snap.handle.value(), p[0], p[1]))
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
        match self.target {
            None => hit(&self.images, pt)
                .map(|i| vec![PreviewWire::with_color(self.images[i].outline(), CYAN)])
                .unwrap_or_default(),
            Some(t) => {
                // A small crosshair marks the pixel that will be sampled.
                let snap = &self.images[t];
                let (fw, fh) = snap.frame.world_size();
                let r = (fw.hypot(fh) * 0.01).max(1e-9);
                vec![
                    PreviewWire::with_color(snap.visible_outline(), CYAN),
                    PreviewWire::line([pt[0] - r, pt[1], pt[2]], [pt[0] + r, pt[1], pt[2]], Some(ORANGE)),
                    PreviewWire::line([pt[0], pt[1] - r, pt[2]], [pt[0], pt[1] + r, pt[2]], Some(ORANGE)),
                ]
            }
        }
    }
}

pub struct TranspDialog {
    handle: Handle,
    x: u32,
    y: u32,
    def: DialogDef,
    done: bool,
}

impl TranspDialog {
    /// `lossy`: JPEG/WebP, whose colours are never perfectly even — a higher
    /// default tolerance.
    /// `layer` / `ink`: the image's layer and its colour, for the option
    /// that colours the rest of the picture.
    pub fn new(handle: Handle, x: u32, y: u32, color: [u8; 3], lossy: bool, layer: String, ink: [u8; 3]) -> Self {
        let hex = crate::pixels::hex_color(color);
        let ink_hex = crate::pixels::hex_color(ink);
        let [r, g, b] = color;
        // Colouring suits a scan on light paper; a photo keeps its colours.
        let light = color.iter().all(|&c| c >= 200);
        let def = DialogDef::new(tr!("Colore trasparente", "Transparent colour"))
            .with_control(info(tr!(
                format!("Colore scelto: {hex}  (R {r}, G {g}, B {b})"),
                format!("Chosen colour: {hex}  (R {r}, G {g}, B {b})")
            )))
            .with_control(DialogControl::Number {
                id: "tol".into(),
                label: tr!(
                    "Tolleranza % (0 = solo questo colore esatto)",
                    "Tolerance % (0 = this exact colour only)"
                )
                .into(),
                default: if lossy { 10.0 } else { 2.0 },
                min: Some(0.0),
                max: Some(100.0),
                step: Some(1.0),
            })
            .with_control(DialogControl::Checkbox {
                id: "area".into(),
                label: tr!(
                    "Solo l'area collegata al punto cliccato",
                    "Only the area connected to the clicked point"
                )
                .into(),
                default: false,
            })
            .with_control(DialogControl::Checkbox {
                id: "tint".into(),
                label: tr!(
                    format!("Colora il resto con il colore del layer {layer} ({ink_hex}), bordi sfumati"),
                    format!("Colour the rest with layer {layer}'s colour ({ink_hex}), soft edges")
                ),
                default: light,
            })
            .with_control(info(
                tr!(
                    "Colorare è adatto alle scansioni di mappe e disegni: le linee nere diventano del \
                     colore del layer e risaltano sullo sfondo scuro (vale per tutta l'immagine, \
                     anche con l'opzione dell'area). Viene creata una copia PNG accanto al file \
                     originale, che non viene modificato. Per tornare indietro: Annulla (Ctrl+Z) \
                     oppure RT_TRANSPOFF.",
                    "Colouring suits scanned maps and drawings: black lines take the layer's colour \
                     and stand out on the dark background (it applies to the whole image, even with \
                     the area option). A PNG copy is created next to the original file, which is left \
                     unchanged. To go back: Undo (Ctrl+Z) or RT_TRANSPOFF."
                )
                .into(),
            ))
            .submit_label(tr!("Rendi trasparente", "Make transparent"))
            .cancel_label(tr!("Annulla", "Cancel"))
            .width(500.0);
        TranspDialog { handle, x, y, def, done: false }
    }
}

impl InteractiveCommand for TranspDialog {
    fn prompt(&self) -> String {
        tr!("RT_TRANSP: imposta la tolleranza nella finestra", "RT_TRANSP: set the tolerance in the dialog").into()
    }
    fn on_point(&mut self, _pt: [f64; 3]) -> CommandStep {
        crate::diag::log("TranspDialog: point ignored");
        CommandStep::NeedPoint
    }
    fn on_enter(&mut self) -> CommandStep {
        crate::diag::log("TranspDialog: Enter → cancel");
        CommandStep::Cancel
    }
    fn modal_dialog(&self) -> Option<DialogDef> {
        (!self.done).then(|| self.def.clone())
    }
    fn on_dialog_response(&mut self, response: DialogResponse) -> CommandStep {
        self.done = true;
        crate::diag::log(&format!("TranspDialog: response {response:?}"));
        let DialogResponse::Submitted(m) = response else {
            return CommandStep::Cancel;
        };
        let tol = get_num(&m, "tol").unwrap_or(0.0).clamp(0.0, 100.0);
        let mode = if get_bool(&m, "area", false) { "AREA" } else { "ALL" };
        let tint = if get_bool(&m, "tint", false) { " TINT" } else { "" };
        CommandStep::Dispatch(format!(
            "RT_TRANSPAPPLY {:X} {} {} {} {}{}",
            self.handle.value(),
            self.x,
            self.y,
            tol,
            mode,
            tint
        ))
    }
}

// ── Two-point alignment (georeferencing) ────────────────────────────────────

/// Pick an image, then a point on it and where it must go, then a second
/// pair: the image moves, turns and scales so both points land in place.
/// Enter after the first pair moves it only.
pub struct AlignTool {
    images: Vec<Snapshot>,
    target: Option<usize>,
    /// p1, q1, p2 so far (world).
    pts: Vec<P3>,
    msg: Option<String>,
}

impl AlignTool {
    pub fn new(images: Vec<Snapshot>, preselected: Option<Handle>) -> Self {
        let target = preselected.and_then(|h| index_of(&images, h));
        AlignTool { images, target, pts: Vec::new(), msg: None }
    }

    fn mark(&self, t: usize, p: P3) -> PreviewWire {
        let (fw, fh) = self.images[t].frame.world_size();
        PreviewWire::circle(p, (fw.hypot(fh) * 0.01).max(1e-9), Some(ORANGE))
    }

    fn preview_frame(&self, t: usize, s: &geom::Similarity) -> PreviewWire {
        let snap = &self.images[t];
        let f = s.frame(&snap.frame);
        PreviewWire::with_color(ring(snap.visible.iter().map(|&p| f.to_world(p)).collect()), ORANGE)
    }

    fn apply_line(&self, t: usize, pts: &[P3]) -> String {
        let mut line = format!("RT_ALIGNAPPLY {:X}", self.images[t].handle.value());
        for p in pts {
            line.push_str(&format!(" {} {}", p[0], p[1]));
        }
        line
    }
}

impl InteractiveCommand for AlignTool {
    fn prompt(&self) -> String {
        let p = match (self.target, self.pts.len()) {
            (None, _) => tr!("RT_ALIGN: seleziona l'immagine", "RT_ALIGN: select the image"),
            (Some(_), 0) => tr!(
                "RT_ALIGN: 1° punto sull'immagine (un punto riconoscibile, es. uno spigolo)",
                "RT_ALIGN: 1st point on the image (a recognisable point, e.g. a corner)"
            ),
            (Some(_), 1) => tr!("RT_ALIGN: dove deve andare il 1° punto", "RT_ALIGN: where the 1st point must go"),
            (Some(_), 2) => tr!(
                "RT_ALIGN: 2° punto sull'immagine, lontano dal primo  (Invio = solo spostamento)",
                "RT_ALIGN: 2nd point on the image, far from the first  (Enter = move only)"
            ),
            _ => tr!("RT_ALIGN: dove deve andare il 2° punto", "RT_ALIGN: where the 2nd point must go"),
        };
        with_msg(&self.msg, p.to_string())
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let Some(t) = self.target else {
            match hit(&self.images, pt) {
                Some(i) => {
                    self.target = Some(i);
                    self.msg = None;
                }
                None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
            }
            return CommandStep::NeedPoint;
        };
        if self.pts.len() == 2 && geom::len3([pt[0] - self.pts[0][0], pt[1] - self.pts[0][1], 0.0]) < 1e-9 {
            self.msg = Some(tr!("il 2° punto deve essere diverso dal 1°", "the 2nd point must differ from the 1st").into());
            return CommandStep::NeedPoint;
        }
        self.msg = None;
        self.pts.push(pt);
        if self.pts.len() < 4 {
            return CommandStep::NeedPoint;
        }
        CommandStep::Dispatch(self.apply_line(t, &self.pts))
    }

    fn on_enter(&mut self) -> CommandStep {
        match (self.target, self.pts.len()) {
            (Some(t), 2) => CommandStep::Dispatch(self.apply_line(t, &self.pts)),
            _ => CommandStep::Cancel,
        }
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
            None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
        }
        CommandStep::NeedPoint
    }

    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let Some(t) = self.target else {
            return hit(&self.images, pt)
                .map(|i| vec![PreviewWire::with_color(self.images[i].outline(), CYAN)])
                .unwrap_or_default();
        };
        let mut out = vec![PreviewWire::with_color(self.images[t].visible_outline(), GREY)];
        for p in &self.pts {
            out.push(self.mark(t, *p));
        }
        match self.pts.len() {
            0 => out.push(self.mark(t, pt)),
            1 => {
                let p1 = self.pts[0];
                out.push(PreviewWire::line(p1, pt, Some(ORANGE)));
                out.push(self.preview_frame(t, &geom::Similarity::translation([pt[0] - p1[0], pt[1] - p1[1]])));
            }
            2 => {
                let (p1, q1) = (self.pts[0], self.pts[1]);
                out.push(PreviewWire::line(p1, q1, Some(ORANGE)));
                out.push(self.preview_frame(t, &geom::Similarity::translation([q1[0] - p1[0], q1[1] - p1[1]])));
                out.push(self.mark(t, pt));
            }
            _ => {
                let (p1, q1, p2) = (self.pts[0], self.pts[1], self.pts[2]);
                out.push(PreviewWire::line(p1, q1, Some(ORANGE)));
                out.push(PreviewWire::line(p2, pt, Some(ORANGE)));
                if let Some(s) = geom::Similarity::from_pairs([p1[0], p1[1]], [q1[0], q1[1]], [p2[0], p2[1]], [pt[0], pt[1]]) {
                    out.push(self.preview_frame(t, &s));
                }
            }
        }
        out
    }
}

// ── Rotation ────────────────────────────────────────────────────────────────

/// Pick an image, a pivot, then a reference direction and the direction it
/// must take (aligns the image to a line). Enter instead of the reference
/// point asks for an exact angle.
pub struct RotateTool {
    images: Vec<Snapshot>,
    target: Option<usize>,
    base: Option<P3>,
    reference: Option<(P3, f64)>,
    msg: Option<String>,
}

impl RotateTool {
    pub fn new(images: Vec<Snapshot>, preselected: Option<Handle>) -> Self {
        let target = preselected.and_then(|h| index_of(&images, h));
        RotateTool { images, target, base: None, reference: None, msg: None }
    }

    fn centre(&self, t: usize) -> P3 {
        let (px, _) = self.images[t].visible_corners();
        self.images[t].frame.to_world([(px[0][0] + px[2][0]) / 2.0, (px[0][1] + px[2][1]) / 2.0])
    }
}

impl InteractiveCommand for RotateTool {
    fn prompt(&self) -> String {
        let p = match (self.target, self.base, self.reference) {
            (None, ..) => tr!("RT_ROTATE: seleziona l'immagine", "RT_ROTATE: select the image"),
            (Some(_), None, _) => tr!(
                "RT_ROTATE: punto base (centro di rotazione)  (Invio = centro dell'immagine, angolo esatto)",
                "RT_ROTATE: base point (centre of rotation)  (Enter = image centre, exact angle)"
            ),
            (Some(_), Some(_), None) => tr!(
                "RT_ROTATE: punto di riferimento sull'immagine (es. lungo una linea)  (Invio = angolo esatto)",
                "RT_ROTATE: reference point on the image (e.g. along a line)  (Enter = exact angle)"
            ),
            _ => tr!("RT_ROTATE: nuova direzione del riferimento", "RT_ROTATE: new direction of the reference"),
        };
        with_msg(&self.msg, p.to_string())
    }

    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        let Some(t) = self.target else {
            match hit(&self.images, pt) {
                Some(i) => {
                    self.target = Some(i);
                    self.msg = None;
                }
                None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
            }
            return CommandStep::NeedPoint;
        };
        let Some(base) = self.base else {
            self.base = Some(pt);
            return CommandStep::NeedPoint;
        };
        let Some(dir) = geom::direction(base, pt) else {
            self.msg = Some(tr!("scegli un punto diverso dal punto base", "pick a point other than the base point").into());
            return CommandStep::NeedPoint;
        };
        self.msg = None;
        let Some((_, from)) = self.reference else {
            self.reference = Some((pt, dir));
            return CommandStep::NeedPoint;
        };
        CommandStep::Dispatch(format!(
            "RT_ROTATEAPPLY {:X} {} {} {}",
            self.images[t].handle.value(),
            (dir - from).to_degrees(),
            base[0],
            base[1]
        ))
    }

    fn on_enter(&mut self) -> CommandStep {
        let Some(t) = self.target else { return CommandStep::Cancel };
        if self.reference.is_some() {
            return CommandStep::Cancel;
        }
        let c = self.base.unwrap_or_else(|| self.centre(t));
        deferred("RT_ROTATEDLG", format!("{:X} {} {}", self.images[t].handle.value(), c[0], c[1]))
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
            None => self.msg = Some(tr!("non è un'immagine raster", "not a raster image").into()),
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
        let mut out = vec![PreviewWire::with_color(snap.visible_outline(), GREY)];
        let Some(base) = self.base else { return out };
        let (fw, fh) = snap.frame.world_size();
        out.push(PreviewWire::circle(base, (fw.hypot(fh) * 0.01).max(1e-9), Some(ORANGE)));
        match self.reference {
            None => out.push(PreviewWire::line(base, pt, Some(GREY))),
            Some((rp, from)) => {
                out.push(PreviewWire::line(base, rp, Some(GREY)));
                out.push(PreviewWire::line(base, pt, Some(ORANGE)));
                if let Some(dir) = geom::direction(base, pt) {
                    let r = geom::Similarity::rotation([base[0], base[1]], dir - from);
                    let f = r.frame(&snap.frame);
                    out.push(PreviewWire::with_color(ring(snap.visible.iter().map(|&p| f.to_world(p)).collect()), ORANGE));
                }
            }
        }
        out
    }
}

/// Exact rotation angle around a given point.
pub struct RotateDialog {
    handle: Handle,
    centre: P2,
    def: DialogDef,
    done: bool,
}

impl RotateDialog {
    pub fn new(handle: Handle, centre: P2) -> Self {
        let def = DialogDef::new(tr!("Ruota immagine", "Rotate image"))
            .with_control(DialogControl::Number {
                id: "angle".into(),
                label: tr!("Angolo in gradi (positivo = antiorario)", "Angle in degrees (positive = counter-clockwise)").into(),
                default: 0.0,
                min: Some(-360.0),
                max: Some(360.0),
                step: Some(1.0),
            })
            .with_control(info(
                tr!(
                    format!("Centro di rotazione: {:.3}, {:.3}.", centre[0], centre[1]),
                    format!("Centre of rotation: {:.3}, {:.3}.", centre[0], centre[1])
                ),
            ))
            .submit_label(tr!("Ruota", "Rotate"));
        RotateDialog { handle, centre, def, done: false }
    }
}

impl InteractiveCommand for RotateDialog {
    fn prompt(&self) -> String {
        tr!("RT_ROTATE: angolo nella finestra", "RT_ROTATE: angle in the dialog").into()
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
        let angle = get_num(&m, "angle").unwrap_or(0.0);
        if angle.abs() < 1e-12 {
            return CommandStep::Cancel;
        }
        CommandStep::Dispatch(format!("RT_ROTATEAPPLY {:X} {} {} {}", self.handle.value(), angle, self.centre[0], self.centre[1]))
    }
}

// ── RT_PURGE confirmation ───────────────────────────────────────────────────

pub struct PurgeDialog {
    def: DialogDef,
    done: bool,
}

impl PurgeDialog {
    pub fn new(paths: Vec<String>) -> Self {
        let n = paths.len();
        let mut list: Vec<String> = paths.iter().take(12).map(|p| format!("• {}", commands::file_name_only(p))).collect();
        if n > 12 {
            list.push(tr!(format!("… e altre {}", n - 12), format!("… and {} more", n - 12)));
        }
        let def = DialogDef::new(tr!("Pulisci le copie trasparenti", "Clean up transparent copies"))
            .with_control(info(tr!(
                format!("{n} copie create da \"Colore trasparente\" non sono usate da nessuna immagine di questo disegno:"),
                format!("{n} copies made by \"Transparent colour\" are not used by any image of this drawing:")
            )))
            .with_control(info(list.join("\n")))
            .with_control(info(
                tr!(
                    "Verranno spostate nella sottocartella \"_trasp-non-usate\", non cancellate: \
                     un altro disegno potrebbe usarle. Quando sei sicuro, cancella quella cartella.",
                    "They will be moved to the \"_trasp-non-usate\" subfolder, not deleted: \
                     another drawing may use them. Once you are sure, delete that folder."
                )
                .into(),
            ))
            .submit_label(tr!("Sposta", "Move"))
            .cancel_label(tr!("Annulla", "Cancel"))
            .width(560.0);
        PurgeDialog { def, done: false }
    }
}

impl InteractiveCommand for PurgeDialog {
    fn prompt(&self) -> String {
        tr!("RT_PURGE: conferma nella finestra", "RT_PURGE: confirm in the dialog").into()
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
        match response {
            DialogResponse::Submitted(_) => CommandStep::Dispatch("RT_PURGEAPPLY".into()),
            _ => CommandStep::Cancel,
        }
    }
}

// ── RT_GUIDE ────────────────────────────────────────────────────────────────

/// The guide's sections: (title, text), Italian and English.
pub fn guide_sections() -> Vec<(&'static str, &'static str)> {
    if crate::i18n::italian() {
        vec![
            ("Inserisci immagine", "Scegli il file (JPG, PNG, TIFF… o il PDF di un estratto di mappa), il modo di dimensionarla e il layer (di serie RASTER), poi clicca il punto in basso a sinistra. Per una mappa catastale da PDF: DPI 300, scala 1:2000, unità metri. L'immagine va dietro al disegno."),
            ("Allinea con 2 punti", "Georeferenzia: clicca un punto riconoscibile sull'immagine e dove deve andare, poi un secondo punto lontano dal primo e la sua destinazione. L'immagine si sposta, ruota e scala da sola. Invio dopo la prima coppia = solo spostamento."),
            ("Ruota", "Clicca il centro di rotazione, poi un punto di riferimento sull'immagine e la nuova direzione (allinea a una linea). Invio = angolo esatto in una finestra."),
            ("Porta dietro", "Mette le immagini scelte dietro alle linee del disegno."),
            ("Ritaglia / Foro / Separa pezzo", "Clic sull'immagine e due angoli: tiene l'interno, nasconde l'interno o lo stacca in un'immagine nuova da spostare. Dal menu ▾: con un poligono disegnato a clic o con una polilinea chiusa, un cerchio, un'ellisse. Togli ritaglio annulla."),
            ("Colore trasparente", "Clic sull'immagine, poi sul colore da togliere (es. il bianco della carta). Tolleranza per i colori simili; \"Colora il resto con il colore del layer\" rende le linee del colore del layer, leggibili sullo sfondo scuro. Crea una copia PNG, l'originale resta. Togli trasparenza la spegne."),
            ("Trascina / Dimensioni / DPI", "Ingrandisci a occhio tenendo fermo un angolo, oppure a larghezza, altezza, fattore di scala o DPI esatti."),
            ("Cambia percorso / Cerca in cartella", "Collega l'immagine a un altro file; se le foto sono state spostate, scegli un file nella nuova cartella e il plugin ritrova le altre."),
            ("Elenco / Pulisci copie", "Elenco: immagini con percorso, pixel, dimensioni e stato del file. Pulisci copie: sposta in una sottocartella le copie trasparenti non più usate."),
            ("Altro", "Ogni comando si annulla con Ctrl+Z. Comandi da tastiera: RT_HELP (riepilogo), RT_LANG IT|EN|AUTO (lingua), RT_DIAG ON|OFF (registro diagnostico). Progetto amatoriale, senza garanzia: tieni una copia dei disegni."),
        ]
    } else {
        vec![
            ("Insert image", "Choose the file (JPG, PNG, TIFF… or a map extract PDF), how to size it and the layer (RASTER by default), then click the bottom-left point. For a cadastral map PDF: 300 DPI, scale 1:2000, metres. The image goes behind the drawing."),
            ("Align with 2 points", "Georeference: click a recognisable point on the image and where it must go, then a second point far from the first and its target. The image moves, turns and scales by itself. Enter after the first pair = move only."),
            ("Rotate", "Click the centre of rotation, then a reference point on the image and its new direction (aligns to a line). Enter = exact angle in a dialog."),
            ("Send behind", "Puts the chosen images behind the drawing's lines."),
            ("Clip / Hole / Split piece", "Click the image and two corners: keeps the inside, hides the inside or splits it into a new movable image. From the ▾ menu: with a polygon drawn by clicks or with a closed polyline, circle or ellipse. Remove clip undoes it."),
            ("Transparent colour", "Click the image, then the colour to drop (e.g. the paper's white). Tolerance for similar colours; \"Colour the rest with the layer's colour\" makes the lines the layer's colour, readable on the dark background. Makes a PNG copy, the original stays. Transparency off turns it off."),
            ("Drag / Size / DPI", "Enlarge by eye keeping a corner fixed, or to an exact width, height, scale factor or DPI."),
            ("Change path / Find in folder", "Link the image to another file; if the photos were moved, pick a file in the new folder and the plugin finds the others."),
            ("List / Clean up copies", "List: images with path, pixels, size and file status. Clean up copies: moves unused transparent copies to a subfolder."),
            ("More", "Every command undoes with Ctrl+Z. Typed commands: RT_HELP (summary), RT_LANG IT|EN|AUTO (language), RT_DIAG ON|OFF (diagnostic log). Hobby project, no warranty: keep a copy of your drawings."),
        ]
    }
}

pub struct GuideDialog {
    def: DialogDef,
    done: bool,
}

impl GuideDialog {
    pub fn new() -> Self {
        let mut def = DialogDef::new(tr!("Raster Tools — guida rapida", "Raster Tools — quick guide"));
        for (title, text) in guide_sections() {
            def = def.with_control(info(format!("▸ {title}\n{text}")));
        }
        let def = def.submit_label(tr!("Chiudi", "Close")).cancel_label(tr!("Chiudi", "Close")).width(680.0);
        GuideDialog { def, done: false }
    }
}

impl Default for GuideDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractiveCommand for GuideDialog {
    fn prompt(&self) -> String {
        tr!("RT_GUIDE: guida rapida", "RT_GUIDE: quick guide").into()
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
    fn on_dialog_response(&mut self, _response: DialogResponse) -> CommandStep {
        self.done = true;
        CommandStep::Cancel
    }
}
