//! Surface bodies (open shells) and the operations that turn them into solids: Patch (a face
//! filling a closed loop of edges — planar, or B-spline patches for a loop that is not — or a
//! planar sketch region), Stitch (surfaces sharing edges sewn
//! into one body, a solid when they close), Trim (a surface cut by a tool, one side kept),
//! Thicken (a surface offset along its normals into a solid) and copying faces out of a body.

use std::collections::HashMap;

use mt::{BoundedCurve, InnerSpace, MetricSpace, ParametricCurve, ParametricSurface3D, SearchNearestParameter};
use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};
use truck_modeling as mt;

use crate::body::{Body, from_p3, p3};
use crate::history::{Change, EdgeOrigin, FaceOrigin};
use crate::record::{Rebuild, rebuilt_body};
use crate::{KernelError, Result, guard};

impl Body {
    /// A surface body: some shell does not close.
    pub fn is_surface(&self) -> bool {
        !self.is_mesh() && self.solid.boundaries().iter().any(|s| s.shell_condition() != mt::ShellCondition::Closed)
    }
}

/// A body of faces as they are (no orientation fix: a surface has no inside).
pub(crate) fn surface_body(faces: Vec<mt::Face>) -> Body {
    surface_of(mt::Shell::from(faces))
}

/// A body of a shell as it is.
fn surface_of(shell: mt::Shell) -> Body {
    Body {
        solid: std::sync::Arc::new(mt::Solid::new_unchecked(vec![shell])),
        coarse_missing: None,
        mesh: None,
        color: None,
        paint: None,
        splits: None,
        face_sources: None,
        history: None,
        regularization_protected: false,
    }
}

/// A body from faces: a solid when they close, else a surface; with the history of the
/// operation that made the faces from `operands` as `rec` records.
fn body_of(faces: Vec<mt::Face>, operands: &[&mt::Solid], rec: &Rebuild) -> Result<Body> {
    let shell = mt::Shell::from(faces);
    if shell.shell_condition() == mt::ShellCondition::Closed {
        crate::record::rebuilt(mt::Solid::new_unchecked(vec![shell]), operands, rec)
    } else {
        Ok(rebuilt_body(surface_of(shell), operands, rec))
    }
}

/// A planar surface filling a sketch region. Its history has no operands: the face is new.
pub fn patch_region(plane: &Plane, region: &Region2) -> Result<Body> {
    let f = crate::build::face(plane, region)?;
    let mut rec = Rebuild::default();
    rec.made(&f, Vec::new());
    Ok(rebuilt_body(surface_body(vec![f]), &[], &rec))
}

/// The edge of a body nearest a point (by samples along each edge).
pub(crate) fn edge_near(body: &Body, p: Vec3) -> Option<mt::Edge> {
    let mut best: Option<(f64, mt::Edge)> = None;
    for e in body.solid.edge_iter() {
        let c = e.curve();
        let (t0, t1) = c.range_tuple();
        for k in 0..=32 {
            let d = from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 32.0)).dist(p);
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, e.clone()));
            }
        }
    }
    best.filter(|(d, _)| *d <= body.size() * 0.05 + 1e-6).map(|(_, e)| e)
}

/// A surface filling a closed loop of a body's edges (each picked by a point on it): one face on
/// the body's own edges, planar when the loop is certainly planar ([`crate::plane_fit`]), else a
/// B-spline height field over the loop's plane ([`crate::fill`]; loops that do not bound one are
/// refused). Its history is over `[body]`: the face is new, its edges the body's own.
pub fn patch_edges(body: &Body, at: &[Vec3]) -> Result<Body> {
    body.require_brep("patch")?;
    if at.is_empty() || at.len() > 10_000 {
        return Err(KernelError::Invalid("pick the edges of a closed loop".into()));
    }
    guard("patch", || {
        let chain = picked_loop(body, at)?;
        let Some(face) = crate::plane_fit::planar_face(&chain, crate::fill::planarity_tolerance(body))? else {
            // Not certainly planar: a fitted surface.
            return crate::fill::fill_loop(body, &chain);
        };
        // A new face on the body's own edges.
        let mut rec = Rebuild::default();
        rec.made(&face, Vec::new());
        Ok(rebuilt_body(surface_body(vec![face]), &[&body.solid], &rec))
    })
}

/// The edges of `body` picked by points on them, chained into one closed loop (each turned to
/// run along it). Call it under [`guard`]: sampling an intersection curve can fail in truck.
pub(crate) fn picked_loop(body: &Body, at: &[Vec3]) -> Result<Vec<mt::Edge>> {
    let mut edges: Vec<mt::Edge> = Vec::new();
    for p in at {
        let e = edge_near(body, *p).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}")))?;
        if !edges.iter().any(|x| x.id() == e.id()) {
            edges.push(e);
        }
    }
    let mut pool = edges;
    let first = pool.pop().ok_or_else(|| KernelError::Invalid("no edges".into()))?;
    let start = first.front().id();
    let mut at_v = first.back().clone();
    let mut chain = vec![first];
    while at_v.id() != start {
        let k = pool
            .iter()
            .position(|e| e.front().id() == at_v.id() || e.back().id() == at_v.id())
            .ok_or_else(|| KernelError::Invalid("the edges do not form a closed loop".into()))?;
        let e = pool.remove(k);
        let e = if e.front().id() == at_v.id() { e } else { e.inverse() };
        at_v = e.back().clone();
        chain.push(e);
    }
    if !pool.is_empty() {
        return Err(KernelError::Invalid("the edges form more than one loop".into()));
    }
    Ok(chain)
}

/// Faces of a body copied out as a surface body (faces picked by points on them). Its history is
/// over `[body]`: the faces and their edges are the body's.
pub fn copy_faces(body: &Body, at: &[Vec3]) -> Result<Body> {
    body.require_brep("copy faces")?;
    let faces: Vec<&mt::Face> = body.solid.face_iter().collect();
    let mut chosen = Vec::new();
    for p in at {
        // The face whose mesh passes nearest the point.
        let i = nearest_face(body, *p)?;
        if !chosen.contains(&i) {
            chosen.push(i);
        }
    }
    let copied = surface_body(chosen.into_iter().filter_map(|i| faces.get(i).map(|f| (*f).clone())).collect());
    Ok(rebuilt_body(copied, &[&body.solid], &Rebuild::default()))
}

/// Index of the face nearest a point (over its mesh triangles).
fn nearest_face(body: &Body, p: Vec3) -> Result<usize> {
    let m = body.tessellate(body.size() * 2e-3)?;
    let mut best: Option<(f64, u32)> = None;
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let Some([a, b, c]) = m.tri(t) else { continue };
        let d = ((a + b + c) / 3.0).dist(p).min(a.dist(p)).min(b.dist(p)).min(c.dist(p));
        if best.is_none_or(|(x, _)| d < x) {
            best = Some((d, *f));
        }
    }
    best.map(|(_, f)| f as usize).ok_or_else(|| KernelError::Invalid("no face there".into()))
}

/// Sew surface bodies along edges that coincide (within `tol`): one body, a solid when the
/// faces close up. Faces are turned so that neighbours agree. The result's history is over
/// `bodies`: each face the one it was sewn from, each edge the edges sewn into it.
pub fn stitch(bodies: &[&Body], tol: f64) -> Result<Body> {
    if bodies.is_empty() {
        return Err(KernelError::Invalid("nothing to stitch".into()));
    }
    for b in bodies {
        b.require_brep("stitch")?;
    }
    let size = bodies.iter().map(|b| b.size()).fold(0.0, f64::max);
    let tol = if tol.is_finite() && tol > 0.0 { tol } else { size * 1e-6 + 1e-6 };
    guard("stitch", || {
        let faces: Vec<mt::Face> = bodies.iter().flat_map(|b| b.solid.face_iter().cloned().collect::<Vec<_>>()).collect();
        // Shared vertices by position, shared edges by their ends and middle.
        let mut verts: Vec<mt::Vertex> = Vec::new();
        let mut vmap: HashMap<mt::VertexID, mt::Vertex> = HashMap::new();
        let mut canon = |v: &mt::Vertex, verts: &mut Vec<mt::Vertex>| -> mt::Vertex {
            if let Some(c) = vmap.get(&v.id()) {
                return c.clone();
            }
            let p = v.point();
            let c = match verts.iter().find(|u| u.point().distance(p) <= tol) {
                Some(u) => u.clone(),
                None => {
                    verts.push(v.clone());
                    v.clone()
                }
            };
            vmap.insert(v.id(), c.clone());
            c
        };
        let mut edges: Vec<(mt::Edge, mt::Point3)> = Vec::new();
        let mut out = Vec::with_capacity(faces.len());
        let mut rec = Rebuild::default();
        for f in &faces {
            let mut wires = Vec::new();
            for w in f.absolute_boundaries() {
                let mut nw = Vec::new();
                for e in w.edge_iter() {
                    let abs = if e.orientation() { e.clone() } else { e.inverse() };
                    let (a, b) = (canon(abs.front(), &mut verts), canon(abs.back(), &mut verts));
                    let c = abs.curve();
                    let (t0, t1) = c.range_tuple();
                    let mid = c.subs(0.5 * (t0 + t1));
                    let found = edges.iter().find_map(|(x, m)| {
                        if m.distance(mid) > tol * 10.0 {
                            return None;
                        }
                        if x.front().id() == a.id() && x.back().id() == b.id() {
                            Some(x.clone())
                        } else if x.front().id() == b.id() && x.back().id() == a.id() {
                            Some(x.inverse())
                        } else {
                            None
                        }
                    });
                    let ne = match found {
                        Some(x) => x,
                        None => {
                            let x = mt::Edge::new_unchecked(&a, &b, c);
                            edges.push((x.clone(), mid));
                            x
                        }
                    };
                    rec.piece(&ne, &abs);
                    nw.push(if e.orientation() { ne } else { ne.inverse() });
                }
                wires.push(mt::Wire::from(nw));
            }
            let mut nf = mt::Face::new_unchecked(wires, f.surface());
            if !f.orientation() {
                nf.invert();
            }
            rec.whole(&nf, f);
            out.push(nf);
        }
        // Neighbours run their shared edges opposite ways.
        let mut uses: HashMap<mt::EdgeID, Vec<(usize, bool)>> = HashMap::new();
        for (i, f) in out.iter().enumerate() {
            for e in f.boundaries().iter().flat_map(|w| w.edge_iter()) {
                uses.entry(e.id()).or_default().push((i, e.orientation()));
            }
        }
        let mut flip: Vec<Option<bool>> = vec![None; out.len()];
        for s in 0..out.len() {
            if flip[s].is_some() {
                continue;
            }
            flip[s] = Some(false);
            let mut stack = vec![s];
            while let Some(i) = stack.pop() {
                let fl = flip[i].unwrap_or(false);
                let Some(f) = out.get(i) else { continue };
                for e in f.boundaries().iter().flat_map(|w| w.edge_iter()) {
                    let d = e.orientation() != fl;
                    for &(j, dj) in uses.get(&e.id()).into_iter().flatten() {
                        if j != i && flip[j].is_none() {
                            flip[j] = Some(dj == d);
                            stack.push(j);
                        }
                    }
                }
            }
        }
        let out: Vec<mt::Face> = out.into_iter().zip(flip).map(|(f, fl)| if fl == Some(true) { f.inverse() } else { f }).collect();
        let operands: Vec<&mt::Solid> = bodies.iter().map(|b| &*b.solid).collect();
        body_of(out, &operands, &rec)
    })
}

/// How a face moves along its normal: a plane slides, a circular cylinder changes radius.
#[derive(Clone, Copy)]
enum Offset {
    Plane { n: Vec3 },
    Cylinder { o: Vec3, axis: Vec3, r: f64, out: f64 },
}

impl Offset {
    fn of(f: &mt::Face) -> Option<Offset> {
        match crate::splitface::analytic_field(&f.oriented_surface())? {
            crate::splitface::Analytic::Plane { n } => Some(Offset::Plane { n }),
            crate::splitface::Analytic::Cylinder { o, axis, r, out } => Some(Offset::Cylinder { o, axis, r, out }),
        }
    }
    fn map(&self, p: Vec3, d: f64) -> Vec3 {
        match *self {
            Offset::Plane { n } => p + n * d,
            // Scaled about the axis (control points of a NURBS circle stay exact).
            Offset::Cylinder { o, axis, r, out } => {
                let v = p - o;
                let radial = v - axis * v.dot(axis);
                p + radial * (out * d / r)
            }
        }
    }
}

fn map_pt(off: &Offset, p: mt::Point3, d: f64) -> mt::Point3 {
    p3(off.map(from_p3(p), d))
}

/// Unit normal when every point lies on one plane, otherwise nothing.
fn coplanar_normal(pts: &[Vec3]) -> Option<Vec3> {
    if pts.len() < 3 || pts.len() > 20_000 {
        return None;
    }
    let mut n = Vec3::ZERO;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    let n = n.normalized()?;
    let origin = pts[0];
    let scale = pts.iter().map(|p| p.dist(origin)).fold(0.0_f64, f64::max).max(1.0);
    if pts.iter().all(|p| (*p - origin).dot(n).abs() <= scale * 1e-6 + 1e-7) { Some(n) } else { None }
}

/// A curve moved by an offset (control points mapped; exact for lines, and for circles square
/// to a cylinder's axis or in a plane).
fn map_curve(off: &Offset, c: &mt::Curve, d: f64) -> Result<mt::Curve> {
    Ok(match c {
        mt::Curve::Line(l) => mt::Curve::Line(mt::Line(map_pt(off, l.0, d), map_pt(off, l.1, d))),
        mt::Curve::BSplineCurve(b) => {
            let pts: Vec<mt::Point3> = b.control_points().iter().map(|p| map_pt(off, *p, d)).collect();
            mt::Curve::BSplineCurve(mt::BSplineCurve::new(b.knot_vec().clone(), pts))
        }
        mt::Curve::NurbsCurve(n) => {
            let nb = n.non_rationalized();
            let pts: Vec<mt::Vector4> = nb
                .control_points()
                .iter()
                .map(|h| {
                    let w = h.w;
                    let q = map_pt(off, mt::Point3::new(h.x / w, h.y / w, h.z / w), d);
                    mt::Vector4::new(q.x * w, q.y * w, q.z * w, w)
                })
                .collect();
            mt::Curve::NurbsCurve(mt::NurbsCurve::new(mt::BSplineCurve::new(nb.knot_vec().clone(), pts)))
        }
        other => {
            // Anything else: a cubic through moved samples.
            let (t0, t1) = other.range_tuple();
            let pts: Vec<Vec3> = (0..=32).map(|k| off.map(from_p3(other.subs(t0 + (t1 - t0) * k as f64 / 32.0)), d)).collect();
            mt::Curve::BSplineCurve(crate::build::interpolate_cubic(&pts).ok_or_else(|| KernelError::Failed("offset edge".into()))?)
        }
    })
}

/// A surface moved by an offset (control points mapped; exact for planes and NURBS cylinders).
fn map_surface(off: &Offset, s: &mt::Surface, d: f64) -> Result<mt::Surface> {
    Ok(match s {
        mt::Surface::Plane(pl) => {
            let (o, a, b) = (pl.origin(), pl.u_axis(), pl.v_axis());
            let o2 = map_pt(off, o, d);
            mt::Surface::Plane(mt::Plane::new(o2, o2 + a, o2 + b))
        }
        mt::Surface::BSplineSurface(b) => {
            let rows: Vec<Vec<mt::Point3>> = b.control_points().iter().map(|r| r.iter().map(|p| map_pt(off, *p, d)).collect()).collect();
            mt::Surface::BSplineSurface(mt::BSplineSurface::new((b.uknot_vec().clone(), b.vknot_vec().clone()), rows))
        }
        mt::Surface::NurbsSurface(n) => {
            let nb = n.non_rationalized();
            let rows: Vec<Vec<mt::Vector4>> = nb
                .control_points()
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|h| {
                            let w = h.w;
                            let q = map_pt(off, mt::Point3::new(h.x / w, h.y / w, h.z / w), d);
                            mt::Vector4::new(q.x * w, q.y * w, q.z * w, w)
                        })
                        .collect()
                })
                .collect();
            mt::Surface::NurbsSurface(mt::NurbsSurface::new(mt::BSplineSurface::new((nb.uknot_vec().clone(), nb.vknot_vec().clone()), rows)))
        }
        _ => return Err(KernelError::Invalid("not supported yet: thickening this kind of surface".into())),
    })
}

/// A parallel copy of a surface. Planes slide along their normal; circular cylinders change
/// radius. Sharp edges whose faces disagree, and surfaces that are neither, are refused.
pub fn surface_offset(body: &Body, distance: f64) -> Result<Body> {
    body.require_brep("surface offset")?;
    if !(distance.is_finite() && distance.abs() > 1e-9 && distance.abs() < 1.0e6) {
        return Err(KernelError::Invalid("offset distance must be a finite non-zero length".into()));
    }
    guard("surface.offset", || {
        let faces: Vec<mt::Face> = body.solid.face_iter().cloned().collect();
        if faces.is_empty() || faces.len() > 256 {
            return Err(KernelError::Invalid("not supported yet: offsetting this surface".into()));
        }
        let mut pts = Vec::new();
        for face in &faces {
            for edge in face.absolute_boundaries().iter().flat_map(|w| w.edge_iter()) {
                pts.push(from_p3(edge.front().point()));
            }
        }
        if let Some(n) = coplanar_normal(&pts) {
            return crate::transform(body, n * distance, Vec3::ZERO, Vec3::Z, 0.0);
        }
        let offs: Vec<Offset> = faces
            .iter()
            .map(|f| Offset::of(f).ok_or_else(|| KernelError::Invalid("not supported yet: offsetting this kind of surface".into())))
            .collect::<Result<_>>()?;
        if let Some(Offset::Plane { n }) = offs.first().copied() {
            let n = n.normalized().ok_or_else(|| KernelError::Invalid("the surface has no normal".into()))?;
            let planar = offs.iter().all(|off| match off {
                Offset::Plane { n: m } => m.normalized().is_some_and(|u| u.dist(n) < 1e-6 || u.dist(-n) < 1e-6),
                Offset::Cylinder { .. } => false,
            });
            if planar {
                return crate::transform(body, n * distance, Vec3::ZERO, Vec3::Z, 0.0);
            }
        }
        let tol = body.size() * 1e-4 + 1e-6;
        let mut vmoved: HashMap<mt::VertexID, mt::Vertex> = HashMap::new();
        let mut emoved: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
        for (f, off) in faces.iter().zip(&offs) {
            for e in f.absolute_boundaries().iter().flat_map(|w| w.edge_iter()) {
                let abs = if e.orientation() { e.clone() } else { e.inverse() };
                for v in [abs.front(), abs.back()] {
                    let p = map_pt(off, v.point(), distance);
                    match vmoved.get(&v.id()) {
                        Some(existing) if existing.point().distance(p) > tol => {
                            return Err(KernelError::Invalid("not supported yet: offsetting across a sharp edge".into()));
                        }
                        Some(_) => {}
                        None => {
                            vmoved.insert(v.id(), mt::Vertex::new(p));
                        }
                    }
                }
                if let std::collections::hash_map::Entry::Vacant(slot) = emoved.entry(abs.id()) {
                    let (Some(a), Some(b)) = (vmoved.get(&abs.front().id()), vmoved.get(&abs.back().id())) else {
                        return Err(KernelError::Invalid("offset edge is missing a vertex".into()));
                    };
                    slot.insert(mt::Edge::new_unchecked(a, b, map_curve(off, &abs.curve(), distance)?));
                }
            }
        }
        let mut out = Vec::new();
        for (f, off) in faces.iter().zip(&offs) {
            let wires: Vec<mt::Wire> = f
                .absolute_boundaries()
                .iter()
                .map(|w| {
                    w.edge_iter()
                        .filter_map(|e| {
                            let abs_id = if e.orientation() { e.id() } else { e.inverse().id() };
                            let m = emoved.get(&abs_id)?.clone();
                            Some(if e.orientation() { m } else { m.inverse() })
                        })
                        .collect::<Vec<_>>()
                        .into()
                })
                .collect();
            let mut nf = mt::Face::new_unchecked(wires, map_surface(off, &f.surface(), distance)?);
            if !f.orientation() {
                nf.invert();
            }
            out.push(nf);
        }
        Ok(surface_body(out))
    })
}

/// A surface body thickened into a solid: offset along the faces' normals by `thickness`
/// (negative: against them), or by half each way when `symmetric`. Planar faces and faces on
/// circular cylinders; neighbouring faces must meet smoothly (their offsets agree).
pub fn thicken(body: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    body.require_brep("thicken")?;
    if !(thickness.is_finite() && thickness.abs() > 1e-9) {
        return Err(KernelError::Invalid("thickness must be non-zero".into()));
    }
    let (d0, d1) = if symmetric { (-thickness / 2.0, thickness / 2.0) } else { (0.0, thickness) };
    let (lo, hi) = (d0.min(d1), d0.max(d1));
    let size = body.size();
    let tol = size * 1e-7 + 1e-9;
    guard("thicken", || {
        let faces: Vec<mt::Face> = body.solid.face_iter().cloned().collect();
        if faces.is_empty() {
            return Err(KernelError::Invalid("nothing to thicken".into()));
        }
        let offs: Vec<Offset> = faces
            .iter()
            .map(|f| {
                Offset::of(f).ok_or_else(|| KernelError::Invalid("not supported yet: thickening faces that are not planar or cylindrical".into()))
            })
            .collect::<Result<_>>()?;
        // Each vertex and edge moved to both sides (once; every face using it must agree).
        let mut vmoved: HashMap<mt::VertexID, (mt::Vertex, mt::Vertex)> = HashMap::new();
        let mut emoved: HashMap<mt::EdgeID, (mt::Edge, mt::Edge)> = HashMap::new();
        let mut uses: HashMap<mt::EdgeID, usize> = HashMap::new();
        for (f, off) in faces.iter().zip(&offs) {
            for e in f.absolute_boundaries().iter().flat_map(|w| w.edge_iter()) {
                let abs = if e.orientation() { e.clone() } else { e.inverse() };
                *uses.entry(abs.id()).or_default() += 1;
                for v in [abs.front(), abs.back()] {
                    let (a, b) = (map_pt(off, v.point(), lo), map_pt(off, v.point(), hi));
                    match vmoved.get(&v.id()) {
                        Some((va, vb)) => {
                            if va.point().distance(a) > tol * 1e3 || vb.point().distance(b) > tol * 1e3 {
                                return Err(KernelError::Invalid("not supported yet: thickening across a sharp edge between faces".into()));
                            }
                        }
                        None => {
                            vmoved.insert(v.id(), (mt::Vertex::new(a), mt::Vertex::new(b)));
                        }
                    }
                }
                if let std::collections::hash_map::Entry::Vacant(slot) = emoved.entry(abs.id()) {
                    let (Some((a0, a1)), Some((b0, b1))) = (vmoved.get(&abs.front().id()).cloned(), vmoved.get(&abs.back().id()).cloned()) else {
                        continue;
                    };
                    let c = abs.curve();
                    let lo_e = mt::Edge::new_unchecked(&a0, &b0, map_curve(off, &c, lo)?);
                    let hi_e = mt::Edge::new_unchecked(&a1, &b1, map_curve(off, &c, hi)?);
                    slot.insert((lo_e, hi_e));
                }
            }
        }
        // Lines joining each vertex's two copies.
        let mut sides: HashMap<mt::VertexID, mt::Edge> = HashMap::new();
        for (id, (a, b)) in &vmoved {
            sides.insert(*id, mt::Edge::new_unchecked(a, b, mt::Curve::Line(mt::Line(a.point(), b.point()))));
        }
        let moved_face = |f: &mt::Face, off: &Offset, which: usize, d: f64| -> Result<mt::Face> {
            let wires: Vec<mt::Wire> = f
                .absolute_boundaries()
                .iter()
                .map(|w| {
                    w.edge_iter()
                        .filter_map(|e| {
                            let abs_id = if e.orientation() { e.id() } else { e.inverse().id() };
                            let (a, b) = emoved.get(&abs_id)?;
                            let m = if which == 0 { a.clone() } else { b.clone() };
                            Some(if e.orientation() { m } else { m.inverse() })
                        })
                        .collect::<Vec<_>>()
                        .into()
                })
                .collect();
            let mut nf = mt::Face::new_unchecked(wires, map_surface(off, &f.surface(), d)?);
            if !f.orientation() {
                nf.invert();
            }
            Ok(nf)
        };
        let mut out = Vec::new();
        // History (one operand, the surface): a copy at no distance is the face itself, a copy
        // further out its offset, a wall is made along its free edge.
        let operand = crate::record::Operand::new(body);
        let edge_index = &operand.edges;
        let mut history = Vec::new();
        let copy = |i: usize, d: f64| {
            if d == 0.0 {
                crate::record::from_face(0, i, Change::Kept)
            } else {
                crate::record::generated(vec![FaceOrigin::Offset { operand: 0, face: i }])
            }
        };
        for (i, (f, off)) in faces.iter().zip(&offs).enumerate() {
            // The lower copy faces back, the upper one forward.
            out.push(moved_face(f, off, 0, lo)?.inverse());
            out.push(moved_face(f, off, 1, hi)?);
            history.extend([copy(i, lo), copy(i, hi)]);
            // Walls along free edges, outward across the edge.
            for w in f.boundaries() {
                for e in w.edge_iter() {
                    let abs_id = if e.orientation() { e.id() } else { e.inverse().id() };
                    if uses.get(&abs_id).copied().unwrap_or(0) != 1 {
                        continue;
                    }
                    let Some((le, he)) = emoved.get(&abs_id) else { continue };
                    let (le, he) = if e.orientation() { (le.clone(), he.clone()) } else { (le.inverse(), he.inverse()) };
                    let (Some(sa), Some(sb)) = (sides.get(&e.front().id()), sides.get(&e.back().id())) else { continue };
                    // A → B low, B → B' up, B' → A' high back, A' → A down.
                    let wire: mt::Wire = vec![le.clone(), sb.clone(), he.inverse(), sa.inverse()].into();
                    let ruled = mt::builder::homotopy(&le, &he);
                    let surf = ruled.surface();
                    // The wall's outward normal: across the edge, away from the face.
                    let c = le.oriented_curve();
                    let (t0, t1) = c.range_tuple();
                    let tm = 0.5 * (t0 + t1);
                    let p = c.subs(tm);
                    let tangent = c.der(tm);
                    let n =
                        f.oriented_surface().search_nearest_parameter(p, None, 50).map(|(u, v)| f.oriented_surface().normal(u, v)).unwrap_or(tangent);
                    let outward = tangent.cross(n);
                    let sn = surf.search_nearest_parameter(p, None, 50).map(|(u, v)| surf.normal(u, v)).unwrap_or(outward);
                    let wall = if sn.dot(outward) >= 0.0 {
                        mt::Face::new_unchecked(vec![wire], surf)
                    } else {
                        let mut x = mt::Face::new_unchecked(vec![wire.inverse()], surf);
                        x.invert();
                        x
                    };
                    out.push(wall);
                    let edge = edge_index.get(&abs_id).copied().ok_or_else(|| KernelError::Failed("thicken: an edge outside the surface".into()))?;
                    history.push(crate::record::generated(vec![FaceOrigin::Edge { operand: 0, edge }]));
                }
            }
        }
        let shell = mt::Shell::from(out);
        if shell.shell_condition() != mt::ShellCondition::Closed {
            return Err(KernelError::Failed("the thickened surface does not close".into()));
        }
        // The edges copied at no distance are the surface's own edges, rebuilt.
        let mut copies = HashMap::new();
        for (id, (low, high)) in &emoved {
            for (copy, d) in [(low, lo), (high, hi)] {
                if let Some(&edge) = edge_index.get(id)
                    && d == 0.0
                {
                    copies.insert(copy.id(), EdgeOrigin::Edge { operand: 0, edge });
                }
            }
        }
        crate::record::attach(Body::new(mt::Solid::new_unchecked(vec![shell]))?, &[operand], Some(history), copies)
    })
}

/// A surface body trimmed by a tool: its faces are cut where the tool meets them and the
/// pieces on the side of `keep` are kept. The result's history is over `[body]`.
pub fn trim(body: &Body, tool: &crate::SplitTool, keep: Vec3) -> Result<Body> {
    body.require_brep("trim")?;
    let total = body.solid.face_iter().count();
    let all: Vec<usize> = (0..total).collect();
    let (split, _) = crate::splitface::split_any(body, &all, tool)?;
    let side = crate::splitface::side_of_point(tool, keep)?;
    guard("trim", || {
        let mut kept = Vec::new();
        for f in split.solid.face_iter() {
            let x = crate::splitface::face_side(f, tool, split.size())?;
            if (x >= 0.0) == (side >= 0.0) {
                kept.push(f.clone());
            }
        }
        if kept.is_empty() {
            return Err(KernelError::Invalid("nothing is left on that side of the tool".into()));
        }
        // The kept pieces are the split's own; its history carries them back to `body`.
        let trimmed = rebuilt_body(surface_body(kept), &[&split.solid], &Rebuild::default());
        Ok(crate::record::after(trimmed, &split, &crate::record::counts(&[body])))
    })
}

/// A planar surface extended: the chosen straight edges (picked by points on them) move
/// outward in the face's plane by `distance`, their neighbours lengthened to meet them. The
/// result's history is over `[body]`: the face and each edge the one it was moved from.
pub fn extend(body: &Body, at: &[Vec3], distance: f64) -> Result<Body> {
    body.require_brep("extend")?;
    if !(distance.is_finite() && distance > 0.0) {
        return Err(KernelError::Invalid("the extension distance must be positive".into()));
    }
    let faces: Vec<mt::Face> = body.solid.face_iter().cloned().collect();
    let [face] = faces.as_slice() else { return Err(KernelError::Invalid("not supported yet: extending a surface of more than one face".into())) };
    let Some(crate::splitface::Analytic::Plane { n }) = crate::splitface::analytic_field(&face.oriented_surface()) else {
        return Err(KernelError::Invalid("not supported yet: extending a surface that is not planar".into()));
    };
    let chosen: Vec<mt::EdgeID> = at
        .iter()
        .map(|p| edge_near(body, *p).map(|e| e.id()).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}"))))
        .collect::<Result<_>>()?;
    guard("extend", || {
        let bounds = face.boundaries();
        let [outer] = bounds.as_slice() else { return Err(KernelError::Invalid("not supported yet: extending a surface with holes".into())) };
        let edges: Vec<mt::Edge> = outer.edge_iter().cloned().collect();
        let m = edges.len();
        // Each edge as a line (point, direction) moved outward when chosen.
        let mut lines = Vec::with_capacity(m);
        for e in &edges {
            let (a, b) = (from_p3(e.front().point()), from_p3(e.back().point()));
            if !matches!(e.curve(), mt::Curve::Line(_)) {
                return Err(KernelError::Invalid("not supported yet: extending a surface with curved edges".into()));
            }
            let d = (b - a).normalized().ok_or_else(|| KernelError::Invalid("a zero-length edge".into()))?;
            // Outward: right of the edge seen along the normal (the face is on its left).
            let out = d.cross(n);
            let shift = if chosen.contains(&e.id()) || chosen.contains(&e.inverse().id()) { distance } else { 0.0 };
            lines.push((a + out * shift, d));
        }
        // Corners: where consecutive lines meet.
        let meet = |(p, d): (Vec3, Vec3), (q, e): (Vec3, Vec3)| -> Option<Vec3> {
            let w = q - p;
            let c = d.cross(e);
            let cc = c.dot(c);
            if cc < 1e-24 {
                return None;
            }
            Some(p + d * (w.cross(e).dot(c) / cc))
        };
        let mut corners = Vec::with_capacity(m);
        for i in 0..m {
            let (Some(prev), Some(cur)) = (lines.get((i + m - 1) % m), lines.get(i)) else { continue };
            let p = meet(*prev, *cur).unwrap_or(cur.0);
            corners.push(mt::Vertex::new(p3(p)));
        }
        let mut rec = Rebuild::default();
        let wire: mt::Wire = (0..m)
            .filter_map(|i| {
                let (a, b, old) = (corners.get(i)?, corners.get((i + 1) % m)?, edges.get(i)?);
                let e = mt::Edge::new_unchecked(a, b, mt::Curve::Line(mt::Line(a.point(), b.point())));
                rec.piece(&e, old);
                Some(e)
            })
            .collect::<Vec<_>>()
            .into();
        let mut f = mt::builder::try_attach_plane(&[wire]).map_err(|e| KernelError::Failed(format!("extend: {e}")))?;
        if f.oriented_surface().normal(0.0, 0.0).dot(crate::body::v3(n)) < 0.0 {
            f = f.inverse();
        }
        rec.face(&f, face);
        Ok(rebuilt_body(surface_body(vec![f]), &[&body.solid], &rec))
    })
}

/// A ruled sheet through `stations`, with no caps. Each station is a polyline. Stations of
/// different lengths are resampled onto the longest one (at most 64 points). `close_profile`
/// joins the last profile point to the first. `close_path` joins the last station to the first
/// (a full turn). An open profile is a station that is not closed.
pub fn open_sweep(stations: &[Vec<Vec3>], close_profile: bool, close_path: bool) -> Result<Body> {
    if stations.len() < 2 || stations.len() > 256 {
        return Err(KernelError::Invalid("an open surface needs 2…256 stations".into()));
    }
    let stations = unify_stations(stations, close_profile)?;
    let n = stations.first().map(|s| s.len()).unwrap_or(0);
    if n < 2 {
        return Err(KernelError::Invalid("an open profile needs at least two points".into()));
    }
    if close_profile && n < 3 {
        return Err(KernelError::Invalid("a closed profile needs at least three points".into()));
    }
    let profile_steps = if close_profile { n } else { n - 1 };
    let path_steps = stations.len() - 1 + usize::from(close_path);
    if profile_steps == 0 || profile_steps.saturating_mul(path_steps) > 20_000 {
        return Err(KernelError::Invalid("the open surface has too many faces".into()));
    }
    guard("surface.open_sweep", || {
        let mut faces = Vec::new();
        let emit = |ia: usize, ib: usize, faces: &mut Vec<mt::Face>| -> Result<()> {
            let (Some(sa), Some(sb)) = (stations.get(ia), stations.get(ib)) else {
                return Err(KernelError::Invalid("open surface station".into()));
            };
            for i in 0..profile_steps {
                let j = (i + 1) % n;
                let (Some(&a), Some(&b), Some(&d), Some(&c)) = (sa.get(i), sa.get(j), sb.get(i), sb.get(j)) else {
                    return Err(KernelError::Invalid("open surface point".into()));
                };
                faces.extend(ruled_quad(a, b, c, d)?);
            }
            Ok(())
        };
        for i in 0..stations.len().saturating_sub(1) {
            emit(i, i + 1, &mut faces)?;
        }
        if close_path {
            emit(stations.len() - 1, 0, &mut faces)?;
        }
        if faces.is_empty() {
            return Err(KernelError::Invalid("the open surface has no area".into()));
        }
        let mut rec = Rebuild::default();
        for face in &faces {
            rec.made(face, Vec::new());
        }
        Ok(rebuilt_body(surface_body(faces), &[], &rec))
    })
}

/// Carry an open or closed profile along a 3D polyline and build the sheet. No caps.
pub fn open_sweep_along(profile: &[Vec3], closed: bool, path: &[Vec3]) -> Result<Body> {
    let path = clean_open_path(path)?;
    if crosses_open_path(&path)? {
        return Err(KernelError::Failed("not supported yet: a sweep path that crosses itself".into()));
    }
    let stations = carry_profile(profile, &path)?;
    let closed_path = path.first().zip(path.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-6);
    open_sweep(&stations, closed, closed_path && path.len() > 2)
}

fn clean_open_path(path: &[Vec3]) -> Result<Vec<Vec3>> {
    if path.len() < 2 || path.len() > 8_192 {
        return Err(KernelError::Invalid("a sweep path needs 2…8192 points".into()));
    }
    let mut out = Vec::with_capacity(path.len());
    for p in path {
        if !(p.is_finite() && p.x.abs() < 1e7 && p.y.abs() < 1e7 && p.z.abs() < 1e7) {
            return Err(KernelError::Invalid("the sweep path has a bad point".into()));
        }
        if out.last().is_none_or(|last: &Vec3| last.dist(*p) > 1e-9) {
            out.push(*p);
        }
    }
    if out.len() < 2 {
        return Err(KernelError::Invalid("the sweep path has no length".into()));
    }
    Ok(out)
}

fn crosses_open_path(pts: &[Vec3]) -> Result<bool> {
    let segs = pts.len().saturating_sub(1);
    if segs < 3 {
        return Ok(false);
    }
    if segs > 512 {
        return Err(KernelError::Failed("not supported yet: a sweep path longer than 512 samples".into()));
    }
    for i in 0..segs {
        for j in i + 2..segs {
            if i == 0 && j + 1 == segs {
                continue;
            }
            let (Some(a), Some(b), Some(c), Some(d)) = (pts.get(i), pts.get(i + 1), pts.get(j), pts.get(j + 1)) else { continue };
            if segments_meet(*a, *b, *c, *d) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn segments_meet(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> bool {
    let u = b - a;
    let v = d - c;
    let w = a - c;
    let uu = u.dot(u);
    let vv = v.dot(v);
    let uv = u.dot(v);
    let denom = uu * vv - uv * uv;
    let tol = (uu.sqrt() + vv.sqrt()) * 1e-6 + 1e-5;
    if denom.abs() < 1e-18 {
        return false;
    }
    let uw = u.dot(w);
    let vw = v.dot(w);
    let s = ((uv * vw - vv * uw) / denom).clamp(0.0, 1.0);
    let t = if vv > 1e-18 { ((uv * s + vw) / vv).clamp(0.0, 1.0) } else { 0.0 };
    let interior = s > 1e-3 && s < 0.999 && t > 1e-3 && t < 0.999;
    interior && (w + u * s - v * t).len() <= tol
}

fn carry_profile(profile: &[Vec3], path: &[Vec3]) -> Result<Vec<Vec<Vec3>>> {
    if profile.len() < 2 || profile.len() > 10_000 {
        return Err(KernelError::Invalid("an open profile needs 2…10000 points".into()));
    }
    for p in profile {
        if !(p.is_finite() && p.x.abs() < 1e7 && p.y.abs() < 1e7 && p.z.abs() < 1e7) {
            return Err(KernelError::Invalid("an open surface point is not finite".into()));
        }
    }
    let origin = path.first().copied().ok_or_else(|| KernelError::Invalid("sweep path".into()))?;
    let offsets: Vec<Vec3> = profile.iter().map(|p| *p - origin).collect();
    let tangents = open_tangents(path)?;
    let frames = open_frames(path, &tangents).ok_or_else(|| KernelError::Invalid("the sweep path doubles back".into()))?;
    let f0 = *frames.first().ok_or_else(|| KernelError::Invalid("sweep path".into()))?;
    let mut stations = Vec::with_capacity(path.len());
    for (i, fi) in frames.iter().enumerate() {
        let at = path.get(i).copied().ok_or_else(|| KernelError::Invalid("sweep path".into()))?;
        let map = |v: Vec3| fi[0] * f0[0].dot(v) + fi[1] * f0[1].dot(v) + fi[2] * f0[2].dot(v);
        stations.push(offsets.iter().map(|v| at + map(*v)).collect());
    }
    Ok(stations)
}

fn open_tangents(p: &[Vec3]) -> Result<Vec<Vec3>> {
    let n = p.len();
    (0..n)
        .map(|i| {
            let prev = p.get(i.saturating_sub(1)).copied().unwrap_or(Vec3::ZERO);
            let next = p.get((i + 1).min(n.saturating_sub(1))).copied().unwrap_or(Vec3::ZERO);
            (next - prev).normalized().ok_or_else(|| KernelError::Invalid("the sweep path has no direction".into()))
        })
        .collect()
}

fn open_frames(pts: &[Vec3], tangents: &[Vec3]) -> Option<Vec<[Vec3; 3]>> {
    let t0 = *tangents.first()?;
    let helper = if t0.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let mut r = t0.cross(helper).normalized()?;
    let mut out = vec![[t0, r, t0.cross(r)]];
    for i in 0..pts.len().saturating_sub(1) {
        let (x0, x1, ti, tn) = (*pts.get(i)?, *pts.get(i + 1)?, *tangents.get(i)?, *tangents.get(i + 1)?);
        let v1 = x1 - x0;
        let c1 = v1.dot(v1);
        let (rl, tl) = if c1 > 1e-24 { (r - v1 * (2.0 / c1 * v1.dot(r)), ti - v1 * (2.0 / c1 * v1.dot(ti))) } else { (r, ti) };
        let v2 = tn - tl;
        let c2 = v2.dot(v2);
        r = if c2 > 1e-24 { rl - v2 * (2.0 / c2 * v2.dot(rl)) } else { rl };
        r = (r - tn * r.dot(tn)).normalized()?;
        out.push([tn, r, tn.cross(r)]);
    }
    Some(out)
}

fn unify_stations(stations: &[Vec<Vec3>], closed: bool) -> Result<Vec<Vec<Vec3>>> {
    let stations: Vec<Vec<Vec3>> = stations.iter().map(|s| dedup_station(s, closed)).collect::<Result<_>>()?;
    let n = stations.iter().map(|s| s.len()).fold(0, usize::max);
    if !(2..=10_000).contains(&n) {
        return Err(KernelError::Invalid("an open profile needs 2…10000 points".into()));
    }
    let target = if stations.iter().all(|s| s.len() == n) { n } else { n.clamp(2, 64) };
    stations.iter().map(|s| if s.len() == target { Ok(s.clone()) } else { sample_station(s, target, closed) }).collect()
}

/// Drop repeated points. A closed profile that repeats its first point at the end loses that
/// repeat, so the closing edge is added once.
fn dedup_station(pts: &[Vec3], closed: bool) -> Result<Vec<Vec3>> {
    if pts.len() > 10_000 {
        return Err(KernelError::Invalid("an open profile needs 2…10000 points".into()));
    }
    let mut out = Vec::with_capacity(pts.len());
    for p in pts {
        if !(p.is_finite() && p.x.abs() < 1e7 && p.y.abs() < 1e7 && p.z.abs() < 1e7) {
            return Err(KernelError::Invalid("an open surface point is not finite".into()));
        }
        if out.last().is_none_or(|last: &Vec3| last.dist(*p) > 1e-9) {
            out.push(*p);
        }
    }
    if closed && out.len() >= 2 && out.first().zip(out.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-6) {
        out.pop();
    }
    if out.len() < 2 {
        return Err(KernelError::Invalid("an open profile needs at least two points".into()));
    }
    Ok(out)
}

fn sample_station(pts: &[Vec3], n: usize, closed: bool) -> Result<Vec<Vec3>> {
    if pts.len() < 2 || n < 2 {
        return Err(KernelError::Invalid("an open profile needs at least two points".into()));
    }
    let mut cum = vec![0.0];
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let last = cum.last().copied().unwrap_or(0.0);
        cum.push(last + a.dist(*b));
    }
    if closed && let (Some(a), Some(b)) = (pts.first(), pts.last()) {
        let last = cum.last().copied().unwrap_or(0.0);
        cum.push(last + a.dist(*b));
    }
    let total = cum.last().copied().unwrap_or(0.0);
    if total < 1e-9 {
        return Err(KernelError::Invalid("the open profile has no length".into()));
    }
    let span = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let s = total * i as f64 / span as f64;
        let k = cum.iter().position(|c| *c >= s - 1e-12).unwrap_or(cum.len().saturating_sub(1)).saturating_sub(1);
        let (ca, cb) = (cum.get(k).copied().unwrap_or(0.0), cum.get(k + 1).copied().unwrap_or(total));
        let t = if cb - ca > 1e-15 { ((s - ca) / (cb - ca)).clamp(0.0, 1.0) } else { 0.0 };
        let a = if closed && k >= pts.len() { pts.last().copied() } else { pts.get(k).copied() };
        let b = if closed && k + 1 >= pts.len() { pts.first().copied() } else { pts.get(k + 1).copied() };
        let (Some(a), Some(b)) = (a, b) else { return Err(KernelError::Invalid("open surface point".into())) };
        out.push(a * (1.0 - t) + b * t);
    }
    Ok(out)
}

fn ruled_quad(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> Result<Vec<mt::Face>> {
    let area = (b - a).cross(d - a).len() + (c - b).cross(d - b).len();
    if area < 1e-8 {
        return Ok(Vec::new());
    }
    match planar_loop(&[a, b, c, d]) {
        Ok(face) => Ok(vec![face]),
        Err(_) => {
            let mut faces = Vec::new();
            if let Ok(face) = planar_loop(&[a, b, c]) {
                faces.push(face);
            }
            if let Ok(face) = planar_loop(&[a, c, d]) {
                faces.push(face);
            }
            Ok(faces)
        }
    }
}

fn planar_loop(pts: &[Vec3]) -> Result<mt::Face> {
    let build = |pts: &[Vec3]| -> Result<mt::Face> {
        let verts: Vec<mt::Vertex> = pts.iter().copied().map(|p| mt::Vertex::new(p3(p))).collect();
        let mut edges = Vec::with_capacity(pts.len());
        for i in 0..pts.len() {
            let (Some(a), Some(b)) = (verts.get(i), verts.get((i + 1) % pts.len())) else { continue };
            if a.point().distance(b.point()) < 1e-9 {
                return Err(KernelError::Invalid("a zero-length surface edge".into()));
            }
            edges.push(mt::builder::line(a, b));
        }
        if edges.len() != pts.len() {
            return Err(KernelError::Invalid("open surface face".into()));
        }
        let wire: mt::Wire = edges.into();
        mt::builder::try_attach_plane(&[wire]).map_err(|e| KernelError::Failed(format!("open surface: {e}")))
    };
    match build(pts) {
        Ok(face) => Ok(face),
        Err(_) => {
            let mut rev: Vec<Vec3> = pts.to_vec();
            rev.reverse();
            build(&rev)
        }
    }
}

/// One face on a closed polyline. Planar input keeps the shoelace area. A guide whose ends
/// meet the boundary (including the middle of an edge) splits it into two ruled sheets; a
/// guide that misses, or that leaves a planar boundary, is refused when it does not lie in
/// that plane — a coplanar guide keeps the polygon's area.
pub fn patch_guided(boundary: &[Vec3], guide: Option<&[Vec3]>) -> Result<Body> {
    let loop_pts = clean_boundary(boundary)?;
    let Some(guide) = guide.filter(|g| !g.is_empty()) else {
        return patch_polygon(&loop_pts);
    };
    guide_sheet(&loop_pts, guide)
}

/// A sketch loop (`outer`) plus edge picks (`extras`). Picks that lie on `outer` confirm that
/// loop. Picks that form one coplanar hole inside it become that hole, and the face area is
/// the polygon minus the hole. Anything else is `not supported yet`.
pub fn patch_mixed(outer: &[Vec3], extras: &[Vec3], guide: Option<&[Vec3]>) -> Result<Body> {
    if extras.is_empty() {
        return patch_guided(outer, guide);
    }
    if extras.len() > 10_000 || extras.iter().any(|p| !(p.is_finite() && p.x.abs() < 1e7 && p.y.abs() < 1e7 && p.z.abs() < 1e7)) {
        return Err(KernelError::Invalid("a patch point is not finite".into()));
    }
    let loop_pts = clean_boundary(outer)?;
    let size = loop_pts.iter().fold(0.0_f64, |s, p| s.max(p.len()));
    let tol = (size * 1e-4).max(1e-3);
    if extras.iter().all(|p| dist_to_loop(&loop_pts, *p) <= tol) {
        return patch_guided(&loop_pts, guide);
    }
    if guide.is_some_and(|g| !g.is_empty()) {
        return Err(KernelError::Invalid("not supported yet: a guide curve on a patch with a hole".into()));
    }
    let hole = clean_boundary(extras)?;
    let Some(plane) = polygon_plane(&loop_pts) else {
        return Err(KernelError::Invalid("not supported yet: a patch boundary that is not planar".into()));
    };
    if loop_pts.iter().chain(hole.iter()).any(|p| plane.height(*p).abs() > 1e-3) {
        return Err(KernelError::Invalid("not supported yet: a patch boundary that is not planar".into()));
    }
    if !hole_inside(&plane, &loop_pts, &hole) {
        return Err(KernelError::Invalid("not supported yet: mixed edge and sketch boundaries that do not form one loop or one hole".into()));
    }
    patch_with_hole(&plane, &loop_pts, &hole)
}

fn guide_sheet(loop_pts: &[Vec3], guide: &[Vec3]) -> Result<Body> {
    validate_guide(guide)?;
    let (Some(&g0), Some(&g1)) = (guide.first(), guide.last()) else {
        return Err(KernelError::Invalid("a guide curve needs two ends".into()));
    };
    let size = loop_pts.iter().fold(0.0_f64, |s, p| s.max(p.len()));
    let tol = (size * 1e-6).max(1e-4);
    let h0 = project_onto_loop(loop_pts, g0, tol)?;
    let h1 = project_onto_loop(loop_pts, g1, tol)?;
    if h0.point.dist(h1.point) <= 1e-6 {
        return Err(KernelError::Invalid("the guide meets the boundary at one point".into()));
    }
    if let Some(plane) = polygon_plane(loop_pts)
        && loop_pts.iter().all(|p| plane.height(*p).abs() < 1e-3)
        && guide.iter().all(|p| plane.height(*p).abs() < 1e-3)
    {
        return patch_polygon(loop_pts);
    }
    let (loop_pts, i, j) = insert_hits(loop_pts, h0, h1)?;
    let mut snapped = guide.to_vec();
    if let (Some(first), Some(start)) = (snapped.first_mut(), loop_pts.get(i)) {
        *first = *start;
    }
    if let (Some(last), Some(end)) = (snapped.last_mut(), loop_pts.get(j)) {
        *last = *end;
    }
    let a = chain_between(&loop_pts, i, j);
    let b = chain_between(&loop_pts, j, i);
    if a.len() < 2 || b.len() < 2 {
        return Err(KernelError::Invalid("the guide meets the boundary at one point".into()));
    }
    let mut back = snapped.clone();
    back.reverse();
    let (chain_a, guide_a) = rail_pair(&a, &snapped)?;
    let (chain_b, guide_b) = rail_pair(&b, &back)?;
    let left = open_sweep(&[chain_a, guide_a], false, false)?;
    let right = open_sweep(&[chain_b, guide_b], false, false)?;
    stitch(&[&left, &right], 0.0)
}

fn clean_boundary(boundary: &[Vec3]) -> Result<Vec<Vec3>> {
    if boundary.len() < 3 || boundary.len() > 10_000 {
        return Err(KernelError::Invalid("a patch boundary does not close".into()));
    }
    for p in boundary {
        if !(p.is_finite() && p.x.abs() < 1e7 && p.y.abs() < 1e7 && p.z.abs() < 1e7) {
            return Err(KernelError::Invalid("a patch point is not finite".into()));
        }
    }
    let mut loop_pts = boundary.to_vec();
    if loop_pts.first().zip(loop_pts.last()).is_some_and(|(a, b)| a.dist(*b) < 1e-4) {
        loop_pts.pop();
    }
    if loop_pts.len() < 3 {
        return Err(KernelError::Invalid("a patch boundary does not close".into()));
    }
    Ok(loop_pts)
}

fn validate_guide(guide: &[Vec3]) -> Result<()> {
    if guide.len() < 2 || guide.len() > 10_000 || guide.iter().any(|p| !(p.is_finite() && p.x.abs() < 1e7 && p.y.abs() < 1e7 && p.z.abs() < 1e7)) {
        return Err(KernelError::Invalid("a guide curve needs 2…10000 finite points".into()));
    }
    Ok(())
}

/// Sample both rails at every vertex of either, so a boundary corner and a guide ridge stay put.
fn rail_pair(chain: &[Vec3], guide: &[Vec3]) -> Result<(Vec<Vec3>, Vec<Vec3>)> {
    let mut params = arc_fractions(chain)?;
    for t in arc_fractions(guide)? {
        if params.iter().all(|u| (u - t).abs() > 1e-4) {
            params.push(t);
        }
    }
    params.sort_by(|a, b| a.total_cmp(b));
    if params.len() > 64 {
        let mut slim = vec![0.0];
        for i in 1..63 {
            slim.push(i as f64 / 63.0);
        }
        slim.push(1.0);
        params = slim;
    }
    let chain_s = params.iter().map(|t| point_at_fraction(chain, *t)).collect::<Result<Vec<_>>>()?;
    let guide_s = params.iter().map(|t| point_at_fraction(guide, *t)).collect::<Result<Vec<_>>>()?;
    Ok((chain_s, guide_s))
}

fn arc_fractions(pts: &[Vec3]) -> Result<Vec<f64>> {
    let total = polyline_length(pts)?;
    let mut out = vec![0.0];
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        acc += a.dist(*b);
        out.push(acc / total);
    }
    if let Some(last) = out.last_mut() {
        *last = 1.0;
    }
    Ok(out)
}

fn polyline_length(pts: &[Vec3]) -> Result<f64> {
    let mut total = 0.0;
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        total += a.dist(*b);
    }
    if total < 1e-9 { Err(KernelError::Invalid("the open profile has no length".into())) } else { Ok(total) }
}

fn point_at_fraction(pts: &[Vec3], t: f64) -> Result<Vec3> {
    if t <= 1e-12 {
        return pts.first().copied().ok_or_else(|| KernelError::Invalid("open surface point".into()));
    }
    if t >= 1.0 - 1e-12 {
        return pts.last().copied().ok_or_else(|| KernelError::Invalid("open surface point".into()));
    }
    let total = polyline_length(pts)?;
    let target = total * t;
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let seg = a.dist(*b);
        if acc + seg >= target - 1e-9 {
            let u = if seg > 1e-12 { ((target - acc) / seg).clamp(0.0, 1.0) } else { 0.0 };
            return Ok(*a * (1.0 - u) + *b * u);
        }
        acc += seg;
    }
    pts.last().copied().ok_or_else(|| KernelError::Invalid("open surface point".into()))
}

struct LoopHit {
    seg: usize,
    t: f64,
    point: Vec3,
}

fn project_onto_loop(pts: &[Vec3], p: Vec3, tol: f64) -> Result<LoopHit> {
    let n = pts.len();
    let mut best: Option<(f64, LoopHit)> = None;
    for i in 0..n {
        let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % n.max(1))) else { continue };
        let d = *b - *a;
        let dd = d.dot(d);
        let t = if dd > 1e-18 { ((p - *a).dot(d) / dd).clamp(0.0, 1.0) } else { 0.0 };
        let q = *a + d * t;
        let dist = q.dist(p);
        if best.as_ref().is_none_or(|(bd, _)| dist < *bd) {
            best = Some((dist, LoopHit { seg: i, t, point: q }));
        }
    }
    match best {
        Some((dist, hit)) if dist <= tol => Ok(hit),
        _ => Err(KernelError::Invalid("the guide does not meet the boundary".into())),
    }
}

fn on_vertex(hit: &LoopHit, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    if hit.t <= 1e-8 {
        Some(hit.seg % n)
    } else if hit.t >= 1.0 - 1e-8 {
        Some((hit.seg + 1) % n)
    } else {
        None
    }
}

fn insert_hits(pts: &[Vec3], h0: LoopHit, h1: LoopHit) -> Result<(Vec<Vec3>, usize, usize)> {
    let n = pts.len();
    if n < 3 {
        return Err(KernelError::Invalid("a patch boundary does not close".into()));
    }
    let mut extra = Vec::new();
    if on_vertex(&h0, n).is_none() {
        extra.push((h0.seg, h0.t, h0.point));
    }
    if on_vertex(&h1, n).is_none() && h1.point.dist(h0.point) > 1e-8 {
        extra.push((h1.seg, h1.t, h1.point));
    }
    extra.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)));
    let mut out = pts.to_vec();
    for (seg, _, p) in extra {
        let at = seg.saturating_add(1);
        if at > out.len() {
            return Err(KernelError::Invalid("a patch boundary does not close".into()));
        }
        out.insert(at, p);
    }
    let i = nearest_index(&out, h0.point);
    let j = nearest_index(&out, h1.point);
    if i == j || out.get(i).is_none_or(|p| p.dist(h0.point) > 1e-4) || out.get(j).is_none_or(|p| p.dist(h1.point) > 1e-4) {
        return Err(KernelError::Invalid("the guide meets the boundary at one point".into()));
    }
    Ok((out, i, j))
}

fn hole_inside(plane: &Plane, outer: &[Vec3], hole: &[Vec3]) -> bool {
    let local: Vec<Vec2> = hole.iter().map(|p| plane.to_local(*p)).collect();
    if local.is_empty() {
        return false;
    }
    let c = local.iter().copied().fold(Vec2::ZERO, |s, p| s + p) * (1.0 / local.len() as f64);
    let outer_l: Vec<Vec2> = outer.iter().map(|p| plane.to_local(*p)).collect();
    point_in_poly(&outer_l, c)
}

fn point_in_poly(poly: &[Vec2], p: Vec2) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n.max(1))) else { continue };
        let crosses = (a.y > p.y) != (b.y > p.y);
        let x = if (b.y - a.y).abs() > 1e-18 { (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x } else { a.x };
        if crosses && p.x < x {
            inside = !inside;
        }
    }
    inside
}

fn patch_with_hole(plane: &Plane, outer: &[Vec3], hole: &[Vec3]) -> Result<Body> {
    let outer_l: Vec<Vec2> = outer.iter().map(|p| plane.to_local(*p)).collect();
    let hole_l: Vec<Vec2> = hole.iter().map(|p| plane.to_local(*p)).collect();
    let region = Region2 { outer: Loop2::polygon(&outer_l).ccw(), holes: vec![Loop2::polygon(&hole_l).ccw().reversed()] };
    patch_region(plane, &region)
}

fn patch_polygon(pts: &[Vec3]) -> Result<Body> {
    let plane = polygon_plane(pts).ok_or_else(|| KernelError::Invalid("the patch boundary is not planar".into()))?;
    if pts.iter().any(|p| plane.height(*p).abs() > 1e-3) {
        return Err(KernelError::Invalid("not supported yet: a patch boundary that is not planar".into()));
    }
    let local: Vec<solidcraft_geom::Vec2> = pts.iter().map(|p| plane.to_local(*p)).collect();
    let region = solidcraft_geom::Region2 { outer: solidcraft_geom::Loop2::polygon(&local), holes: Vec::new() };
    patch_region(&plane, &region)
}

fn polygon_plane(pts: &[Vec3]) -> Option<solidcraft_geom::Plane> {
    let mut n = Vec3::ZERO;
    let m = pts.len();
    for i in 0..m {
        let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % m)) else { continue };
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    let origin = pts.first().copied()?;
    solidcraft_geom::Plane::from_normal(origin, n)
}

fn nearest_index(pts: &[Vec3], p: Vec3) -> usize {
    pts.iter().enumerate().min_by(|(_, a), (_, b)| a.dist(p).total_cmp(&b.dist(p))).map(|(i, _)| i).unwrap_or(0)
}

fn chain_between(pts: &[Vec3], i: usize, j: usize) -> Vec<Vec3> {
    let mut out = Vec::new();
    let mut k = i;
    for _ in 0..=pts.len() {
        if let Some(p) = pts.get(k) {
            out.push(*p);
        }
        if k == j {
            break;
        }
        k = (k + 1) % pts.len().max(1);
    }
    out
}

fn dist_to_loop(pts: &[Vec3], p: Vec3) -> f64 {
    let n = pts.len();
    let mut best = f64::MAX;
    for i in 0..n {
        let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        best = best.min(dist_to_seg(*a, *b, p));
    }
    best
}

fn dist_to_seg(a: Vec3, b: Vec3, p: Vec3) -> f64 {
    let d = b - a;
    let dd = d.dot(d);
    let t = if dd > 1e-18 { ((p - a).dot(d) / dd).clamp(0.0, 1.0) } else { 0.0 };
    (a + d * t).dist(p)
}

/// Midpoints of edges that belong to only one face. Empty when the body is closed.
pub fn gap_points(body: &Body) -> Result<Vec<Vec3>> {
    Ok(crate::sew::gap_edges(body)?.into_iter().map(|edge| edge.mid).collect())
}

/// Reverse every face so the next thicken steps the other way.
pub fn flip_normals(body: &Body) -> Result<Body> {
    body.require_brep("reverse")?;
    guard("surface.reverse", || {
        let faces: Vec<mt::Face> = body.solid.face_iter().map(|face| face.inverse()).collect();
        if faces.is_empty() {
            return Err(KernelError::Invalid("nothing to reverse".into()));
        }
        let mut rec = Rebuild::default();
        for face in &faces {
            rec.made(face, Vec::new());
        }
        Ok(rebuilt_body(surface_body(faces), &[&body.solid], &rec))
    })
}

/// Faces that are not caps along `axis` (their plane is not square to it, or they are curved),
/// copied out as a surface body.
pub fn side_skin(body: &Body, axis: Vec3) -> Result<Body> {
    let axis = axis.normalized().ok_or_else(|| KernelError::Invalid("the surface direction is zero".into()))?;
    let tol = (body.size() * 2e-3).max(1e-3);
    let faces = body.faces(tol)?;
    let pts: Vec<Vec3> = faces.iter().filter(|f| f.plane_normal.map(|n| n.dot(axis).abs() < 0.85).unwrap_or(true)).map(|f| f.centroid).collect();
    if pts.is_empty() {
        return Err(KernelError::Invalid("that solid has no side faces to turn into a surface".into()));
    }
    copy_faces(body, &pts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, cylinder, measure};
    use solidcraft_geom::{Loop2, Vec2};

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }

    #[test]
    fn patch_and_thicken_a_square() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        assert!(s.is_surface());
        let t = thicken(&s, 2.0, false).unwrap();
        assert!(!t.is_surface());
        assert!(rel(measure(&t).unwrap().volume, 200.0) < 1e-9);
        let t = thicken(&s, 2.0, true).unwrap();
        let m = measure(&t).unwrap();
        assert!(rel(m.volume, 200.0) < 1e-9 && (m.bbox.min.z + 1.0).abs() < 1e-9, "{:?}", m.bbox);
    }

    #[test]
    fn thicken_a_cylinder_side_and_a_box_top() {
        let c = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        // One half of the side, offset outward by 1: half of a ring.
        let half = copy_faces(&c, &[Vec3::new(7.0, 7.0, 10.0)]).unwrap();
        let v = measure(&thicken(&half, 1.0, false).unwrap()).unwrap().volume;
        let want = std::f64::consts::PI * (121.0 - 100.0) / 2.0 * 20.0;
        assert!(rel(v, want) < 3e-4, "{v} vs {want}"); // (measured on meshes)
        // Both halves (meeting smoothly): the whole ring, inward.
        let d = 10.0 / 2f64.sqrt();
        let side = copy_faces(&c, &[Vec3::new(d, d, 10.0), Vec3::new(-d, -d, 10.0)]).unwrap();
        assert_eq!(side.face_count(), 2);
        let v = measure(&thicken(&side, -1.0, false).unwrap()).unwrap().volume;
        let want = std::f64::consts::PI * (100.0 - 81.0) * 20.0;
        assert!(rel(v, want) < 3e-4, "{v} vs {want}"); // (measured on meshes)
        // Faces meeting at a sharp edge are refused.
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let two = copy_faces(&b, &[Vec3::new(5.0, 5.0, 10.0), Vec3::new(5.0, 0.0, 5.0)]).unwrap();
        assert!(thicken(&two, 1.0, false).is_err());
    }

    #[test]
    fn stitch_six_faces_into_a_box_and_patch_a_hole() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let pts = [
            Vec3::new(5.0, 10.0, 0.0),
            Vec3::new(5.0, 10.0, 30.0),
            Vec3::new(0.0, 10.0, 15.0),
            Vec3::new(10.0, 10.0, 15.0),
            Vec3::new(5.0, 0.0, 15.0),
            Vec3::new(5.0, 20.0, 15.0),
        ];
        let parts: Vec<Body> = pts.iter().map(|p| copy_faces(&b, &[*p]).unwrap()).collect();
        let refs: Vec<&Body> = parts.iter().collect();
        let s = stitch(&refs, 0.0).unwrap();
        assert!(!s.is_surface());
        assert!(rel(measure(&s).unwrap().volume, 6000.0) < 1e-9);
        // Five faces leave an open box; patching the top's edges closes it again.
        let open = stitch(&[refs[0], refs[2], refs[3], refs[4], refs[5]], 0.0).unwrap();
        assert!(open.is_surface());
        assert!(gap_points(&s).unwrap().is_empty());
        assert!(!gap_points(&open).unwrap().is_empty(), "five faces leave a gap loop");
        let lid =
            patch_edges(&open, &[Vec3::new(5.0, 0.0, 30.0), Vec3::new(10.0, 10.0, 30.0), Vec3::new(5.0, 20.0, 30.0), Vec3::new(0.0, 10.0, 30.0)])
                .unwrap();
        let closed = stitch(&[&open, &lid], 0.0).unwrap();
        assert!(!closed.is_surface());
        assert!(rel(measure(&closed).unwrap().volume, 6000.0) < 1e-9);
    }

    #[test]
    fn extend_a_planar_surface() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        let e = extend(&s, &[Vec3::new(10.0, 5.0, 0.0)], 5.0).unwrap();
        assert!(rel(measure(&e).unwrap().area, 150.0) < 1e-9);
        let e = extend(&s, &[Vec3::new(10.0, 5.0, 0.0), Vec3::new(5.0, 10.0, 0.0)], 5.0).unwrap();
        assert!(rel(measure(&e).unwrap().area, 225.0) < 1e-9);
        assert!(extend(&s, &[Vec3::new(10.0, 5.0, 0.0)], -1.0).is_err());
    }

    #[test]
    fn offset_moves_a_plane_and_refuses_a_sharp_solid() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let s = copy_faces(&b, &[Vec3::new(5.0, 5.0, 10.0)]).unwrap();
        let before = measure(&s).unwrap();
        let moved = surface_offset(&s, 2.0).unwrap();
        let after = measure(&moved).unwrap();
        let dz = after.bbox.min.z - before.bbox.min.z;
        assert!((dz.abs() - 2.0).abs() < 1e-4, "before {:?} after {:?}", before.bbox, after.bbox);
        assert!((after.bbox.min.x - before.bbox.min.x).abs() < 1e-4 && (after.bbox.min.y - before.bbox.min.y).abs() < 1e-4);
        assert!((after.area - before.area).abs() < 1e-3, "{} {}", before.area, after.area);
        assert!(surface_offset(&b, 1.0).is_err());
        assert!(surface_offset(&s, f64::NAN).is_err());
    }

    #[test]
    fn an_extruded_line_is_a_sheet_with_no_volume() {
        let a = vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)];
        let b = vec![Vec3::new(0.0, 0.0, 10.0), Vec3::new(10.0, 0.0, 10.0)];
        let sheet = open_sweep(&[a, b], false, false).unwrap();
        assert!(sheet.is_surface());
        let m = measure(&sheet).unwrap();
        assert!(m.volume.abs() < 1e-6, "{}", m.volume);
        assert!((m.area - 100.0).abs() < 1e-3, "{}", m.area);
        assert!(open_sweep(&[], false, false).is_err());
        assert!(open_sweep(&[vec![Vec3::new(f64::NAN, 0.0, 0.0), Vec3::X], vec![Vec3::ZERO, Vec3::X]], false, false).is_err());
    }

    #[test]
    fn trim_keeps_one_side() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        let pl = Plane::new(Vec3::new(3.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let t = trim(&s, &crate::SplitTool::Plane(pl), Vec3::new(1.0, 5.0, 0.0)).unwrap();
        assert!(rel(measure(&t).unwrap().area, 30.0) < 1e-9);
        let t = trim(&s, &crate::SplitTool::Plane(pl), Vec3::new(8.0, 5.0, 0.0)).unwrap();
        assert!(rel(measure(&t).unwrap().area, 70.0) < 1e-9);
        let tool = box_solid(Vec3::new(3.0, -1.0, -2.0), Vec3::new(1.0, 12.0, 4.0)).unwrap();
        let face = tool
            .solid
            .face_iter()
            .position(|f| {
                let pts: Vec<Vec3> = f.edge_iter().map(|e| from_p3(e.front().point())).collect();
                pts.len() >= 3 && pts.iter().all(|p| (p.x - 3.0).abs() < 1e-6)
            })
            .unwrap();
        let cut = trim(&s, &crate::SplitTool::Face { body: &tool, face }, Vec3::new(1.0, 5.0, 0.0)).unwrap();
        let kept = measure(&cut).unwrap().area;
        let other = trim(&s, &crate::SplitTool::Face { body: &tool, face }, Vec3::new(8.0, 5.0, 0.0)).unwrap();
        let rest = measure(&other).unwrap().area;
        assert!(rel(kept + rest, 100.0) < 1e-6, "{kept} + {rest}");
        assert!((kept - rest).abs() > 10.0, "the face tool keeps one side");
    }

    /// Feature: fusion360-feature-parity, Property 3: Surface-create yields a zero-thickness surface
    #[test]
    fn open_sweep_sheets_have_no_volume() {
        for i in 0..100 {
            let len = 1.0 + (i % 17) as f64;
            let h = 0.5 + (i % 13) as f64 * 0.3;
            let a = vec![Vec3::ZERO, Vec3::new(len, 0.0, 0.0)];
            let b = vec![Vec3::new(0.0, 0.0, h), Vec3::new(len, 0.0, h)];
            let sheet = open_sweep(&[a, b], false, false).unwrap();
            assert!(sheet.is_surface());
            let m = measure(&sheet).unwrap();
            assert!(m.volume.abs() < 1e-4, "{i} {}", m.volume);
            assert!((m.area - len * h).abs() < 1e-3 * len * h, "{i} {} vs {}", m.area, len * h);
        }
        let square = vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 8.0, 0.0), Vec3::new(0.0, 8.0, 0.0)];
        let raised: Vec<Vec3> = square.iter().map(|p| *p + Vec3::new(0.0, 0.0, 6.0)).collect();
        let mut repeated = square.clone();
        repeated.push(Vec3::ZERO);
        let tube = open_sweep(&[repeated, raised.clone()], true, false).unwrap();
        assert!(tube.is_surface(), "a closed profile extrude has no caps");
        assert_eq!(tube.face_count(), 4);
        let m = measure(&tube).unwrap();
        assert!(m.volume.abs() < 1e-4, "open tube volume {}", m.volume);
        assert!((m.area - 36.0 * 6.0).abs() < 1e-2, "area {} faces {}", m.area, tube.face_count());
        assert!(open_sweep(&[square, raised], true, false).is_ok());
    }

    /// Feature: fusion360-feature-parity, Property 4: Planar patch area equals the enclosed area
    #[test]
    fn planar_patch_area_matches_the_polygon() {
        for n in 3..8 {
            for k in 0..20 {
                let r = 2.0 + (k % 7) as f64;
                let phase = k as f64 * 0.17;
                let pts: Vec<Vec3> = (0..n)
                    .map(|i| {
                        let a = phase + i as f64 * std::f64::consts::TAU / n as f64;
                        Vec3::new(r * a.cos(), r * a.sin(), 0.0)
                    })
                    .collect();
                let want = 0.5 * n as f64 * r * r * (std::f64::consts::TAU / n as f64).sin();
                let sheet = patch_guided(&pts, None).unwrap();
                assert!(sheet.is_surface());
                let area = measure(&sheet).unwrap().area;
                assert!(rel(area, want) < 1e-6, "n={n} k={k} {area} vs {want}");
                let chord = vec![pts[0], pts[n / 2]];
                let guided = patch_guided(&pts, Some(&chord)).unwrap();
                assert!(rel(measure(&guided).unwrap().area, want) < 1e-6, "guide n={n}");
            }
        }
        let outer = vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(0.0, 10.0, 0.0)];
        let hole = vec![Vec3::new(2.0, 2.0, 0.0), Vec3::new(4.0, 2.0, 0.0), Vec3::new(4.0, 4.0, 0.0), Vec3::new(2.0, 4.0, 0.0)];
        let with_hole = patch_mixed(&outer, &hole, None).unwrap();
        assert!(rel(measure(&with_hole).unwrap().area, 96.0) < 1e-6);
        let on_loop = patch_mixed(&outer, &[Vec3::new(5.0, 0.0, 0.0), Vec3::new(10.0, 5.0, 0.0)], None).unwrap();
        assert!(rel(measure(&on_loop).unwrap().area, 100.0) < 1e-6);
        let err = patch_mixed(&outer, &[Vec3::new(30.0, 30.0, 0.0), Vec3::new(32.0, 30.0, 0.0), Vec3::new(31.0, 32.0, 0.0)], None).unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
    }

    #[test]
    fn a_guide_that_meets_an_edge_lifts_the_patch() {
        let outer = vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(0.0, 10.0, 0.0)];
        let guide = vec![Vec3::new(0.0, 5.0, 0.0), Vec3::new(5.0, 5.0, 2.0), Vec3::new(10.0, 5.0, 0.0)];
        let sheet = patch_guided(&outer, Some(&guide)).unwrap();
        assert!(sheet.is_surface());
        let m = measure(&sheet).unwrap();
        assert!(m.bbox.max.z > 1.5, "{:?}", m.bbox);
        assert!(m.area > 100.0, "area {} faces {} bbox {:?}", m.area, sheet.face_count(), m.bbox);
        let gaps = gap_points(&sheet).unwrap();
        assert!(gaps.iter().all(|p| p.dist(Vec3::new(5.0, 5.0, 2.0)) > 0.4), "the guide is not a gap: {gaps:?}");
        assert!(patch_guided(&outer, Some(&[Vec3::new(5.0, 5.0, 4.0), Vec3::new(6.0, 5.0, 4.0)])).is_err());
    }

    /// Feature: fusion360-feature-parity, Property 5: Trim then the other side restores the area
    #[test]
    fn trim_sides_sum_to_the_sheet() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        for i in 0..100 {
            let x = 0.5 + (i % 19) as f64 * 0.45;
            let pl = Plane::new(Vec3::new(x, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
            let a = measure(&trim(&s, &crate::SplitTool::Plane(pl), Vec3::new(0.0, 5.0, 0.0)).unwrap()).unwrap().area;
            let b = measure(&trim(&s, &crate::SplitTool::Plane(pl), Vec3::new(10.0, 5.0, 0.0)).unwrap()).unwrap().area;
            assert!(rel(a + b, 100.0) < 1e-6, "{i} {a}+{b}");
        }
    }

    /// Feature: fusion360-feature-parity, Property 6: Stitch closes to a solid exactly when faces bound a volume
    #[test]
    fn stitch_closes_only_a_full_box() {
        for k in 0..20 {
            let s = 1.0 + k as f64;
            let b = box_solid(Vec3::ZERO, Vec3::new(s, 2.0 * s, 3.0 * s)).unwrap();
            let pts = [
                Vec3::new(s * 0.5, s, 0.0),
                Vec3::new(s * 0.5, s, 3.0 * s),
                Vec3::new(0.0, s, 1.5 * s),
                Vec3::new(s, s, 1.5 * s),
                Vec3::new(s * 0.5, 0.0, 1.5 * s),
                Vec3::new(s * 0.5, 2.0 * s, 1.5 * s),
            ];
            let parts: Vec<Body> = pts.iter().map(|p| copy_faces(&b, &[*p]).unwrap()).collect();
            for n in 1..=6 {
                let sewn = crate::sew::stitch_tolerant(&parts[..n], 0.0).unwrap();
                assert_eq!(sewn.closed, n == 6, "k={k} n={n}");
                assert_eq!(sewn.gaps.is_empty(), n == 6);
                assert_eq!(!sewn.body.is_surface(), n == 6);
                if n == 6 {
                    assert!(rel(measure(&sewn.body).unwrap().volume, s * 2.0 * s * 3.0 * s) < 1e-6);
                } else {
                    assert!(!gap_points(&sewn.body).unwrap().is_empty());
                }
            }
        }
    }

    /// Feature: fusion360-feature-parity, Property 7: Thicken volume equals area times thickness
    #[test]
    fn thicken_volume_is_area_times_thickness() {
        let sheets = {
            let plane = patch_region(
                &Plane::XY,
                &Region2 { outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 4.0), Vec2::new(0.0, 4.0)]), holes: vec![] },
            )
            .unwrap();
            let cyl = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
            let side = copy_faces(&cyl, &[Vec3::new(7.0, 7.0, 10.0)]).unwrap();
            crate::offset::thicken_bodies(&[&plane, &side], 1.0, false).unwrap()
        };
        assert_eq!(sheets.len(), 2);
        assert!(sheets.iter().all(|b| !b.is_surface()));
        let plane_v = measure(sheets.first().unwrap()).unwrap().volume.abs();
        assert!(rel(plane_v, 40.0) < 1e-6, "{plane_v}");
        for i in 0..100 {
            let w = 1.0 + (i % 11) as f64;
            let h = 1.0 + (i % 7) as f64;
            let t = 0.2 + (i % 5) as f64 * 0.3;
            let sheet = patch_region(
                &Plane::XY,
                &Region2 { outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)]), holes: vec![] },
            )
            .unwrap();
            let area = measure(&sheet).unwrap().area;
            let solid = thicken(&sheet, t, false).unwrap();
            assert!(!solid.is_surface());
            let volume = measure(&solid).unwrap().volume.abs();
            assert!(rel(volume, area * t) < 1e-6, "{i} {volume} vs {}", area * t);
        }
        let sharp = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0)).unwrap();
        let err = thicken(&sharp, 1.0, false).unwrap_err().to_string();
        assert!(err.contains("not supported yet"), "{err}");
        assert!(crate::offset::thicken_bodies(&[], 1.0, false).is_err());
    }
}
