//! Pure selection and view-orientation math. The desktop shell presents the results; it does
//! not decide them.

use solidcraft_geom::{Vec2, Vec3};

/// What a pick refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    Face,
    Edge,
    Vertex,
    SketchCurve,
    Body,
    Component,
}

impl PickKind {
    pub fn parse(name: &str) -> Option<PickKind> {
        Some(match name {
            "face" | "faces" => PickKind::Face,
            "edge" | "edges" => PickKind::Edge,
            "vertex" | "vertices" => PickKind::Vertex,
            "sketch_curve" | "sketch_curves" => PickKind::SketchCurve,
            "body" | "bodies" => PickKind::Body,
            "component" | "components" => PickKind::Component,
            _ => return None,
        })
    }
}

/// A candidate under the cursor or inside a drag, already projected to the view plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pickable {
    pub id: u64,
    pub kind: PickKind,
    /// Distance along the pick ray, smaller in front.
    pub depth: f64,
    pub min: Vec2,
    pub max: Vec2,
    /// A point that must lie inside a lasso or paint stroke.
    pub anchor: Vec2,
}

/// Which kinds a pick may return. Every flag false means every kind is allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectFilter {
    pub faces: bool,
    pub edges: bool,
    pub vertices: bool,
    pub sketch_curves: bool,
    pub bodies: bool,
    pub components: bool,
}

impl SelectFilter {
    pub const ALL: SelectFilter =
        SelectFilter { faces: false, edges: false, vertices: false, sketch_curves: false, bodies: false, components: false };

    pub fn permits(self, kind: PickKind) -> bool {
        let any = self.faces || self.edges || self.vertices || self.sketch_curves || self.bodies || self.components;
        if !any {
            return true;
        }
        match kind {
            PickKind::Face => self.faces,
            PickKind::Edge => self.edges,
            PickKind::Vertex => self.vertices,
            PickKind::SketchCurve => self.sketch_curves,
            PickKind::Body => self.bodies,
            PickKind::Component => self.components,
        }
    }
}

/// How a single click resolves against overlapping candidates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    /// Nearest permitted entity of any kind.
    Entity,
    /// Prefer a body or component; otherwise the nearest permitted pick.
    Body,
    /// Frontmost after sorting by ray depth (depth-stack head).
    Occluded,
}

/// How a drag result combines with the selection that was already there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectCompose {
    Replace,
    Add,
    Subtract,
}

/// Sign of a window drag. Left to right encloses; right to left also crosses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragDir {
    LeftToRight,
    RightToLeft,
}

impl DragDir {
    pub fn from_dx(dx: f64) -> DragDir {
        if dx >= 0.0 { DragDir::LeftToRight } else { DragDir::RightToLeft }
    }
}

/// Shaded, shaded with visible edges, shaded with hidden edges, or wireframe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualStyle {
    Shaded,
    ShadedEdges,
    HiddenEdges,
    Wireframe,
}

impl VisualStyle {
    pub fn parse(name: &str) -> Option<VisualStyle> {
        Some(match name {
            "shaded" => VisualStyle::Shaded,
            "shaded_edges" | "shaded-edges" => VisualStyle::ShadedEdges,
            "hidden_edges" | "hidden-edges" => VisualStyle::HiddenEdges,
            "wireframe" => VisualStyle::Wireframe,
            _ => return None,
        })
    }
}

fn finite_box(p: &Pickable) -> bool {
    p.min.x.is_finite()
        && p.min.y.is_finite()
        && p.max.x.is_finite()
        && p.max.y.is_finite()
        && p.anchor.x.is_finite()
        && p.anchor.y.is_finite()
        && p.depth.is_finite()
}

fn enclosed(p: &Pickable, min: Vec2, max: Vec2) -> bool {
    p.min.x >= min.x && p.min.y >= min.y && p.max.x <= max.x && p.max.y <= max.y
}

fn crosses(p: &Pickable, min: Vec2, max: Vec2) -> bool {
    p.min.x <= max.x && p.max.x >= min.x && p.min.y <= max.y && p.max.y >= min.y
}

fn filtered(candidates: &[Pickable], filter: SelectFilter) -> impl Iterator<Item = &Pickable> {
    candidates.iter().filter(move |p| finite_box(p) && filter.permits(p.kind))
}

/// Window selection. Left-to-right keeps candidates fully inside the box; right-to-left also
/// keeps candidates the box crosses. Order follows the input.
pub fn select_window(min: Vec2, max: Vec2, dir: DragDir, candidates: &[Pickable], filter: SelectFilter) -> Vec<Pickable> {
    if !(min.x.is_finite() && min.y.is_finite() && max.x.is_finite() && max.y.is_finite()) {
        return Vec::new();
    }
    let (lo, hi) = (Vec2::new(min.x.min(max.x), min.y.min(max.y)), Vec2::new(min.x.max(max.x), min.y.max(max.y)));
    filtered(candidates, filter)
        .filter(|p| match dir {
            DragDir::LeftToRight => enclosed(p, lo, hi),
            DragDir::RightToLeft => crosses(p, lo, hi),
        })
        .copied()
        .collect()
}

fn inside_polygon(point: Vec2, polygon: &[Vec2]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[j], polygon[i]);
        let cross = (b.y > point.y) != (a.y > point.y);
        if cross {
            let x = a.x + (point.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if point.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Freeform selection: the anchor lies inside the closed drag polygon.
pub fn select_freeform(lasso: &[Vec2], candidates: &[Pickable], filter: SelectFilter) -> Vec<Pickable> {
    if lasso.len() > 4_096 || lasso.iter().any(|p| !(p.x.is_finite() && p.y.is_finite())) {
        return Vec::new();
    }
    filtered(candidates, filter).filter(|p| inside_polygon(p.anchor, lasso)).copied().collect()
}

fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let len2 = ab.x * ab.x + ab.y * ab.y;
    if len2 < 1e-12 {
        return (p - a).x.hypot((p - a).y);
    }
    let t = ((p - a).x * ab.x + (p - a).y * ab.y) / len2;
    let t = t.clamp(0.0, 1.0);
    let q = Vec2::new(a.x + ab.x * t, a.y + ab.y * t);
    (p - q).x.hypot((p - q).y)
}

/// Paint selection: anchors within `radius` of the stroked path. Order follows the input.
pub fn select_paint(path: &[Vec2], radius: f64, candidates: &[Pickable], filter: SelectFilter) -> Vec<Pickable> {
    if !(radius.is_finite() && (0.0..1.0e6).contains(&radius)) || path.len() > 8_192 || path.iter().any(|p| !(p.x.is_finite() && p.y.is_finite())) {
        return Vec::new();
    }
    if path.is_empty() {
        return Vec::new();
    }
    filtered(candidates, filter)
        .filter(|p| {
            path.windows(2).any(|w| dist_to_segment(p.anchor, w[0], w[1]) <= radius)
                || path.first().is_some_and(|a| (p.anchor - *a).x.hypot((p.anchor - *a).y) <= radius)
        })
        .copied()
        .collect()
}

/// Candidates the ray hits, nearest first. Nothing is hidden.
pub fn depth_stack(candidates: &[Pickable], filter: SelectFilter) -> Vec<Pickable> {
    let mut out: Vec<Pickable> = filtered(candidates, filter).copied().collect();
    out.sort_by(|a, b| a.depth.partial_cmp(&b.depth).unwrap_or(std::cmp::Ordering::Equal).then(a.id.cmp(&b.id)));
    out
}

/// Resolve a click against candidates. Deterministic: same inputs → same pick. Returns only
/// kinds the filter permits; `None` when nothing qualifies.
pub fn pick(candidates: &[Pickable], mode: SelectMode, filter: SelectFilter) -> Option<Pickable> {
    let stack = depth_stack(candidates, filter);
    match mode {
        SelectMode::Entity | SelectMode::Occluded => stack.into_iter().next(),
        SelectMode::Body => {
            if let Some(body) = stack.iter().copied().find(|p| matches!(p.kind, PickKind::Body | PickKind::Component)) {
                Some(body)
            } else {
                stack.into_iter().next()
            }
        }
    }
}

/// Apply replace, add, or subtract. Ids stay unique and keep the previous order, with new hits
/// appended.
pub fn compose_selection(previous: &[u64], hits: &[u64], mode: SelectCompose) -> Vec<u64> {
    match mode {
        SelectCompose::Replace => {
            let mut out = Vec::new();
            for id in hits {
                if !out.contains(id) {
                    out.push(*id);
                }
            }
            out
        }
        SelectCompose::Add => {
            let mut out = previous.to_vec();
            for id in hits {
                if !out.contains(id) {
                    out.push(*id);
                }
            }
            out
        }
        SelectCompose::Subtract => previous.iter().copied().filter(|id| !hits.contains(id)).collect(),
    }
}

/// The 26 axis and diagonal directions a view widget can face, as integer steps in −1, 0, 1.
pub fn view_directions() -> [[i8; 3]; 26] {
    let mut out = [[0i8; 3]; 26];
    let mut n = 0;
    for z in -1..=1 {
        for y in -1..=1 {
            for x in -1..=1 {
                if x == 0 && y == 0 && z == 0 {
                    continue;
                }
                if let Some(slot) = out.get_mut(n) {
                    *slot = [x, y, z];
                }
                n += 1;
            }
        }
    }
    out
}

/// Unit vector from the target toward the eye for one of the 26 directions.
pub fn eye_direction(dir: [i8; 3]) -> Option<Vec3> {
    if dir.iter().any(|c| !(-1..=1).contains(c)) || dir == [0, 0, 0] {
        return None;
    }
    Vec3::new(dir[0] as f64, dir[1] as f64, dir[2] as f64).normalized()
}

/// Screen-up for that eye direction. World +Z stays upward except when the eye is on the Z axis,
/// where up is ±Y so the view stays square.
pub fn view_up(dir: [i8; 3]) -> Option<Vec3> {
    let eye = eye_direction(dir)?;
    if eye.x.abs() < 1e-9 && eye.y.abs() < 1e-9 {
        return Some(Vec3::new(0.0, eye.z.signum(), 0.0));
    }
    (Vec3::Z - eye * eye.dot(Vec3::Z)).normalized()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: u64, kind: PickKind, depth: f64, min: Vec2, max: Vec2) -> Pickable {
        Pickable { id, kind, depth, min, max, anchor: Vec2::new((min.x + max.x) * 0.5, (min.y + max.y) * 0.5) }
    }

    #[test]
    fn window_drag_direction_changes_what_is_enclosed() {
        let inside = cand(1, PickKind::Face, 1.0, Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0));
        let crossing = cand(2, PickKind::Face, 2.0, Vec2::new(0.0, 0.0), Vec2::new(3.0, 1.5));
        let c = [inside, crossing];
        let box_min = Vec2::new(0.5, 0.5);
        let box_max = Vec2::new(2.5, 2.5);
        let enclosed = select_window(box_min, box_max, DragDir::LeftToRight, &c, SelectFilter::ALL);
        assert_eq!(enclosed.len(), 1);
        assert_eq!(enclosed[0].id, 1);
        let crossed = select_window(box_max, box_min, DragDir::RightToLeft, &c, SelectFilter::ALL);
        assert_eq!(crossed.len(), 2);
    }

    #[test]
    fn filter_drops_other_kinds_and_depth_is_sorted() {
        let face = cand(1, PickKind::Face, 3.0, Vec2::ZERO, Vec2::new(1.0, 1.0));
        let edge = cand(2, PickKind::Edge, 1.0, Vec2::ZERO, Vec2::new(1.0, 1.0));
        let filter = SelectFilter { faces: true, edges: false, vertices: false, sketch_curves: false, bodies: false, components: false };
        let stack = depth_stack(&[face, edge], filter);
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0].id, 1);
        let both = depth_stack(&[face, edge], SelectFilter::ALL);
        assert_eq!(both[0].id, 2);
        assert_eq!(both[1].id, 1);
    }

    #[test]
    fn lasso_and_paint_are_deterministic() {
        let a = cand(1, PickKind::Body, 0.0, Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0));
        let b = cand(2, PickKind::Body, 0.0, Vec2::new(5.0, 5.0), Vec2::new(6.0, 6.0));
        let lasso = [Vec2::new(0.0, 0.0), Vec2::new(3.0, 0.0), Vec2::new(3.0, 3.0), Vec2::new(0.0, 3.0)];
        let hit = select_freeform(&lasso, &[b, a], SelectFilter::ALL);
        assert_eq!(hit.iter().map(|p| p.id).collect::<Vec<_>>(), vec![1]);
        let paint = select_paint(&[Vec2::new(5.5, 0.0), Vec2::new(5.5, 6.0)], 0.6, &[a, b], SelectFilter::ALL);
        assert_eq!(paint.iter().map(|p| p.id).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn compose_modes_and_z_up_views() {
        assert_eq!(compose_selection(&[1, 2], &[2, 3], SelectCompose::Add), vec![1, 2, 3]);
        assert_eq!(compose_selection(&[1, 2], &[2], SelectCompose::Subtract), vec![1]);
        assert_eq!(compose_selection(&[1], &[4, 4], SelectCompose::Replace), vec![4]);
        assert_eq!(view_directions().len(), 26);
        for dir in view_directions() {
            let eye = eye_direction(dir).unwrap();
            let up = view_up(dir).unwrap();
            assert!(up.dot(eye).abs() < 1e-6, "{dir:?}");
            if dir[0] == 0 && dir[1] == 0 {
                assert!(up.z.abs() < 1e-9);
            } else {
                assert!(up.z > 0.0, "{dir:?} up {up:?}");
            }
        }
    }

    #[test]
    fn pick_respects_mode_and_filter() {
        let face = cand(1, PickKind::Face, 2.0, Vec2::ZERO, Vec2::new(1.0, 1.0));
        let body = cand(2, PickKind::Body, 3.0, Vec2::ZERO, Vec2::new(1.0, 1.0));
        let edge = cand(3, PickKind::Edge, 1.0, Vec2::ZERO, Vec2::new(1.0, 1.0));
        let all = [face, body, edge];
        assert_eq!(pick(&all, SelectMode::Entity, SelectFilter::ALL).map(|p| p.id), Some(3));
        assert_eq!(pick(&all, SelectMode::Occluded, SelectFilter::ALL).map(|p| p.id), Some(3));
        assert_eq!(pick(&all, SelectMode::Body, SelectFilter::ALL).map(|p| p.id), Some(2));
        let faces_only = SelectFilter { faces: true, edges: false, vertices: false, sketch_curves: false, bodies: false, components: false };
        assert_eq!(pick(&all, SelectMode::Entity, faces_only).map(|p| p.id), Some(1));
        assert_eq!(pick(&all, SelectMode::Body, faces_only).map(|p| p.id), Some(1));
    }

    /// Feature: fusion360-feature-parity, Property 19: Selection respects mode and filter, deterministically
    #[test]
    fn property_19_selection_is_deterministic() {
        let kinds = [PickKind::Face, PickKind::Edge, PickKind::Vertex, PickKind::Body, PickKind::Component, PickKind::SketchCurve];
        for n in 0..120u64 {
            let mut c = Vec::new();
            for i in 0..6 {
                let k = kinds[usize::try_from(i).unwrap_or(0) % kinds.len()];
                let d = ((n + i) % 7) as f64;
                c.push(cand(n * 10 + i, k, d, Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)));
            }
            let filter = if n % 3 == 0 {
                SelectFilter::ALL
            } else {
                SelectFilter { faces: n % 2 == 0, edges: n % 3 == 0, vertices: n % 5 == 0, sketch_curves: false, bodies: true, components: false }
            };
            let a = pick(&c, SelectMode::Entity, filter);
            let b = pick(&c, SelectMode::Entity, filter);
            assert_eq!(a, b);
            let stack = depth_stack(&c, filter);
            if let Some(front) = stack.first() {
                assert_eq!(pick(&c, SelectMode::Occluded, filter).map(|p| p.id), Some(front.id));
            }
            let win = select_window(Vec2::new(-0.5, -0.5), Vec2::new(1.5, 1.5), DragDir::LeftToRight, &c, filter);
            let win2 = select_window(Vec2::new(-0.5, -0.5), Vec2::new(1.5, 1.5), DragDir::LeftToRight, &c, filter);
            assert_eq!(win.iter().map(|p| p.id).collect::<Vec<_>>(), win2.iter().map(|p| p.id).collect::<Vec<_>>());
        }
    }

    /// Feature: fusion360-feature-parity, Property 25: Drag selection is deterministic
    #[test]
    fn property_25_drag_selection_composes() {
        let a = cand(1, PickKind::Face, 1.0, Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0));
        let b = cand(2, PickKind::Face, 2.0, Vec2::new(2.0, 2.0), Vec2::new(3.0, 3.0));
        let c = [a, b];
        for _ in 0..100 {
            let hits = select_window(Vec2::new(-0.1, -0.1), Vec2::new(1.1, 1.1), DragDir::LeftToRight, &c, SelectFilter::ALL);
            let ids: Vec<u64> = hits.iter().map(|p| p.id).collect();
            assert_eq!(compose_selection(&[], &ids, SelectCompose::Replace), ids);
            assert_eq!(compose_selection(&[2], &ids, SelectCompose::Add), vec![2, 1]);
            assert_eq!(compose_selection(&[1, 2], &ids, SelectCompose::Subtract), vec![2]);
        }
    }

    /// Feature: fusion360-feature-parity, Property 26: The depth chooser lists candidates ordered by ray depth
    #[test]
    fn property_26_depth_stack_orders_and_hides_nothing() {
        for n in 0..100u64 {
            let c = [
                cand(1, PickKind::Face, (n % 5) as f64 + 0.1, Vec2::ZERO, Vec2::new(1.0, 1.0)),
                cand(2, PickKind::Edge, (n % 7) as f64 + 0.2, Vec2::ZERO, Vec2::new(1.0, 1.0)),
                cand(3, PickKind::Body, (n % 3) as f64 + 0.3, Vec2::ZERO, Vec2::new(1.0, 1.0)),
            ];
            let stack = depth_stack(&c, SelectFilter::ALL);
            assert_eq!(stack.len(), 3, "depth chooser must not hide bodies");
            for w in stack.windows(2) {
                assert!(w[0].depth <= w[1].depth);
            }
        }
    }
}
