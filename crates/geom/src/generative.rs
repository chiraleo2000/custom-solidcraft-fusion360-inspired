//! Preserve/obstacle candidates for generative design.
//!
//! [`section_reduction`] keeps one axis-aligned box (mass trims width, then
//! obstacles cut faces). [`structured_lattice`] goes beyond a single AABB: it
//! fills the preserve envelope with a capped regular cell grid, drops cells that
//! hit obstacles, and thins the rest for a mass objective into an edge-and-strut
//! lattice. Both methods are SolidCraft's own; neither reads another project's
//! sources. Recorded names are [`METHOD`] and [`LATTICE_METHOD`].

/// Recorded on every section-reduction candidate.
pub const METHOD: &str = "solidcraft-section-reduction";

/// Recorded on every structured-lattice candidate.
pub const LATTICE_METHOD: &str = "solidcraft-structured-lattice";

/// Width kept by a mass objective, as a fraction of the preserve width.
pub const MASS_WIDTH_FRACTION: f64 = 0.6;

/// Most preserve or obstacle boxes one study may pass.
pub const MAX_REGIONS: usize = 32;

/// Most divisions along one lattice axis.
pub const MAX_LATTICE_DIV: usize = 4;

/// Most lattice cells a candidate may keep.
pub const MAX_LATTICE_CELLS: usize = 64;

const MAX_DIMENSION: f64 = 1.0e6;

/// An axis-aligned box, millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxRegion {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// What the candidate is asked to favour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Objective {
    /// Keep the preserve envelope, minus obstacles.
    Stiffness,
    /// Keep [`MASS_WIDTH_FRACTION`] of the width, then remove obstacles.
    Mass,
}

/// The one box a section-reduction study can promote.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub method: &'static str,
    pub origin: [f64; 3],
    pub size: [f64; 3],
}

/// One cell of a structured lattice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatticeCell {
    pub origin: [f64; 3],
    pub size: [f64; 3],
}

/// A multi-cell lattice candidate. `origin`/`size` are the AABB of the kept cells.
#[derive(Clone, Debug, PartialEq)]
pub struct LatticeCandidate {
    pub method: &'static str,
    pub origin: [f64; 3],
    pub size: [f64; 3],
    pub cells: Vec<LatticeCell>,
}

/// Build the single-box candidate. An empty preserve list, a non-finite box, a size past the
/// cap, or an obstacle set that removes every bit of material is an error.
pub fn section_reduction(preserve: &[BoxRegion], obstacle: &[BoxRegion], objective: Objective) -> Result<Candidate, &'static str> {
    if preserve.is_empty() {
        return Err("a generative study needs a preserve body");
    }
    if preserve.len() > MAX_REGIONS || obstacle.len() > MAX_REGIONS {
        return Err("the study exceeds the size cap");
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for region in preserve {
        let (lo, hi) = finite_box(*region)?;
        for axis in 0..3 {
            if let Some(slot) = min.get_mut(axis) {
                *slot = slot.min(component(lo, axis));
            }
            if let Some(slot) = max.get_mut(axis) {
                *slot = slot.max(component(hi, axis));
            }
        }
    }
    if objective == Objective::Mass {
        let width = component(max, 1) - component(min, 1);
        let kept = width * MASS_WIDTH_FRACTION;
        if !(kept.is_finite() && kept > 1e-6) {
            return Err("the preserve region has no usable width");
        }
        if let Some(slot) = max.get_mut(1) {
            *slot = component(min, 1) + kept;
        }
    }
    for region in obstacle {
        let (lo, hi) = finite_box(*region)?;
        (min, max) = cut_obstacle(min, max, lo, hi)?;
    }
    let mut size = [0.0; 3];
    for axis in 0..3 {
        let span = component(max, axis) - component(min, axis);
        if !(span.is_finite() && span > 1e-6) {
            return Err("the obstacles leave no material");
        }
        if let Some(slot) = size.get_mut(axis) {
            *slot = span;
        }
    }
    Ok(Candidate { method: METHOD, origin: min, size })
}

/// Divisions per axis for a lattice study, capped at [`MAX_LATTICE_DIV`].
pub fn lattice_divisions(wanted: usize) -> usize {
    wanted.clamp(1, MAX_LATTICE_DIV)
}

/// Build a capped structured lattice inside the preserve envelope.
///
/// Stiffness keeps every cell that survives obstacles. Mass keeps only the grid
/// edges plus a mid-plane strut along X — a full shell (or shell+strut) collapses
/// to a near-solid fill at the capped division counts, so mass must be thinner.
pub fn structured_lattice(
    preserve: &[BoxRegion],
    obstacle: &[BoxRegion],
    objective: Objective,
    divisions: usize,
) -> Result<LatticeCandidate, &'static str> {
    if preserve.is_empty() {
        return Err("a generative study needs a preserve body");
    }
    if preserve.len() > MAX_REGIONS || obstacle.len() > MAX_REGIONS {
        return Err("the study exceeds the size cap");
    }
    let n = lattice_divisions(divisions);
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for region in preserve {
        let (lo, hi) = finite_box(*region)?;
        for axis in 0..3 {
            if let Some(slot) = min.get_mut(axis) {
                *slot = slot.min(component(lo, axis));
            }
            if let Some(slot) = max.get_mut(axis) {
                *slot = slot.max(component(hi, axis));
            }
        }
    }
    let mut pitch = [0.0; 3];
    for axis in 0..3 {
        let span = component(max, axis) - component(min, axis);
        if !(span.is_finite() && span > 1e-6) {
            return Err("the preserve region has no usable width");
        }
        if let Some(slot) = pitch.get_mut(axis) {
            *slot = span / n as f64;
        }
    }
    let obstacles: Vec<_> = obstacle.iter().copied().map(finite_box).collect::<Result<Vec<_>, _>>()?;
    let mut cells = Vec::new();
    let mid = n / 2;
    for iz in 0..n {
        for iy in 0..n {
            for ix in 0..n {
                let origin =
                    [component(min, 0) + ix as f64 * pitch[0], component(min, 1) + iy as f64 * pitch[1], component(min, 2) + iz as f64 * pitch[2]];
                let size = pitch;
                let cell_max = [origin[0] + size[0], origin[1] + size[1], origin[2] + size[2]];
                let center = [origin[0] + size[0] * 0.5, origin[1] + size[1] * 0.5, origin[2] + size[2] * 0.5];
                if obstacles.iter().any(|&(lo, hi)| point_in_box(center, lo, hi)) {
                    continue;
                }
                let on_x = ix == 0 || ix + 1 == n;
                let on_y = iy == 0 || iy + 1 == n;
                let on_z = iz == 0 || iz + 1 == n;
                // A cell on two or more faces is an edge/corner. Face interiors are
                // dropped for mass; shell||strut equals a solid fill for n<=3.
                let edge = (on_x as u8) + (on_y as u8) + (on_z as u8) >= 2;
                let strut = iy == mid && iz == mid;
                let keep = match objective {
                    Objective::Stiffness => true,
                    Objective::Mass => edge || strut,
                };
                if !keep {
                    continue;
                }
                if cells.len() >= MAX_LATTICE_CELLS {
                    return Err("the study exceeds the size cap");
                }
                let _ = cell_max;
                cells.push(LatticeCell { origin, size });
            }
        }
    }
    if cells.is_empty() {
        return Err("the obstacles leave no material");
    }
    let mut lo = cells[0].origin;
    let mut hi = [cells[0].origin[0] + cells[0].size[0], cells[0].origin[1] + cells[0].size[1], cells[0].origin[2] + cells[0].size[2]];
    for cell in cells.iter().skip(1) {
        for axis in 0..3 {
            if let Some(slot) = lo.get_mut(axis) {
                *slot = slot.min(component(cell.origin, axis));
            }
            let end = component(cell.origin, axis) + component(cell.size, axis);
            if let Some(slot) = hi.get_mut(axis) {
                *slot = slot.max(end);
            }
        }
    }
    let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    if size.iter().any(|span| !(span.is_finite() && *span > 1e-6)) {
        return Err("the obstacles leave no material");
    }
    Ok(LatticeCandidate { method: LATTICE_METHOD, origin: lo, size, cells })
}

fn component(value: [f64; 3], axis: usize) -> f64 {
    match axis {
        0 => value[0],
        1 => value[1],
        _ => value[2],
    }
}

fn set_component(value: &mut [f64; 3], axis: usize, next: f64) {
    match axis {
        0 => value[0] = next,
        1 => value[1] = next,
        _ => value[2] = next,
    }
}

fn point_in_box(point: [f64; 3], min: [f64; 3], max: [f64; 3]) -> bool {
    (0..3).all(|axis| component(point, axis) >= component(min, axis) - 1e-9 && component(point, axis) <= component(max, axis) + 1e-9)
}

fn finite_box(region: BoxRegion) -> Result<([f64; 3], [f64; 3]), &'static str> {
    for axis in 0..3 {
        let lo = component(region.min, axis);
        let hi = component(region.max, axis);
        if !(lo.is_finite() && hi.is_finite()) {
            return Err("a region is not finite");
        }
        if hi - lo <= 1e-9 {
            return Err("a region has no size");
        }
        if (hi - lo) > MAX_DIMENSION || lo.abs() > MAX_DIMENSION || hi.abs() > MAX_DIMENSION {
            return Err("the study exceeds the size cap");
        }
    }
    Ok((region.min, region.max))
}

fn overlaps(min: [f64; 3], max: [f64; 3], other_min: [f64; 3], other_max: [f64; 3]) -> bool {
    (0..3).all(|axis| component(min, axis) < component(other_max, axis) && component(max, axis) > component(other_min, axis))
}

/// Merge face-adjacent lattice cells into maximal axis-aligned bars.
///
/// Neighbouring grid cells share a face with no overlap; promoting each cell as
/// its own box then fails the kernel join. Coalescing first cuts the number of
/// joins and leaves only corner/edge contacts between bars.
pub fn coalesce_lattice_cells(cells: &[LatticeCell]) -> Vec<LatticeCell> {
    let mut pieces: Vec<LatticeCell> = cells.to_vec();
    let mut merged = true;
    while merged {
        merged = false;
        let mut next = Vec::with_capacity(pieces.len());
        let mut used = vec![false; pieces.len()];
        for i in 0..pieces.len() {
            if used.get(i).copied().unwrap_or(true) {
                continue;
            }
            let Some(used_i) = used.get_mut(i) else {
                continue;
            };
            *used_i = true;
            let Some(mut current) = pieces.get(i).copied() else {
                continue;
            };
            loop {
                let mut grew = false;
                for j in 0..pieces.len() {
                    if used.get(j).copied().unwrap_or(true) {
                        continue;
                    }
                    let Some(other) = pieces.get(j).copied() else {
                        continue;
                    };
                    if let Some(combined) = try_merge_face_adjacent(current, other) {
                        if let Some(slot) = used.get_mut(j) {
                            *slot = true;
                        }
                        current = combined;
                        grew = true;
                        merged = true;
                    }
                }
                if !grew {
                    break;
                }
            }
            next.push(current);
        }
        pieces = next;
    }
    pieces
}

fn try_merge_face_adjacent(a: LatticeCell, b: LatticeCell) -> Option<LatticeCell> {
    const TOL: f64 = 1e-9;
    let a_max = [a.origin[0] + a.size[0], a.origin[1] + a.size[1], a.origin[2] + a.size[2]];
    let b_max = [b.origin[0] + b.size[0], b.origin[1] + b.size[1], b.origin[2] + b.size[2]];
    for axis in 0..3 {
        let a_lo = component(a.origin, axis);
        let a_hi = component(a_max, axis);
        let b_lo = component(b.origin, axis);
        let b_hi = component(b_max, axis);
        let touch = (a_hi - b_lo).abs() <= TOL || (b_hi - a_lo).abs() <= TOL;
        if !touch {
            continue;
        }
        let mut ok = true;
        for other in 0..3 {
            if other == axis {
                continue;
            }
            if (component(a.origin, other) - component(b.origin, other)).abs() > TOL
                || (component(a_max, other) - component(b_max, other)).abs() > TOL
            {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }
        let origin = [a.origin[0].min(b.origin[0]), a.origin[1].min(b.origin[1]), a.origin[2].min(b.origin[2])];
        let max = [a_max[0].max(b_max[0]), a_max[1].max(b_max[1]), a_max[2].max(b_max[2])];
        let size = [max[0] - origin[0], max[1] - origin[1], max[2] - origin[2]];
        if size.iter().any(|span| !(span.is_finite() && *span > TOL)) {
            return None;
        }
        return Some(LatticeCell { origin, size });
    }
    None
}

fn span_volume(min: [f64; 3], max: [f64; 3]) -> f64 {
    let mut volume = 1.0;
    for axis in 0..3 {
        let span = component(max, axis) - component(min, axis);
        if !(span.is_finite() && span > 1e-6) {
            return 0.0;
        }
        volume *= span;
    }
    if volume.is_finite() { volume } else { 0.0 }
}

/// Drop the overlap by trimming one face. The largest remaining box wins.
fn cut_obstacle(min: [f64; 3], max: [f64; 3], obstacle_min: [f64; 3], obstacle_max: [f64; 3]) -> Result<([f64; 3], [f64; 3]), &'static str> {
    if !overlaps(min, max, obstacle_min, obstacle_max) {
        return Ok((min, max));
    }
    let mut best: Option<([f64; 3], [f64; 3])> = None;
    let mut best_volume = 0.0;
    for axis in 0..3 {
        let mut high = max;
        set_component(&mut high, axis, component(obstacle_min, axis));
        let high_volume = span_volume(min, high);
        if high_volume > best_volume {
            best = Some((min, high));
            best_volume = high_volume;
        }
        let mut low = min;
        set_component(&mut low, axis, component(obstacle_max, axis));
        let low_volume = span_volume(low, max);
        if low_volume > best_volume {
            best = Some((low, max));
            best_volume = low_volume;
        }
    }
    best.ok_or("the obstacles leave no material")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_at(size: [f64; 3]) -> BoxRegion {
        BoxRegion { min: [0.0, 0.0, 0.0], max: size }
    }

    #[test]
    fn mass_keeps_sixty_percent_of_the_width_and_stays_inside() {
        let preserve = box_at([20.0, 10.0, 8.0]);
        let candidate = section_reduction(&[preserve], &[], Objective::Mass).expect("mass");
        assert_eq!(candidate.method, METHOD);
        assert!((candidate.size[0] - 20.0).abs() < 1e-9);
        assert!((candidate.size[1] - 6.0).abs() < 1e-9);
        assert!((candidate.size[2] - 8.0).abs() < 1e-9);
        assert_eq!(candidate.origin, [0.0, 0.0, 0.0]);
        let full = section_reduction(&[preserve], &[], Objective::Stiffness).expect("stiffness");
        assert!((full.size[1] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn an_obstacle_is_cut_out_and_the_candidate_stays_in_the_preserve_box() {
        for i in 1..=100 {
            let width = 10.0 + i as f64;
            let preserve = BoxRegion { min: [0.0, 0.0, 0.0], max: [40.0, width, 12.0] };
            let obstacle = BoxRegion { min: [30.0, -1.0, -1.0], max: [48.0, width + 1.0, 20.0] };
            let candidate = section_reduction(&[preserve], &[obstacle], Objective::Mass).expect("cut");
            assert_eq!(candidate.method, METHOD);
            assert!((candidate.size[1] - width * MASS_WIDTH_FRACTION).abs() < 1e-6, "{i} {}", candidate.size[1]);
            assert!((candidate.origin[0] + candidate.size[0] - 30.0).abs() < 1e-6, "{i} length {}", candidate.size[0]);
            assert!(candidate.origin[0] >= preserve.min[0] - 1e-9);
            assert!(candidate.origin[1] >= preserve.min[1] - 1e-9);
            assert!(candidate.origin[0] + candidate.size[0] <= preserve.max[0] + 1e-9);
            assert!(candidate.origin[1] + candidate.size[1] <= preserve.max[1] + 1e-9);
            let max = [candidate.origin[0] + candidate.size[0], candidate.origin[1] + candidate.size[1], candidate.origin[2] + candidate.size[2]];
            assert!(!overlaps(candidate.origin, max, obstacle.min, obstacle.max), "{i}");
        }
    }

    #[test]
    fn a_covering_obstacle_and_a_non_finite_region_are_refused() {
        let preserve = box_at([10.0, 10.0, 10.0]);
        let cover = BoxRegion { min: [-1.0, -1.0, -1.0], max: [11.0, 11.0, 11.0] };
        assert!(section_reduction(&[preserve], &[cover], Objective::Stiffness).is_err());
        let bad = BoxRegion { min: [0.0, 0.0, 0.0], max: [f64::NAN, 1.0, 1.0] };
        assert!(section_reduction(&[bad], &[], Objective::Mass).is_err());
        assert!(section_reduction(&[], &[], Objective::Mass).is_err());
        let huge = BoxRegion { min: [0.0, 0.0, 0.0], max: [MAX_DIMENSION * 2.0, 1.0, 1.0] };
        assert!(section_reduction(&[huge], &[], Objective::Mass).is_err());
    }

    #[test]
    fn face_adjacent_cells_coalesce_into_one_bar() {
        let a = LatticeCell { origin: [0.0, 0.0, 0.0], size: [10.0, 2.0, 2.0] };
        let b = LatticeCell { origin: [10.0, 0.0, 0.0], size: [10.0, 2.0, 2.0] };
        let c = LatticeCell { origin: [0.0, 0.0, 2.0], size: [2.0, 2.0, 10.0] };
        let merged = coalesce_lattice_cells(&[a, b, c]);
        assert_eq!(merged.len(), 2, "{merged:?}");
        assert!(merged.iter().any(|cell| (cell.size[0] - 20.0).abs() < 1e-9 && (cell.size[1] - 2.0).abs() < 1e-9));
        assert!(merged.iter().any(|cell| (cell.size[2] - 10.0).abs() < 1e-9 && (cell.size[0] - 2.0).abs() < 1e-9));
    }

    #[test]
    fn a_mass_lattice_is_thinner_than_a_stiffness_fill_and_avoids_obstacles() {
        let preserve = box_at([30.0, 30.0, 30.0]);
        let obstacle = BoxRegion { min: [20.0, -1.0, -1.0], max: [40.0, 40.0, 40.0] };
        let stiff = structured_lattice(&[preserve], &[obstacle], Objective::Stiffness, 3).expect("stiff");
        let mass = structured_lattice(&[preserve], &[obstacle], Objective::Mass, 3).expect("mass");
        assert_eq!(stiff.method, LATTICE_METHOD);
        assert_eq!(mass.method, LATTICE_METHOD);
        assert!(mass.cells.len() < stiff.cells.len(), "{} vs {}", mass.cells.len(), stiff.cells.len());
        assert!(!stiff.cells.is_empty());
        for cell in stiff.cells.iter().chain(mass.cells.iter()) {
            let center = [cell.origin[0] + cell.size[0] * 0.5, cell.origin[1] + cell.size[1] * 0.5, cell.origin[2] + cell.size[2] * 0.5];
            assert!(!point_in_box(center, obstacle.min, obstacle.max), "{cell:?}");
            assert!(cell.origin[0] + cell.size[0] <= 20.0 + 1e-6, "{cell:?}");
        }
        assert!(stiff.origin[0] + stiff.size[0] <= 20.0 + 1e-6, "{stiff:?}");
    }
}
