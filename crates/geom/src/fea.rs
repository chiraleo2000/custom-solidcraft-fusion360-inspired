//! Linear static beam stiffness, written for SolidCraft.
//!
//! The element is the standard 2-node Hermitian Euler-Bernoulli beam: transverse
//! deflection along Z and rotation about Y, length along X. The stiffness matrix is
//! assembled here and solved by Gaussian elimination with partial pivoting. Nothing
//! in this module is taken from another solver, and it does not link a FEA library.
//!
//! A general hex or tet mesh is not this method. The recorded method name is
//! [`METHOD`].

/// Recorded on every result this module produces.
pub const METHOD: &str = "solidcraft-beam-stiffness";

/// Most elements a study may assemble. Past this the caller must refuse.
pub const MAX_ELEMENTS: usize = 32;

/// Largest beam size accepted, in millimetres.
pub const MAX_DIMENSION: f64 = 1.0e6;

/// Typical room-temperature Young's moduli in N/mm².
///
/// These are SolidCraft's own round values for the named materials. They are not a
/// vendor material library. `Default` uses the same 200000 N/mm² the static command
/// has always used when no modulus is given.
const MODULI: &[(&str, f64)] = &[
    ("default", 200_000.0),
    ("steel", 200_000.0),
    ("stainless steel", 193_000.0),
    ("aluminum", 69_000.0),
    ("aluminium", 69_000.0),
    ("brass", 100_000.0),
    ("copper", 117_000.0),
    ("titanium", 114_000.0),
    ("cast iron", 100_000.0),
    ("abs plastic", 2_300.0),
    ("pla", 3_500.0),
    ("nylon", 2_500.0),
    ("polycarbonate", 2_300.0),
    ("oak", 12_000.0),
    ("glass", 70_000.0),
];

/// Young's modulus for a material name, if SolidCraft knows one.
pub fn youngs_modulus(material: &str) -> Option<f64> {
    let name = material.trim();
    if name.is_empty() {
        return None;
    }
    MODULI.iter().find(|(entry, _)| entry.eq_ignore_ascii_case(name)).map(|(_, modulus)| *modulus)
}

/// Prismatic section along X. Bending is about Y (deflection in Z).
#[derive(Clone, Copy, Debug)]
pub struct BeamSection {
    pub length: f64,
    pub width: f64,
    pub height: f64,
    pub modulus: f64,
    pub elements: usize,
}

/// An end of the beam, along X.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeamEnd {
    MinX,
    MaxX,
}

/// A support. A clamp removes deflection and rotation. A pin removes deflection only.
#[derive(Clone, Copy, Debug)]
pub struct BeamSupport {
    pub end: BeamEnd,
    pub clamp: bool,
}

/// A transverse point force at an end. Positive `force_z` points toward +Z.
#[derive(Clone, Copy, Debug)]
pub struct BeamLoad {
    pub end: BeamEnd,
    pub force_z: f64,
}

/// A solved static field. Stress samples are the absolute outer-fiber bending stress
/// at each node, in N/mm². `stress_min` and `stress_max` are the signed extremes
/// (−peak and +peak), because bending has a tension face and a compression face.
#[derive(Clone, Debug, PartialEq)]
pub struct BeamField {
    pub method: &'static str,
    pub displacement_min: f64,
    pub displacement_max: f64,
    /// Transverse displacement at max X, in millimetres.
    pub tip: f64,
    pub stress_min: f64,
    pub stress_max: f64,
    pub displacement: Vec<f64>,
    pub stress: Vec<f64>,
}

/// Assemble and solve. An under-supported beam, a non-finite input, or a section past
/// the size cap is an error: this function does not return a made-up stress.
pub fn solve_beam(section: BeamSection, supports: &[BeamSupport], loads: &[BeamLoad]) -> Result<BeamField, &'static str> {
    if supports.len() > 32 || loads.len() > 32 {
        return Err("the study exceeds the size cap");
    }
    if section.elements == 0 || section.elements > MAX_ELEMENTS {
        return Err("the study exceeds the size cap");
    }
    if !(section.length.is_finite() && section.width.is_finite() && section.height.is_finite() && section.modulus.is_finite()) {
        return Err("the beam and the load must be finite");
    }
    if section.length <= 1e-6 || section.width <= 1e-6 || section.height <= 1e-6 || section.modulus <= 0.0 {
        return Err("the beam and the load must be finite");
    }
    if section.length > MAX_DIMENSION || section.width > MAX_DIMENSION || section.height > MAX_DIMENSION || section.modulus > 1.0e12 {
        return Err("the study exceeds the size cap");
    }
    for load in loads {
        if !load.force_z.is_finite() || load.force_z.abs() > 1.0e12 {
            return Err("the beam and the load must be finite");
        }
    }
    if supports.is_empty() {
        return Err("under-constrained: the beam can move as a rigid body");
    }

    let n = section.elements;
    let nodes = n + 1;
    let ndof = nodes * 2;
    let length = section.length / n as f64;
    let inertia = section.width * section.height * section.height * section.height / 12.0;
    let ei = section.modulus * inertia;
    if !(inertia.is_finite() && ei.is_finite() && ei > 0.0 && length.is_finite() && length > 0.0) {
        return Err("the beam and the load must be finite");
    }

    let mut fixed = vec![false; ndof];
    for support in supports {
        let node = match support.end {
            BeamEnd::MinX => 0,
            BeamEnd::MaxX => n,
        };
        let deflection = node * 2;
        if let Some(slot) = fixed.get_mut(deflection) {
            *slot = true;
        }
        if support.clamp
            && let Some(slot) = fixed.get_mut(deflection + 1)
        {
            *slot = true;
        }
    }

    let mut force = vec![0.0; ndof];
    for load in loads {
        let node = match load.end {
            BeamEnd::MinX => 0,
            BeamEnd::MaxX => n,
        };
        if let Some(slot) = force.get_mut(node * 2) {
            *slot += load.force_z;
        }
    }

    let mut stiffness = vec![0.0; ndof * ndof];
    for element in 0..n {
        let mut local = [0.0; 16];
        element_stiffness(ei, length, &mut local);
        let dofs = [element * 2, element * 2 + 1, (element + 1) * 2, (element + 1) * 2 + 1];
        for row in 0..4 {
            for col in 0..4 {
                let Some(grow) = dofs.get(row).copied() else { continue };
                let Some(gcol) = dofs.get(col).copied() else { continue };
                let add = local.get(row * 4 + col).copied().unwrap_or(0.0);
                if let Some(slot) = stiffness.get_mut(grow * ndof + gcol) {
                    *slot += add;
                }
            }
        }
    }

    let free: Vec<usize> = (0..ndof).filter(|dof| fixed.get(*dof).copied() != Some(true)).collect();
    let mut displacement = vec![0.0; ndof];
    if !free.is_empty() {
        let m = free.len();
        let mut reduced = vec![0.0; m * m];
        let mut rhs = vec![0.0; m];
        for (i, &global_i) in free.iter().enumerate() {
            if let Some(slot) = rhs.get_mut(i) {
                *slot = force.get(global_i).copied().unwrap_or(0.0);
            }
            for (j, &global_j) in free.iter().enumerate() {
                let value = stiffness.get(global_i * ndof + global_j).copied().unwrap_or(0.0);
                if let Some(slot) = reduced.get_mut(i * m + j) {
                    *slot = value;
                }
            }
        }
        solve_linear(&mut reduced, &mut rhs, m)?;
        for (i, &global) in free.iter().enumerate() {
            if let Some(slot) = displacement.get_mut(global) {
                *slot = rhs.get(i).copied().unwrap_or(0.0);
            }
        }
    }

    let mut deflection = Vec::with_capacity(nodes);
    let mut dmin = f64::INFINITY;
    let mut dmax = f64::NEG_INFINITY;
    for node in 0..nodes {
        let value = displacement.get(node * 2).copied().unwrap_or(0.0);
        if !value.is_finite() {
            return Err("the study did not produce a finite result");
        }
        dmin = dmin.min(value);
        dmax = dmax.max(value);
        deflection.push(value);
    }

    let mut moment = vec![0.0; nodes];
    for element in 0..n {
        let mut local = [0.0; 16];
        element_stiffness(ei, length, &mut local);
        let dofs = [element * 2, element * 2 + 1, (element + 1) * 2, (element + 1) * 2 + 1];
        let mut end = [0.0; 4];
        for row in 0..4 {
            let mut sum = 0.0;
            for col in 0..4 {
                let Some(dof) = dofs.get(col).copied() else { continue };
                let value = displacement.get(dof).copied().unwrap_or(0.0);
                sum += local.get(row * 4 + col).copied().unwrap_or(0.0) * value;
            }
            if let Some(slot) = end.get_mut(row) {
                *slot = sum;
            }
        }
        if let Some(slot) = moment.get_mut(element) {
            *slot = f64::max(*slot, end[1].abs());
        }
        if let Some(slot) = moment.get_mut(element + 1) {
            *slot = f64::max(*slot, end[3].abs());
        }
    }

    let half = section.height * 0.5;
    let mut stress = Vec::with_capacity(nodes);
    let mut peak = 0.0_f64;
    for value in &moment {
        let sigma = value * half / inertia;
        if !sigma.is_finite() || sigma < 0.0 {
            return Err("the study did not produce a finite result");
        }
        peak = peak.max(sigma);
        stress.push(sigma);
    }
    if !(dmin.is_finite() && dmax.is_finite() && peak.is_finite()) {
        return Err("the study did not produce a finite result");
    }
    let tip = deflection.last().copied().unwrap_or(0.0);
    Ok(BeamField {
        method: METHOD,
        displacement_min: dmin,
        displacement_max: dmax,
        tip,
        stress_min: -peak,
        stress_max: peak,
        displacement: deflection,
        stress,
    })
}

/// 4×4 Euler-Bernoulli stiffness, row-major, for DOFs (v1, θ1, v2, θ2).
fn element_stiffness(ei: f64, length: f64, out: &mut [f64; 16]) {
    let length2 = length * length;
    let length3 = length2 * length;
    let scale = ei / length3;
    let entries = [
        12.0,
        6.0 * length,
        -12.0,
        6.0 * length,
        6.0 * length,
        4.0 * length2,
        -6.0 * length,
        2.0 * length2,
        -12.0,
        -6.0 * length,
        12.0,
        -6.0 * length,
        6.0 * length,
        2.0 * length2,
        -6.0 * length,
        4.0 * length2,
    ];
    for (slot, entry) in out.iter_mut().zip(entries) {
        *slot = scale * entry;
    }
}

/// Dense Gaussian elimination with partial pivoting. `matrix` is row-major `n`×`n`
/// and is overwritten. A pivot below the matrix scale is treated as a rigid-body mode.
fn solve_linear(matrix: &mut [f64], rhs: &mut [f64], n: usize) -> Result<(), &'static str> {
    if n == 0 {
        return Ok(());
    }
    if matrix.len() < n * n || rhs.len() < n {
        return Err("the study did not produce a finite result");
    }
    let mut scale = 0.0_f64;
    for value in matrix.iter().take(n * n) {
        scale = scale.max(value.abs());
    }
    if !(scale.is_finite() && scale > 0.0) {
        return Err("under-constrained: the beam can move as a rigid body");
    }
    let tolerance = scale * 1e-12;
    for k in 0..n {
        let mut pivot = k;
        let mut best = 0.0;
        for row in k..n {
            let value = matrix.get(row * n + k).copied().unwrap_or(0.0).abs();
            if value > best {
                best = value;
                pivot = row;
            }
        }
        if best <= tolerance {
            return Err("under-constrained: the beam can move as a rigid body");
        }
        if pivot != k {
            for col in 0..n {
                let left = k * n + col;
                let right = pivot * n + col;
                let a = matrix.get(left).copied().unwrap_or(0.0);
                let b = matrix.get(right).copied().unwrap_or(0.0);
                if let Some(slot) = matrix.get_mut(left) {
                    *slot = b;
                }
                if let Some(slot) = matrix.get_mut(right) {
                    *slot = a;
                }
            }
            let a = rhs.get(k).copied().unwrap_or(0.0);
            let b = rhs.get(pivot).copied().unwrap_or(0.0);
            if let Some(slot) = rhs.get_mut(k) {
                *slot = b;
            }
            if let Some(slot) = rhs.get_mut(pivot) {
                *slot = a;
            }
        }
        let diag = matrix.get(k * n + k).copied().unwrap_or(0.0);
        if diag.abs() <= tolerance {
            return Err("under-constrained: the beam can move as a rigid body");
        }
        for row in (k + 1)..n {
            let factor = matrix.get(row * n + k).copied().unwrap_or(0.0) / diag;
            if !factor.is_finite() {
                return Err("the study did not produce a finite result");
            }
            for col in k..n {
                let sub = factor * matrix.get(k * n + col).copied().unwrap_or(0.0);
                if let Some(slot) = matrix.get_mut(row * n + col) {
                    *slot -= sub;
                }
            }
            let sub = factor * rhs.get(k).copied().unwrap_or(0.0);
            if let Some(slot) = rhs.get_mut(row) {
                *slot -= sub;
            }
        }
    }
    for row in (0..n).rev() {
        let mut sum = rhs.get(row).copied().unwrap_or(0.0);
        for col in (row + 1)..n {
            sum -= matrix.get(row * n + col).copied().unwrap_or(0.0) * rhs.get(col).copied().unwrap_or(0.0);
        }
        let diag = matrix.get(row * n + row).copied().unwrap_or(0.0);
        if diag.abs() <= tolerance || !sum.is_finite() {
            return Err("under-constrained: the beam can move as a rigid body");
        }
        let value = sum / diag;
        if !value.is_finite() {
            return Err("the study did not produce a finite result");
        }
        if let Some(slot) = rhs.get_mut(row) {
            *slot = value;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(got: f64, want: f64) -> f64 {
        (got - want).abs() / want.abs().max(1e-12)
    }

    fn cantilever(elements: usize) -> BeamField {
        solve_beam(
            BeamSection { length: 100.0, width: 10.0, height: 10.0, modulus: 200_000.0, elements },
            &[BeamSupport { end: BeamEnd::MinX, clamp: true }],
            &[BeamLoad { end: BeamEnd::MaxX, force_z: 100.0 }],
        )
        .expect("cantilever")
    }

    #[test]
    fn a_cantilever_matches_beam_theory_for_one_and_several_elements() {
        let inertia = 10.0 * 1000.0 / 12.0;
        let tip = 100.0 * 1_000_000.0 / (3.0 * 200_000.0 * inertia);
        let peak = 100.0 * 100.0 * 5.0 / inertia;
        for elements in [1_usize, 4, 8, MAX_ELEMENTS] {
            let field = cantilever(elements);
            assert_eq!(field.method, METHOD);
            assert!(rel(field.tip, tip) < 1e-8, "{elements}: {} vs {tip}", field.tip);
            assert!(rel(field.displacement_max, tip) < 1e-8, "{elements}");
            assert!(field.displacement_min.abs() < 1e-9, "{elements}: {}", field.displacement_min);
            assert!(rel(field.stress_max, peak) < 1e-8, "{elements}: {} vs {peak}", field.stress_max);
            assert!(rel(field.stress_min, -peak) < 1e-8, "{elements}");
            assert_eq!(field.stress.len(), elements + 1);
            let root = field.stress.first().copied().unwrap_or(0.0);
            let end = field.stress.last().copied().unwrap_or(-1.0);
            assert!(rel(root, peak) < 1e-6, "{elements}: root {root}");
            assert!(end.abs() < 1e-6 * peak, "{elements}: tip stress {end}");
        }
    }

    #[test]
    fn a_free_beam_and_a_single_pin_are_under_constrained() {
        let section = BeamSection { length: 80.0, width: 8.0, height: 12.0, modulus: 69_000.0, elements: 4 };
        let load = [BeamLoad { end: BeamEnd::MaxX, force_z: 10.0 }];
        let free = solve_beam(section, &[], &load).expect_err("free");
        assert!(free.contains("under-constrained"), "{free}");
        let pin = solve_beam(section, &[BeamSupport { end: BeamEnd::MinX, clamp: false }], &load).expect_err("pin");
        assert!(pin.contains("under-constrained"), "{pin}");
    }

    #[test]
    fn pins_at_both_ends_carry_an_end_load_with_no_displacement() {
        let field = solve_beam(
            BeamSection { length: 50.0, width: 10.0, height: 10.0, modulus: 200_000.0, elements: 4 },
            &[BeamSupport { end: BeamEnd::MinX, clamp: false }, BeamSupport { end: BeamEnd::MaxX, clamp: false }],
            &[BeamLoad { end: BeamEnd::MaxX, force_z: 40.0 }],
        )
        .expect("pinned");
        assert!(field.displacement_max.abs() < 1e-9, "{field:?}");
        assert!(field.stress_max.abs() < 1e-6, "{field:?}");
        assert_eq!(field.method, METHOD);
    }

    #[test]
    fn non_finite_and_over_cap_inputs_are_refused() {
        let ok = BeamSection { length: 10.0, width: 2.0, height: 2.0, modulus: 1_000.0, elements: 2 };
        let support = [BeamSupport { end: BeamEnd::MinX, clamp: true }];
        let load = [BeamLoad { end: BeamEnd::MaxX, force_z: 1.0 }];
        assert!(solve_beam(BeamSection { length: f64::NAN, ..ok }, &support, &load).is_err());
        assert!(solve_beam(BeamSection { elements: MAX_ELEMENTS + 1, ..ok }, &support, &load).is_err());
        assert!(solve_beam(BeamSection { length: MAX_DIMENSION * 2.0, ..ok }, &support, &load).is_err());
        assert!(solve_beam(ok, &support, &[BeamLoad { end: BeamEnd::MaxX, force_z: f64::INFINITY }]).is_err());
        assert!(youngs_modulus("Steel") == Some(200_000.0));
        assert!(youngs_modulus("no-such-metal").is_none());
        for (_, modulus) in MODULI {
            assert!(modulus.is_finite() && *modulus > 0.0);
        }
    }
}
