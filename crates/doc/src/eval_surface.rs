//! Evaluation of the surface features: Patch, Stitch, Thicken and Trim (kernel `surfaces`).

use super::*;

pub(super) fn eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    match &f.kind {
        FeatureKind::Patch { sketch, profiles, body, edges, guide } => {
            let mut made = Vec::new();
            if !guide.is_empty() && (guide.len() < 2 || guide.len() > 10_000 || guide.iter().any(|p| !p.is_finite())) {
                return Err(DocError::Invalid("a guide curve needs 2…10000 finite points".into()));
            }
            if let Some(sk) = sketch {
                let ss = st.sketch(*sk).ok_or_else(|| DocError::Unknown(format!("sketch {sk} (it must come earlier in the timeline)")))?;
                let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, profiles)?);
                if !guide.is_empty() {
                    let Some(region) = regions.first() else { return Err(DocError::Invalid("the patch boundary does not close".into())) };
                    let boundary = region_boundary(ss, region);
                    if !edges.is_empty() && edges.iter().any(|p| !near_loop(&boundary, *p, 1.0)) {
                        return Err(DocError::Invalid("the boundary does not close".into()));
                    }
                    made.push(kernel::patch_guided(&boundary, Some(guide.as_slice()))?);
                } else if !edges.is_empty() {
                    // Mixed sketch outer + edge picks: one face (confirm-on-loop, or a hole).
                    if regions.len() != 1 {
                        return Err(DocError::Invalid("not supported yet: mixed edge and sketch patch of several regions".into()));
                    }
                    let Some(region) = regions.first() else { return Err(DocError::Invalid("the patch boundary does not close".into())) };
                    made.push(kernel::patch_mixed(&region_boundary(ss, region), edges, None)?);
                } else {
                    for r in &regions {
                        made.push(kernel::patch_region(&ss.plane, r)?);
                    }
                }
            } else if !edges.is_empty() {
                let i = body_at(st, body, edges)?;
                let b = st.bodies.get(i).ok_or_else(|| DocError::Invalid("body".into()))?;
                made.push(kernel::patch_edges(&b.body, edges)?);
            }
            if made.is_empty() {
                return Err(DocError::Invalid("a patch needs a sketch region or a loop of edges".into()));
            }
            for (k, b) in made.into_iter().enumerate() {
                let name = new_name(st, f, k);
                st.bodies.push(ModelBody::new(name, b, f.id));
            }
            Ok(())
        }
        FeatureKind::Stitch { bodies, tolerance } => {
            let tol = val(vals, tolerance, Kind::Length)?;
            let idx: Vec<usize> = bodies
                .iter()
                .map(|n| st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`"))))
                .collect::<Result<_>>()?;
            let Some(&first) = idx.first() else { return Err(DocError::Invalid("pick the surfaces to stitch".into())) };
            let parts: Vec<kernel::Body> = idx.iter().filter_map(|i| st.bodies.get(*i).map(|b| b.body.clone())).collect();
            if parts.len() != idx.len() {
                return Err(DocError::Invalid("body".into()));
            }
            let sewn = kernel::stitch_tolerant(&parts, tol)?;
            let Some(mb) = st.bodies.get(first).cloned() else { return Err(DocError::Invalid("body".into())) };
            if let Some(slot) = st.bodies.get_mut(first) {
                *slot = ModelBody::new(mb.name, sewn.body, mb.feature);
            }
            // The others are now part of the first.
            let gone: Vec<String> = idx.iter().skip(1).filter_map(|i| st.bodies.get(*i).map(|b| b.name.clone())).collect();
            st.bodies.retain(|b| !gone.contains(&b.name));
            Ok(())
        }
        FeatureKind::Thicken { bodies, thickness, symmetric, operation, targets } => {
            let t = val(vals, thickness, Kind::Length)?;
            let refs: Vec<&kernel::Body> =
                bodies.iter().map(|n| st.body(n).map(|b| &b.body).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))).collect::<Result<_>>()?;
            let tools = kernel::thicken_bodies(&refs, t, *symmetric)?;
            if tools.is_empty() {
                return Err(DocError::Invalid("pick the surfaces to thicken".into()));
            }
            apply_op(st, f, tools, *operation, targets)
        }
        FeatureKind::SurfaceTrim { body, plane, tool, keep, curve_sketch, curve } => {
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let curve_plane = match (curve_sketch, curve) {
                (Some(sk), Some(id)) => {
                    let ss = st.sketch(*sk).ok_or_else(|| DocError::Unknown(format!("sketch {sk}")))?;
                    let curve = ss.sketch.curves.iter().find(|c| &c.id == id).ok_or_else(|| DocError::Unknown(format!("curve `{id}`")))?;
                    let solidcraft_sketch::CurveKind::Line { a, b } = curve.kind else {
                        return Err(DocError::Invalid("not supported yet: trimming with a curve that is not a line".into()));
                    };
                    let (Some(p), Some(q)) = (ss.sketch.point(a), ss.sketch.point(b)) else {
                        return Err(DocError::Invalid("the trim curve has no ends".into()));
                    };
                    let origin = ss.plane.to_world(p);
                    let dir = ss.plane.dir_to_world(q - p);
                    let _normal =
                        dir.cross(ss.plane.normal()).normalized().ok_or_else(|| DocError::Invalid("the trim curve has no direction".into()))?;
                    let x = dir.normalized().ok_or_else(|| DocError::Invalid("the trim curve has no direction".into()))?;
                    Some(
                        solidcraft_geom::Plane::new(origin, x, ss.plane.normal())
                            .ok_or_else(|| DocError::Invalid("the trim curve does not define a plane".into()))?,
                    )
                }
                _ => None,
            };
            let trimmed = if let Some(plane) = curve_plane {
                kernel::trim(&mb.body, &kernel::SplitTool::Plane(plane), *keep)?
            } else {
                match tool {
                    Some(t) => {
                        let tb = st.body(&t.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", t.body)))?;
                        let face = crate::appearance::face_index_at(tb, t.point)
                            .ok_or_else(|| DocError::Invalid(format!("no face of `{}` at the tool point", t.body)))?;
                        kernel::trim(&mb.body, &kernel::SplitTool::Face { body: &tb.body, face }, *keep)?
                    }
                    None => kernel::trim(&mb.body, &kernel::SplitTool::Plane(doc.resolve_plane_in(vals, plane, 0, Some(st))?), *keep)?,
                }
            };
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::new(mb.name, trimmed, mb.feature);
            }
            Ok(())
        }
        FeatureKind::SurfaceExtend { body, edges, distance } => {
            let d = val(vals, distance, Kind::Length)?;
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let out = kernel::extend(&mb.body, edges, d)?;
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::new(mb.name, out, mb.feature);
            }
            Ok(())
        }
        _ => Err(DocError::Invalid("not a surface feature".into())),
    }
}

fn region_boundary(ss: &SolvedSketch, region: &Region2) -> Vec<Vec3> {
    let mut boundary: Vec<Vec3> = region.outer.polyline(0.05).into_iter().map(|q| ss.plane.to_world(q)).collect();
    if let Some(first) = boundary.first().copied()
        && boundary.last().is_none_or(|last| last.dist(first) > 1e-4)
    {
        boundary.push(first);
    }
    boundary
}

fn near_loop(loop_pts: &[Vec3], p: Vec3, tol: f64) -> bool {
    let n = loop_pts.len();
    if n == 0 {
        return false;
    }
    for i in 0..n {
        let (Some(a), Some(b)) = (loop_pts.get(i), loop_pts.get((i + 1) % n)) else { continue };
        let d = *b - *a;
        let dd = d.dot(d);
        let t = if dd > 1e-18 { ((p - *a).dot(d) / dd).clamp(0.0, 1.0) } else { 0.0 };
        if (*a + d * t).dist(p) <= tol {
            return true;
        }
    }
    false
}
