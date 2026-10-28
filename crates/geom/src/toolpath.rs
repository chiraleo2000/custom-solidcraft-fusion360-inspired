//! Tool-radius offsets, depth steps and scallop spacing for machining.
//!
//! The offset is a mitered shift of each edge of a closed polyline. The scallop
//! formula is the circular cusp left by a ball-nosed tool on a flat. Nothing here
//! comes from another project's sources.

use crate::{Mesh, Vec2, Vec3};

const MAX_LOOP: usize = 512;

/// Right-handed work coordinates. Toolpaths are written in this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkCoords {
    pub origin: Vec3,
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
}

impl WorkCoords {
    /// Model axes, origin at the model origin.
    pub fn model() -> Self {
        Self { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y, z: Vec3::Z }
    }

    /// Build a frame from an origin, an X axis and a Z (spindle) axis.
    /// Y is `Z × X`. Parallel or zero axes are refused.
    pub fn from_xz(origin: Vec3, x: Vec3, z: Vec3) -> Result<Self, &'static str> {
        if !origin.is_finite() {
            return Err("the origin is not finite");
        }
        let Some(x) = x.normalized() else { return Err("the X axis is zero") };
        let Some(z) = z.normalized() else { return Err("the Z axis is zero") };
        let y_raw = z.cross(x);
        if y_raw.len() < 1e-6 {
            return Err("the X and Z axes are parallel");
        }
        let y = y_raw / y_raw.len();
        let Some(z) = x.cross(y).normalized() else { return Err("the axes are not usable") };
        Ok(Self { origin, x, y, z })
    }

    /// Model point → coordinates in this frame (millimetres along X, Y, Z).
    pub fn to_local(self, world: Vec3) -> Vec3 {
        let d = world - self.origin;
        Vec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }
}

/// Absolute area of a closed polyline (shoelace).
pub fn polygon_area(points: &[Vec2]) -> f64 {
    signed_area(points).abs()
}

fn signed_area(points: &[Vec2]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let Some(p) = points.get(i) else { continue };
        let Some(q) = points.get((i + 1) % n) else { continue };
        sum += p.cross(*q);
    }
    sum * 0.5
}

/// Drop duplicates and collinear points. `None` when the loop is not usable.
fn polish(points: &[Vec2]) -> Result<Vec<Vec2>, &'static str> {
    if points.len() > 20_000 {
        return Err("the profile has too many points");
    }
    let mut raw = Vec::new();
    for p in points {
        if !p.is_finite() {
            return Err("the profile has a point that is not finite");
        }
        if raw.last().is_none_or(|q: &Vec2| q.dist(*p) > 1e-6) {
            raw.push(*p);
        }
    }
    if let (Some(first), Some(last)) = (raw.first().copied(), raw.last().copied())
        && first.dist(last) <= 1e-6
    {
        raw.pop();
    }
    let n = raw.len();
    if n < 3 {
        return Err("the profile needs at least 3 points");
    }
    let mut out = Vec::with_capacity(n.min(MAX_LOOP));
    for i in 0..n {
        let Some(prev) = raw.get((i + n - 1) % n).copied() else { continue };
        let Some(cur) = raw.get(i).copied() else { continue };
        let Some(next) = raw.get((i + 1) % n).copied() else { continue };
        let ab = cur - prev;
        let bc = next - cur;
        let scale = ab.len() * bc.len();
        if scale > 1e-12 && ab.cross(bc).abs() <= scale * 1e-8 {
            continue;
        }
        out.push(cur);
    }
    if out.len() < 3 {
        return Err("the profile needs at least 3 points");
    }
    if out.len() > MAX_LOOP {
        let stride = out.len().div_ceil(MAX_LOOP).max(1);
        out = out.into_iter().step_by(stride).collect();
    }
    if out.len() < 3 {
        return Err("the profile needs at least 3 points");
    }
    Ok(out)
}

fn intersect_lines(p: Vec2, d: Vec2, q: Vec2, e: Vec2) -> Option<Vec2> {
    let den = d.cross(e);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (q - p).cross(e) / den;
    let hit = p + d * t;
    hit.is_finite().then_some(hit)
}

/// Distance from `q` to the infinite line through an edge.
fn dist_to_line(q: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let len = ab.len();
    if len < 1e-15 { q.dist(a) } else { ab.cross(q - a).abs() / len }
}

/// Distance from `q` to the nearest supporting line of a closed polyline.
fn dist_to_loop(points: &[Vec2], q: Vec2) -> f64 {
    let n = points.len();
    let mut best = f64::MAX;
    for i in 0..n {
        let Some(a) = points.get(i).copied() else { continue };
        let Some(b) = points.get((i + 1) % n).copied() else { continue };
        best = best.min(dist_to_line(q, a, b));
    }
    best
}

fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let ab = b - a;
    let cd = d - c;
    let den = ab.cross(cd);
    if den.abs() < 1e-14 {
        return false;
    }
    let t = (c - a).cross(cd) / den;
    let u = (c - a).cross(ab) / den;
    (1e-6..1.0 - 1e-6).contains(&t) && (1e-6..1.0 - 1e-6).contains(&u)
}

fn crosses_itself(points: &[Vec2]) -> bool {
    let n = points.len();
    if n < 4 {
        return false;
    }
    if n > 600 {
        return true;
    }
    for i in 0..n {
        let Some(a) = points.get(i).copied() else { continue };
        let Some(b) = points.get((i + 1) % n).copied() else { continue };
        for j in (i + 2)..n {
            if i == 0 && j + 1 == n {
                continue;
            }
            let Some(c) = points.get(j).copied() else { continue };
            let Some(d) = points.get((j + 1) % n).copied() else { continue };
            if segments_cross(a, b, c, d) {
                return true;
            }
        }
    }
    false
}

/// Offset a closed loop by `distance` millimetres.
///
/// Positive grows the loop (tool centre outside a part). Negative shrinks it
/// (tool centre inside a pocket). The winding is kept.
pub fn offset_closed(points: &[Vec2], distance: f64) -> Result<Vec<Vec2>, &'static str> {
    if !distance.is_finite() || distance.abs() > 1e5 {
        return Err("the offset distance is not usable");
    }
    let pts = polish(points)?;
    let n = pts.len();
    let area0 = signed_area(&pts);
    if area0.abs() < 1e-8 {
        return Err("the profile has no area");
    }
    // Left of a CCW edge points inward, so a growing offset steps to the right.
    let left = if area0 > 0.0 { -distance } else { distance };
    let mut raw = Vec::with_capacity(n * 2);
    for i in 0..n {
        let Some(prev) = pts.get((i + n - 1) % n).copied() else { continue };
        let Some(cur) = pts.get(i).copied() else { continue };
        let Some(next) = pts.get((i + 1) % n).copied() else { continue };
        let Some(d0) = (cur - prev).normalized() else { continue };
        let Some(d1) = (next - cur).normalized() else { continue };
        let n0 = d0.perp();
        let n1 = d1.perp();
        let p0 = prev + n0 * left;
        let p1 = cur + n1 * left;
        match intersect_lines(p0, d0, p1, d1) {
            Some(hit) => {
                let limit = left.abs() * 4.0 + 1e-6;
                if hit.dist(cur) > limit {
                    raw.push(cur + n0 * left);
                    raw.push(cur + n1 * left);
                } else {
                    raw.push(hit);
                }
            }
            None => raw.push(cur + n0 * left),
        }
    }
    let out = polish(&raw).map_err(|_| "the tool does not fit inside the profile")?;
    let area1 = signed_area(&out);
    if area0 * area1 <= 0.0 {
        return Err("the tool does not fit inside the profile");
    }
    if distance < -1e-9 && area1.abs() > area0.abs() + 1e-4 {
        return Err("the tool does not fit inside the profile");
    }
    if distance > 1e-9 && area1.abs() + 1e-4 < area0.abs() {
        return Err("the offset did not grow");
    }
    // A miter vertex lies on the offset lines, so it stays `|distance|` from the
    // original boundary. An inset that has crossed the middle is closer than that.
    if distance.abs() > 1e-9 {
        let want = distance.abs();
        let tol = (want * 1e-3).max(1e-4);
        for q in &out {
            if (dist_to_loop(&pts, *q) - want).abs() > tol {
                return Err("the tool does not fit inside the profile");
            }
        }
    }
    if crosses_itself(&out) {
        return Err("not supported yet: a profile the offset crosses");
    }
    Ok(out)
}

/// Even-odd containment against a closed polyline.
pub fn point_in_closed(poly: &[Vec2], q: Vec2) -> bool {
    if !q.is_finite() || poly.len() < 3 || poly.len() > 8_000 {
        return false;
    }
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        if (a.y > q.y) != (b.y > q.y) {
            let x = a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if q.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Grow each island (hole) by the tool radius into the pocket material. The hole
/// loop is normalised to CCW so a positive offset expands into the pocket.
pub fn island_keepouts(holes: &[Vec<Vec2>], tool_radius: f64) -> Result<Vec<Vec<Vec2>>, &'static str> {
    if !(tool_radius.is_finite() && tool_radius > 0.0 && tool_radius < 1e5) {
        return Err("the tool radius must be positive");
    }
    if holes.len() > 64 {
        return Err("too many islands");
    }
    let mut out = Vec::with_capacity(holes.len());
    for hole in holes {
        let pts = polish(hole)?;
        let area = signed_area(&pts);
        if area.abs() < 1e-8 {
            return Err("an island has no area");
        }
        // Holes are stored clockwise; flip to CCW so a positive offset grows into the pocket.
        let ccw: Vec<Vec2> = if area > 0.0 { pts } else { pts.into_iter().rev().collect() };
        out.push(offset_closed(&ccw, tool_radius)?);
    }
    Ok(out)
}

/// Drop pocket passes that enter any island keep-out. Empty after filtering is an error
/// when keep-outs were supplied (the tool does not fit around the islands).
pub fn filter_pocket_passes(passes: Vec<Vec<Vec2>>, keepouts: &[Vec<Vec2>]) -> Result<Vec<Vec<Vec2>>, &'static str> {
    if keepouts.is_empty() {
        return Ok(passes);
    }
    let mut kept = Vec::with_capacity(passes.len());
    for pass in passes {
        let clear = pass.iter().all(|q| keepouts.iter().all(|k| !point_in_closed(k, *q)));
        if clear {
            kept.push(pass);
        }
    }
    if kept.is_empty() {
        return Err("the tool does not fit around the islands");
    }
    Ok(kept)
}

/// Contour tool-centre loops: outer grown by the tool radius, plus one shrunk loop
/// per island (tool in the void against the island wall).
pub fn contour_passes(outer: &[Vec2], holes: &[Vec<Vec2>], tool_radius: f64) -> Result<Vec<Vec<Vec2>>, &'static str> {
    if !(tool_radius.is_finite() && tool_radius > 0.0 && tool_radius < 1e5) {
        return Err("the tool radius must be positive");
    }
    if holes.len() > 64 {
        return Err("too many islands");
    }
    let mut passes = vec![offset_closed(outer, tool_radius)?];
    for hole in holes {
        let pts = polish(hole)?;
        let area = signed_area(&pts);
        if area.abs() < 1e-8 {
            return Err("an island has no area");
        }
        let ccw: Vec<Vec2> = if area > 0.0 { pts } else { pts.into_iter().rev().collect() };
        passes.push(offset_closed(&ccw, -tool_radius)?);
    }
    Ok(passes)
}

/// Tool-centre loops for a pocket: the first is inset by the tool radius, then
/// by `stepover` each time, until the remaining island will not take the tool.
pub fn inset_passes(profile: &[Vec2], tool_radius: f64, stepover: f64) -> Result<Vec<Vec<Vec2>>, &'static str> {
    if !(tool_radius.is_finite() && stepover.is_finite()) || tool_radius <= 0.0 || stepover <= 0.0 || tool_radius > 1e5 || stepover > 1e5 {
        return Err("the tool radius and stepover must be positive");
    }
    let mut passes = Vec::new();
    let mut inset = tool_radius;
    for _ in 0..400 {
        match offset_closed(profile, -inset) {
            Ok(loop_) => passes.push(loop_),
            Err(_) if passes.is_empty() => return Err("the tool does not fit inside the profile"),
            Err(_) => return Ok(passes),
        }
        inset += stepover;
    }
    Err("the stepover is too fine")
}

/// Like [`inset_passes`], then drop loops that enter island keep-outs.
pub fn inset_passes_around(profile: &[Vec2], holes: &[Vec<Vec2>], tool_radius: f64, stepover: f64) -> Result<Vec<Vec<Vec2>>, &'static str> {
    let raw = inset_passes(profile, tool_radius, stepover)?;
    if holes.is_empty() {
        return Ok(raw);
    }
    let keepouts = island_keepouts(holes, tool_radius)?;
    filter_pocket_passes(raw, &keepouts)
}

/// Positive cut depths, stepping by `stepdown` and ending on `depth`.
pub fn depth_levels(depth: f64, stepdown: f64) -> Result<Vec<f64>, &'static str> {
    if !(depth.is_finite() && stepdown.is_finite()) || depth <= 0.0 || stepdown <= 0.0 || depth > 1e6 || stepdown > 1e6 {
        return Err("depth and stepdown must be positive");
    }
    let n = (depth / stepdown).ceil();
    if !n.is_finite() || n > 5_000.0 {
        return Err("too many depth steps");
    }
    let n = n as usize;
    let mut out = Vec::with_capacity(n.max(1));
    for i in 1..=n {
        let z = (stepdown * i as f64).min(depth);
        if out.last().is_some_and(|prev: &f64| (z - prev).abs() < 1e-9) {
            continue;
        }
        out.push(z);
    }
    if let Some(last) = out.last_mut() {
        *last = depth;
    } else {
        out.push(depth);
    }
    Ok(out)
}

/// Distance between ball-nose passes that leaves a cusp of height `cusp` on a flat.
pub fn scallop_stepover(tool_radius: f64, cusp: f64) -> Result<f64, &'static str> {
    if !(tool_radius.is_finite() && cusp.is_finite()) || tool_radius <= 0.0 || cusp <= 0.0 {
        return Err("the tool radius and cusp must be positive");
    }
    if cusp >= tool_radius {
        return Err("the cusp is deeper than the tool radius");
    }
    let inside = 2.0 * tool_radius * cusp - cusp * cusp;
    if inside <= 0.0 {
        return Err("the cusp does not fit the tool");
    }
    let step = 2.0 * inside.sqrt();
    if !step.is_finite() || step <= 0.0 {
        return Err("the stepover is not usable");
    }
    Ok(step)
}

/// Cusp left by a ball of `tool_radius` whose passes are `stepover` apart, on a flat.
pub fn cusp_height(tool_radius: f64, stepover: f64) -> Result<f64, &'static str> {
    if !(tool_radius.is_finite() && stepover.is_finite()) || tool_radius <= 0.0 || stepover <= 0.0 {
        return Err("the tool radius and stepover must be positive");
    }
    if stepover >= tool_radius * 2.0 {
        return Err("the stepover is wider than the tool");
    }
    let under = tool_radius * tool_radius - (stepover * 0.5) * (stepover * 0.5);
    if under <= 0.0 {
        return Err("the stepover is wider than the tool");
    }
    let h = tool_radius - under.sqrt();
    if !h.is_finite() || h < 0.0 {
        return Err("the cusp is not usable");
    }
    Ok(h)
}

/// Stations from `min` to `max` spaced by `step`. The last gap may be shorter so the
/// far side is still visited. More than `cap` stations is refused.
pub fn stepover_stations(min: f64, max: f64, step: f64, cap: usize) -> Result<Vec<f64>, &'static str> {
    if !(min.is_finite() && max.is_finite() && step.is_finite()) || step <= 0.0 || step > 1e6 || cap == 0 || cap > 10_000 {
        return Err("the stepover is not usable");
    }
    if max + 1e-9 < min {
        return Err("the stepover range is not usable");
    }
    let mut out = Vec::new();
    out.push(min);
    if max - min <= step * 1e-9 {
        return Ok(out);
    }
    let mut cursor = min;
    loop {
        if out.len() >= cap {
            return Err("the stepover is too fine");
        }
        let next = cursor + step;
        if next >= max - step * 1e-9 {
            if max - cursor > step * 1e-6 {
                out.push(max);
            }
            break;
        }
        out.push(next);
        cursor = next;
    }
    Ok(out)
}

fn upward_on_triangle(tri: [Vec3; 3], x: f64, y: f64) -> Option<(Vec3, Vec3)> {
    let (a, b, c) = (tri[0], tri[1], tri[2]);
    if x < a.x.min(b.x).min(c.x) - 1e-8 || x > a.x.max(b.x).max(c.x) + 1e-8 || y < a.y.min(b.y).min(c.y) - 1e-8 || y > a.y.max(b.y).max(c.y) + 1e-8 {
        return None;
    }
    let ab = b - a;
    let ac = c - a;
    let den = ab.x * ac.y - ac.x * ab.y;
    if den.abs() < 1e-12 {
        return None;
    }
    let px = x - a.x;
    let py = y - a.y;
    let u = (px * ac.y - ac.x * py) / den;
    let v = (ab.x * py - px * ab.y) / den;
    if u < -1e-6 || v < -1e-6 || u + v > 1.0 + 1e-6 {
        return None;
    }
    let z = a.z + ab.z * u + ac.z * v;
    let n = ab.cross(ac).normalized()?;
    if n.z <= 1e-3 || !z.is_finite() {
        return None;
    }
    Some((Vec3::new(x, y, z), n))
}

/// Highest upward-facing triangle under a vertical ray. The point lies on the
/// triangle; the normal points to the side the triangle's winding faces.
pub fn upward_hit(mesh: &Mesh, x: f64, y: f64) -> Option<(Vec3, Vec3)> {
    if !(x.is_finite() && y.is_finite()) || mesh.triangles.len() > 80_000 {
        return None;
    }
    let mut best: Option<(f64, Vec3, Vec3)> = None;
    for tri in &mesh.triangles {
        let Some(corners) = mesh.tri(tri) else { continue };
        let Some((point, normal)) = upward_on_triangle(corners, x, y) else { continue };
        if best.as_ref().is_none_or(|(z, _, _)| point.z > *z) {
            best = Some((point.z, point, normal));
        }
    }
    best.map(|(_, point, normal)| (point, normal))
}

/// Centre of a ball of `radius` touching `point` with the given outward normal.
pub fn ball_center(point: Vec3, normal: Vec3, radius: f64) -> Option<Vec3> {
    if !(radius.is_finite() && radius > 0.0 && radius < 1e5 && point.is_finite()) {
        return None;
    }
    let n = normal.normalized()?;
    let c = point + n * radius;
    c.is_finite().then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Vec2> {
        vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]
    }

    #[test]
    fn a_square_offsets_by_the_tool_radius() {
        let grown = offset_closed(&square(), 1.0).unwrap();
        let shrunk = offset_closed(&square(), -1.0).unwrap();
        assert!((polygon_area(&grown) - 144.0).abs() < 1e-9, "{grown:?}");
        assert!((polygon_area(&shrunk) - 64.0).abs() < 1e-9, "{shrunk:?}");
        assert!(shrunk.iter().any(|p| p.dist(Vec2::new(1.0, 1.0)) < 1e-9), "{shrunk:?}");
        assert!(offset_closed(&square(), -5.0).is_err());
        assert!(offset_closed(&square(), -6.0).is_err());
        assert!(offset_closed(&square(), f64::NAN).is_err());
    }

    #[test]
    fn pocket_passes_stop_when_the_tool_no_longer_fits() {
        let passes = inset_passes(&square(), 1.0, 2.0).unwrap();
        assert_eq!(passes.len(), 2, "{passes:?}");
        assert!((polygon_area(&passes[0]) - 64.0).abs() < 1e-9);
        assert!((polygon_area(&passes[1]) - 16.0).abs() < 1e-9);
    }

    #[test]
    fn contour_and_pocket_respect_islands() {
        let outer = square();
        let island = [Vec2::new(3.0, 3.0), Vec2::new(7.0, 3.0), Vec2::new(7.0, 7.0), Vec2::new(3.0, 7.0)];
        // Clockwise hole winding, as sketch regions store holes.
        let hole: Vec<Vec2> = island.iter().copied().rev().collect();
        let contours = contour_passes(&outer, std::slice::from_ref(&hole), 1.0).unwrap();
        assert_eq!(contours.len(), 2, "{contours:?}");
        assert!((polygon_area(&contours[0]) - 144.0).abs() < 1e-6, "{:?}", contours[0]);
        assert!((polygon_area(&contours[1]) - 4.0).abs() < 1e-6, "{:?}", contours[1]);
        let keepouts = island_keepouts(std::slice::from_ref(&hole), 1.0).unwrap();
        assert!(point_in_closed(&keepouts[0], Vec2::new(5.0, 5.0)));
        assert!(point_in_closed(&keepouts[0], Vec2::new(2.5, 5.0)));
        assert!(!point_in_closed(&keepouts[0], Vec2::new(0.5, 0.5)));
        let pocket = inset_passes_around(&outer, &[hole], 1.0, 2.0).unwrap();
        assert!(!pocket.is_empty(), "{pocket:?}");
        assert!(pocket.iter().all(|pass| pass.iter().all(|q| !point_in_closed(&keepouts[0], *q))));
    }

    #[test]
    fn depth_steps_end_on_the_floor_and_a_cusp_round_trips() {
        assert_eq!(depth_levels(10.0, 3.0).unwrap(), vec![3.0, 6.0, 9.0, 10.0]);
        assert_eq!(depth_levels(3.0, 10.0).unwrap(), vec![3.0]);
        assert!(depth_levels(f64::INFINITY, 1.0).is_err());
        let step = scallop_stepover(5.0, 0.1).unwrap();
        let cusp = cusp_height(5.0, step).unwrap();
        assert!((cusp - 0.1).abs() < 1e-9, "{cusp} from {step}");
        assert!(scallop_stepover(5.0, 5.0).is_err());
    }

    #[test]
    fn stations_keep_the_step_and_a_shorter_last_gap() {
        let rows = stepover_stations(0.0, 10.0, 4.0, 20).unwrap();
        assert_eq!(rows, vec![0.0, 4.0, 8.0, 10.0]);
        assert!(stepover_stations(0.0, 100.0, 0.01, 20).is_err());
        assert!(WorkCoords::from_xz(Vec3::ZERO, Vec3::Z, Vec3::Z).is_err());
        let frame = WorkCoords::from_xz(Vec3::new(10.0, 0.0, 0.0), Vec3::X, Vec3::Z).unwrap();
        let local = frame.to_local(Vec3::new(12.0, 3.0, 4.0));
        assert!((local.x - 2.0).abs() < 1e-12 && (local.y - 3.0).abs() < 1e-12 && (local.z - 4.0).abs() < 1e-12, "{local:?}");
    }

    #[test]
    fn a_vertical_ray_hits_the_highest_upward_triangle() {
        let mesh = Mesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 1.0),
                Vec3::new(10.0, 0.0, 1.0),
                Vec3::new(0.0, 10.0, 1.0),
                Vec3::new(0.0, 0.0, 4.0),
                Vec3::new(10.0, 0.0, 4.0),
                Vec3::new(0.0, 10.0, 4.0),
                Vec3::new(0.0, 0.0, 6.0),
                Vec3::new(0.0, 10.0, 6.0),
                Vec3::new(10.0, 0.0, 6.0),
            ],
            triangles: vec![[0, 1, 2], [3, 4, 5], [6, 7, 8]],
            ..Mesh::default()
        };
        let (point, normal) = upward_hit(&mesh, 1.0, 1.0).unwrap();
        assert!((point.z - 4.0).abs() < 1e-9, "{point:?}");
        assert!(normal.z > 0.9, "{normal:?}");
        let centre = ball_center(point, normal, 2.0).unwrap();
        assert!((centre.z - 6.0).abs() < 1e-9, "{centre:?}");
        assert!(upward_hit(&mesh, 9.0, 9.0).is_none());
    }
}
