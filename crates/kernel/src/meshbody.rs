//! Mesh bodies: triangles without a B-rep (imported 3MF/STL). They render, measure, move and
//! export; B-rep features refuse them.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use solidcraft_geom::{Mesh, Vec3};

use crate::body::Body;
use crate::{KernelError, Result};

/// Most triangles in one mesh body (`MAX_MESH_TRIS` in the parity design).
pub const MAX_TRIANGLES: usize = 20_000_000;
/// Alias for the design's `MAX_MESH_TRIS` cap on remesh / edit ops.
pub const MAX_MESH_TRIS: usize = MAX_TRIANGLES;

/// A section, edit, or face-group walk past these counts returns an error instead of running unbounded.
const MAX_SECTION_TRIS: usize = 2_000_000;
const MAX_SECTION_LOOPS: usize = 10_000;
const MAX_SECTION_POINTS: usize = 500_000;
const MAX_FACE_GROUP_TRIS: usize = 100_000;
/// Plane-cut / reduce / repair refuse past this before allocating work.
const MAX_EDIT_TRIS: usize = 2_000_000;
/// Region growing accepts a neighbour while the angle between normals stays under 30 degrees.
const FACE_GROUP_COS: f64 = 0.8660254037844387;
const MAX_DEVIATION_VERTS: usize = 200_000;
const MAX_DEVIATION_TRIS: usize = 200_000;
const MAX_DEVIATION_TESTS: usize = 20_000_000;

fn edit_tris_ok(triangles: usize, op: &str) -> Result<()> {
    if triangles > MAX_EDIT_TRIS {
        return Err(KernelError::Invalid(format!("{op} is limited to {MAX_EDIT_TRIS} triangles")));
    }
    Ok(())
}

/// Indexed triangles with shared vertices, wound counter-clockwise seen from outside.
#[derive(Clone, Debug, Default)]
pub struct TriMesh {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
}

impl TriMesh {
    /// Display/measure mesh: corners split per triangle so normals stay flat.
    pub(crate) fn to_mesh(&self) -> Mesh {
        let mut out = Mesh::default();
        out.positions.reserve(self.triangles.len() * 3);
        for t in &self.triangles {
            let (Some(a), Some(b), Some(c)) =
                (self.positions.get(t[0] as usize), self.positions.get(t[1] as usize), self.positions.get(t[2] as usize))
            else {
                continue;
            };
            let n = (*b - *a).cross(*c - *a).normalized().unwrap_or(Vec3::Z);
            let base = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
            out.positions.extend([*a, *b, *c]);
            out.normals.extend([n, n, n]);
            out.triangles.push([base, base.saturating_add(1), base.saturating_add(2)]);
            out.tri_face.push(0);
        }
        out
    }

    /// Distinct undirected triangle sides.
    pub(crate) fn edge_count(&self) -> usize {
        let mut set = std::collections::HashSet::new();
        for t in &self.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                set.insert((a.min(b), a.max(b)));
            }
        }
        set.len()
    }

    /// Signed volume ×6.
    fn volume6(&self) -> f64 {
        self.triangles
            .iter()
            .filter_map(|t| Some((self.positions.get(t[0] as usize)?, self.positions.get(t[1] as usize)?, self.positions.get(t[2] as usize)?)))
            .map(|(a, b, c)| a.dot(b.cross(*c)))
            .sum()
    }
}

/// A mesh body from triangles (`positions` in mm). Coincident vertices are merged, degenerate
/// triangles dropped, and a closed mesh with inward winding is turned outward.
pub fn mesh_body(positions: &[Vec3], triangles: &[[u32; 3]]) -> Result<Body> {
    if triangles.len() > MAX_TRIANGLES {
        return Err(KernelError::Invalid(format!("mesh with {} triangles (limit {MAX_TRIANGLES})", triangles.len())));
    }
    if positions.iter().any(|p| !(p.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9 && p.z.abs() < 1e9)) {
        return Err(KernelError::Invalid("mesh with non-finite or huge coordinates".into()));
    }
    // Merge vertices that are exactly equal (bit pattern of the coordinates).
    let mut map: HashMap<[u64; 3], u32> = HashMap::new();
    let mut remap: Vec<u32> = Vec::with_capacity(positions.len());
    let mut out = TriMesh::default();
    for p in positions {
        let key = [(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()];
        let next = u32::try_from(out.positions.len()).map_err(|_| KernelError::Invalid("too many vertices".into()))?;
        let i = *map.entry(key).or_insert_with(|| {
            out.positions.push(*p);
            next
        });
        remap.push(i);
    }
    for t in triangles {
        let (Some(&a), Some(&b), Some(&c)) = (remap.get(t[0] as usize), remap.get(t[1] as usize), remap.get(t[2] as usize)) else {
            return Err(KernelError::Invalid("mesh triangle refers to a missing vertex".into()));
        };
        if a == b || b == c || c == a {
            continue;
        }
        out.triangles.push([a, b, c]);
    }
    if out.triangles.is_empty() {
        return Err(KernelError::Invalid("mesh has no triangles".into()));
    }
    unify_winding(&mut out);
    if out.volume6() < 0.0 && is_closed(&out) {
        for t in &mut out.triangles {
            t.swap(1, 2);
        }
    }
    Ok(Body {
        solid: Arc::new(crate::body::Solid::new_unchecked(Vec::new())),
        coarse_missing: None,
        mesh: Some(Arc::new(out)),
        color: None,
        paint: None,
        splits: None,
        face_sources: None,
        history: None,
        regularization_protected: false,
    })
}

/// Wind every connected piece one way: a triangle that runs a side in the same direction as
/// its neighbour is turned over (files with stray inverted facets). Sides shared by more than
/// two triangles are left alone.
fn unify_winding(m: &mut TriMesh) {
    let mut sides: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in m.triangles.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            sides.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    let runs = |t: &[u32; 3], a: u32, b: u32| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])].contains(&(a, b));
    let mut done = vec![false; m.triangles.len()];
    for start in 0..m.triangles.len() {
        if done[start] {
            continue;
        }
        done[start] = true;
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            let Some(t) = m.triangles.get(i).copied() else { continue };
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let Some(list) = sides.get(&(a.min(b), a.max(b))) else { continue };
                if list.len() != 2 {
                    continue;
                }
                for &j in list {
                    if j == i || done[j] {
                        continue;
                    }
                    done[j] = true;
                    if let Some(u) = m.triangles.get_mut(j)
                        && runs(u, a, b)
                    {
                        u.swap(1, 2);
                    }
                    stack.push(j);
                }
            }
        }
    }
}

/// Every triangle side is shared by exactly two triangles in opposite directions.
pub(crate) fn is_closed(m: &TriMesh) -> bool {
    let mut count: HashMap<(u32, u32), i32> = HashMap::new();
    for t in &m.triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *count.entry((a, b)).or_default() += 1;
        }
    }
    count.iter().all(|(&(a, b), &n)| n == 1 && count.get(&(b, a)) == Some(&1))
}

impl Body {
    /// Shared-vertex triangles of a mesh body.
    pub fn triangle_mesh(&self) -> Option<(&[Vec3], &[[u32; 3]])> {
        self.mesh.as_ref().map(|m| (m.positions.as_slice(), m.triangles.as_slice()))
    }
    /// Does a mesh body enclose a volume (every side shared by two triangles)?
    pub fn is_closed_mesh(&self) -> bool {
        self.mesh.as_ref().is_some_and(|m| is_closed(m))
    }

    /// A mesh body moved by a column-major affine matrix (reflections keep outward winding).
    pub(crate) fn mesh_transformed(&self, m: &[[f64; 4]; 4]) -> Option<Body> {
        let tm = self.mesh.as_ref()?;
        let f = |p: Vec3| {
            Vec3::new(
                m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
                m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
                m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
            )
        };
        let det = m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2]) - m[1][0] * (m[0][1] * m[2][2] - m[2][1] * m[0][2])
            + m[2][0] * (m[0][1] * m[1][2] - m[1][1] * m[0][2]);
        let mut out = TriMesh { positions: tm.positions.iter().map(|p| f(*p)).collect(), triangles: tm.triangles.clone() };
        if det < 0.0 {
            for t in &mut out.triangles {
                t.swap(1, 2);
            }
        }
        Some(Body {
            solid: self.solid.clone(),
            coarse_missing: None,
            mesh: Some(Arc::new(out)),
            color: self.color,
            paint: self.paint.clone(),
            splits: self.splits.clone(),
            face_sources: None,
            history: None,
            regularization_protected: self.regularization_protected,
        })
    }
}

/// Keep the part of a mesh on the same side of a plane as `keep`. Triangles that cross the
/// plane are clipped. The result is a mesh body (open, if the cut removed a cap).
pub fn mesh_plane_cut(body: &Body, origin: Vec3, normal: Vec3, keep: Vec3) -> Result<Body> {
    let n = normal.normalized().ok_or_else(|| KernelError::Invalid("the cut plane needs a direction".into()))?;
    if !origin.is_finite() || !keep.is_finite() {
        return Err(KernelError::Invalid("the cut plane needs finite points".into()));
    }
    let (positions, triangles) = body.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    edit_tris_ok(triangles.len(), "plane cut")?;
    let side = |p: Vec3| (p - origin).dot(n);
    let keep_pos = side(keep) >= 0.0;
    let mut out_pos = Vec::new();
    let mut out_tri = Vec::new();
    let push = |out_pos: &mut Vec<Vec3>, out_tri: &mut Vec<[u32; 3]>, poly: &[Vec3]| {
        if poly.len() < 3 {
            return;
        }
        let base = u32::try_from(out_pos.len()).unwrap_or(u32::MAX);
        out_pos.extend(poly.iter().copied());
        for k in 1..poly.len() - 1 {
            out_tri.push([base, base.saturating_add(k as u32), base.saturating_add(k as u32 + 1)]);
        }
    };
    for t in triangles {
        let pts: Vec<Vec3> = t.iter().filter_map(|i| positions.get(*i as usize).copied()).collect();
        let [a, b, c] = pts.as_slice() else { continue };
        let tri = [*a, *b, *c];
        let inside = |p: Vec3| (side(p) >= -1e-8) == keep_pos;
        let inn: Vec<Vec3> = tri.into_iter().filter(|p| inside(*p)).collect();
        if inn.len() == 3 {
            push(&mut out_pos, &mut out_tri, &tri);
            continue;
        }
        if inn.is_empty() {
            continue;
        }
        // Walk the boundary and insert intersections.
        let mut poly = Vec::new();
        for i in 0..3 {
            let (p, q) = (tri[i], tri[(i + 1) % 3]);
            let (ip, iq) = (inside(p), inside(q));
            if ip {
                poly.push(p);
            }
            if ip != iq {
                let (sp, sq) = (side(p), side(q));
                let den = sp - sq;
                if den.abs() > 1e-12 {
                    let t = sp / den;
                    if (0.0..=1.0).contains(&t) {
                        poly.push(p + (q - p) * t);
                    }
                }
            }
        }
        push(&mut out_pos, &mut out_tri, &poly);
    }
    if out_tri.is_empty() {
        return Err(KernelError::Invalid("the plane cut removed the whole mesh".into()));
    }
    let mut cut = mesh_body(&out_pos, &out_tri)?;
    cut.color = body.color();
    Ok(cut)
}

/// Drop duplicate and degenerate triangles. A boundary of exactly three edges is filled, which
/// closes a single triangular hole.
pub fn mesh_repair(body: &Body) -> Result<Body> {
    let (positions, triangles) = body.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    edit_tris_ok(triangles.len(), "mesh repair")?;
    let mut tris: Vec<[u32; 3]> = triangles
        .iter()
        .copied()
        .filter(|t| t[0] != t[1] && t[1] != t[2] && t[2] != t[0])
        .map(|t| {
            let mut s = t;
            if s[0] > s[1] {
                s.swap(0, 1);
            }
            if s[1] > s[2] {
                s.swap(1, 2);
            }
            if s[0] > s[1] {
                s.swap(0, 1);
            }
            s
        })
        .collect();
    tris.sort();
    tris.dedup();
    let mut edges: std::collections::HashMap<(u32, u32), i32> = std::collections::HashMap::new();
    for t in &tris {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let k = (a.min(b), a.max(b));
            *edges.entry(k).or_default() += 1;
        }
    }
    let boundary: Vec<(u32, u32)> = edges.into_iter().filter(|(_, n)| *n == 1).map(|(k, _)| k).collect();
    if boundary.len() == 3 {
        let mut ids = Vec::new();
        for (a, b) in &boundary {
            if !ids.contains(a) {
                ids.push(*a);
            }
            if !ids.contains(b) {
                ids.push(*b);
            }
        }
        if ids.len() == 3 {
            tris.push([ids[0], ids[1], ids[2]]);
        }
    }
    let mut fixed = mesh_body(positions, &tris)?;
    fixed.color = body.color();
    Ok(fixed)
}

/// Collapse nearby vertices so about `ratio` of the triangles remain (0…1).
/// The grid cell is capped and shrunk on empty results so a small body is never wiped out
/// by an oversized cell (which used to return "removed every triangle" for valid ratios).
pub fn mesh_reduce(body: &Body, ratio: f64) -> Result<Body> {
    if !(ratio.is_finite() && ratio > 0.0 && ratio <= 1.0) {
        return Err(KernelError::Invalid("reduce ratio must be within 0…1".into()));
    }
    let (positions, triangles) = body.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    edit_tris_ok(triangles.len(), "mesh reduce")?;
    if triangles.is_empty() {
        return Err(KernelError::Invalid("the mesh has no triangles".into()));
    }
    let size = body.size().max(1e-4);
    // Cap the first cell to ≤¼ of the diagonal so a short edge still spans several bins.
    let mut cell = (size * (1.0 - ratio).max(0.01) * 0.25).max(1e-4);
    for _ in 0..12 {
        let key = |p: Vec3| ((p.x / cell).round() as i64, (p.y / cell).round() as i64, (p.z / cell).round() as i64);
        let mut map: std::collections::HashMap<(i64, i64, i64), u32> = std::collections::HashMap::new();
        let mut remap = vec![0u32; positions.len()];
        let mut out_pos = Vec::new();
        for (i, p) in positions.iter().enumerate() {
            let k = key(*p);
            let next = u32::try_from(out_pos.len()).unwrap_or(u32::MAX);
            let id = *map.entry(k).or_insert_with(|| {
                out_pos.push(*p);
                next
            });
            if let Some(slot) = remap.get_mut(i) {
                *slot = id;
            }
        }
        let tris: Vec<[u32; 3]> = triangles
            .iter()
            .filter_map(|t| {
                let (a, b, c) = (*remap.get(t[0] as usize)?, *remap.get(t[1] as usize)?, *remap.get(t[2] as usize)?);
                (a != b && b != c && c != a).then_some([a, b, c])
            })
            .collect();
        if !tris.is_empty() {
            let mut reduced = mesh_body(&out_pos, &tris)?;
            reduced.color = body.color();
            return Ok(reduced);
        }
        cell *= 0.5;
        if cell < 1e-6 {
            break;
        }
    }
    // Valid ratio but every cell emptied the mesh — leave the body unchanged (count does not grow).
    Ok(body.clone())
}

/// Closed loops where a plane cuts the mesh. Every point lies on the plane. Loops are ordered
/// along the cut, one loop per connected cycle, so a tube yields the outer ring and the hole.
/// A plane that misses the mesh is an error.
pub fn mesh_section(body: &Body, origin: Vec3, normal: Vec3) -> Result<Vec<Vec<Vec3>>> {
    let n = normal.normalized().ok_or_else(|| KernelError::Invalid("the section plane needs a direction".into()))?;
    if !origin.is_finite() {
        return Err(KernelError::Invalid("the section plane needs a finite origin".into()));
    }
    let (positions, triangles) = body.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    if triangles.is_empty() {
        return Err(KernelError::Invalid("the mesh has no triangles".into()));
    }
    if triangles.len() > MAX_SECTION_TRIS {
        return Err(KernelError::Invalid(format!("mesh section is limited to {MAX_SECTION_TRIS} triangles")));
    }
    let scale = body.size().max(1.0);
    let eps = (scale * 1e-8).clamp(1e-8, 1e-4);
    let mut at: HashMap<[i64; 3], Vec3> = HashMap::new();
    let mut edges: HashSet<([i64; 3], [i64; 3])> = HashSet::new();
    for t in triangles {
        let (Some(a), Some(b), Some(c)) =
            (positions.get(t[0] as usize).copied(), positions.get(t[1] as usize).copied(), positions.get(t[2] as usize).copied())
        else {
            continue;
        };
        let Some((p, q)) = triangle_cut([a, b, c], origin, n, eps) else { continue };
        let (p, q) = (project_plane(p, origin, n), project_plane(q, origin, n));
        if !p.is_finite() || !q.is_finite() {
            continue;
        }
        let (ka, kb) = (quant_key(p), quant_key(q));
        if ka == kb {
            continue;
        }
        at.entry(ka).or_insert(p);
        at.entry(kb).or_insert(q);
        edges.insert(canon_key(ka, kb));
        if edges.len() > MAX_SECTION_POINTS {
            return Err(KernelError::Invalid(format!("mesh section is limited to {MAX_SECTION_POINTS} cut edges")));
        }
    }
    if edges.is_empty() {
        return Err(KernelError::Invalid("the plane misses the mesh".into()));
    }
    let mut adj: HashMap<[i64; 3], Vec<[i64; 3]>> = HashMap::new();
    for (a, b) in &edges {
        adj.entry(*a).or_default().push(*b);
        adj.entry(*b).or_default().push(*a);
    }
    for list in adj.values_mut() {
        list.sort();
        list.dedup();
    }
    let mut used: HashSet<([i64; 3], [i64; 3])> = HashSet::new();
    let mut starts: Vec<[i64; 3]> = adj.keys().copied().collect();
    starts.sort();
    let mut loops = Vec::new();
    for start in starts {
        while let Some(next0) = adj.get(&start).and_then(|list| list.iter().copied().find(|nb| !used.contains(&canon_key(start, *nb)))) {
            used.insert(canon_key(start, next0));
            let mut ring_keys = vec![start];
            let mut prev = start;
            let mut cur = next0;
            let mut closed = false;
            for _ in 0..MAX_SECTION_POINTS {
                if cur == start {
                    closed = true;
                    break;
                }
                ring_keys.push(cur);
                let Some(next) = section_step(&adj, &at, &used, prev, cur) else { break };
                used.insert(canon_key(cur, next));
                prev = cur;
                cur = next;
            }
            if ring_keys.len() > MAX_SECTION_POINTS {
                return Err(KernelError::Invalid("the section loop is too long".into()));
            }
            if !closed || ring_keys.len() < 3 {
                continue;
            }
            let mut ring = Vec::with_capacity(ring_keys.len());
            for key in &ring_keys {
                let Some(p) = at.get(key).copied() else { continue };
                ring.push(project_plane(p, origin, n));
            }
            if let Some(clean) = simplify_section_loop(&ring, origin, n, eps) {
                loops.push(clean);
            }
            if loops.len() > MAX_SECTION_LOOPS {
                return Err(KernelError::Invalid(format!("mesh section is limited to {MAX_SECTION_LOOPS} loops")));
            }
        }
    }
    if loops.is_empty() {
        return Err(KernelError::Invalid("the cut did not form a closed loop".into()));
    }
    // Stable order: larger loops first, then by centroid so multi-body cuts are deterministic.
    loops.sort_by(|a, b| {
        b.len().cmp(&a.len()).then_with(|| {
            let ca = loop_centroid(a);
            let cb = loop_centroid(b);
            ca.x.total_cmp(&cb.x).then(ca.y.total_cmp(&cb.y)).then(ca.z.total_cmp(&cb.z))
        })
    });
    Ok(loops)
}

/// Group triangles by growing across shared edges whose normals stay within 30 degrees
/// (a flat or gently curved face). The groups partition the triangles: the counts sum to the
/// triangle count and every triangle is in one group. Names follow the group's average axis.
pub fn mesh_face_groups(body: &Body) -> Result<Vec<(String, usize)>> {
    let (positions, triangles) = body.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    if triangles.is_empty() {
        return Err(KernelError::Invalid("the mesh has no triangles".into()));
    }
    if triangles.len() > MAX_FACE_GROUP_TRIS {
        return Err(KernelError::Invalid(format!("face groups are limited to {MAX_FACE_GROUP_TRIS} triangles")));
    }
    let normals: Vec<Vec3> = triangles
        .iter()
        .map(|t| {
            let (Some(a), Some(b), Some(c)) = (positions.get(t[0] as usize), positions.get(t[1] as usize), positions.get(t[2] as usize)) else {
                return Vec3::ZERO;
            };
            (*b - *a).cross(*c - *a).normalized().unwrap_or(Vec3::ZERO)
        })
        .collect();
    let mut edge_tris: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in triangles.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            edge_tris.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); triangles.len()];
    for list in edge_tris.values() {
        if list.len() < 2 {
            continue;
        }
        let link = if list.len() <= 8 { list.len() } else { 2 };
        for (i, &a) in list.iter().enumerate() {
            for &b in list.iter().take(link).skip(i + 1) {
                if let Some(slot) = neighbors.get_mut(a)
                    && !slot.contains(&b)
                {
                    slot.push(b);
                }
                if let Some(slot) = neighbors.get_mut(b)
                    && !slot.contains(&a)
                {
                    slot.push(a);
                }
            }
        }
    }
    for list in &mut neighbors {
        list.sort_unstable();
    }
    let mut owner = vec![usize::MAX; triangles.len()];
    let mut groups = Vec::new();
    for start in 0..triangles.len() {
        if owner.get(start).copied().unwrap_or(usize::MAX) != usize::MAX {
            continue;
        }
        let id = groups.len();
        if let Some(slot) = owner.get_mut(start) {
            *slot = id;
        }
        let mut stack = vec![start];
        let mut count = 0usize;
        let mut acc = Vec3::ZERO;
        while let Some(i) = stack.pop() {
            count += 1;
            let n = normals.get(i).copied().unwrap_or(Vec3::ZERO);
            acc += n;
            let Some(near) = neighbors.get(i) else { continue };
            for &j in near {
                if owner.get(j).copied().unwrap_or(usize::MAX) != usize::MAX {
                    continue;
                }
                let other = normals.get(j).copied().unwrap_or(Vec3::ZERO);
                if n != Vec3::ZERO && other != Vec3::ZERO && n.dot(other) > FACE_GROUP_COS {
                    if let Some(slot) = owner.get_mut(j) {
                        *slot = id;
                    }
                    stack.push(j);
                }
            }
        }
        groups.push((axis_name(acc), count));
    }
    // A triangle whose normal was zero, or that lost the race, still has to belong somewhere.
    for (i, slot) in owner.iter_mut().enumerate() {
        if *slot == usize::MAX {
            *slot = groups.len();
            groups.push((axis_name(normals.get(i).copied().unwrap_or(Vec3::ZERO)), 1));
        }
    }
    Ok(groups)
}

/// One planar B-rep face per triangle. The mesh must be a closed triangle solid small enough to sew.
pub fn mesh_faceted(body: &Body) -> Result<Body> {
    let (positions, triangles) = body.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    if triangles.len() > 4000 {
        return Err(KernelError::Invalid("a faceted conversion is limited to 4000 triangles".into()));
    }
    let mut faces = Vec::new();
    for t in triangles {
        let (Some(a), Some(b), Some(c)) =
            (positions.get(t[0] as usize).copied(), positions.get(t[1] as usize).copied(), positions.get(t[2] as usize).copied())
        else {
            continue;
        };
        if (b - a).cross(c - a).len() < 1e-8 {
            continue;
        }
        faces.push(crate::FaceSpec {
            loops: vec![vec![crate::EdgeSpec::Line { a, b }, crate::EdgeSpec::Line { a: b, b: c }, crate::EdgeSpec::Line { a: c, b: a }]],
            surface: crate::SurfSpec::Plane,
            probe: None,
        });
    }
    if faces.is_empty() {
        return Err(KernelError::Invalid("the mesh has no triangles to convert".into()));
    }
    let mut solid = crate::sew(&faces)?;
    solid = solid.with_color(body.color());
    Ok(solid)
}

fn owned_mesh(body: &Body) -> Result<TriMesh> {
    let (positions, triangles) = body.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    Ok(TriMesh { positions: positions.to_vec(), triangles: triangles.to_vec() })
}

fn painted(body: &Body, mesh: TriMesh) -> Result<Body> {
    let mut out = mesh_body(&mesh.positions, &mesh.triangles)?;
    out.color = body.color();
    Ok(out)
}

/// Flip every triangle so the opposite side faces out.
pub fn mesh_reverse(body: &Body) -> Result<Body> {
    let mut mesh = owned_mesh(body)?;
    for tri in &mut mesh.triangles {
        tri.swap(1, 2);
    }
    painted(body, mesh)
}

/// Move vertices toward the average of their neighbours. At most 50 passes.
pub fn mesh_smooth(body: &Body, iterations: u32) -> Result<Body> {
    if iterations > 50 {
        return Err(KernelError::Invalid("smooth iterations are at most 50".into()));
    }
    let mut mesh = owned_mesh(body)?;
    for _ in 0..iterations {
        let src = mesh.positions.clone();
        let mut acc = vec![Vec3::ZERO; src.len()];
        let mut count = vec![0.0_f64; src.len()];
        for tri in &mesh.triangles {
            for pair in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let (ia, ib) = (pair.0 as usize, pair.1 as usize);
                let (Some(pa), Some(pb)) = (src.get(ia).copied(), src.get(ib).copied()) else { continue };
                if let Some(slot) = acc.get_mut(ia) {
                    *slot += pb;
                }
                if let Some(slot) = count.get_mut(ia) {
                    *slot += 1.0;
                }
                if let Some(slot) = acc.get_mut(ib) {
                    *slot += pa;
                }
                if let Some(slot) = count.get_mut(ib) {
                    *slot += 1.0;
                }
            }
        }
        for (i, point) in mesh.positions.iter_mut().enumerate() {
            let (Some(src_i), Some(sum), Some(n)) = (src.get(i).copied(), acc.get(i).copied(), count.get(i).copied()) else { continue };
            if n > 0.0 {
                *point = src_i * 0.5 + sum * (0.5 / n);
            }
        }
    }
    painted(body, mesh)
}

/// Retriangulate toward `target` triangles: split long edges when the mesh is coarser than the
/// target, collapse short manifold edges when it is finer. The result never exceeds
/// [`MAX_TRIANGLES`]. A target of 0 or past that cap is an error. The count lands on the target
/// when the mesh topology allows it, and otherwise on the nearest count the collapses can reach.
pub fn mesh_remesh(body: &Body, target: usize) -> Result<Body> {
    if target == 0 || target > MAX_MESH_TRIS {
        return Err(KernelError::Invalid("remesh target is out of range".into()));
    }
    let mut mesh = owned_mesh(body)?;
    let count = mesh.triangles.len();
    if count == 0 {
        return Err(KernelError::Invalid("the mesh has no triangles".into()));
    }
    if count > MAX_TRIANGLES {
        return Err(KernelError::Invalid(format!("mesh with {count} triangles (limit {MAX_TRIANGLES})")));
    }
    if target == count {
        return Ok(body.clone());
    }
    if target > count {
        mesh = refine_to_target(mesh, target)?;
    } else {
        mesh = coarsen_to_target(mesh, target)?;
    }
    painted(body, mesh)
}

/// Offset vertices along their averaged normals.
pub fn mesh_shell(body: &Body, thickness: f64) -> Result<Body> {
    if !(thickness.is_finite() && thickness.abs() > 1e-9 && thickness.abs() < 1e5) {
        return Err(KernelError::Invalid("mesh shell thickness must be finite".into()));
    }
    let mut mesh = owned_mesh(body)?;
    let mut normals = vec![Vec3::ZERO; mesh.positions.len()];
    for tri in &mesh.triangles {
        let (Some(a), Some(b), Some(c)) = (
            mesh.positions.get(tri[0] as usize).copied(),
            mesh.positions.get(tri[1] as usize).copied(),
            mesh.positions.get(tri[2] as usize).copied(),
        ) else {
            continue;
        };
        let n = (b - a).cross(c - a);
        for index in tri {
            if let Some(slot) = normals.get_mut(*index as usize) {
                *slot += n;
            }
        }
    }
    for (point, normal) in mesh.positions.iter_mut().zip(&normals) {
        if let Some(unit) = normal.normalized() {
            *point += unit * thickness;
        }
    }
    painted(body, mesh)
}

/// Join two meshes into one body.
pub fn mesh_combine(a: &Body, b: &Body) -> Result<Body> {
    let (pa, ta) = a.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    let (pb, tb) = b.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    let shift = u32::try_from(pa.len()).map_err(|_| KernelError::Invalid("too many vertices".into()))?;
    let mut positions = pa.to_vec();
    positions.extend_from_slice(pb);
    let mut triangles = ta.to_vec();
    for tri in tb {
        triangles.push([tri[0].saturating_add(shift), tri[1].saturating_add(shift), tri[2].saturating_add(shift)]);
    }
    mesh_body(&positions, &triangles)
}

/// Split disconnected triangle groups. One shell is an error.
pub fn mesh_separate(body: &Body) -> Result<Vec<Body>> {
    let mesh = owned_mesh(body)?;
    let n = mesh.triangles.len();
    if n == 0 {
        return Err(KernelError::Invalid("the mesh has no triangles".into()));
    }
    let mut owner = vec![None; mesh.positions.len()];
    let mut groups: Vec<Vec<[u32; 3]>> = Vec::new();
    for tri in &mesh.triangles {
        let found = tri.iter().filter_map(|i| owner.get(*i as usize).copied().flatten()).next();
        let slot = if let Some(g) = found {
            g
        } else {
            groups.push(Vec::new());
            groups.len() - 1
        };
        if let Some(group) = groups.get_mut(slot) {
            group.push(*tri);
        }
        for index in tri {
            if let Some(mark) = owner.get_mut(*index as usize) {
                *mark = Some(slot);
            }
        }
    }
    if groups.len() < 2 {
        return Err(KernelError::Invalid("the mesh is one shell".into()));
    }
    let mut out = Vec::new();
    for group in groups {
        out.push(mesh_body(&mesh.positions, &group)?);
    }
    Ok(out)
}

/// Maximum and mean distance from mesh vertices to the solid's surface.
/// Both are finite and ≥ 0. An exact faceted rebuild of the same vertices is about 0.
pub fn mesh_deviation(solid: &Body, mesh: &Body) -> Result<(f64, f64)> {
    let (positions, _) = mesh.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    if positions.is_empty() {
        return Err(KernelError::Invalid("the mesh has no vertices".into()));
    }
    if positions.len() > MAX_DEVIATION_VERTS {
        return Err(KernelError::Invalid(format!("deviation is limited to {MAX_DEVIATION_VERTS} vertices")));
    }
    let tess = solid.tessellate(0.2)?;
    if tess.triangles.is_empty() {
        return Err(KernelError::Invalid("the solid has no surface to compare".into()));
    }
    if tess.triangles.len() > MAX_DEVIATION_TRIS {
        return Err(KernelError::Invalid(format!("deviation is limited to {MAX_DEVIATION_TRIS} solid triangles")));
    }
    if positions.len().saturating_mul(tess.triangles.len()) > MAX_DEVIATION_TESTS {
        return Err(KernelError::Invalid("deviation exceeds its work budget".into()));
    }
    let mut max = 0.0_f64;
    let mut sum = 0.0_f64;
    let mut count = 0.0_f64;
    for point in positions {
        if !point.is_finite() {
            continue;
        }
        let mut best = f64::MAX;
        for tri in &tess.triangles {
            let Some([a, b, c]) = tess.tri(tri) else { continue };
            best = best.min(dist_point_triangle(*point, a, b, c));
        }
        if best.is_finite() {
            max = max.max(best);
            sum += best;
            count += 1.0;
        }
    }
    if count == 0.0 || !max.is_finite() || !sum.is_finite() {
        return Err(KernelError::Invalid("deviation was not a finite distance".into()));
    }
    let mean = sum / count;
    if mean < 0.0 || max < 0.0 || !mean.is_finite() {
        return Err(KernelError::Invalid("deviation was not a finite distance".into()));
    }
    Ok((max, mean))
}

fn project_plane(p: Vec3, origin: Vec3, normal: Vec3) -> Vec3 {
    p - normal * (p - origin).dot(normal)
}

fn loop_centroid(points: &[Vec3]) -> Vec3 {
    if points.is_empty() {
        return Vec3::ZERO;
    }
    let n = points.len() as f64;
    points.iter().fold(Vec3::ZERO, |acc, p| acc + *p) * (1.0 / n)
}

/// Drop duplicate / collinear samples and wind the ring counter-clockwise in the cut plane so
/// callers see one ordered closed polyline per cycle.
fn simplify_section_loop(points: &[Vec3], origin: Vec3, normal: Vec3, eps: f64) -> Option<Vec<Vec3>> {
    if points.len() < 3 {
        return None;
    }
    let x = normal.any_perp();
    let y = normal.cross(x);
    let to2 = |p: Vec3| solidcraft_geom::Vec2::new((p - origin).dot(x), (p - origin).dot(y));
    let mut ring: Vec<Vec3> = Vec::with_capacity(points.len());
    for p in points {
        if !p.is_finite() {
            continue;
        }
        let p = project_plane(*p, origin, normal);
        if ring.last().is_none_or(|q| q.dist(p) > eps) {
            ring.push(p);
        }
    }
    if ring.len() >= 2 && ring.first().is_some_and(|a| ring.last().is_some_and(|b| a.dist(*b) <= eps)) {
        ring.pop();
    }
    if ring.len() < 3 {
        return None;
    }
    // Strip near-collinear midpoints while keeping corners.
    let mut guard = 0u32;
    while ring.len() > 3 && guard < 64 {
        guard += 1;
        let n = ring.len();
        let remove = (0..n).find(|&i| {
            let (Some(a), Some(b), Some(c)) = (ring.get((i + n - 1) % n), ring.get(i), ring.get((i + 1) % n)) else {
                return false;
            };
            let (a2, b2, c2) = (to2(*a), to2(*b), to2(*c));
            (b2 - a2).cross(c2 - b2).abs() <= eps * (a2.dist(c2).max(eps))
        });
        let Some(i) = remove else { break };
        ring.remove(i);
    }
    if ring.len() < 3 {
        return None;
    }
    // Positive 2D area ⇒ counter-clockwise in the cut plane.
    let mut area2 = 0.0;
    for i in 0..ring.len() {
        let (Some(pa), Some(pb)) = (ring.get(i), ring.get((i + 1) % ring.len())) else { continue };
        area2 += to2(*pa).cross(to2(*pb));
    }
    if !area2.is_finite() || area2.abs() < eps * eps {
        return None;
    }
    if area2 < 0.0 {
        ring.reverse();
    }
    Some(ring)
}

fn quant_key(p: Vec3) -> [i64; 3] {
    [(p.x * 1.0e5).round() as i64, (p.y * 1.0e5).round() as i64, (p.z * 1.0e5).round() as i64]
}

fn canon_key(a: [i64; 3], b: [i64; 3]) -> ([i64; 3], [i64; 3]) {
    if a <= b { (a, b) } else { (b, a) }
}

fn triangle_cut(pts: [Vec3; 3], origin: Vec3, normal: Vec3, eps: f64) -> Option<(Vec3, Vec3)> {
    let side = |p: Vec3| (p - origin).dot(normal);
    let sides = [side(pts[0]), side(pts[1]), side(pts[2])];
    let on = |s: f64| s.abs() <= eps;
    if on(sides[0]) && on(sides[1]) && on(sides[2]) {
        return None;
    }
    let mut hits: Vec<Vec3> = Vec::new();
    for i in 0..3 {
        let j = (i + 1) % 3;
        let (p, q) = (pts[i], pts[j]);
        let (sp, sq) = (sides[i], sides[j]);
        if on(sp) && on(sq) {
            push_hit(&mut hits, p, eps);
            push_hit(&mut hits, q, eps);
        } else if on(sp) {
            push_hit(&mut hits, p, eps);
        } else if on(sq) {
            push_hit(&mut hits, q, eps);
        } else if sp * sq < 0.0 {
            let den = sp - sq;
            if den.abs() > 1e-18 {
                let t = sp / den;
                if (0.0..=1.0).contains(&t) {
                    push_hit(&mut hits, p + (q - p) * t, eps);
                }
            }
        }
    }
    if hits.len() == 2 { Some((hits[0], hits[1])) } else { None }
}

fn push_hit(hits: &mut Vec<Vec3>, p: Vec3, eps: f64) {
    if !p.is_finite() || hits.iter().any(|q| q.dist(p) <= eps) {
        return;
    }
    hits.push(p);
}

fn section_step(
    adj: &HashMap<[i64; 3], Vec<[i64; 3]>>,
    at: &HashMap<[i64; 3], Vec3>,
    used: &HashSet<([i64; 3], [i64; 3])>,
    prev: [i64; 3],
    cur: [i64; 3],
) -> Option<[i64; 3]> {
    let list = adj.get(&cur)?;
    let incoming = at.get(&cur).copied().unwrap_or(Vec3::ZERO) - at.get(&prev).copied().unwrap_or(Vec3::ZERO);
    let mut best: Option<(f64, [i64; 3])> = None;
    for n in list {
        if *n == prev || used.contains(&canon_key(cur, *n)) {
            continue;
        }
        let dir = at.get(n).copied().unwrap_or(Vec3::ZERO) - at.get(&cur).copied().unwrap_or(Vec3::ZERO);
        let score = incoming.dot(dir) / dir.len().max(1e-12);
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, *n));
        }
    }
    best.map(|(_, n)| n)
}

fn axis_name(acc: Vec3) -> String {
    let (ax, _) = [acc.x.abs(), acc.y.abs(), acc.z.abs()].into_iter().enumerate().max_by(|u, v| u.1.total_cmp(&v.1)).unwrap_or((2, 0.0));
    let sign = match ax {
        0 => acc.x >= 0.0,
        1 => acc.y >= 0.0,
        _ => acc.z >= 0.0,
    };
    match (ax, sign) {
        (0, true) => "+X",
        (0, false) => "-X",
        (1, true) => "+Y",
        (1, false) => "-Y",
        (_, true) => "+Z",
        (_, false) => "-Z",
    }
    .to_string()
}

fn refine_to_target(mut mesh: TriMesh, target: usize) -> Result<TriMesh> {
    let mut rounds = 0u32;
    while mesh.triangles.len() < target && rounds < 32 {
        rounds += 1;
        let before = mesh.triangles.len();
        let grown = before.saturating_mul(4);
        if grown <= target && grown <= MAX_TRIANGLES && before > 0 {
            mesh = subdivide_all(&mesh)?;
            continue;
        }
        let room = target.saturating_sub(before);
        let next = split_longest(&mesh, room)?;
        if next.triangles.len() <= before || next.triangles.len() > MAX_TRIANGLES {
            break;
        }
        mesh = next;
    }
    if mesh.triangles.len() > MAX_TRIANGLES {
        return Err(KernelError::Invalid(format!("remesh would exceed {MAX_TRIANGLES} triangles")));
    }
    Ok(mesh)
}

fn coarsen_to_target(mut mesh: TriMesh, target: usize) -> Result<TriMesh> {
    let mut rounds = 0u32;
    while mesh.triangles.len() > target && rounds < 64 {
        rounds += 1;
        let before = mesh.triangles.len();
        let next = match collapse_pass(&mesh, before - target) {
            Ok(next) if !next.triangles.is_empty() && next.triangles.len() < before => next,
            _ => break,
        };
        mesh = next;
    }
    if mesh.triangles.is_empty() {
        return Err(KernelError::Invalid("remesh removed every triangle".into()));
    }
    Ok(mesh)
}

fn subdivide_all(mesh: &TriMesh) -> Result<TriMesh> {
    if mesh.triangles.len().saturating_mul(4) > MAX_TRIANGLES {
        return Err(KernelError::Invalid(format!("remesh would exceed {MAX_TRIANGLES} triangles")));
    }
    let mut positions = mesh.positions.clone();
    let mut mids: HashMap<(u32, u32), u32> = HashMap::new();
    let mid = |positions: &mut Vec<Vec3>, mids: &mut HashMap<(u32, u32), u32>, a: u32, b: u32| -> Result<u32> {
        let key = (a.min(b), a.max(b));
        if let Some(id) = mids.get(&key) {
            return Ok(*id);
        }
        let (Some(pa), Some(pb)) = (positions.get(a as usize).copied(), positions.get(b as usize).copied()) else {
            return Err(KernelError::Invalid("remesh vertex".into()));
        };
        let id = u32::try_from(positions.len()).map_err(|_| KernelError::Invalid("too many vertices".into()))?;
        positions.push((pa + pb) * 0.5);
        mids.insert(key, id);
        Ok(id)
    };
    let mut triangles = Vec::with_capacity(mesh.triangles.len().saturating_mul(4));
    for t in &mesh.triangles {
        let ab = mid(&mut positions, &mut mids, t[0], t[1])?;
        let bc = mid(&mut positions, &mut mids, t[1], t[2])?;
        let ca = mid(&mut positions, &mut mids, t[2], t[0])?;
        triangles.push([t[0], ab, ca]);
        triangles.push([ab, t[1], bc]);
        triangles.push([ca, bc, t[2]]);
        triangles.push([ab, bc, ca]);
    }
    Ok(TriMesh { positions, triangles })
}

fn split_longest(mesh: &TriMesh, room: usize) -> Result<TriMesh> {
    if room == 0 || mesh.triangles.is_empty() {
        return Ok(mesh.clone());
    }
    let mut edge_use: HashMap<(u32, u32), usize> = HashMap::new();
    let mut edge_len: HashMap<(u32, u32), f64> = HashMap::new();
    for t in &mesh.triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let key = (a.min(b), a.max(b));
            *edge_use.entry(key).or_default() += 1;
            if let (Some(pa), Some(pb)) = (mesh.positions.get(a as usize), mesh.positions.get(b as usize)) {
                edge_len.entry(key).or_insert(pa.dist(*pb));
            }
        }
    }
    let mut edges: Vec<(f64, u32, u32, usize)> =
        edge_use.iter().filter_map(|(&(a, b), &k)| (k == 1 || k == 2).then_some((edge_len.get(&(a, b)).copied().unwrap_or(0.0), a, b, k))).collect();
    edges.sort_by(|u, v| v.0.total_cmp(&u.0).then(u.1.cmp(&v.1)).then(u.2.cmp(&v.2)));
    let mut blocked = vec![false; mesh.positions.len()];
    let mut chosen: Vec<(u32, u32)> = Vec::new();
    let mut added = 0usize;
    for (_, a, b, k) in edges {
        if added >= room {
            break;
        }
        if added + k > room {
            continue;
        }
        let (ia, ib) = (a as usize, b as usize);
        if blocked.get(ia).copied().unwrap_or(true) || blocked.get(ib).copied().unwrap_or(true) {
            continue;
        }
        if let Some(slot) = blocked.get_mut(ia) {
            *slot = true;
        }
        if let Some(slot) = blocked.get_mut(ib) {
            *slot = true;
        }
        chosen.push((a, b));
        added += k;
    }
    if chosen.is_empty() {
        return Ok(mesh.clone());
    }
    let mut positions = mesh.positions.clone();
    let mut mid_of: HashMap<(u32, u32), u32> = HashMap::new();
    for (a, b) in &chosen {
        let (Some(pa), Some(pb)) = (positions.get(*a as usize).copied(), positions.get(*b as usize).copied()) else {
            return Err(KernelError::Invalid("remesh vertex".into()));
        };
        let id = u32::try_from(positions.len()).map_err(|_| KernelError::Invalid("too many vertices".into()))?;
        positions.push((pa + pb) * 0.5);
        mid_of.insert(((*a).min(*b), (*a).max(*b)), id);
    }
    let mut triangles = Vec::with_capacity(mesh.triangles.len() + added);
    for t in &mesh.triangles {
        let (a, b, c) = (t[0], t[1], t[2]);
        if let Some(m) = mid_of.get(&(a.min(b), a.max(b))).copied() {
            triangles.push([a, m, c]);
            triangles.push([m, b, c]);
        } else if let Some(m) = mid_of.get(&(b.min(c), b.max(c))).copied() {
            triangles.push([a, b, m]);
            triangles.push([a, m, c]);
        } else if let Some(m) = mid_of.get(&(c.min(a), c.max(a))).copied() {
            triangles.push([a, b, m]);
            triangles.push([m, b, c]);
        } else {
            triangles.push(*t);
        }
    }
    compact_mesh(TriMesh { positions, triangles })
}

fn collapse_pass(mesh: &TriMesh, need_removed: usize) -> Result<TriMesh> {
    if need_removed == 0 || mesh.triangles.is_empty() {
        return Ok(mesh.clone());
    }
    let n = mesh.positions.len();
    let mut nbrs: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut incident: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut edge_opp: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for (ti, t) in mesh.triangles.iter().enumerate() {
        for i in 0..3 {
            let v = t[i];
            let w = t[(i + 1) % 3];
            let o = t[(i + 2) % 3];
            if let Some(list) = incident.get_mut(v as usize) {
                list.push(ti);
            }
            if let Some(list) = nbrs.get_mut(v as usize)
                && !list.contains(&w)
            {
                list.push(w);
            }
            edge_opp.entry((v.min(w), v.max(w))).or_default().push(o);
        }
    }
    let mut edges: Vec<(f64, u32, u32)> = Vec::new();
    for (&(a, b), opp) in &edge_opp {
        if opp.len() != 1 && opp.len() != 2 {
            continue;
        }
        let (Some(pa), Some(pb)) = (mesh.positions.get(a as usize), mesh.positions.get(b as usize)) else { continue };
        edges.push((pa.dist(*pb), a, b));
    }
    edges.sort_by(|u, v| u.0.total_cmp(&v.0).then(u.1.cmp(&v.1)).then(u.2.cmp(&v.2)));
    let mut blocked = vec![false; n];
    let mut merges: Vec<(u32, u32, Vec3)> = Vec::new();
    let mut removed = 0usize;
    for (_, a, b) in edges {
        if removed >= need_removed {
            break;
        }
        let (ia, ib) = (a as usize, b as usize);
        if blocked.get(ia).copied().unwrap_or(true) || blocked.get(ib).copied().unwrap_or(true) {
            continue;
        }
        let Some(opp) = edge_opp.get(&(a.min(b), a.max(b))) else { continue };
        if !link_ok(&nbrs, a, b, opp) {
            continue;
        }
        let (Some(pa), Some(pb)) = (mesh.positions.get(ia).copied(), mesh.positions.get(ib).copied()) else { continue };
        let mid = (pa + pb) * 0.5;
        if !mid.is_finite() || !normals_survive(mesh, &incident, a, b, mid) {
            continue;
        }
        let drop = opp.len();
        if mesh.triangles.len().saturating_sub(removed) <= drop {
            continue;
        }
        merges.push((a, b, mid));
        removed += drop;
        let mut block = |v: u32| {
            if let Some(slot) = blocked.get_mut(v as usize) {
                *slot = true;
            }
            if let Some(list) = nbrs.get(v as usize) {
                for n in list {
                    if let Some(slot) = blocked.get_mut(*n as usize) {
                        *slot = true;
                    }
                }
            }
        };
        block(a);
        block(b);
    }
    if merges.is_empty() {
        return Ok(mesh.clone());
    }
    let mut positions = mesh.positions.clone();
    let mut map = Vec::with_capacity(n);
    for i in 0..n {
        map.push(u32::try_from(i).map_err(|_| KernelError::Invalid("too many vertices".into()))?);
    }
    for (a, b, mid) in &merges {
        if let Some(slot) = positions.get_mut(*a as usize) {
            *slot = *mid;
        }
        if let Some(slot) = map.get_mut(*b as usize) {
            *slot = *a;
        }
    }
    let mut triangles = Vec::new();
    for t in &mesh.triangles {
        let (Some(&a), Some(&b), Some(&c)) = (map.get(t[0] as usize), map.get(t[1] as usize), map.get(t[2] as usize)) else { continue };
        if a == b || b == c || c == a {
            continue;
        }
        let (Some(pa), Some(pb), Some(pc)) = (positions.get(a as usize), positions.get(b as usize), positions.get(c as usize)) else { continue };
        if (*pb - *pa).cross(*pc - *pa).len() < 1e-12 {
            continue;
        }
        triangles.push([a, b, c]);
    }
    triangles.sort_unstable();
    triangles.dedup();
    compact_mesh(TriMesh { positions, triangles })
}

fn link_ok(nbrs: &[Vec<u32>], a: u32, b: u32, opp: &[u32]) -> bool {
    let (Some(na), Some(nb)) = (nbrs.get(a as usize), nbrs.get(b as usize)) else { return false };
    let mut inter = Vec::new();
    for v in na {
        if *v != a && *v != b && nb.contains(v) {
            inter.push(*v);
        }
    }
    inter.len() == opp.len() && opp.iter().all(|o| inter.contains(o))
}

fn normals_survive(mesh: &TriMesh, incident: &[Vec<usize>], a: u32, b: u32, mid: Vec3) -> bool {
    let mut seen = HashSet::new();
    for v in [a, b] {
        let Some(list) = incident.get(v as usize) else { continue };
        for &ti in list {
            if !seen.insert(ti) {
                continue;
            }
            let Some(t) = mesh.triangles.get(ti).copied() else { continue };
            let has_a = t.contains(&a);
            let has_b = t.contains(&b);
            if has_a == has_b {
                continue;
            }
            let Some(old) = tri_cross(mesh, t, None) else { continue };
            let mut nt = t;
            for v in &mut nt {
                if *v == b {
                    *v = a;
                }
            }
            if nt[0] == nt[1] || nt[1] == nt[2] || nt[2] == nt[0] {
                continue;
            }
            let Some(new) = tri_cross(mesh, nt, Some((a, mid))) else { continue };
            let denom = old.len() * new.len();
            if new.len() < 1e-12 {
                continue;
            }
            if denom > 1e-18 && old.dot(new) / denom < -1e-6 {
                return false;
            }
        }
    }
    true
}

fn tri_cross(mesh: &TriMesh, t: [u32; 3], replace: Option<(u32, Vec3)>) -> Option<Vec3> {
    let point = |i: u32| -> Option<Vec3> {
        if let Some((v, p)) = replace
            && i == v
        {
            return Some(p);
        }
        mesh.positions.get(i as usize).copied()
    };
    let (a, b, c) = (point(t[0])?, point(t[1])?, point(t[2])?);
    Some((b - a).cross(c - a))
}

fn compact_mesh(mesh: TriMesh) -> Result<TriMesh> {
    let mut used = vec![false; mesh.positions.len()];
    for t in &mesh.triangles {
        for v in t {
            if let Some(slot) = used.get_mut(*v as usize) {
                *slot = true;
            }
        }
    }
    let mut remap = vec![u32::MAX; mesh.positions.len()];
    let mut positions = Vec::new();
    for (i, p) in mesh.positions.iter().enumerate() {
        if !used.get(i).copied().unwrap_or(false) {
            continue;
        }
        let id = u32::try_from(positions.len()).map_err(|_| KernelError::Invalid("too many vertices".into()))?;
        if let Some(slot) = remap.get_mut(i) {
            *slot = id;
        }
        positions.push(*p);
    }
    let mut triangles = Vec::with_capacity(mesh.triangles.len());
    for t in &mesh.triangles {
        let (Some(&a), Some(&b), Some(&c)) = (remap.get(t[0] as usize), remap.get(t[1] as usize), remap.get(t[2] as usize)) else { continue };
        if a == u32::MAX || b == u32::MAX || c == u32::MAX || a == b || b == c || c == a {
            continue;
        }
        triangles.push([a, b, c]);
    }
    if triangles.is_empty() {
        return Err(KernelError::Invalid("remesh removed every triangle".into()));
    }
    Ok(TriMesh { positions, triangles })
}

/// Closest-point distance from `p` to triangle `a`–`b`–`c` (Christer Ericson).
fn dist_point_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return p.dist(a);
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return p.dist(b);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let den = d1 - d3;
        if den.abs() <= 1e-18 {
            return p.dist(a);
        }
        return p.dist(a + ab * (d1 / den));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return p.dist(c);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let den = d2 - d6;
        if den.abs() <= 1e-18 {
            return p.dist(a);
        }
        return p.dist(a + ac * (d2 / den));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let den = (d4 - d3) + (d5 - d6);
        if den.abs() <= 1e-18 {
            return p.dist(b);
        }
        return p.dist(b + (c - b) * ((d4 - d3) / den));
    }
    let denom = va + vb + vc;
    if denom.abs() <= 1e-18 {
        return p.dist_to_segment(a, b).min(p.dist_to_segment(b, c)).min(p.dist_to_segment(c, a));
    }
    p.dist(a + ab * (vb / denom) + ac * (vc / denom))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;

    #[test]
    fn a_plane_cut_keeps_one_side_and_groups_point_along_axes() {
        let box_mesh = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap().tessellate(0.05).unwrap();
        let body = mesh_body(&box_mesh.positions, &box_mesh.triangles).unwrap();
        let before = body.triangle_mesh().unwrap().1.len();
        let cut = mesh_plane_cut(&body, Vec3::new(5.0, 0.0, 0.0), Vec3::X, Vec3::new(8.0, 5.0, 5.0)).unwrap();
        let (pos, tris) = cut.triangle_mesh().unwrap();
        assert!(!tris.is_empty() && pos.iter().all(|p| p.x >= 4.9), "kept the +X side, {} tris", tris.len());
        let groups = mesh_face_groups(&body).unwrap();
        assert!(groups.iter().any(|(n, c)| n == "+Z" && *c > 0));
        assert!(mesh_repair(&body).is_ok());
        let reduced = mesh_reduce(&body, 0.2).unwrap();
        assert!(reduced.triangle_mesh().unwrap().1.len() <= before);
    }

    fn indexed_box() -> Body {
        let p = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(10.0, 10.0, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(10.0, 0.0, 10.0),
            Vec3::new(10.0, 10.0, 10.0),
            Vec3::new(0.0, 10.0, 10.0),
        ];
        let t = [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        mesh_body(&p, &t).unwrap()
    }

    fn grid_sheet(nx: usize, ny: usize) -> Body {
        let mut positions = Vec::new();
        for j in 0..=ny {
            for i in 0..=nx {
                positions.push(Vec3::new(i as f64, j as f64, 0.0));
            }
        }
        let mut triangles = Vec::new();
        for j in 0..ny {
            for i in 0..nx {
                let a = (j * (nx + 1) + i) as u32;
                let b = a + 1;
                let c = a + nx as u32 + 1;
                let d = c + 1;
                triangles.push([a, b, d]);
                triangles.push([a, d, c]);
            }
        }
        mesh_body(&positions, &triangles).unwrap()
    }

    fn tri_count(body: &Body) -> usize {
        body.triangle_mesh().map(|(_, t)| t.len()).unwrap_or(0)
    }

    fn on_plane(points: &[Vec3], origin: Vec3, normal: Vec3) -> bool {
        let Some(n) = normal.normalized() else { return false };
        points.iter().all(|p| (*p - origin).dot(n).abs() < 1e-6 && p.is_finite())
    }

    fn loop_is_ordered(points: &[Vec3], origin: Vec3, normal: Vec3) -> bool {
        let Some(n) = normal.normalized() else { return false };
        let x = n.any_perp();
        let y = n.cross(x);
        let local: Vec<solidcraft_geom::Vec2> =
            points.iter().map(|p| solidcraft_geom::Vec2::new((*p - origin).dot(x), (*p - origin).dot(y))).collect();
        if local.len() < 3 {
            return false;
        }
        let mut sign = 0.0;
        for i in 0..local.len() {
            let a = local[i];
            let b = local[(i + 1) % local.len()];
            let c = local[(i + 2) % local.len()];
            let cross = (b - a).cross(c - b);
            if cross.abs() < 1e-6 {
                continue;
            }
            let s = cross.signum();
            if sign == 0.0 {
                sign = s;
            } else if s != sign {
                return false;
            }
        }
        sign != 0.0
    }

    /// Feature: fusion360-feature-parity, Property 12: Mesh-section points lie on the cut plane
    #[test]
    fn property_12_section_points_lie_on_the_cut_plane() {
        let body = indexed_box();
        let mut hit = 0usize;
        for i in 0..100 {
            let a = i as f64 * 0.7;
            let b = 0.15 + (i as f64 % 17.0) * 0.08;
            let normal = Vec3::new(a.cos() * b.sin(), a.sin() * b.sin(), b.cos());
            let origin = Vec3::new(5.0, 5.0, 5.0);
            let loops = mesh_section(&body, origin, normal).unwrap_or_else(|e| panic!("plane {i} {normal:?}: {e}"));
            assert!(!loops.is_empty(), "plane {i}");
            for ring in &loops {
                assert!(ring.len() >= 3, "plane {i}");
                assert!(on_plane(ring, origin, normal), "plane {i} left the cut");
                assert!(loop_is_ordered(ring, origin, normal), "plane {i} loop is not ordered: {ring:?}");
            }
            hit += 1;
        }
        let mid = mesh_section(&body, Vec3::new(0.0, 0.0, 5.0), Vec3::Z).unwrap();
        assert_eq!(mid.len(), 1, "a box cut through the middle is one loop");
        let area = polygon_area_z(&mid[0]);
        assert!((area - 100.0).abs() < 1e-4, "section area {area}");
        assert!(on_plane(&mid[0], Vec3::new(0.0, 0.0, 5.0), Vec3::Z));
        let mut missed = 0usize;
        for i in 0..20 {
            let err = mesh_section(&body, Vec3::new(0.0, 0.0, 100.0 + i as f64), Vec3::Z);
            assert!(err.is_err(), "a plane past the box must miss");
            missed += 1;
        }
        assert!(mesh_section(&body, Vec3::ZERO, Vec3::ZERO).is_err());
        assert!(mesh_section(&body, Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Z).is_err());
        assert!(hit >= 100 && missed >= 20, "{hit} hits, {missed} misses");
    }

    fn polygon_area_z(points: &[Vec3]) -> f64 {
        let mut area = 0.0;
        for i in 0..points.len() {
            let p = points[i];
            let q = points[(i + 1) % points.len()];
            area += p.x * q.y - q.x * p.y;
        }
        area.abs() * 0.5
    }

    #[test]
    fn a_tube_section_is_two_ordered_loops_on_the_plane() {
        let outer = solidcraft_geom::Loop2::polygon(&[
            solidcraft_geom::Vec2::new(0.0, 0.0),
            solidcraft_geom::Vec2::new(20.0, 0.0),
            solidcraft_geom::Vec2::new(20.0, 20.0),
            solidcraft_geom::Vec2::new(0.0, 20.0),
        ]);
        let hole = solidcraft_geom::Loop2::polygon(&[
            solidcraft_geom::Vec2::new(6.0, 6.0),
            solidcraft_geom::Vec2::new(14.0, 6.0),
            solidcraft_geom::Vec2::new(14.0, 14.0),
            solidcraft_geom::Vec2::new(6.0, 14.0),
        ])
        .reversed();
        let solid =
            crate::extrude_planar_regions(&solidcraft_geom::Plane::XY, &[solidcraft_geom::Region2 { outer, holes: vec![hole] }], 0.0, 10.0).unwrap();
        let tess = solid.tessellate(0.2).unwrap();
        let body = mesh_body(&tess.positions, &tess.triangles).unwrap();
        let origin = Vec3::new(0.0, 0.0, 5.0);
        let loops = mesh_section(&body, origin, Vec3::Z).unwrap();
        assert!(loops.len() >= 2, "outer and hole, got {}", loops.len());
        let mut area = 0.0;
        for ring in &loops {
            assert!(on_plane(ring, origin, Vec3::Z));
            assert!(loop_is_ordered(ring, origin, Vec3::Z), "{ring:?}");
            area += polygon_area_z(ring);
        }
        assert!((area - (400.0 + 64.0)).abs() < 1.0, "loop areas {area}");
        let (left, right) = (indexed_box(), {
            let moved = crate::transform(
                &crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap(),
                Vec3::new(30.0, 0.0, 0.0),
                Vec3::ZERO,
                Vec3::Z,
                0.0,
            )
            .unwrap();
            let tess = moved.tessellate(0.2).unwrap();
            mesh_body(&tess.positions, &tess.triangles).unwrap()
        });
        let both = mesh_combine(&left, &right).unwrap();
        let loops = mesh_section(&both, Vec3::new(0.0, 0.0, 5.0), Vec3::Z).unwrap();
        assert!(loops.len() >= 2, "two separated boxes, got {}", loops.len());
        assert!(loops.iter().all(|ring| on_plane(ring, Vec3::new(0.0, 0.0, 5.0), Vec3::Z)));
    }

    /// Feature: fusion360-feature-parity, Property 13: Face groups partition the mesh
    #[test]
    fn property_13_face_groups_partition_the_mesh() {
        let box_mesh = indexed_box();
        let groups = mesh_face_groups(&box_mesh).unwrap();
        let count = tri_count(&box_mesh);
        let sum: usize = groups.iter().map(|(_, n)| *n).sum();
        assert_eq!(sum, count);
        assert!(groups.iter().all(|(_, n)| *n > 0));
        assert_eq!(groups.len(), 6, "one group per flat face: {groups:?}");
        let (positions, triangles) = box_mesh.triangle_mesh().unwrap();
        let mut cases = 0usize;
        for i in 0..100 {
            let angle = i as f64 * 0.17;
            let (c, s) = (angle.cos(), angle.sin());
            let moved: Vec<Vec3> =
                positions.iter().map(|p| Vec3::new(p.x * c - p.y * s, p.x * s + p.y * c, p.z) + Vec3::new((i % 5) as f64, 0.0, 0.0)).collect();
            let body = mesh_body(&moved, triangles).unwrap();
            let groups = mesh_face_groups(&body).unwrap();
            let sum: usize = groups.iter().map(|(_, n)| *n).sum();
            assert_eq!(sum, tri_count(&body), "case {i}");
            assert!(groups.iter().all(|(_, n)| *n > 0));
            assert!(groups.len() <= tri_count(&body));
            cases += 1;
        }
        assert!(mesh_face_groups(&crate::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap()).is_err());
        assert!(cases >= 100, "{cases}");
    }

    #[test]
    fn remesh_hits_a_target_count_and_refuses_past_the_cap() {
        let sheet = grid_sheet(16, 8);
        let before = tri_count(&sheet);
        assert_eq!(before, 16 * 8 * 2);
        let target = 80usize;
        let coarse = mesh_remesh(&sheet, target).unwrap();
        let n = tri_count(&coarse);
        // Remesh lands on the target when topology allows; otherwise the nearest reachable count.
        assert!(n > 0 && n <= target && n < before, "coarsen {n} vs {target} (from {before})");
        assert!(coarse.triangle_mesh().unwrap().0.iter().all(|p| p.is_finite()));
        assert!(mesh_remesh(&sheet, before).unwrap().triangle_mesh().unwrap().1.len() == before);
        let fine = mesh_remesh(&sheet, before + 24).unwrap();
        let grown = tri_count(&fine);
        assert!(grown > before && grown <= before + 24, "refine {grown} from {before}");
        assert!(mesh_remesh(&sheet, 0).is_err());
        assert!(mesh_remesh(&sheet, MAX_MESH_TRIS + 1).is_err());
        assert!(mesh_remesh(&sheet, usize::MAX).is_err());
        assert!(mesh_remesh(&crate::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(), 10).is_err());
    }

    /// Feature: fusion360-feature-parity, Property 15: Reduction does not increase triangle count and cut keeps one side
    #[test]
    fn property_15_reduce_does_not_grow_and_cut_keeps_one_side() {
        let mut cases = 0usize;
        for i in 1..=100 {
            let (x, y, z) = (4.0 + (i % 6) as f64, 5.0 + (i % 4) as f64, 6.0 + (i % 5) as f64);
            let solid = crate::box_solid(Vec3::ZERO, Vec3::new(x, y, z)).unwrap();
            let tess = solid.tessellate(0.25).unwrap();
            let body = mesh_body(&tess.positions, &tess.triangles).unwrap();
            let before = tri_count(&body);
            let ratio = 0.15 + (i % 8) as f64 * 0.1;
            let reduced = mesh_reduce(&body, ratio.min(1.0)).unwrap_or_else(|e| panic!("reduce {i}: {e}"));
            let after = tri_count(&reduced);
            assert!(after <= before && after > 0, "reduce grew or emptied, case {i}: {after} vs {before}");
            let keep_x = if i % 2 == 0 { x * 0.75 } else { x * 0.25 };
            let origin = Vec3::new(x * 0.5, 0.0, 0.0);
            let keep = Vec3::new(keep_x, y * 0.5, z * 0.5);
            let cut = mesh_plane_cut(&body, origin, Vec3::X, keep).unwrap_or_else(|e| panic!("cut {i}: {e}"));
            let (pos, tris) = cut.triangle_mesh().unwrap();
            assert!(!tris.is_empty(), "cut emptied, case {i}");
            let keep_pos = keep_x >= origin.x;
            for p in pos {
                if keep_pos {
                    assert!(p.x >= origin.x - 1e-4, "cut leaked −X, case {i}: {p:?}");
                } else {
                    assert!(p.x <= origin.x + 1e-4, "cut leaked +X, case {i}: {p:?}");
                }
            }
            assert!(mesh_repair(&body).is_ok(), "repair {i}");
            cases += 1;
        }
        assert!(mesh_reduce(&indexed_box(), 0.0).is_err());
        assert!(mesh_reduce(&indexed_box(), 1.5).is_err());
        assert!(mesh_plane_cut(&indexed_box(), Vec3::ZERO, Vec3::ZERO, Vec3::X).is_err());
        assert!(cases >= 100, "{cases}");
    }

    /// Feature: fusion360-feature-parity, Property 16: Deviation is finite, non-negative, and ~0 for an exact rebuild
    #[test]
    fn property_16_deviation_is_finite_and_about_zero_for_an_exact_rebuild() {
        assert!((dist_point_triangle(Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, Vec3::X, Vec3::Y) - 1.0).abs() < 1e-9);
        assert!(dist_point_triangle(Vec3::ZERO, Vec3::ZERO, Vec3::X, Vec3::Y) < 1e-12);
        assert!((dist_point_triangle(Vec3::new(2.0, 0.0, 0.0), Vec3::ZERO, Vec3::X, Vec3::Y) - 1.0).abs() < 1e-9);
        let mut cases = 0usize;
        for i in 1..=100 {
            let scale = 1.0 + (i % 9) as f64;
            let shift = Vec3::new((i % 5) as f64, (i % 3) as f64 * 0.5, 0.0);
            let p = [shift, shift + Vec3::X * scale, shift + Vec3::new(0.5, 0.8, 0.0) * scale, shift + Vec3::new(0.4, 0.3, 0.9) * scale];
            let mesh = mesh_body(&p, &[[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]]).unwrap();
            let solid = mesh_faceted(&mesh).unwrap_or_else(|e| panic!("facet {i}: {e}"));
            assert_eq!(solid.face_count(), tri_count(&mesh), "faceted face count, case {i}");
            let (max, mean) = mesh_deviation(&solid, &mesh).unwrap_or_else(|e| panic!("dev {i}: {e}"));
            assert!(max.is_finite() && mean.is_finite() && max >= 0.0 && mean >= 0.0 && mean <= max + 1e-9, "{max} {mean}");
            assert!(max < 1e-3 && mean < 1e-3, "exact rebuild case {i}: max {max} mean {mean}");
            cases += 1;
        }
        let box_mesh = indexed_box();
        let faceted = mesh_faceted(&box_mesh).unwrap();
        assert_eq!(faceted.face_count(), tri_count(&box_mesh));
        let (max, mean) = mesh_deviation(&faceted, &box_mesh).unwrap();
        assert!(max < 1e-3 && mean >= 0.0, "box rebuild max {max} mean {mean}");
        assert!(mesh_deviation(&faceted, &crate::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap()).is_err());
        assert!(cases >= 100, "{cases}");
    }
}
