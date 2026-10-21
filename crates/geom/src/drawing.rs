//! View-plane projection for drawings: orthographic frames, hidden-line classification, and
//! planar section curves.
//!
//! Hidden lines use a face test that is exact for convex prismatic solids (an edge is hidden when
//! every adjacent face points away from the camera, including an edge shared with a grazing side
//! face) plus a ray toward the camera when the tessellation is small enough to catch a front edge
//! sitting behind other material. Section curves are the straight cuts where triangles meet the
//! cutting plane. Nothing here calls a modelling kernel.

use crate::Vec3;

/// Paper gap left where a break view removes a stretch of the model, in millimetres, applied in
/// view space before the view scale.
pub const BREAK_GAP: f64 = 8.0;

const MAX_CURVES: usize = 4_000;
const MAX_TRIS: usize = 8_000;
const MAX_RAY_TRIS: usize = 1_500;
const MAX_RAY_SEGS: usize = 2_000;

/// Orthonormal sheet axes for a look direction. `dir` points from the camera into the scene,
/// `right` and `up` are the sheet axes, and world +Z stays up unless the view looks along Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewFrame {
    pub dir: Vec3,
    pub right: Vec3,
    pub up: Vec3,
}

impl ViewFrame {
    pub fn looking(dir: Vec3) -> Option<Self> {
        let dir = dir.normalized()?;
        let up_cand = if dir.z.abs() > 0.9 { Vec3::Y } else { Vec3::Z };
        let up = (up_cand - dir * up_cand.dot(dir)).normalized()?;
        let right = dir.cross(up).normalized()?;
        Some(Self { dir, right, up })
    }

    pub fn project(self, p: Vec3) -> [f64; 2] {
        [p.dot(self.right), p.dot(self.up)]
    }

    /// Look direction for a view placed on `name` (`right`, `left`, `above`, `below`) of this one.
    /// First-angle projection looks the opposite way from third-angle.
    pub fn side(self, name: &str, first_angle: bool) -> Option<Vec3> {
        let mut v = match name {
            "right" => self.right,
            "left" => -self.right,
            "above" | "top" => self.up,
            "below" | "bottom" => -self.up,
            _ => return None,
        };
        if first_angle {
            v = -v;
        }
        v.normalized()
    }
}

/// One tessellated edge and the faces it bounds.
#[derive(Clone, Debug)]
pub struct ProjEdge {
    pub points: Vec<Vec3>,
    pub faces: Vec<u32>,
}

/// One triangle. `face` matches [`ProjEdge::faces`].
#[derive(Clone, Copy, Debug)]
pub struct ProjTri {
    pub p: [Vec3; 3],
    pub face: u32,
}

/// Keep the half-space `(p - origin) · normal >= 0` and record the cut.
#[derive(Clone, Copy, Debug)]
pub struct SectionPlane {
    pub origin: Vec3,
    pub normal: Vec3,
}

/// Circular window in the view's unscaled local coordinates.
#[derive(Clone, Copy, Debug)]
pub struct DetailWindow {
    pub center: [f64; 2],
    pub radius: f64,
}

/// A projected segment in sheet millimetres (origin and scale already applied).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjCurve {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub hidden: bool,
    pub section: bool,
}

/// What to project. Empty edges produce no curves.
pub struct ProjRequest<'a> {
    pub frame: ViewFrame,
    pub edges: &'a [ProjEdge],
    pub tris: &'a [ProjTri],
    pub section: Option<SectionPlane>,
    pub detail: Option<DetailWindow>,
    pub stations: &'a [f64],
    pub scale: f64,
    pub origin: [f64; 2],
}

struct LocalSeg {
    a: [f64; 2],
    b: [f64; 2],
    hidden: bool,
    section: bool,
}

/// Project edges and the section cut into sheet space. Non-finite input is skipped.
pub fn project_view(req: &ProjRequest<'_>) -> Vec<ProjCurve> {
    let scale = if req.scale.is_finite() && req.scale > 0.0 { req.scale } else { 1.0 };
    let origin = if req.origin.iter().all(|v| v.is_finite()) { req.origin } else { [0.0, 0.0] };
    let frame = req.frame;
    let tris: Vec<&ProjTri> = req.tris.iter().filter(|t| t.p.iter().all(|p| p.is_finite())).take(MAX_TRIS).collect();
    let normals = face_normals(&tris);
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for t in &tris {
        for p in t.p {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let diag = (hi - lo).len();
    let nudge = if diag.is_finite() { (diag * 1e-4).clamp(1e-3, 0.25) } else { 1e-3 };
    let seg_est: usize = req.edges.iter().map(|e| e.points.len().saturating_sub(1)).sum();
    let use_rays = tris.len() <= MAX_RAY_TRIS && seg_est <= MAX_RAY_SEGS;
    let plane = req.section.filter(|p| p.normal.normalized().is_some() && p.origin.is_finite());

    let mut local = Vec::new();
    if let Some(plane) = plane {
        let normal = plane.normal.normalized().unwrap_or(frame.dir);
        for t in &tris {
            if local.len() >= MAX_CURVES {
                break;
            }
            let Some((a, b)) = tri_section(t.p, plane.origin, normal) else { continue };
            push_local(&mut local, frame.project(a), frame.project(b), false, true);
        }
    }
    for edge in req.edges {
        if local.len() >= MAX_CURVES {
            break;
        }
        let (front, back) = face_flags(&edge.faces, &normals, frame.dir);
        let points = &edge.points;
        let n = points.len();
        let steps = n.saturating_sub(1).min(64);
        if steps == 0 || n < 2 {
            continue;
        }
        let face_hidden = !edge.faces.is_empty() && !front && back;
        for i in 0..steps {
            if local.len() >= MAX_CURVES {
                break;
            }
            let (Some(a), Some(b)) = (points.get(i), points.get(i + 1).or_else(|| points.last())) else { continue };
            if !a.is_finite() || !b.is_finite() {
                continue;
            }
            let kept = match plane {
                Some(plane) => clip_half(*a, *b, plane.origin, plane.normal),
                None => Some((*a, *b)),
            };
            let Some((a, b)) = kept else { continue };
            let mut hidden = face_hidden;
            if !hidden && use_rays && !edge.faces.is_empty() {
                let mid = (a + b) * 0.5;
                hidden = occluded(mid, frame.dir, &tris, &edge.faces, nudge);
            }
            push_local(&mut local, frame.project(a), frame.project(b), hidden, false);
        }
    }
    if let Some(window) = req.detail.filter(|w| w.radius.is_finite() && w.radius > 0.0 && w.center.iter().all(|v| v.is_finite())) {
        local = clip_window(local, window);
    }
    if let Some((a, b)) = break_stations(req.stations) {
        local = break_all(local, a, b, BREAK_GAP);
        add_break_marks(&mut local, a, BREAK_GAP);
    }
    let mut out = Vec::with_capacity(local.len());
    for seg in local {
        if out.len() >= MAX_CURVES {
            break;
        }
        out.push(ProjCurve { a: place(seg.a, scale, origin), b: place(seg.b, scale, origin), hidden: seg.hidden, section: seg.section });
    }
    out
}

fn place(p: [f64; 2], scale: f64, origin: [f64; 2]) -> [f64; 2] {
    [origin[0] + p[0] * scale, origin[1] + p[1] * scale]
}

fn push_local(out: &mut Vec<LocalSeg>, a: [f64; 2], b: [f64; 2], hidden: bool, section: bool) {
    if !a.iter().chain(b.iter()).all(|v| v.is_finite()) {
        return;
    }
    if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6 {
        return;
    }
    if out.len() >= MAX_CURVES {
        return;
    }
    out.push(LocalSeg { a, b, hidden, section });
}

fn face_normals(tris: &[&ProjTri]) -> Vec<(u32, Vec3)> {
    let mut acc: Vec<(u32, Vec3)> = Vec::new();
    for t in tris {
        let n = (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]);
        if let Some(slot) = acc.iter_mut().find(|s| s.0 == t.face) {
            slot.1 += n;
        } else if acc.len() < 4_096 {
            acc.push((t.face, n));
        }
    }
    for slot in &mut acc {
        slot.1 = slot.1.normalized().unwrap_or(Vec3::ZERO);
    }
    acc
}

fn face_flags(faces: &[u32], normals: &[(u32, Vec3)], dir: Vec3) -> (bool, bool) {
    let mut front = false;
    let mut back = false;
    for id in faces {
        let Some(n) = normals.iter().find(|(f, _)| f == id).map(|(_, n)| *n) else { continue };
        if n.len() < 0.5 {
            continue;
        }
        let d = n.dot(dir);
        if d < -1e-3 {
            front = true;
        } else if d > 1e-3 {
            back = true;
        }
    }
    (front, back)
}

fn occluded(mid: Vec3, dir: Vec3, tris: &[&ProjTri], own: &[u32], nudge: f64) -> bool {
    let origin = mid - dir * nudge;
    let ray = -dir;
    tris.iter().any(|t| !own.contains(&t.face) && ray_tri(origin, ray, t.p))
}

fn ray_tri(origin: Vec3, dir: Vec3, tri: [Vec3; 3]) -> bool {
    let e1 = tri[1] - tri[0];
    let e2 = tri[2] - tri[0];
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return false;
    }
    let inv = 1.0 / det;
    let tvec = origin - tri[0];
    let u = tvec.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = tvec.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let t = e2.dot(q) * inv;
    t > 1e-4
}

fn clip_half(a: Vec3, b: Vec3, origin: Vec3, normal: Vec3) -> Option<(Vec3, Vec3)> {
    let Some(normal) = normal.normalized() else { return Some((a, b)) };
    let da = (a - origin).dot(normal);
    let db = (b - origin).dot(normal);
    let keep = |d: f64| d >= -1e-8;
    match (keep(da), keep(db)) {
        (true, true) => Some((a, b)),
        (false, false) => None,
        _ => {
            let den = da - db;
            if den.abs() < 1e-15 {
                return None;
            }
            let t = (da / den).clamp(0.0, 1.0);
            if !t.is_finite() {
                return None;
            }
            let p = a + (b - a) * t;
            if keep(da) { Some((a, p)) } else { Some((p, b)) }
        }
    }
}

fn tri_section(tri: [Vec3; 3], origin: Vec3, normal: Vec3) -> Option<(Vec3, Vec3)> {
    let d = [(tri[0] - origin).dot(normal), (tri[1] - origin).dot(normal), (tri[2] - origin).dot(normal)];
    let mut pts: Vec<Vec3> = Vec::new();
    for i in 0..3 {
        let j = (i + 1) % 3;
        let (Some(pi), Some(pj)) = (tri.get(i).copied(), tri.get(j).copied()) else { continue };
        let (Some(di), Some(dj)) = (d.get(i).copied(), d.get(j).copied()) else { continue };
        if di.abs() <= 1e-8 && dj.abs() <= 1e-8 {
            push_pt(&mut pts, pi);
            push_pt(&mut pts, pj);
            continue;
        }
        if di * dj < 0.0 {
            let t = di / (di - dj);
            if t.is_finite() {
                push_pt(&mut pts, pi + (pj - pi) * t);
            }
        } else if di.abs() <= 1e-8 {
            push_pt(&mut pts, pi);
        }
    }
    let (a, b) = (pts.first().copied()?, pts.get(1).copied()?);
    (a.dist(b) > 1e-6).then_some((a, b))
}

fn push_pt(pts: &mut Vec<Vec3>, p: Vec3) {
    if pts.len() >= 2 || !p.is_finite() {
        return;
    }
    if pts.iter().any(|q| q.dist(p) < 1e-6) {
        return;
    }
    pts.push(p);
}

fn clip_window(segs: Vec<LocalSeg>, window: DetailWindow) -> Vec<LocalSeg> {
    let mut out = Vec::new();
    for seg in segs {
        if out.len() >= MAX_CURVES {
            break;
        }
        if let Some((a, b)) = clip_circle(seg.a, seg.b, window.center, window.radius) {
            push_local(&mut out, a, b, seg.hidden, seg.section);
        }
    }
    out
}

fn clip_circle(a: [f64; 2], b: [f64; 2], c: [f64; 2], r: f64) -> Option<([f64; 2], [f64; 2])> {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let fx = a[0] - c[0];
    let fy = a[1] - c[1];
    let qa = dx * dx + dy * dy;
    let qb = 2.0 * (fx * dx + fy * dy);
    let qc = fx * fx + fy * fy - r * r;
    let inside = |t: f64| {
        let x = a[0] + dx * t - c[0];
        let y = a[1] + dy * t - c[1];
        x * x + y * y <= r * r + 1e-6
    };
    if qa < 1e-18 {
        return inside(0.0).then_some((a, b));
    }
    let mut ts = Vec::new();
    if inside(0.0) {
        ts.push(0.0);
    }
    if inside(1.0) {
        ts.push(1.0);
    }
    let disc = qb * qb - 4.0 * qa * qc;
    if disc >= 0.0 {
        let s = disc.sqrt();
        for t in [(-qb - s) / (2.0 * qa), (-qb + s) / (2.0 * qa)] {
            if t.is_finite() && (0.0..=1.0).contains(&t) {
                ts.push(t);
            }
        }
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for t in ts {
        lo = lo.min(t);
        hi = hi.max(t);
    }
    if !(hi - lo > 1e-9 && lo.is_finite()) {
        return None;
    }
    let at = |t: f64| [a[0] + dx * t, a[1] + dy * t];
    Some((at(lo), at(hi)))
}

fn break_stations(stations: &[f64]) -> Option<(f64, f64)> {
    let (a, b) = (stations.first().copied()?, stations.get(1).copied()?);
    if !a.is_finite() || !b.is_finite() {
        return None;
    }
    let (lo, hi) = (a.min(b), a.max(b));
    (hi - lo > 1e-6).then_some((lo, hi))
}

fn break_all(segs: Vec<LocalSeg>, left: f64, right: f64, gap: f64) -> Vec<LocalSeg> {
    let mut out = Vec::new();
    for seg in segs {
        if out.len() >= MAX_CURVES {
            break;
        }
        for (a, b) in break_segment(seg.a, seg.b, left, right, gap) {
            push_local(&mut out, a, b, seg.hidden, seg.section);
        }
    }
    out
}

fn break_segment(a: [f64; 2], b: [f64; 2], left: f64, right: f64, gap: f64) -> Vec<([f64; 2], [f64; 2])> {
    let shift = (right - left) - gap;
    let map = |p: [f64; 2]| if p[0] >= right - 1e-9 { [p[0] - shift, p[1]] } else { p };
    let mut ts = vec![0.0, 1.0];
    let dx = b[0] - a[0];
    if dx.abs() > 1e-12 {
        for station in [left, right] {
            let t = (station - a[0]) / dx;
            if t.is_finite() && (0.0..=1.0).contains(&t) {
                ts.push(t);
            }
        }
    }
    ts.sort_by(|u, v| u.partial_cmp(v).unwrap_or(std::cmp::Ordering::Equal));
    ts.dedup_by(|u, v| (*u - *v).abs() < 1e-9);
    let mut out = Vec::new();
    for pair in ts.windows(2) {
        let (Some(t0), Some(t1)) = (pair.first().copied(), pair.get(1).copied()) else { continue };
        if t1 - t0 < 1e-9 {
            continue;
        }
        let mid = lerp2(a, b, (t0 + t1) * 0.5);
        if mid[0] > left && mid[0] < right {
            continue;
        }
        let p0 = map(lerp2(a, b, t0));
        let p1 = map(lerp2(a, b, t1));
        if (p0[0] - p1[0]).hypot(p0[1] - p1[1]) > 1e-6 {
            out.push((p0, p1));
        }
    }
    out
}

fn lerp2(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn add_break_marks(local: &mut Vec<LocalSeg>, left: f64, gap: f64) {
    if local.is_empty() {
        return;
    }
    let mut y0 = f64::INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for seg in local.iter() {
        y0 = y0.min(seg.a[1]).min(seg.b[1]);
        y1 = y1.max(seg.a[1]).max(seg.b[1]);
    }
    if !y0.is_finite() || !y1.is_finite() {
        return;
    }
    let mid = (y0 + y1) * 0.5;
    let h = ((y1 - y0) * 0.15).clamp(1.0, 8.0);
    for x in [left, left + gap] {
        push_local(local, [x - 1.5, mid - h], [x + 1.5, mid], false, false);
        push_local(local, [x + 1.5, mid], [x - 1.5, mid + h], false, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_solid(min: Vec3, max: Vec3, face0: u32) -> (Vec<ProjEdge>, Vec<ProjTri>) {
        let p = [
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(max.x, max.y, max.z),
            Vec3::new(min.x, max.y, max.z),
        ];
        let v = |i: usize| p[i];
        let tris =
            [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        let face_of = [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5];
        let tris = tris.iter().zip(face_of).map(|(t, f)| ProjTri { p: [v(t[0]), v(t[1]), v(t[2])], face: face0 + f }).collect();
        let edges = [
            (0, 1, [0, 2]),
            (1, 2, [0, 3]),
            (2, 3, [0, 4]),
            (3, 0, [0, 5]),
            (4, 5, [1, 2]),
            (5, 6, [1, 3]),
            (6, 7, [1, 4]),
            (7, 4, [1, 5]),
            (0, 4, [2, 5]),
            (1, 5, [2, 3]),
            (2, 6, [4, 3]),
            (3, 7, [4, 5]),
        ];
        let edges =
            edges.iter().map(|(a, b, faces)| ProjEdge { points: vec![v(*a), v(*b)], faces: faces.iter().map(|f| face0 + f).collect() }).collect();
        (edges, tris)
    }

    fn project(edges: &[ProjEdge], tris: &[ProjTri], dir: Vec3) -> Vec<ProjCurve> {
        let frame = ViewFrame::looking(dir).expect("frame");
        project_view(&ProjRequest { frame, edges, tris, section: None, detail: None, stations: &[], scale: 1.0, origin: [0.0, 0.0] })
    }

    fn span(curves: &[ProjCurve]) -> (f64, f64) {
        let mut x0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y0 = f64::INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for c in curves {
            for p in [c.a, c.b] {
                x0 = x0.min(p[0]);
                x1 = x1.max(p[0]);
                y0 = y0.min(p[1]);
                y1 = y1.max(p[1]);
            }
        }
        (x1 - x0, y1 - y0)
    }

    #[test]
    fn top_frame_keeps_x_right_and_first_angle_flips() {
        let frame = ViewFrame::looking(Vec3::new(0.0, 0.0, -1.0)).expect("frame");
        assert!(frame.right.dist(Vec3::X) < 1e-9);
        assert!(frame.up.dist(Vec3::Y) < 1e-9);
        let third = frame.side("right", false).expect("side");
        let first = frame.side("right", true).expect("side");
        assert!(third.dist(Vec3::X) < 1e-9);
        assert!(first.dist(-Vec3::X) < 1e-9);
        assert!(frame.side("nope", false).is_none());
    }

    #[test]
    fn a_cube_from_the_corner_hides_the_three_far_edges() {
        let (edges, tris) = box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), 0);
        let curves = project(&edges, &tris, Vec3::new(1.0, 1.0, 1.0));
        let hidden = curves.iter().filter(|c| c.hidden).count();
        let visible = curves.iter().filter(|c| !c.hidden).count();
        assert_eq!(hidden, 3, "hidden {hidden} visible {visible}");
        assert_eq!(visible, 9, "hidden {hidden} visible {visible}");
    }

    #[test]
    fn a_front_edge_behind_another_box_is_hidden() {
        let (ea, ta) = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0), 0);
        let (eb, tb) = box_solid(Vec3::new(2.0, 2.0, 12.0), Vec3::new(8.0, 8.0, 20.0), 100);
        let alone = project(&eb, &tb, Vec3::Z);
        let mut edges = ea.clone();
        edges.extend(eb);
        let mut tris = ta.clone();
        tris.extend(tb);
        let both = project(&edges, &tris, Vec3::Z);
        let hidden = |c: &[ProjCurve]| c.iter().filter(|s| s.hidden).count();
        let visible = |c: &[ProjCurve]| c.iter().filter(|s| !s.hidden).count();
        assert!(hidden(&both) > hidden(&alone), "occlusion should hide the rear front face: {:?} vs {:?}", hidden(&both), hidden(&alone));
        assert!(visible(&both) < visible(&alone) + visible(&project(&ea, &ta, Vec3::Z)));
    }

    #[test]
    fn a_mid_plane_section_is_the_cross_section() {
        let (edges, tris) = box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), 0);
        let frame = ViewFrame::looking(Vec3::Z).expect("frame");
        let curves = project_view(&ProjRequest {
            frame,
            edges: &edges,
            tris: &tris,
            section: Some(SectionPlane { origin: Vec3::new(0.5, 0.5, 0.5), normal: Vec3::Z }),
            detail: None,
            stations: &[],
            scale: 1.0,
            origin: [0.0, 0.0],
        });
        let cut: Vec<_> = curves.iter().filter(|c| c.section).copied().collect();
        assert!(cut.len() >= 4, "{cut:?}");
        let (w, h) = span(&cut);
        assert!((w - 1.0).abs() < 1e-6 && (h - 1.0).abs() < 1e-6, "{w} {h}");
    }

    /// Feature: fusion360-feature-parity, Property 22: A drawing view's projection tracks the model size
    #[test]
    fn property_22_projected_span_tracks_the_model() {
        let mut cases = 0usize;
        for i in 1..=100 {
            let length = i as f64 * 0.5;
            let (edges, tris) = box_solid(Vec3::ZERO, Vec3::new(length, 10.0, 8.0), 0);
            let curves = project(&edges, &tris, Vec3::new(0.0, 0.0, -1.0));
            let (w, h) = span(&curves);
            assert!((w - length).abs() < 1e-6, "length {length} drew {w}");
            assert!((h - 10.0).abs() < 1e-6, "height drew {h}");
            assert!(curves.iter().all(|c| c.a.iter().chain(c.b.iter()).all(|v| v.is_finite())));
            let frame = ViewFrame::looking(Vec3::new((i as f64).cos(), (i as f64 * 0.7).sin(), ((i as f64) * 0.3).cos())).expect("dir");
            let any = project_view(&ProjRequest {
                frame,
                edges: &edges,
                tris: &tris,
                section: None,
                detail: None,
                stations: &[],
                scale: 1.0,
                origin: [0.0, 0.0],
            });
            assert!(any.iter().all(|c| c.a.iter().chain(c.b.iter()).all(|v| v.is_finite())));
            cases += 1;
        }
        assert_eq!(cases, 100);
        let (edges, tris) = box_solid(Vec3::ZERO, Vec3::new(20.0, 10.0, 8.0), 0);
        let frame = ViewFrame::looking(Vec3::new(0.0, 0.0, -1.0)).expect("frame");
        let broken = project_view(&ProjRequest {
            frame,
            edges: &edges,
            tris: &tris,
            section: None,
            detail: None,
            stations: &[5.0, 15.0],
            scale: 1.0,
            origin: [0.0, 0.0],
        });
        let (w, _) = span(&broken);
        assert!((w - 18.0).abs() < 0.6, "break should drop 10 mm and leave an 8 mm gap, drew {w}");
        let empty =
            project_view(&ProjRequest { frame, edges: &[], tris: &[], section: None, detail: None, stations: &[], scale: 1.0, origin: [0.0, 0.0] });
        assert!(empty.is_empty());
    }
}
