//! Pure geometry for raster image frames.
//!
//! No codec or host types live here, so this module unit-tests with plain
//! `rustc --test` and its maths can be checked without a host build.
//!
//! Three coordinate systems are involved:
//!
//! * **world** — drawing coordinates (`[x, y, z]`);
//! * **pixel** (`px`) — the image frame's own axes: `a` along the u-vector,
//!   `b` along the v-vector, `(0, 0)` at the insertion corner (bottom-left),
//!   `(w, h)` at the opposite corner. `world = ins + a·u + b·v`;
//! * **clip** — what a DWG stores in an IMAGE clip boundary: pixel centres at
//!   whole numbers, row 0 at the *top*, so the frame edge sits at -0.5.
//!   `clip = (a - 0.5, h - b - 0.5)`. This mirrors Open CAD Studio's own
//!   `image_world_to_clip` / `image_clip_polygon`, so a boundary written here
//!   draws exactly where it was picked.

pub type P2 = [f64; 2];
pub type P3 = [f64; 3];

const EPS: f64 = 1e-9;

/// An image frame: insertion corner, per-pixel u/v vectors, size in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub ins: P3,
    pub u: P3,
    pub v: P3,
    pub w: f64,
    pub h: f64,
}

fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: P3, k: f64) -> P3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub fn len3(a: P3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

impl Frame {
    /// Pixel → world.
    pub fn to_world(&self, p: P2) -> P3 {
        add(self.ins, add(mul(self.u, p[0]), mul(self.v, p[1])))
    }

    /// World → pixel, solved in the image plane's x/y (as the host does).
    /// `None` for a degenerate frame.
    pub fn to_pixel(&self, p: P3) -> Option<P2> {
        let (u, v) = (self.u, self.v);
        let d = [p[0] - self.ins[0], p[1] - self.ins[1]];
        let det = u[0] * v[1] - u[1] * v[0];
        if det.abs() < 1e-300 || !det.is_finite() {
            return None;
        }
        Some([
            (d[0] * v[1] - d[1] * v[0]) / det,
            (u[0] * d[1] - u[1] * d[0]) / det,
        ])
    }

    /// The whole frame as a counter-clockwise pixel polygon.
    pub fn full_px(&self) -> Vec<P2> {
        vec![[0.0, 0.0], [self.w, 0.0], [self.w, self.h], [0.0, self.h]]
    }

    /// The four world corners, counter-clockwise from the insertion corner.
    pub fn corners(&self) -> [P3; 4] {
        [
            self.to_world([0.0, 0.0]),
            self.to_world([self.w, 0.0]),
            self.to_world([self.w, self.h]),
            self.to_world([0.0, self.h]),
        ]
    }

    /// Whether a world point lies on the image (inside its frame).
    pub fn contains(&self, p: P3) -> bool {
        match self.to_pixel(p) {
            Some([a, b]) => a >= -EPS && b >= -EPS && a <= self.w + EPS && b <= self.h + EPS,
            None => false,
        }
    }

    /// World width / height of the full frame.
    pub fn world_size(&self) -> (f64, f64) {
        (len3(self.u) * self.w, len3(self.v) * self.h)
    }

    /// Frame scaled by `sx` along u and `sy` along v so that the pixel
    /// position `anchor_px` stays where it is in the world. Returns the new
    /// `(insertion, u, v)`.
    pub fn scaled(&self, anchor_px: P2, sx: f64, sy: f64) -> (P3, P3, P3) {
        let anchor = self.to_world(anchor_px);
        let u2 = mul(self.u, sx);
        let v2 = mul(self.v, sy);
        let ins2 = sub(sub(anchor, mul(u2, anchor_px[0])), mul(v2, anchor_px[1]));
        (ins2, u2, v2)
    }
}

/// Pixel → clip space.
pub fn px_to_clip(h: f64, p: P2) -> P2 {
    [p[0] - 0.5, h - p[1] - 0.5]
}

/// Clip → pixel space.
pub fn clip_to_px(h: f64, c: P2) -> P2 {
    [c[0] + 0.5, h - c[1] - 0.5]
}

/// Signed area (positive = counter-clockwise).
pub fn signed_area(poly: &[P2]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        s += p[0] * q[1] - q[0] * p[1];
    }
    s * 0.5
}

/// Drop consecutive duplicates and a closing vertex equal to the first.
pub fn dedup(poly: &[P2], eps: f64) -> Vec<P2> {
    let mut out: Vec<P2> = Vec::with_capacity(poly.len());
    for &p in poly {
        if let Some(last) = out.last() {
            if (last[0] - p[0]).abs() <= eps && (last[1] - p[1]).abs() <= eps {
                continue;
            }
        }
        out.push(p);
    }
    while out.len() > 1 {
        let (f, l) = (out[0], out[out.len() - 1]);
        if (f[0] - l[0]).abs() <= eps && (f[1] - l[1]).abs() <= eps {
            out.pop();
        } else {
            break;
        }
    }
    out
}

/// Sutherland–Hodgman: `subject` clipped by the convex polygon `clip`
/// (either orientation). The result may be empty.
pub fn clip_convex(subject: &[P2], clip: &[P2]) -> Vec<P2> {
    let mut clip: Vec<P2> = dedup(clip, 0.0);
    if clip.len() < 3 {
        return Vec::new();
    }
    if signed_area(&clip) < 0.0 {
        clip.reverse();
    }
    let inside = |p: P2, a: P2, b: P2| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) >= -1e-12;
    let intersect = |p: P2, q: P2, a: P2, b: P2| -> P2 {
        let (r, s) = ([q[0] - p[0], q[1] - p[1]], [b[0] - a[0], b[1] - a[1]]);
        let den = r[0] * s[1] - r[1] * s[0];
        if den.abs() < 1e-300 {
            return q;
        }
        let t = ((a[0] - p[0]) * s[1] - (a[1] - p[1]) * s[0]) / den;
        [p[0] + t * r[0], p[1] + t * r[1]]
    };
    let mut out: Vec<P2> = dedup(subject, 0.0);
    for i in 0..clip.len() {
        if out.is_empty() {
            break;
        }
        let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let cur = input[j];
            let prev = input[(j + input.len() - 1) % input.len()];
            let (ci, pi) = (inside(cur, a, b), inside(prev, a, b));
            if ci {
                if !pi {
                    out.push(intersect(prev, cur, a, b));
                }
                out.push(cur);
            } else if pi {
                out.push(intersect(prev, cur, a, b));
            }
        }
    }
    out
}

/// Axis-aligned bounding box of a polygon.
pub fn bbox(poly: &[P2]) -> Option<(P2, P2)> {
    let first = *poly.first()?;
    let (mut lo, mut hi) = (first, first);
    for p in &poly[1..] {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    Some((lo, hi))
}

/// `Some((min, max))` when the polygon is an axis-aligned rectangle.
pub fn as_axis_rect(poly: &[P2], eps: f64) -> Option<(P2, P2)> {
    let p = dedup(poly, eps);
    let (lo, hi) = bbox(&p)?;
    if p.len() != 4 {
        return None;
    }
    let on_corner = |q: P2| {
        ((q[0] - lo[0]).abs() <= eps || (q[0] - hi[0]).abs() <= eps)
            && ((q[1] - lo[1]).abs() <= eps || (q[1] - hi[1]).abs() <= eps)
    };
    if p.iter().all(|&q| on_corner(q)) {
        Some((lo, hi))
    } else {
        None
    }
}

/// The four pixel-space corners of the world axis-aligned rectangle spanned
/// by `p1`/`p2` (a window drawn on screen). `None` for a degenerate frame.
pub fn world_rect_to_px(frame: &Frame, p1: P3, p2: P3) -> Option<Vec<P2>> {
    let (x0, x1) = (p1[0].min(p2[0]), p1[0].max(p2[0]));
    let (y0, y1) = (p1[1].min(p2[1]), p1[1].max(p2[1]));
    let z = p1[2];
    [[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]]
        .iter()
        .map(|&c| frame.to_pixel(c))
        .collect()
}

/// A clip boundary in DWG clip space.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// Two opposite corners `(min, max)`.
    Rect(P2, P2),
    /// Polygon, closed (first vertex repeated at the end, as DWG stores it).
    Poly(Vec<P2>),
}

impl Shape {
    pub fn vertices(&self) -> Vec<P2> {
        match self {
            Shape::Rect(a, b) => vec![*a, *b],
            Shape::Poly(p) => p.clone(),
        }
    }
}

/// Smallest region worth keeping, in square pixels.
pub const MIN_AREA_PX: f64 = 1e-6;

/// Turn a pixel-space region into a clip-space boundary: a rectangle when it
/// is one, otherwise a closed polygon. `None` when it is (nearly) empty.
pub fn shape_from_px(h: f64, poly_px: &[P2]) -> Option<Shape> {
    let poly = ccw(&dedup(poly_px, 1e-9));
    if poly.len() < 3 || signed_area(&poly).abs() < MIN_AREA_PX {
        return None;
    }
    if let Some((lo, hi)) = as_axis_rect(&poly, 1e-7) {
        let (c0, c1) = (px_to_clip(h, lo), px_to_clip(h, hi));
        return Some(Shape::Rect(
            [c0[0].min(c1[0]), c0[1].min(c1[1])],
            [c0[0].max(c1[0]), c0[1].max(c1[1])],
        ));
    }
    let mut ring: Vec<P2> = poly.iter().map(|&p| px_to_clip(h, p)).collect();
    ring.push(ring[0]);
    Some(Shape::Poly(ring))
}

/// An existing clip boundary (clip space) as a pixel-space polygon.
/// `rect` selects the two-corner rectangular form.
pub fn region_px_from_clip(h: f64, rect: bool, verts: &[P2]) -> Option<Vec<P2>> {
    if rect {
        if verts.len() < 2 {
            return None;
        }
        let (a, b) = (clip_to_px(h, verts[0]), clip_to_px(h, verts[1]));
        let (lo, hi) = ([a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]);
        Some(vec![lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]])
    } else {
        let p: Vec<P2> = dedup(verts, 0.0).iter().map(|&c| clip_to_px(h, c)).collect();
        (p.len() >= 3).then_some(p)
    }
}

/// Clip-space vertices re-expressed for an image whose pixel count changed
/// from `ow × oh` to `nw × nh` while it keeps covering the same frame.
pub fn rescale_clip(verts: &[P2], ow: f64, oh: f64, nw: f64, nh: f64) -> Vec<P2> {
    let (kx, ky) = (nw / ow, nh / oh);
    verts
        .iter()
        .map(|c| [(c[0] + 0.5) * kx - 0.5, (c[1] + 0.5) * ky - 0.5])
        .collect()
}

/// Factor that brings `opp` to the cursor along the anchor→opp diagonal
/// (proportional resize by dragging a corner).
pub fn drag_factor(anchor: P3, opp: P3, cursor: P3) -> f64 {
    let d = [opp[0] - anchor[0], opp[1] - anchor[1]];
    let c = [cursor[0] - anchor[0], cursor[1] - anchor[1]];
    let dd = d[0] * d[0] + d[1] * d[1];
    if dd < 1e-300 {
        return 1.0;
    }
    (c[0] * d[0] + c[1] * d[1]) / dd
}

/// Index of the point nearest `p` (in x/y).
pub fn nearest(points: &[P3], p: P3) -> usize {
    let mut best = (0usize, f64::INFINITY);
    for (i, q) in points.iter().enumerate() {
        let d = (q[0] - p[0]).hypot(q[1] - p[1]);
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

/// Drawing units per inch for a named length unit, for DPI sizing.
pub fn units_per_inch(unit: &str) -> Option<f64> {
    match unit.to_ascii_uppercase().as_str() {
        "MM" => Some(25.4),
        "CM" => Some(2.54),
        "M" => Some(0.0254),
        "IN" | "INCH" | "POLLICI" => Some(1.0),
        _ => None,
    }
}

// ── Polygons ────────────────────────────────────────────────────────────────

/// Counter-clockwise copy of a polygon.
pub fn ccw(poly: &[P2]) -> Vec<P2> {
    let mut p = poly.to_vec();
    if signed_area(&p) < 0.0 {
        p.reverse();
    }
    p
}

/// Whether a (deduplicated) polygon is convex.
pub fn is_convex(poly: &[P2]) -> bool {
    let p = dedup(poly, 1e-12);
    let n = p.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0f64;
    for i in 0..n {
        let (a, b, c) = (p[i], p[(i + 1) % n], p[(i + 2) % n]);
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if cross.abs() <= 1e-12 * (1.0 + cross.abs()) {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn segments_cross(a: P2, b: P2, c: P2, d: P2) -> bool {
    let orient = |p: P2, q: P2, r: P2| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let on_seg = |p: P2, q: P2, r: P2| {
        r[0] >= p[0].min(q[0]) - 1e-12
            && r[0] <= p[0].max(q[0]) + 1e-12
            && r[1] >= p[1].min(q[1]) - 1e-12
            && r[1] <= p[1].max(q[1]) + 1e-12
    };
    let (d1, d2, d3, d4) = (orient(c, d, a), orient(c, d, b), orient(a, b, c), orient(a, b, d));
    let eps = 1e-12;
    if ((d1 > eps && d2 < -eps) || (d1 < -eps && d2 > eps)) && ((d3 > eps && d4 < -eps) || (d3 < -eps && d4 > eps)) {
        return true;
    }
    (d1.abs() <= eps && on_seg(c, d, a))
        || (d2.abs() <= eps && on_seg(c, d, b))
        || (d3.abs() <= eps && on_seg(a, b, c))
        || (d4.abs() <= eps && on_seg(a, b, d))
}

/// Whether a closed polygon has no self-intersections (edges that are not
/// neighbours never touch).
pub fn is_simple(poly: &[P2]) -> bool {
    let p = dedup(poly, 1e-12);
    let n = p.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        let (a, b) = (p[i], p[(i + 1) % n]);
        for j in (i + 1)..n {
            // Skip the edge itself and its two neighbours.
            if j == i || j == (i + 1) % n || (j + 1) % n == i {
                continue;
            }
            let (c, d) = (p[j], p[(j + 1) % n]);
            if segments_cross(a, b, c, d) {
                return false;
            }
        }
    }
    true
}

/// Even-odd point-in-polygon test (x/y).
pub fn point_in_polygon(poly: &[P2], p: P2) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
            if p[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Points of a polyline whose vertices carry DXF bulges (`tan(sweep/4)`,
/// positive = counter-clockwise), arcs flattened to chords of at most
/// `max_step` radians. `closed` adds the segment back to the first vertex.
/// The last vertex is not repeated.
pub fn flatten_bulged(verts: &[(P2, f64)], closed: bool, max_step: f64) -> Vec<P2> {
    let n = verts.len();
    let mut out = Vec::with_capacity(n * 2);
    if n == 0 {
        return out;
    }
    let segs = if closed { n } else { n - 1 };
    out.push(verts[0].0);
    for i in 0..segs {
        let (p, b) = verts[i];
        let q = verts[(i + 1) % n].0;
        if b.abs() > 1e-12 {
            let theta = 4.0 * b.atan();
            let (mx, my) = ((p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5);
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            // Centre: chord midpoint moved along the chord's left normal.
            let k = (1.0 - b * b) / (4.0 * b);
            let c = [mx - dy * k, my + dx * k];
            let r = (p[0] - c[0]).hypot(p[1] - c[1]);
            let a0 = (p[1] - c[1]).atan2(p[0] - c[0]);
            let steps = ((theta.abs() / max_step).ceil() as usize).max(1);
            for s in 1..steps {
                let a = a0 + theta * s as f64 / steps as f64;
                out.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
            }
        }
        if i + 1 < n || !closed {
            out.push(q);
        }
    }
    out
}

/// A full ellipse (a circle when `ratio == 1`) as a ring of `steps` points.
pub fn ellipse_ring(center: P2, major: P2, ratio: f64, steps: usize) -> Vec<P2> {
    let minor = [-major[1] * ratio, major[0] * ratio];
    (0..steps)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / steps as f64;
            let (c, s) = (t.cos(), t.sin());
            [center[0] + major[0] * c + minor[0] * s, center[1] + major[1] * c + minor[1] * s]
        })
        .collect()
}

/// Proportional placement of a `w × h` pixel image from a fixed corner
/// `anchor` towards `cursor` (any quadrant): the frame is as large as needed
/// to reach the cursor along x or y. Returns the bottom-left corner and the
/// size of one pixel in drawing units.
pub fn fit_proportional(anchor: P3, cursor: P3, w: f64, h: f64) -> Option<(P3, f64)> {
    let (dx, dy) = (cursor[0] - anchor[0], cursor[1] - anchor[1]);
    let k = (dx.abs() / w).max(dy.abs() / h);
    if !(k.is_finite() && k > 1e-12) {
        return None;
    }
    let (ww, hh) = (k * w, k * h);
    let x0 = if dx < 0.0 { anchor[0] - ww } else { anchor[0] };
    let y0 = if dy < 0.0 { anchor[1] - hh } else { anchor[1] };
    Some(([x0, y0, anchor[2]], k))
}

/// A plane similarity `p ↦ a·p + t`, with `a` a complex number (`[re, im]`):
/// rotation by `arg a`, scale `|a|`, then translation `t`. Z is kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Similarity {
    pub a: P2,
    pub t: P2,
}

impl Similarity {
    /// The map that takes `p1` to `q1` and `p2` to `q2` (two-point
    /// alignment: move, rotate and scale at once). `None` when `p1` and
    /// `p2` coincide.
    pub fn from_pairs(p1: P2, q1: P2, p2: P2, q2: P2) -> Option<Similarity> {
        let dp = [p2[0] - p1[0], p2[1] - p1[1]];
        let dq = [q2[0] - q1[0], q2[1] - q1[1]];
        let n = dp[0] * dp[0] + dp[1] * dp[1];
        if n < 1e-18 || !n.is_finite() {
            return None;
        }
        // a = dq / dp (complex division)
        let a = [(dq[0] * dp[0] + dq[1] * dp[1]) / n, (dq[1] * dp[0] - dq[0] * dp[1]) / n];
        if a[0].hypot(a[1]) < 1e-12 {
            return None;
        }
        let t = [q1[0] - (a[0] * p1[0] - a[1] * p1[1]), q1[1] - (a[0] * p1[1] + a[1] * p1[0])];
        Some(Similarity { a, t })
    }

    /// Rotation by `angle` radians (counter-clockwise) around `centre`.
    pub fn rotation(centre: P2, angle: f64) -> Similarity {
        let a = [angle.cos(), angle.sin()];
        let t = [centre[0] - (a[0] * centre[0] - a[1] * centre[1]), centre[1] - (a[0] * centre[1] + a[1] * centre[0])];
        Similarity { a, t }
    }

    /// A pure move.
    pub fn translation(d: P2) -> Similarity {
        Similarity { a: [1.0, 0.0], t: d }
    }

    pub fn scale(&self) -> f64 {
        self.a[0].hypot(self.a[1])
    }

    /// Rotation in degrees, in (-180, 180].
    pub fn angle_deg(&self) -> f64 {
        self.a[1].atan2(self.a[0]).to_degrees()
    }

    pub fn point(&self, p: P3) -> P3 {
        [
            self.a[0] * p[0] - self.a[1] * p[1] + self.t[0],
            self.a[0] * p[1] + self.a[1] * p[0] + self.t[1],
            p[2],
        ]
    }

    /// A direction (no translation).
    pub fn vector(&self, v: P3) -> P3 {
        [self.a[0] * v[0] - self.a[1] * v[1], self.a[0] * v[1] + self.a[1] * v[0], v[2]]
    }

    /// The frame moved by this map: the same pixels land on the mapped points.
    pub fn frame(&self, f: &Frame) -> Frame {
        Frame { ins: self.point(f.ins), u: self.vector(f.u), v: self.vector(f.v), ..*f }
    }
}

/// The angle, in radians, of the direction from `a` to `b`.
pub fn direction(a: P3, b: P3) -> Option<f64> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    (dx.hypot(dy) > EPS).then(|| dy.atan2(dx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close3(a: P3, b: P3) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9 && (a[2] - b[2]).abs() < 1e-9
    }

    #[test]
    fn two_point_alignment() {
        // image 100 x 50 px, 1 unit per pixel, at the origin
        let f = Frame { ins: [0.0, 0.0, 0.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], w: 100.0, h: 50.0 };
        // pixel corners (0,0) and (100,0) must land on (1000,2000) and (1000,2200):
        // 90° turn, scale 2
        let s = Similarity::from_pairs([0.0, 0.0], [1000.0, 2000.0], [100.0, 0.0], [1000.0, 2200.0]).unwrap();
        assert!((s.scale() - 2.0).abs() < 1e-12 && (s.angle_deg() - 90.0).abs() < 1e-9);
        let g = s.frame(&f);
        assert!(close3(g.ins, [1000.0, 2000.0, 0.0]));
        assert!(close3(g.to_world([100.0, 0.0]), [1000.0, 2200.0, 0.0]));
        assert!(close3(g.to_world([0.0, 50.0]), [900.0, 2000.0, 0.0]));
        assert!(close3(g.u, [0.0, 2.0, 0.0]) && close3(g.v, [-2.0, 0.0, 0.0]));
        // the two picked points map exactly, also off the corners
        let s = Similarity::from_pairs([12.5, 7.0], [3.0, 4.0], [80.0, 33.0], [10.0, -1.0]).unwrap();
        assert!(close3(s.point([12.5, 7.0, 5.0]), [3.0, 4.0, 5.0]));
        assert!(close3(s.point([80.0, 33.0, 0.0]), [10.0, -1.0, 0.0]));
        assert!(Similarity::from_pairs([1.0, 1.0], [0.0, 0.0], [1.0, 1.0], [5.0, 5.0]).is_none());
    }

    #[test]
    fn rotation_about_a_point() {
        let r = Similarity::rotation([10.0, 10.0], std::f64::consts::FRAC_PI_2);
        assert!(close3(r.point([20.0, 10.0, 1.0]), [10.0, 20.0, 1.0]));
        assert!(close3(r.point([10.0, 10.0, 0.0]), [10.0, 10.0, 0.0]));
        assert!((r.scale() - 1.0).abs() < 1e-12);
        let m = Similarity::translation([5.0, -2.0]);
        assert!(close3(m.point([1.0, 1.0, 0.0]), [6.0, -1.0, 0.0]));
        assert!((direction([0.0, 0.0, 0.0], [0.0, 3.0, 0.0]).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn proportional_fit_any_quadrant() {
        // 200 x 100 px; cursor 50 right, 10 up → width rules: 0.25/px.
        let (c, k) = fit_proportional([10.0, 10.0, 0.0], [60.0, 20.0, 0.0], 200.0, 100.0).unwrap();
        assert!(near(k, 0.25) && near(c[0], 10.0) && near(c[1], 10.0));
        // cursor down-left, height rules: 40/100 = 0.4/px → 80 x 40 frame.
        let (c, k) = fit_proportional([10.0, 10.0, 0.0], [0.0, -30.0, 0.0], 200.0, 100.0).unwrap();
        assert!(near(k, 0.4) && near(c[0], -70.0) && near(c[1], -30.0));
        assert!(fit_proportional([1.0, 1.0, 0.0], [1.0, 1.0, 0.0], 10.0, 10.0).is_none());
    }

    #[test]
    fn convexity_and_simplicity() {
        let square = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        let ell = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 2.0], [0.0, 2.0]];
        let bow = [[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]];
        assert!(is_convex(&square) && is_simple(&square));
        assert!(!is_convex(&ell) && is_simple(&ell));
        assert!(!is_simple(&bow));
        assert!(point_in_polygon(&ell, [0.5, 1.5]));
        assert!(!point_in_polygon(&ell, [1.5, 1.5]));
    }

    #[test]
    fn bulge_semicircle() {
        // From (0,0) to (2,0) with bulge 1: a CCW half circle through (1,-1).
        let pts = flatten_bulged(&[([0.0, 0.0], 1.0), ([2.0, 0.0], 0.0)], false, 0.1);
        assert!(pts.len() > 10);
        for p in &pts {
            assert!(near((p[0] - 1.0).hypot(p[1]), 1.0), "{p:?}");
            assert!(p[1] <= 1e-9);
        }
        assert_eq!(*pts.last().unwrap(), [2.0, 0.0]);
        // Closed square with one arc side: no repeated first vertex.
        let ring = flatten_bulged(
            &[([0.0, 0.0], 0.0), ([2.0, 0.0], 0.0), ([2.0, 2.0], 0.0), ([0.0, 2.0], 0.0)],
            true,
            0.1,
        );
        assert_eq!(ring.len(), 4);
    }

    #[test]
    fn concave_polygon_inside_frame_keeps_its_shape() {
        let f = Frame { ins: [0.0, 0.0, 0.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], w: 100.0, h: 100.0 };
        let ell = vec![[10.0, 10.0], [60.0, 10.0], [60.0, 30.0], [30.0, 30.0], [30.0, 60.0], [10.0, 60.0]];
        let r = clip_convex(&ell, &f.full_px());
        match shape_from_px(f.h, &r).unwrap() {
            Shape::Poly(ring) => assert_eq!(ring.len(), 7),
            s => panic!("{s:?}"),
        }
    }

    #[test]
    fn ellipse_ring_is_on_curve() {
        let r = ellipse_ring([1.0, 2.0], [3.0, 0.0], 0.5, 32);
        assert_eq!(r.len(), 32);
        for p in r {
            let (x, y) = ((p[0] - 1.0) / 3.0, (p[1] - 2.0) / 1.5);
            assert!(near(x * x + y * y, 1.0));
        }
    }

    fn sample() -> Frame {
        // 200 x 100 px, 0.5 units per pixel, inserted at (10, 20).
        Frame { ins: [10.0, 20.0, 0.0], u: [0.5, 0.0, 0.0], v: [0.0, 0.5, 0.0], w: 200.0, h: 100.0 }
    }

    #[test]
    fn pixel_world_round_trip() {
        let f = sample();
        let p = f.to_world([40.0, 30.0]);
        assert!(near(p[0], 30.0) && near(p[1], 35.0));
        let q = f.to_pixel(p).unwrap();
        assert!(near(q[0], 40.0) && near(q[1], 30.0));
    }

    #[test]
    fn clip_space_matches_host_convention() {
        // Bottom-left frame corner → (-0.5, h - 0.5); top-right → (w - 0.5, -0.5).
        let c = px_to_clip(100.0, [0.0, 0.0]);
        assert!(near(c[0], -0.5) && near(c[1], 99.5));
        let c = px_to_clip(100.0, [200.0, 100.0]);
        assert!(near(c[0], 199.5) && near(c[1], -0.5));
        let back = clip_to_px(100.0, c);
        assert!(near(back[0], 200.0) && near(back[1], 100.0));
    }

    #[test]
    fn window_inside_axis_aligned_image_is_a_rect() {
        let f = sample();
        // World window (20,25)-(40,40) → px (20,10)-(60,40).
        let px = world_rect_to_px(&f, [40.0, 40.0, 0.0], [20.0, 25.0, 0.0]).unwrap();
        let region = clip_convex(&f.full_px(), &px);
        match shape_from_px(f.h, &region).unwrap() {
            Shape::Rect(lo, hi) => {
                assert!(near(lo[0], 19.5) && near(hi[0], 59.5), "{lo:?} {hi:?}");
                // b 10..40 → clip y = 100 - b - 0.5 → 59.5 .. 89.5
                assert!(near(lo[1], 59.5) && near(hi[1], 89.5), "{lo:?} {hi:?}");
            }
            s => panic!("expected rect, got {s:?}"),
        }
    }

    #[test]
    fn window_overhanging_the_image_is_trimmed_to_the_frame() {
        let f = sample();
        let px = world_rect_to_px(&f, [0.0, 0.0, 0.0], [30.0, 1000.0, 0.0]).unwrap();
        let region = clip_convex(&f.full_px(), &px);
        let (lo, hi) = as_axis_rect(&region, 1e-9).unwrap();
        assert!(near(lo[0], 0.0) && near(lo[1], 0.0) && near(hi[0], 40.0) && near(hi[1], 100.0));
    }

    #[test]
    fn window_off_the_image_is_empty() {
        let f = sample();
        let px = world_rect_to_px(&f, [500.0, 500.0, 0.0], [600.0, 600.0, 0.0]).unwrap();
        let region = clip_convex(&f.full_px(), &px);
        assert!(shape_from_px(f.h, &region).is_none());
    }

    #[test]
    fn rotated_image_gives_a_closed_polygon() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let f = Frame { ins: [0.0, 0.0, 0.0], u: [s, s, 0.0], v: [-s, s, 0.0], w: 100.0, h: 100.0 };
        let px = world_rect_to_px(&f, [-10.0, 10.0, 0.0], [10.0, 60.0, 0.0]).unwrap();
        let region = clip_convex(&f.full_px(), &px);
        match shape_from_px(f.h, &region).unwrap() {
            Shape::Poly(ring) => {
                assert!(ring.len() >= 4);
                assert_eq!(ring.first(), ring.last());
            }
            s => panic!("expected polygon, got {s:?}"),
        }
    }

    #[test]
    fn existing_clip_round_trips() {
        let verts = [[19.5, 59.5], [59.5, 89.5]];
        let px = region_px_from_clip(100.0, true, &verts).unwrap();
        let back = shape_from_px(100.0, &px).unwrap();
        assert_eq!(back, Shape::Rect([19.5, 59.5], [59.5, 89.5]));
    }

    #[test]
    fn nested_clip_intersects() {
        let f = sample();
        let old = region_px_from_clip(100.0, true, &[[19.5, 59.5], [59.5, 89.5]]).unwrap(); // px 20..60 x 10..40
        let win = world_rect_to_px(&f, [30.0, 0.0, 0.0], [100.0, 100.0, 0.0]).unwrap(); // px a>=40
        let r = clip_convex(&old, &win);
        let (lo, hi) = as_axis_rect(&r, 1e-9).unwrap();
        assert!(near(lo[0], 40.0) && near(hi[0], 60.0) && near(lo[1], 10.0) && near(hi[1], 40.0));
    }

    #[test]
    fn scaling_keeps_the_anchor_fixed() {
        let f = sample();
        let anchor = [200.0, 100.0]; // top-right corner
        let before = f.to_world(anchor);
        let (ins, u, v) = f.scaled(anchor, 2.0, 2.0);
        let g = Frame { ins, u, v, ..f };
        let after = g.to_world(anchor);
        assert!(near(before[0], after[0]) && near(before[1], after[1]));
        let (w, h) = g.world_size();
        assert!(near(w, 200.0) && near(h, 100.0));
    }

    #[test]
    fn drag_factor_projects_on_diagonal() {
        let k = drag_factor([0.0, 0.0, 0.0], [10.0, 5.0, 0.0], [20.0, 10.0, 0.0]);
        assert!(near(k, 2.0));
        let k = drag_factor([0.0, 0.0, 0.0], [10.0, 5.0, 0.0], [5.0, 2.5, 0.0]);
        assert!(near(k, 0.5));
    }

    #[test]
    fn rescale_clip_keeps_frame_fraction() {
        // The full-image boundary stays the full-image boundary.
        let out = rescale_clip(&[[-0.5, -0.5], [199.5, 99.5]], 200.0, 100.0, 400.0, 50.0);
        assert!(near(out[0][0], -0.5) && near(out[0][1], -0.5));
        assert!(near(out[1][0], 399.5) && near(out[1][1], 49.5));
    }

    #[test]
    fn contains_and_nearest() {
        let f = sample();
        assert!(f.contains([10.0, 20.0, 0.0]));
        assert!(f.contains([60.0, 45.0, 0.0]));
        assert!(!f.contains([111.0, 45.0, 0.0]));
        let c = f.corners();
        assert_eq!(nearest(&c, [109.0, 69.0, 0.0]), 2);
    }

    #[test]
    fn units() {
        assert_eq!(units_per_inch("mm"), Some(25.4));
        assert_eq!(units_per_inch("IN"), Some(1.0));
        assert_eq!(units_per_inch("furlong"), None);
    }
}
