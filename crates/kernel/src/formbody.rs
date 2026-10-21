//! A form sheet: a quad cage (a flat grid, an open cylinder, a sphere, a quadball or a torus)
//! whose control points can be pulled, creased and mirrored, then optionally Catmull–Clark
//! subdivided, returned as a surface body.
//!
//! Caps (`MAX_CAGE_VERTS`, `MAX_SUBDIV_LEVEL`) are checked before the refined cage is allocated.
//! Truck runs only inside [`crate::tspline_sheet`], which uses [`crate::guard`].

use solidcraft_geom::Vec3;
use solidcraft_geom::bspline::NurbsSurface;
use solidcraft_geom::tspline::TSpline;

use crate::{KernelError, Result};

/// How a 4×4 form cage is edited before it becomes a sheet.
#[derive(Clone, Debug, Default)]
pub struct FormBuild {
    /// `(index, delta)` on the 4×4 grid, index 0 at the origin corner.
    pub pulls: Vec<(u32, Vec3)>,
    /// Control-point indices whose edges stay sharp.
    pub creases: Vec<u32>,
    /// Grid edges (0..24) opened so an inserted edge reads in the sheet.
    pub separate_edges: Vec<u32>,
    /// Catmull–Clark passes, at most 6, and never past [`MAX_CAGE_VERTS`].
    pub subdiv: u32,
    /// `yz`, `xz` or `xy`.
    pub symmetry: Option<String>,
}

/// Control points after subdivision. A request past this is refused before the cage is allocated.
pub const MAX_CAGE_VERTS: usize = 20_000;
/// Midpoint / Catmull–Clark passes on the 4×4 cage.
pub const MAX_SUBDIV_LEVEL: u32 = 6;

/// Weight that pulls the sheet onto a creased control point.
const CREASE_WEIGHT: f64 = 8.0;
/// Fraction of the way a neighbour moves toward a creased edge.
const PINCH: f64 = 0.65;

/// A rotate-about-Z or scale-about-centre edit, as the pull delta that realises it.
#[derive(Clone, Copy, Debug)]
pub enum PointXform {
    /// Radians about Z through the cage centre.
    RotateZ(f64),
    /// Factor about the cage centre. `1` leaves the point where it is.
    Scale(f64),
}

/// Control points after `level` refinements of the 4×4 cage.
pub fn cage_points(level: u32) -> Result<usize> {
    if level > MAX_SUBDIV_LEVEL {
        return Err(KernelError::Invalid("subdivision level is at most 6".into()));
    }
    let mut n = 4usize;
    for _ in 0..level {
        n = n.saturating_mul(2).saturating_sub(1);
    }
    let count = n.saturating_mul(n);
    if count > MAX_CAGE_VERTS {
        return Err(KernelError::Invalid("the form cage is larger than 20000 points".into()));
    }
    Ok(count)
}

/// `shape` is `box` or `plane` (a flat grid), `cylinder` (an open half-cylinder), `sphere`,
/// `quadball` or `torus`. `length` and `width` are the cage size in millimetres.
pub fn form_sheet(shape: &str, length: f64, width: f64, build: &FormBuild) -> Result<crate::Body> {
    let cage = prepare(shape, length, width, build)?;
    let knots = clamped_knots(cage.n)?;
    let nurbs = NurbsSurface::new([3, 3], [knots.clone(), knots], &cage.points, &cage.weights)
        .map_err(|e| KernelError::Invalid(format!("form surface: {e}")))?;
    let ts = TSpline::from_nurbs(&nurbs).map_err(|e| KernelError::Invalid(format!("form surface: {e}")))?;
    crate::tspline_sheet(&ts)
}

/// Pull delta that rotates or scales one control point about the cage centre.
pub fn point_edit_delta(shape: &str, length: f64, width: f64, pulls: &[(u32, Vec3)], index: u32, xform: &PointXform) -> Result<Vec3> {
    if pulls.len() > MAX_CAGE_VERTS {
        return Err(KernelError::Invalid("not supported yet: cage too large".into()));
    }
    let mut pts = base_grid(shape, length, width)?;
    apply_pulls(&mut pts, pulls)?;
    let centre = cage_centre(&pts)?;
    let Some(p) = pts.get(index as usize).copied() else {
        return Err(KernelError::Invalid("a form edit names a control point that is not on the 4×4 grid".into()));
    };
    let d = p - centre;
    let moved = match *xform {
        PointXform::RotateZ(turns) => {
            if !turns.is_finite() {
                return Err(KernelError::Invalid("a form edit must be finite".into()));
            }
            let (c, s) = (turns.cos(), turns.sin());
            centre + Vec3::new(d.x * c - d.y * s, d.x * s + d.y * c, d.z)
        }
        PointXform::Scale(factor) => {
            if !(factor.is_finite() && factor.abs() < 1.0e6) {
                return Err(KernelError::Invalid("a form edit must be finite".into()));
            }
            centre + d * factor
        }
    };
    let delta = moved - p;
    if !delta.is_finite() {
        return Err(KernelError::Invalid("a form edit must be finite".into()));
    }
    Ok(delta)
}

/// Pull deltas that arch two boundary control points toward each other.
pub fn bridge_deltas(shape: &str, length: f64, width: f64, pulls: &[(u32, Vec3)], a: u32, b: u32) -> Result<(Vec3, Vec3)> {
    if pulls.len() > MAX_CAGE_VERTS {
        return Err(KernelError::Invalid("not supported yet: cage too large".into()));
    }
    if a == b {
        return Err(KernelError::Invalid("not supported yet: bridging a control point to itself".into()));
    }
    if !boundary_point(a) || !boundary_point(b) {
        return Err(KernelError::Invalid("not supported yet: bridging a point that is not on the cage boundary".into()));
    }
    let mut pts = base_grid(shape, length, width)?;
    apply_pulls(&mut pts, pulls)?;
    let (Some(pa), Some(pb)) = (pts.get(a as usize).copied(), pts.get(b as usize).copied()) else {
        return Err(KernelError::Invalid("a form bridge names a control point that is not on the 4×4 grid".into()));
    };
    let span = pb - pa;
    let len = span.len();
    if !(len.is_finite() && len > 1e-6) {
        return Err(KernelError::Invalid("not supported yet: bridging control points that already meet".into()));
    }
    let mid = (pa + pb) * 0.5;
    let up = arch_dir(span);
    let rise = len * 0.35;
    let (pa2, pb2) = (pa.lerp(mid, 0.4) + up * rise, pb.lerp(mid, 0.4) + up * rise);
    let (da, db) = (pa2 - pa, pb2 - pb);
    if !(da.is_finite() && db.is_finite()) {
        return Err(KernelError::Invalid("a form bridge must be finite".into()));
    }
    Ok((da, db))
}

struct Prepared {
    points: Vec<Vec3>,
    weights: Vec<f64>,
    n: usize,
}

fn prepare(shape: &str, length: f64, width: f64, build: &FormBuild) -> Result<Prepared> {
    check_lists(build)?;
    let plane = plane_name(&build.symmetry)?;
    let mut pts = base_grid(shape, length, width)?;
    apply_pulls(&mut pts, &build.pulls)?;
    for edge in &build.separate_edges {
        open_edge(&mut pts, *edge)?;
    }
    let mut creases = build.creases.clone();
    if let Some(plane) = plane.as_deref() {
        symmetrize_points(&mut pts, 4, plane)?;
        creases = mirror_creases(&creases, 4, plane)?;
    }
    pinch_creases(&mut pts, 4, &creases)?;
    if let Some(plane) = plane.as_deref() {
        symmetrize_points(&mut pts, 4, plane)?;
    }
    let mut weights = vec![1.0; pts.len()];
    boost_creases(&mut weights, &creases)?;
    if let Some(plane) = plane.as_deref() {
        symmetrize_weights(&mut weights, 4, plane)?;
    }
    let mut n = 4usize;
    for _ in 0..build.subdiv {
        (pts, weights) = refine(&pts, &weights, n, &creases)?;
        creases = map_creases(n, &creases)?;
        n = n.saturating_mul(2).saturating_sub(1);
        if n.saturating_mul(n) != pts.len() {
            return Err(KernelError::Invalid("the form cage is not a square grid".into()));
        }
        if let Some(plane) = plane.as_deref() {
            creases = mirror_creases(&creases, n, plane)?;
            symmetrize_points(&mut pts, n, plane)?;
        }
        boost_creases(&mut weights, &creases)?;
        if let Some(plane) = plane.as_deref() {
            symmetrize_weights(&mut weights, n, plane)?;
        }
    }
    if weights.len() != pts.len() {
        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
    }
    Ok(Prepared { points: pts, weights, n })
}

fn check_lists(build: &FormBuild) -> Result<()> {
    if build.subdiv > MAX_SUBDIV_LEVEL {
        return Err(KernelError::Invalid("subdivision level is at most 6".into()));
    }
    // Final cage size, before any refined grid is allocated.
    let _ = cage_points(build.subdiv)?;
    if build.pulls.len() > MAX_CAGE_VERTS || build.creases.len() > MAX_CAGE_VERTS || build.separate_edges.len() > MAX_CAGE_VERTS {
        return Err(KernelError::Invalid("not supported yet: cage too large".into()));
    }
    for (index, delta) in &build.pulls {
        if !delta.is_finite() {
            return Err(KernelError::Invalid("a form pull must be finite".into()));
        }
        if *index > 15 {
            return Err(KernelError::Invalid("a form pull names a control point that is not on the 4×4 grid".into()));
        }
    }
    for edge in &build.separate_edges {
        if *edge >= 24 {
            return Err(KernelError::Invalid("a form edge is 0…23".into()));
        }
    }
    for crease in &build.creases {
        if *crease > 15 {
            return Err(KernelError::Invalid("a crease names a control point that is not on the 4×4 grid".into()));
        }
    }
    Ok(())
}

fn plane_name(symmetry: &Option<String>) -> Result<Option<String>> {
    let Some(plane) = symmetry.as_deref() else { return Ok(None) };
    let plane = plane.to_ascii_lowercase();
    if !matches!(plane.as_str(), "yz" | "xz" | "xy") {
        return Err(KernelError::Invalid("symmetry plane must be yz, xz or xy".into()));
    }
    Ok(Some(plane))
}

fn boundary_point(index: u32) -> bool {
    if index > 15 {
        return false;
    }
    let col = index % 4;
    let row = index / 4;
    col == 0 || col == 3 || row == 0 || row == 3
}

fn arch_dir(span: Vec3) -> Vec3 {
    let from_z = span.cross(Vec3::Z).cross(span);
    if let Some(n) = from_z.normalized() {
        return if n.z < 0.0 { -n } else { n };
    }
    let from_x = span.cross(Vec3::X).cross(span);
    if let Some(n) = from_x.normalized() {
        return if n.z < 0.0 { -n } else { n };
    }
    Vec3::Z
}

fn cage_centre(pts: &[Vec3]) -> Result<Vec3> {
    if pts.is_empty() || pts.len() > MAX_CAGE_VERTS {
        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
    }
    let mut acc = Vec3::ZERO;
    for p in pts {
        if !p.is_finite() {
            return Err(KernelError::Invalid("a form pull must be finite".into()));
        }
        acc += *p;
    }
    Ok(acc * (1.0 / pts.len() as f64))
}

fn base_grid(shape: &str, length: f64, width: f64) -> Result<Vec<Vec3>> {
    if !(length.is_finite() && width.is_finite() && length > 1e-6 && width > 1e-6 && length < 1e6 && width < 1e6) {
        return Err(KernelError::Invalid("form size must be positive".into()));
    }
    let mut pts = Vec::with_capacity(16);
    for j in 0..4 {
        for i in 0..4 {
            pts.push(cage_point(shape, length, width, i as f64 / 3.0, j as f64 / 3.0)?);
        }
    }
    Ok(pts)
}

fn apply_pulls(pts: &mut [Vec3], pulls: &[(u32, Vec3)]) -> Result<()> {
    for (index, delta) in pulls {
        if !delta.is_finite() {
            return Err(KernelError::Invalid("a form pull must be finite".into()));
        }
        let Some(p) = pts.get_mut(*index as usize) else {
            return Err(KernelError::Invalid("a form pull names a control point that is not on the 4×4 grid".into()));
        };
        *p += *delta;
        if !p.is_finite() {
            return Err(KernelError::Invalid("a form pull must be finite".into()));
        }
    }
    Ok(())
}

fn cage_point(shape: &str, length: f64, width: f64, u: f64, v: f64) -> Result<Vec3> {
    let p = match shape {
        "box" | "plane" => Vec3::new(length * u, width * v, 0.0),
        "cylinder" => {
            let ang = u * std::f64::consts::PI;
            Vec3::new((length * 0.5) * ang.cos(), width * v, (length * 0.5) * ang.sin())
        }
        "sphere" | "quadball" => {
            let r = length * 0.5;
            let theta = if shape == "quadball" { (u - 0.5) * std::f64::consts::PI } else { u * std::f64::consts::TAU };
            let phi = v * std::f64::consts::PI;
            Vec3::new(r * phi.sin() * theta.cos(), r * phi.sin() * theta.sin(), r * phi.cos())
        }
        "torus" => {
            let major = length * 0.5;
            let minor = (width * 0.25).min(major * 0.5);
            let theta = u * std::f64::consts::TAU;
            let phi = v * std::f64::consts::TAU;
            Vec3::new((major + minor * phi.cos()) * theta.cos(), (major + minor * phi.cos()) * theta.sin(), minor * phi.sin())
        }
        _ => return Err(KernelError::Invalid("form shape must be box, plane, cylinder, sphere, quadball or torus".into())),
    };
    if !p.is_finite() {
        return Err(KernelError::Invalid("form size must be positive".into()));
    }
    Ok(p)
}

/// Endpoints of one of the 24 edges of a 4×4 grid.
pub fn edge_ends(edge: u32) -> Result<(usize, usize)> {
    if edge < 12 {
        let j = (edge / 3) as usize;
        let i = (edge % 3) as usize;
        return Ok((j * 4 + i, j * 4 + i + 1));
    }
    if edge < 24 {
        let e = edge - 12;
        let col = (e / 3) as usize;
        let row = (e % 3) as usize;
        return Ok((row * 4 + col, (row + 1) * 4 + col));
    }
    Err(KernelError::Invalid("a form edge is 0…23".into()))
}

/// Open one cage edge: its ends move apart and the adjacent row or column is pushed away,
/// which is the sheet an inserted edge leaves on this quad grid.
fn open_edge(pts: &mut [Vec3], edge: u32) -> Result<()> {
    let (a, b) = edge_ends(edge)?;
    let (Some(pa), Some(pb)) = (pts.get(a).copied(), pts.get(b).copied()) else {
        return Err(KernelError::Invalid("a form edge is outside the 4×4 grid".into()));
    };
    let span = pb - pa;
    let len = span.len();
    if !(len.is_finite() && len > 1e-6) {
        return Err(KernelError::Invalid("not supported yet: inserting an edge where the cage points meet".into()));
    }
    let Some(dir) = span.normalized() else {
        return Err(KernelError::Invalid("not supported yet: inserting an edge where the cage points meet".into()));
    };
    let gap = len * 0.22;
    let (pa2, pb2) = (pa - dir * gap, pb + dir * gap);
    if !(pa2.is_finite() && pb2.is_finite()) {
        return Err(KernelError::Invalid("a form edge move must be finite".into()));
    }
    if let Some(p) = pts.get_mut(a) {
        *p = pa2;
    }
    if let Some(p) = pts.get_mut(b) {
        *p = pb2;
    }
    let n = 4usize;
    let (horizontal, i, j) = if edge < 12 {
        (true, (edge % 3) as usize, (edge / 3) as usize)
    } else {
        let e = edge - 12;
        (false, (e / 3) as usize, (e % 3) as usize)
    };
    let mut neighbors = Vec::with_capacity(4);
    if horizontal {
        if j > 0 {
            neighbors.push((j - 1) * n + i);
            neighbors.push((j - 1) * n + i + 1);
        }
        if j + 1 < n {
            neighbors.push((j + 1) * n + i);
            neighbors.push((j + 1) * n + i + 1);
        }
    } else if i + 1 < n {
        neighbors.push(j * n + i + 1);
        neighbors.push((j + 1) * n + i + 1);
        if i > 0 {
            neighbors.push(j * n + (i - 1));
            neighbors.push((j + 1) * n + (i - 1));
        }
    } else if i > 0 {
        neighbors.push(j * n + (i - 1));
        neighbors.push((j + 1) * n + (i - 1));
    }
    let mid = (pa2 + pb2) * 0.5;
    for idx in neighbors {
        if idx == a || idx == b {
            continue;
        }
        let Some(p) = pts.get(idx).copied() else { continue };
        let away = p - mid;
        if away.len() < 1e-9 {
            continue;
        }
        let moved = p + away * 0.18;
        if !moved.is_finite() {
            return Err(KernelError::Invalid("a form edge move must be finite".into()));
        }
        if let Some(slot) = pts.get_mut(idx) {
            *slot = moved;
        }
    }
    Ok(())
}

fn clamped_knots(n: usize) -> Result<Vec<f64>> {
    if !(4..=400).contains(&n) {
        return Err(KernelError::Invalid("form cage size".into()));
    }
    let interior = n - 4;
    let mut knots = Vec::with_capacity(n + 4);
    knots.extend(std::iter::repeat_n(0.0, 4));
    let steps = interior as f64 + 1.0;
    for i in 1..=interior {
        knots.push(i as f64 / steps);
    }
    knots.extend(std::iter::repeat_n(1.0, 4));
    Ok(knots)
}

fn at(pts: &[Vec3], n: usize, i: usize, j: usize) -> Result<Vec3> {
    let idx = j.checked_mul(n).and_then(|k| k.checked_add(i)).ok_or_else(|| KernelError::Invalid("the form cage is not a square grid".into()))?;
    pts.get(idx).copied().ok_or_else(|| KernelError::Invalid("the form cage is not a square grid".into()))
}

fn wt(weights: &[f64], n: usize, i: usize, j: usize) -> Result<f64> {
    let idx = j.checked_mul(n).and_then(|k| k.checked_add(i)).ok_or_else(|| KernelError::Invalid("the form cage is not a square grid".into()))?;
    weights.get(idx).copied().ok_or_else(|| KernelError::Invalid("the form cage is not a square grid".into()))
}

fn put(out: &mut [Vec3], wout: &mut [f64], m: usize, i: usize, j: usize, p: Vec3, w: f64) -> Result<()> {
    if !(p.is_finite() && w.is_finite()) {
        return Err(KernelError::Invalid("a form cage point must be finite".into()));
    }
    let idx = j.checked_mul(m).and_then(|k| k.checked_add(i)).ok_or_else(|| KernelError::Invalid("the form cage is not a square grid".into()))?;
    let Some(slot) = out.get_mut(idx) else {
        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
    };
    *slot = p;
    let Some(slot) = wout.get_mut(idx) else {
        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
    };
    *slot = w;
    Ok(())
}

/// One Catmull–Clark pass. Boundary edges and creased edges stay sharp (edge points are
/// midpoints; a corner stays). Smooth interior vertices use the regular quad rule.
fn refine(pts: &[Vec3], weights: &[f64], n: usize, creases: &[u32]) -> Result<(Vec<Vec3>, Vec<f64>)> {
    if n < 2 || n * n != pts.len() || weights.len() != pts.len() {
        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
    }
    let m = n.saturating_mul(2).saturating_sub(1);
    if m < 2 || m.saturating_mul(m) > MAX_CAGE_VERTS {
        return Err(KernelError::Invalid("the form cage is larger than 20000 points".into()));
    }
    let (h_sharp, v_sharp) = sharp_edges(n, creases)?;
    let faces = n - 1;
    let mut face_p = vec![Vec3::ZERO; faces * faces];
    let mut face_w = vec![1.0; faces * faces];
    for j in 0..faces {
        for i in 0..faces {
            let p = (at(pts, n, i, j)? + at(pts, n, i + 1, j)? + at(pts, n, i, j + 1)? + at(pts, n, i + 1, j + 1)?) * 0.25;
            let w = (wt(weights, n, i, j)? + wt(weights, n, i + 1, j)? + wt(weights, n, i, j + 1)? + wt(weights, n, i + 1, j + 1)?) * 0.25;
            let slot = j * faces + i;
            if let Some(q) = face_p.get_mut(slot) {
                *q = p;
            }
            if let Some(q) = face_w.get_mut(slot) {
                *q = w;
            }
        }
    }
    let mut out = vec![Vec3::ZERO; m * m];
    let mut wout = vec![1.0; m * m];
    for j in 0..n {
        for i in 0..n {
            let (p, w) = vertex_rule(pts, weights, n, i, j, &h_sharp, &v_sharp, &face_p, &face_w)?;
            put(&mut out, &mut wout, m, i * 2, j * 2, p, w)?;
        }
    }
    for j in 0..n {
        for i in 0..faces {
            let (p, w) = edge_rule(pts, weights, n, i, j, true, &h_sharp, &face_p, &face_w)?;
            put(&mut out, &mut wout, m, i * 2 + 1, j * 2, p, w)?;
        }
    }
    for j in 0..faces {
        for i in 0..n {
            let (p, w) = edge_rule(pts, weights, n, i, j, false, &v_sharp, &face_p, &face_w)?;
            put(&mut out, &mut wout, m, i * 2, j * 2 + 1, p, w)?;
        }
    }
    for j in 0..faces {
        for i in 0..faces {
            let slot = j * faces + i;
            let (Some(p), Some(w)) = (face_p.get(slot).copied(), face_w.get(slot).copied()) else {
                return Err(KernelError::Invalid("the form cage is not a square grid".into()));
            };
            put(&mut out, &mut wout, m, i * 2 + 1, j * 2 + 1, p, w)?;
        }
    }
    Ok((out, wout))
}

fn sharp_edges(n: usize, creases: &[u32]) -> Result<(Vec<bool>, Vec<bool>)> {
    let mut mark = vec![false; n * n];
    for crease in creases {
        let Some(slot) = mark.get_mut(*crease as usize) else {
            return Err(KernelError::Invalid("a crease names a control point that is not on the cage".into()));
        };
        *slot = true;
    }
    let mut horizontal = vec![false; n * n.saturating_sub(1)];
    let mut vertical = vec![false; n.saturating_sub(1) * n];
    for j in 0..n {
        for i in 0..n.saturating_sub(1) {
            let user = mark.get(j * n + i).copied().unwrap_or(false) && mark.get(j * n + i + 1).copied().unwrap_or(false);
            let boundary = j == 0 || j + 1 == n;
            if let Some(slot) = horizontal.get_mut(j * (n - 1) + i) {
                *slot = user || boundary;
            }
        }
    }
    for j in 0..n.saturating_sub(1) {
        for i in 0..n {
            let user = mark.get(j * n + i).copied().unwrap_or(false) && mark.get((j + 1) * n + i).copied().unwrap_or(false);
            let boundary = i == 0 || i + 1 == n;
            if let Some(slot) = vertical.get_mut(j * n + i) {
                *slot = user || boundary;
            }
        }
    }
    Ok((horizontal, vertical))
}

fn face_sample(face_p: &[Vec3], face_w: &[f64], faces: usize, i: usize, j: usize) -> Result<(Vec3, f64)> {
    let slot = j * faces + i;
    match (face_p.get(slot).copied(), face_w.get(slot).copied()) {
        (Some(p), Some(w)) => Ok((p, w)),
        _ => Err(KernelError::Invalid("the form cage is not a square grid".into())),
    }
}

fn edge_rule(
    pts: &[Vec3],
    weights: &[f64],
    n: usize,
    i: usize,
    j: usize,
    horizontal: bool,
    sharp: &[bool],
    face_p: &[Vec3],
    face_w: &[f64],
) -> Result<(Vec3, f64)> {
    let faces = n - 1;
    let (ia, ja, ib, jb, sharp_at) = if horizontal { (i, j, i + 1, j, j * faces + i) } else { (i, j, i, j + 1, j * n + i) };
    let (pa, pb) = (at(pts, n, ia, ja)?, at(pts, n, ib, jb)?);
    let (wa, wb) = (wt(weights, n, ia, ja)?, wt(weights, n, ib, jb)?);
    if sharp.get(sharp_at).copied().unwrap_or(false) {
        return Ok(((pa + pb) * 0.5, (wa + wb) * 0.5));
    }
    let mut acc = pa + pb;
    let mut wacc = wa + wb;
    let mut count = 2.0;
    if horizontal {
        if j > 0 {
            let (p, w) = face_sample(face_p, face_w, faces, i, j - 1)?;
            acc += p;
            wacc += w;
            count += 1.0;
        }
        if j < n - 1 {
            let (p, w) = face_sample(face_p, face_w, faces, i, j)?;
            acc += p;
            wacc += w;
            count += 1.0;
        }
    } else {
        if i > 0 {
            let (p, w) = face_sample(face_p, face_w, faces, i - 1, j)?;
            acc += p;
            wacc += w;
            count += 1.0;
        }
        if i + 1 < n {
            let (p, w) = face_sample(face_p, face_w, faces, i, j)?;
            acc += p;
            wacc += w;
            count += 1.0;
        }
    }
    Ok((acc * (1.0 / count), wacc * (1.0 / count)))
}

fn vertex_rule(
    pts: &[Vec3],
    weights: &[f64],
    n: usize,
    i: usize,
    j: usize,
    h_sharp: &[bool],
    v_sharp: &[bool],
    face_p: &[Vec3],
    face_w: &[f64],
) -> Result<(Vec3, f64)> {
    let p = at(pts, n, i, j)?;
    let w = wt(weights, n, i, j)?;
    let faces = n - 1;
    let mut horiz: Vec<(Vec3, f64)> = Vec::new();
    if i > 0 && h_sharp.get(j * faces + (i - 1)).copied().unwrap_or(false) {
        horiz.push((at(pts, n, i - 1, j)?, wt(weights, n, i - 1, j)?));
    }
    if i + 1 < n && h_sharp.get(j * faces + i).copied().unwrap_or(false) {
        horiz.push((at(pts, n, i + 1, j)?, wt(weights, n, i + 1, j)?));
    }
    let mut vert: Vec<(Vec3, f64)> = Vec::new();
    if j > 0 && v_sharp.get((j - 1) * n + i).copied().unwrap_or(false) {
        vert.push((at(pts, n, i, j - 1)?, wt(weights, n, i, j - 1)?));
    }
    if j + 1 < n && v_sharp.get(j * n + i).copied().unwrap_or(false) {
        vert.push((at(pts, n, i, j + 1)?, wt(weights, n, i, j + 1)?));
    }
    if horiz.len() == 2 && vert.is_empty() {
        let (a, aw) = horiz[0];
        let (b, bw) = horiz[1];
        return Ok(((a + p * 6.0 + b) * 0.125, (aw + w * 6.0 + bw) * 0.125));
    }
    if vert.len() == 2 && horiz.is_empty() {
        let (a, aw) = vert[0];
        let (b, bw) = vert[1];
        return Ok(((a + p * 6.0 + b) * 0.125, (aw + w * 6.0 + bw) * 0.125));
    }
    if !horiz.is_empty() || !vert.is_empty() {
        return Ok((p, w));
    }
    if i == 0 || j == 0 || i + 1 >= n || j + 1 >= n {
        return Ok((p, w));
    }
    let mut f = Vec3::ZERO;
    let mut fw = 0.0;
    for (fi, fj) in [(i - 1, j - 1), (i, j - 1), (i - 1, j), (i, j)] {
        let (fp, weight) = face_sample(face_p, face_w, faces, fi, fj)?;
        f += fp;
        fw += weight;
    }
    let mut r = Vec3::ZERO;
    let mut rw = 0.0;
    for (ni, nj) in [(i - 1, j), (i + 1, j), (i, j - 1), (i, j + 1)] {
        r += (p + at(pts, n, ni, nj)?) * 0.5;
        rw += (w + wt(weights, n, ni, nj)?) * 0.5;
    }
    Ok(((f * 0.25 + r * 0.5 + p) * 0.25, (fw * 0.25 + rw * 0.5 + w) * 0.25))
}

fn map_creases(n: usize, creases: &[u32]) -> Result<Vec<u32>> {
    let m = n.saturating_mul(2).saturating_sub(1);
    let mut mark = vec![false; n.saturating_mul(n)];
    for crease in creases {
        let Some(slot) = mark.get_mut(*crease as usize) else {
            return Err(KernelError::Invalid("a crease names a control point that is not on the cage".into()));
        };
        *slot = true;
    }
    let mut out = Vec::new();
    let push = |out: &mut Vec<u32>, col: usize, row: usize| -> Result<()> {
        let idx =
            row.checked_mul(m).and_then(|k| k.checked_add(col)).ok_or_else(|| KernelError::Invalid("the form cage is not a square grid".into()))?;
        let idx = u32::try_from(idx).map_err(|_| KernelError::Invalid("the form cage is not a square grid".into()))?;
        out.push(idx);
        Ok(())
    };
    for j in 0..n {
        for i in 0..n {
            if mark.get(j * n + i).copied().unwrap_or(false) {
                push(&mut out, i * 2, j * 2)?;
            }
        }
    }
    for j in 0..n {
        for i in 0..n.saturating_sub(1) {
            if mark.get(j * n + i).copied().unwrap_or(false) && mark.get(j * n + i + 1).copied().unwrap_or(false) {
                push(&mut out, i * 2 + 1, j * 2)?;
            }
        }
    }
    for j in 0..n.saturating_sub(1) {
        for i in 0..n {
            if mark.get(j * n + i).copied().unwrap_or(false) && mark.get((j + 1) * n + i).copied().unwrap_or(false) {
                push(&mut out, i * 2, j * 2 + 1)?;
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn mirror_creases(creases: &[u32], n: usize, plane: &str) -> Result<Vec<u32>> {
    if n < 2 || n > 400 {
        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
    }
    let mut out = Vec::with_capacity(creases.len().saturating_mul(2));
    for &index in creases {
        let i = index as usize;
        if i >= n.saturating_mul(n) {
            return Err(KernelError::Invalid("a crease names a control point that is not on the cage".into()));
        }
        out.push(index);
        let (col, row) = (i % n, i / n);
        let mirrored = match plane {
            "yz" => row * n + (n - 1 - col),
            "xz" => (n - 1 - row) * n + col,
            "xy" => i,
            _ => return Err(KernelError::Invalid("symmetry plane must be yz, xz or xy".into())),
        };
        let mirrored = u32::try_from(mirrored).map_err(|_| KernelError::Invalid("a crease names a control point that is not on the cage".into()))?;
        out.push(mirrored);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn boost_creases(weights: &mut [f64], creases: &[u32]) -> Result<()> {
    for crease in creases {
        let Some(w) = weights.get_mut(*crease as usize) else {
            return Err(KernelError::Invalid("a crease names a control point that is not on the cage".into()));
        };
        *w = CREASE_WEIGHT;
    }
    Ok(())
}

fn pinch_creases(pts: &mut [Vec3], n: usize, creases: &[u32]) -> Result<()> {
    if n.saturating_mul(n) != pts.len() {
        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
    }
    let mut mark = vec![false; pts.len()];
    for crease in creases {
        let Some(slot) = mark.get_mut(*crease as usize) else {
            return Err(KernelError::Invalid("a crease names a control point that is not on the cage".into()));
        };
        *slot = true;
    }
    let mut acc = vec![Vec3::ZERO; pts.len()];
    let mut count = vec![0u32; pts.len()];
    let add = |acc: &mut [Vec3], count: &mut [u32], mark: &[bool], idx: usize, target: Vec3| {
        if mark.get(idx).copied().unwrap_or(true) {
            return;
        }
        if let Some(slot) = acc.get_mut(idx) {
            *slot += target;
        }
        if let Some(slot) = count.get_mut(idx) {
            *slot = slot.saturating_add(1);
        }
    };
    for j in 0..n {
        for i in 0..n.saturating_sub(1) {
            let a = j * n + i;
            let b = a + 1;
            if !(mark.get(a).copied().unwrap_or(false) && mark.get(b).copied().unwrap_or(false)) {
                continue;
            }
            let (Some(pa), Some(pb)) = (pts.get(a).copied(), pts.get(b).copied()) else { continue };
            let rows = [j.checked_sub(1), (j + 1 < n).then_some(j + 1)];
            for row in rows.into_iter().flatten() {
                for col in [i, i + 1] {
                    let idx = row * n + col;
                    if let Some(p) = pts.get(idx).copied() {
                        add(&mut acc, &mut count, &mark, idx, p.lerp(closest_on_segment(p, pa, pb), PINCH));
                    }
                }
            }
        }
    }
    for j in 0..n.saturating_sub(1) {
        for i in 0..n {
            let a = j * n + i;
            let b = a + n;
            if !(mark.get(a).copied().unwrap_or(false) && mark.get(b).copied().unwrap_or(false)) {
                continue;
            }
            let (Some(pa), Some(pb)) = (pts.get(a).copied(), pts.get(b).copied()) else { continue };
            let cols = [i.checked_sub(1), (i + 1 < n).then_some(i + 1)];
            for col in cols.into_iter().flatten() {
                for row in [j, j + 1] {
                    let idx = row * n + col;
                    if let Some(p) = pts.get(idx).copied() {
                        add(&mut acc, &mut count, &mark, idx, p.lerp(closest_on_segment(p, pa, pb), PINCH));
                    }
                }
            }
        }
    }
    for i in 0..pts.len() {
        let nsum = count.get(i).copied().unwrap_or(0);
        if nsum == 0 {
            continue;
        }
        let Some(sum) = acc.get(i).copied() else { continue };
        let moved = sum * (1.0 / f64::from(nsum));
        if !moved.is_finite() {
            return Err(KernelError::Invalid("a crease move must be finite".into()));
        }
        if let Some(p) = pts.get_mut(i) {
            *p = moved;
        }
    }
    Ok(())
}

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let len2 = ab.len2();
    if !(len2 > 1e-18) {
        return a;
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    a + ab * t
}

fn symmetrize_points(pts: &mut [Vec3], n: usize, plane: &str) -> Result<()> {
    if n < 2 || n.saturating_mul(n) != pts.len() {
        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
    }
    match plane {
        "yz" => {
            for j in 0..n {
                for i in 0..n / 2 {
                    let a = j * n + i;
                    let b = j * n + (n - 1 - i);
                    let (Some(pa), Some(pb)) = (pts.get(a).copied(), pts.get(b).copied()) else {
                        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
                    };
                    let avg = (pa + Vec3::new(-pb.x, pb.y, pb.z)) * 0.5;
                    if !avg.is_finite() {
                        return Err(KernelError::Invalid("a form symmetry move must be finite".into()));
                    }
                    if let Some(p) = pts.get_mut(a) {
                        *p = avg;
                    }
                    if let Some(p) = pts.get_mut(b) {
                        *p = Vec3::new(-avg.x, avg.y, avg.z);
                    }
                }
                if n % 2 == 1
                    && let Some(p) = pts.get_mut(j * n + n / 2)
                {
                    p.x = 0.0;
                }
            }
        }
        "xz" => {
            for i in 0..n {
                for j in 0..n / 2 {
                    let a = j * n + i;
                    let b = (n - 1 - j) * n + i;
                    let (Some(pa), Some(pb)) = (pts.get(a).copied(), pts.get(b).copied()) else {
                        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
                    };
                    let avg = (pa + Vec3::new(pb.x, -pb.y, pb.z)) * 0.5;
                    if !avg.is_finite() {
                        return Err(KernelError::Invalid("a form symmetry move must be finite".into()));
                    }
                    if let Some(p) = pts.get_mut(a) {
                        *p = avg;
                    }
                    if let Some(p) = pts.get_mut(b) {
                        *p = Vec3::new(avg.x, -avg.y, avg.z);
                    }
                }
                if n % 2 == 1
                    && let Some(p) = pts.get_mut((n / 2) * n + i)
                {
                    p.y = 0.0;
                }
            }
        }
        "xy" => {
            for p in pts.iter_mut() {
                p.z = 0.0;
            }
        }
        _ => return Err(KernelError::Invalid("symmetry plane must be yz, xz or xy".into())),
    }
    Ok(())
}

fn symmetrize_weights(weights: &mut [f64], n: usize, plane: &str) -> Result<()> {
    if n < 2 || n.saturating_mul(n) != weights.len() {
        return Err(KernelError::Invalid("the form cage is not a square grid".into()));
    }
    let pair = |weights: &mut [f64], a: usize, b: usize| -> Result<()> {
        let (Some(wa), Some(wb)) = (weights.get(a).copied(), weights.get(b).copied()) else {
            return Err(KernelError::Invalid("the form cage is not a square grid".into()));
        };
        let avg = (wa + wb) * 0.5;
        if let Some(w) = weights.get_mut(a) {
            *w = avg;
        }
        if let Some(w) = weights.get_mut(b) {
            *w = avg;
        }
        Ok(())
    };
    match plane {
        "yz" => {
            for j in 0..n {
                for i in 0..n / 2 {
                    pair(weights, j * n + i, j * n + (n - 1 - i))?;
                }
            }
        }
        "xz" => {
            for i in 0..n {
                for j in 0..n / 2 {
                    pair(weights, j * n + i, (n - 1 - j) * n + i)?;
                }
            }
        }
        "xy" => {}
        _ => return Err(KernelError::Invalid("symmetry plane must be yz, xz or xy".into())),
    }
    Ok(())
}

/// A ruled sheet through two polylines, sampled onto the same 4×4 sheet.
pub fn ruled_sheet(a: &[Vec3], b: &[Vec3]) -> Result<crate::Body> {
    if a.len() < 2 || b.len() < 2 || a.len() > MAX_CAGE_VERTS || b.len() > MAX_CAGE_VERTS {
        return Err(KernelError::Invalid("a ruled surface needs two curves".into()));
    }
    if a.iter().chain(b).any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid("a ruled surface point must be finite".into()));
    }
    let sample = |poly: &[Vec3]| -> Vec<Vec3> {
        (0..4)
            .map(|i| {
                let t = i as f64 / 3.0 * (poly.len() - 1) as f64;
                let i0 = (t.floor() as usize).min(poly.len() - 1);
                let i1 = (i0 + 1).min(poly.len() - 1);
                let f = t - i0 as f64;
                poly[i0] * (1.0 - f) + poly[i1] * f
            })
            .collect()
    };
    let (sa, sb) = (sample(a), sample(b));
    let mut pts = Vec::with_capacity(16);
    for row in 0..4 {
        let t = row as f64 / 3.0;
        for col in 0..4 {
            pts.push(sa[col] * (1.0 - t) + sb[col] * t);
        }
    }
    let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let weights = vec![1.0; pts.len()];
    let nurbs = NurbsSurface::new([3, 3], [knots.clone(), knots], &pts, &weights).map_err(|e| KernelError::Invalid(format!("ruled surface: {e}")))?;
    let ts = TSpline::from_nurbs(&nurbs).map_err(|e| KernelError::Invalid(format!("ruled surface: {e}")))?;
    crate::tspline_sheet(&ts)
}

#[cfg(test)]
mod tests {
    use solidcraft_geom::Vec3;

    use super::{FormBuild, MAX_CAGE_VERTS, MAX_SUBDIV_LEVEL, PointXform, bridge_deltas, cage_points, form_sheet, point_edit_delta, prepare};
    use crate::fuzz::Rng;
    use crate::measure;

    fn sheet(shape: &str, build: &FormBuild) -> crate::Body {
        form_sheet(shape, 20.0, 12.0, build).unwrap_or_else(|e| panic!("{shape}: {e}"))
    }

    fn area(shape: &str, build: &FormBuild) -> f64 {
        measure(&sheet(shape, build)).unwrap_or_else(|e| panic!("{e}")).area
    }

    fn symmetric(points: &[Vec3], weights: &[f64], n: usize, plane: &str) -> bool {
        for j in 0..n {
            for i in 0..n {
                let Some(p) = points.get(j * n + i).copied() else { return false };
                let Some(w) = weights.get(j * n + i).copied() else { return false };
                let (mi, mj) = match plane {
                    "yz" => (n - 1 - i, j),
                    "xz" => (i, n - 1 - j),
                    "xy" => (i, j),
                    _ => return false,
                };
                let Some(q) = points.get(mj * n + mi).copied() else { return false };
                let mirror = match plane {
                    "yz" => Vec3::new(-p.x, p.y, p.z),
                    "xz" => Vec3::new(p.x, -p.y, p.z),
                    "xy" => Vec3::new(p.x, p.y, -p.z),
                    _ => return false,
                };
                if q.dist(mirror) > 1e-8 {
                    return false;
                }
                if weights.get(mj * n + mi).is_none_or(|mw| (mw - w).abs() > 1e-8) {
                    return false;
                }
            }
        }
        true
    }

    #[test]
    fn caps_fail_before_a_refined_cage_is_built() {
        assert_eq!(MAX_CAGE_VERTS, 20_000);
        assert_eq!(MAX_SUBDIV_LEVEL, 6);
        assert!(cage_points(5).unwrap() <= MAX_CAGE_VERTS);
        assert!(cage_points(6).unwrap_err().to_string().contains("20000"));
        assert!(cage_points(7).unwrap_err().to_string().contains("at most 6"));
        let huge = FormBuild { subdiv: 6, ..FormBuild::default() };
        assert!(form_sheet("box", 10.0, 10.0, &huge).unwrap_err().to_string().contains("20000"));
        let mut pulls = Vec::new();
        pulls.resize(MAX_CAGE_VERTS + 1, (0, Vec3::ZERO));
        let over = FormBuild { pulls, ..FormBuild::default() };
        assert!(form_sheet("box", 10.0, 10.0, &over).unwrap_err().to_string().contains("not supported yet"));
        let nan = FormBuild { pulls: vec![(3, Vec3::new(f64::NAN, 0.0, 1.0))], ..FormBuild::default() };
        assert!(form_sheet("box", 10.0, 10.0, &nan).is_err());
    }

    #[test]
    fn a_crease_sharpens_and_subdivision_keeps_the_cage_count() {
        let pulled = FormBuild { pulls: vec![(5, Vec3::new(0.0, 0.0, 4.0))], ..FormBuild::default() };
        let creased = FormBuild { pulls: pulled.pulls.clone(), creases: vec![0, 1], ..FormBuild::default() };
        assert!((area("box", &pulled) - area("box", &creased)).abs() > 1e-3);
        let cage = prepare("box", 20.0, 12.0, &FormBuild { subdiv: 1, ..FormBuild::default() }).unwrap();
        assert_eq!(cage.points.len(), cage_points(1).unwrap());
        assert_eq!(cage.n, 7);
        let opened = FormBuild { separate_edges: vec![4], ..FormBuild::default() };
        assert!((area("cylinder", &FormBuild::default()) - area("cylinder", &opened)).abs() > 1e-3);
        let pole = FormBuild { separate_edges: vec![0], ..FormBuild::default() };
        assert!(form_sheet("sphere", 20.0, 12.0, &pole).unwrap_err().to_string().contains("not supported yet"));
        let plain = area("box", &FormBuild::default());
        assert!((plain - area("box", &pulled)).abs() > 1e-3);
        let sym = FormBuild { pulls: vec![(0, Vec3::new(1.0, 0.0, 4.0))], symmetry: Some("yz".into()), ..FormBuild::default() };
        let mirrored = measure(&sheet("box", &sym)).unwrap();
        assert!((mirrored.bbox.min.x + mirrored.bbox.max.x).abs() < 0.2, "{:?}", mirrored.bbox);
        assert!(mirrored.centroid.x.abs() < 0.2, "{:?}", mirrored.centroid);
    }

    #[test]
    fn rotate_scale_and_bridge_are_real_deltas() {
        let rotate = point_edit_delta("box", 30.0, 18.0, &[], 0, &PointXform::RotateZ(std::f64::consts::FRAC_PI_2)).unwrap();
        assert!((rotate.x - 24.0).abs() < 1e-9 && (rotate.y + 6.0).abs() < 1e-9 && rotate.z.abs() < 1e-9, "{rotate:?}");
        let scale = point_edit_delta("box", 30.0, 18.0, &[], 0, &PointXform::Scale(2.0)).unwrap();
        assert!((scale.x + 15.0).abs() < 1e-9 && (scale.y + 9.0).abs() < 1e-9 && scale.z.abs() < 1e-9, "{scale:?}");
        assert!(point_edit_delta("box", 30.0, 18.0, &[], 0, &PointXform::RotateZ(f64::NAN)).is_err());
        let (da, db) = bridge_deltas("box", 30.0, 18.0, &[], 0, 3).unwrap();
        assert!(da.z > 1.0 && db.z > 1.0, "{da:?} {db:?}");
        assert!(bridge_deltas("box", 30.0, 18.0, &[], 5, 6).unwrap_err().to_string().contains("not supported yet"));
        assert!(bridge_deltas("box", 30.0, 18.0, &[], 4, 4).unwrap_err().to_string().contains("not supported yet"));
    }

    /// Feature: fusion360-feature-parity, Property 9: Cage edits re-evaluate deterministically and change the body
    /// Feature: fusion360-feature-parity, Property 10: Symmetric sculpting yields a symmetric body
    #[test]
    fn property_9_and_10_cage_edits_replay_and_symmetry_holds() {
        let shapes = ["box", "plane", "cylinder", "sphere", "quadball", "torus"];
        let planes = ["yz", "xz", "xy"];
        let mut rng = Rng::new(0xF0_4D_C0_DE);
        for n in 0..100 {
            let shape = shapes[n % shapes.len()];
            let length = 10.0 + rng.unit() * 30.0;
            let width = 8.0 + rng.unit() * 24.0;
            let plane = planes[n % planes.len()];
            let mut pulls = Vec::new();
            for _ in 0..rng.below(4) {
                pulls.push((rng.below(16) as u32, Vec3::new((rng.unit() - 0.5) * 6.0, (rng.unit() - 0.5) * 6.0, (rng.unit() - 0.5) * 6.0)));
            }
            let mut creases = Vec::new();
            if rng.unit() < 0.5 {
                let index = rng.below(16) as u32;
                creases.push(index);
                creases.push(index.saturating_add(1).min(15));
            }
            let mut separate_edges = Vec::new();
            if matches!(shape, "box" | "plane" | "cylinder") && rng.unit() < 0.4 {
                separate_edges.push(rng.below(24) as u32);
            }
            let subdiv = u32::from(n % 11 == 0);
            let build = FormBuild { pulls, creases, separate_edges, subdiv, symmetry: Some(plane.into()) };
            let once = prepare(shape, length, width, &build).unwrap_or_else(|e| panic!("{shape} {n}: {e}"));
            let twice = prepare(shape, length, width, &build).unwrap();
            assert_eq!(once.points, twice.points);
            assert_eq!(once.weights, twice.weights);
            assert!(symmetric(&once.points, &once.weights, once.n, plane), "{shape} {plane} cage {n}");
            if n % 5 == 0 {
                let (a, b) = (
                    form_sheet(shape, length, width, &build).unwrap_or_else(|e| panic!("{shape} {n}: {e}")),
                    form_sheet(shape, length, width, &build).unwrap(),
                );
                let (ma, mb) = (measure(&a).unwrap(), measure(&b).unwrap());
                assert!((ma.area - mb.area).abs() < 1e-6 && (ma.volume - mb.volume).abs() < 1e-6, "{shape} {n}");
                assert!((ma.centroid.x - mb.centroid.x).abs() < 1e-6, "{shape} {n}");
            }
        }
    }
}
