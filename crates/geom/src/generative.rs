//! A preserve/obstacle candidate reduced to one axis-aligned box.
//!
//! The preserve regions are united. A mass objective keeps 60% of the width,
//! anchored at the minimum Y. Each obstacle then cuts away the smaller overlap,
//! leaving the larger remaining box, so the candidate stays inside the preserve
//! bounds and does not overlap an obstacle. This is SolidCraft's own section
//! reduction ([`METHOD`]). It is not a topology optimiser and it does not read
//! another project's sources.

/// Recorded on every candidate this module produces.
pub const METHOD: &str = "solidcraft-section-reduction";

/// Width kept by a mass objective, as a fraction of the preserve width.
pub const MASS_WIDTH_FRACTION: f64 = 0.6;

/// Most preserve or obstacle boxes one study may pass.
pub const MAX_REGIONS: usize = 32;

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

/// The one box a study can promote.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub method: &'static str,
    pub origin: [f64; 3],
    pub size: [f64; 3],
}

/// Build the candidate. An empty preserve list, a non-finite box, a size past the
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
}
