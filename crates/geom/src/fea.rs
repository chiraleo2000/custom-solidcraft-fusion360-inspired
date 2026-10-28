//! Linear static solvers written for SolidCraft.
//!
//! [`solve_beam`] is the standard 2-node Hermitian Euler-Bernoulli beam (transverse
//! deflection along Z, rotation about Y, length along X). [`solve_hex_grid`] is a
//! capped voxel bar-lattice on occupied cells of an axis-aligned grid — enough to
//! carry a non-box solid under face supports and loads without a GPL FEA library.
//! Both assemble here and solve by Gaussian elimination with partial pivoting.
//!
//! Recorded method names: [`METHOD`] (beam) and [`HEX_METHOD`] (hex grid).

/// Recorded on every beam result.
pub const METHOD: &str = "solidcraft-beam-stiffness";

/// Recorded on every capped hex-grid result.
pub const HEX_METHOD: &str = "solidcraft-hex-grid";

/// Most beam elements a study may assemble. Past this the caller must refuse.
pub const MAX_ELEMENTS: usize = 32;

/// Most divisions along one hex-grid axis.
pub const MAX_HEX_DIV: usize = 4;

/// Most occupied cells a hex-grid study may assemble.
pub const MAX_HEX_CELLS: usize = 64;

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

/// A face of the hex-grid bounding box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HexFace {
    MinX,
    MaxX,
    MinY,
    MaxY,
    MinZ,
    MaxZ,
}

/// A support on a hex-grid face. A clamp fixes all three translations; a pin fixes
/// only the translation normal to the face.
#[derive(Clone, Copy, Debug)]
pub struct HexSupport {
    pub face: HexFace,
    pub clamp: bool,
}

/// A total force on a hex-grid face, newtons, distributed over occupied cells on that face.
#[derive(Clone, Copy, Debug)]
pub struct HexLoad {
    pub face: HexFace,
    pub force: [f64; 3],
}

/// Occupied cells of a regular axis-aligned grid. `occupied` is row-major `nx*ny*nz`.
#[derive(Clone, Debug)]
pub struct HexSection {
    pub origin: [f64; 3],
    pub pitch: [f64; 3],
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
    pub occupied: Vec<bool>,
    pub modulus: f64,
}

/// Shared static result shape (beam or hex grid).
pub type StaticField = BeamField;

/// Divisions per axis for a hex study, capped at [`MAX_HEX_DIV`].
pub fn hex_divisions(elements: usize) -> usize {
    elements.clamp(1, MAX_HEX_DIV)
}

/// Mark cells whose centres pass `inside`. Empty or over-cap occupancy is an error.
pub fn fill_occupancy(
    origin: [f64; 3],
    pitch: [f64; 3],
    nx: usize,
    ny: usize,
    nz: usize,
    inside: &dyn Fn([f64; 3]) -> bool,
) -> Result<Vec<bool>, &'static str> {
    if nx == 0 || ny == 0 || nz == 0 || nx > MAX_HEX_DIV || ny > MAX_HEX_DIV || nz > MAX_HEX_DIV {
        return Err("the study exceeds the size cap");
    }
    for value in origin.iter().chain(pitch.iter()) {
        if !value.is_finite() {
            return Err("the beam and the load must be finite");
        }
    }
    if pitch[0] <= 1e-6 || pitch[1] <= 1e-6 || pitch[2] <= 1e-6 {
        return Err("the beam and the load must be finite");
    }
    let total = nx.checked_mul(ny).and_then(|v| v.checked_mul(nz)).ok_or("the study exceeds the size cap")?;
    if total > MAX_HEX_CELLS {
        return Err("the study exceeds the size cap");
    }
    let mut occupied = vec![false; total];
    let mut count = 0usize;
    for iz in 0..nz {
        for iy in 0..ny {
            for ix in 0..nx {
                let center =
                    [origin[0] + (ix as f64 + 0.5) * pitch[0], origin[1] + (iy as f64 + 0.5) * pitch[1], origin[2] + (iz as f64 + 0.5) * pitch[2]];
                if !center.iter().all(|v| v.is_finite()) {
                    return Err("the beam and the load must be finite");
                }
                if inside(center) {
                    let index = ix + nx * (iy + ny * iz);
                    if let Some(slot) = occupied.get_mut(index) {
                        *slot = true;
                        count += 1;
                    }
                }
            }
        }
    }
    if count == 0 {
        return Err("not supported yet: static stress of a body with no usable volume");
    }
    if count > MAX_HEX_CELLS {
        return Err("the study exceeds the size cap");
    }
    Ok(occupied)
}

fn hex_index(section: &HexSection, ix: usize, iy: usize, iz: usize) -> Option<usize> {
    if ix >= section.nx || iy >= section.ny || iz >= section.nz {
        return None;
    }
    Some(ix + section.nx * (iy + section.ny * iz))
}

fn hex_on_face(section: &HexSection, ix: usize, iy: usize, iz: usize, face: HexFace) -> bool {
    match face {
        HexFace::MinX => ix == 0,
        HexFace::MaxX => section.nx > 0 && ix + 1 == section.nx,
        HexFace::MinY => iy == 0,
        HexFace::MaxY => section.ny > 0 && iy + 1 == section.ny,
        HexFace::MinZ => iz == 0,
        HexFace::MaxZ => section.nz > 0 && iz + 1 == section.nz,
    }
}

fn hex_face_normal(face: HexFace) -> usize {
    match face {
        HexFace::MinX | HexFace::MaxX => 0,
        HexFace::MinY | HexFace::MaxY => 1,
        HexFace::MinZ | HexFace::MaxZ => 2,
    }
}

/// Assemble a capped hex-grid bar lattice and solve. An empty support set, a free
/// mechanism, non-finite input, or a grid past the size cap is an error.
pub fn solve_hex_grid(section: &HexSection, supports: &[HexSupport], loads: &[HexLoad]) -> Result<StaticField, &'static str> {
    if supports.len() > 32 || loads.len() > 32 {
        return Err("the study exceeds the size cap");
    }
    if section.nx == 0 || section.ny == 0 || section.nz == 0 || section.nx > MAX_HEX_DIV || section.ny > MAX_HEX_DIV || section.nz > MAX_HEX_DIV {
        return Err("the study exceeds the size cap");
    }
    let total = section.nx.checked_mul(section.ny).and_then(|v| v.checked_mul(section.nz)).ok_or("the study exceeds the size cap")?;
    if section.occupied.len() != total || total > MAX_HEX_CELLS {
        return Err("the study exceeds the size cap");
    }
    if !(section.modulus.is_finite() && section.modulus > 0.0 && section.modulus <= 1.0e12) {
        return Err("the beam and the load must be finite");
    }
    for value in section.origin.iter().chain(section.pitch.iter()) {
        if !value.is_finite() {
            return Err("the beam and the load must be finite");
        }
    }
    if section.pitch[0] <= 1e-6 || section.pitch[1] <= 1e-6 || section.pitch[2] <= 1e-6 {
        return Err("the beam and the load must be finite");
    }
    if section.pitch[0] > MAX_DIMENSION || section.pitch[1] > MAX_DIMENSION || section.pitch[2] > MAX_DIMENSION {
        return Err("the study exceeds the size cap");
    }
    if supports.is_empty() {
        return Err("under-constrained: the beam can move as a rigid body");
    }
    for load in loads {
        for component in &load.force {
            if !component.is_finite() || component.abs() > 1.0e12 {
                return Err("the beam and the load must be finite");
            }
        }
    }

    let mut nodes: Vec<(usize, usize, usize)> = Vec::new();
    let mut local_of = vec![None; total];
    for iz in 0..section.nz {
        for iy in 0..section.ny {
            for ix in 0..section.nx {
                let Some(flat) = hex_index(section, ix, iy, iz) else { continue };
                if section.occupied.get(flat).copied() != Some(true) {
                    continue;
                }
                if nodes.len() >= MAX_HEX_CELLS {
                    return Err("the study exceeds the size cap");
                }
                let local = nodes.len();
                nodes.push((ix, iy, iz));
                if let Some(slot) = local_of.get_mut(flat) {
                    *slot = Some(local);
                }
            }
        }
    }
    if nodes.is_empty() {
        return Err("not supported yet: static stress of a body with no usable volume");
    }
    let n_nodes = nodes.len();
    let ndof = n_nodes.checked_mul(3).ok_or("the study exceeds the size cap")?;
    if ndof > MAX_HEX_CELLS * 3 {
        return Err("the study exceeds the size cap");
    }

    let mut fixed = vec![false; ndof];
    let mut supported = 0usize;
    for support in supports {
        let axis = hex_face_normal(support.face);
        for (local, &(ix, iy, iz)) in nodes.iter().enumerate() {
            if !hex_on_face(section, ix, iy, iz, support.face) {
                continue;
            }
            supported += 1;
            if support.clamp {
                for dof in 0..3 {
                    if let Some(slot) = fixed.get_mut(local * 3 + dof) {
                        *slot = true;
                    }
                }
            } else if let Some(slot) = fixed.get_mut(local * 3 + axis) {
                *slot = true;
            }
        }
    }
    if supported == 0 {
        return Err("under-constrained: the beam can move as a rigid body");
    }

    let mut force = vec![0.0; ndof];
    for load in loads {
        let mut face_nodes: Vec<usize> = Vec::new();
        for (local, &(ix, iy, iz)) in nodes.iter().enumerate() {
            if hex_on_face(section, ix, iy, iz, load.face) {
                face_nodes.push(local);
            }
        }
        if face_nodes.is_empty() {
            return Err("not supported yet: a load face with no material");
        }
        let share = 1.0 / face_nodes.len() as f64;
        for local in face_nodes {
            for dof in 0..3 {
                if let Some(slot) = force.get_mut(local * 3 + dof) {
                    *slot += load.force.get(dof).copied().unwrap_or(0.0) * share;
                }
            }
        }
    }

    let mut stiffness = vec![0.0; ndof * ndof];
    let areas = [section.pitch[1] * section.pitch[2], section.pitch[0] * section.pitch[2], section.pitch[0] * section.pitch[1]];
    let neighbours = [(1usize, 0usize, 0usize, 0usize), (0, 1, 0, 1), (0, 0, 1, 2)];
    for (local, &(ix, iy, iz)) in nodes.iter().enumerate() {
        for &(dx, dy, dz, axis) in &neighbours {
            let (jx, jy, jz) = (ix + dx, iy + dy, iz + dz);
            let Some(flat) = hex_index(section, jx, jy, jz) else { continue };
            let Some(other) = local_of.get(flat).copied().flatten() else { continue };
            let length = section.pitch.get(axis).copied().unwrap_or(0.0);
            let area = areas.get(axis).copied().unwrap_or(0.0);
            if !(length > 1e-9 && area > 1e-12) {
                continue;
            }
            let k = section.modulus * area / length;
            if !k.is_finite() || k <= 0.0 {
                return Err("the beam and the load must be finite");
            }
            let a = local * 3 + axis;
            let b = other * 3 + axis;
            for (row, col, sign) in [(a, a, 1.0), (b, b, 1.0), (a, b, -1.0), (b, a, -1.0)] {
                if let Some(slot) = stiffness.get_mut(row * ndof + col) {
                    *slot += sign * k;
                }
            }
            // Soft shear on the other DOFs so a transverse tip load is not a free mechanism.
            let shear = k * 0.05;
            for dof in 0..3 {
                if dof == axis {
                    continue;
                }
                let sa = local * 3 + dof;
                let sb = other * 3 + dof;
                for (row, col, sign) in [(sa, sa, 1.0), (sb, sb, 1.0), (sa, sb, -1.0), (sb, sa, -1.0)] {
                    if let Some(slot) = stiffness.get_mut(row * ndof + col) {
                        *slot += sign * shear;
                    }
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

    let mut samples = Vec::with_capacity(n_nodes);
    let mut dmin = f64::INFINITY;
    let mut dmax = f64::NEG_INFINITY;
    let mut tip = 0.0_f64;
    let mut tip_set = false;
    for (local, _) in nodes.iter().enumerate() {
        let ux = displacement.get(local * 3).copied().unwrap_or(0.0);
        let uy = displacement.get(local * 3 + 1).copied().unwrap_or(0.0);
        let uz = displacement.get(local * 3 + 2).copied().unwrap_or(0.0);
        if !(ux.is_finite() && uy.is_finite() && uz.is_finite()) {
            return Err("the study did not produce a finite result");
        }
        let mag = (ux * ux + uy * uy + uz * uz).sqrt();
        dmin = dmin.min(mag);
        dmax = dmax.max(mag);
        samples.push(mag);
        if !tip_set || mag > tip {
            tip = mag;
            tip_set = true;
        }
    }

    let mut peak = 0.0_f64;
    let mut stress = vec![0.0; n_nodes];
    for (local, &(ix, iy, iz)) in nodes.iter().enumerate() {
        let mut local_peak = 0.0_f64;
        for &(dx, dy, dz, axis) in &neighbours {
            let (jx, jy, jz) = (ix + dx, iy + dy, iz + dz);
            let Some(flat) = hex_index(section, jx, jy, jz) else { continue };
            let Some(other) = local_of.get(flat).copied().flatten() else { continue };
            let length = section.pitch.get(axis).copied().unwrap_or(0.0);
            let area = areas.get(axis).copied().unwrap_or(0.0);
            if !(length > 1e-9 && area > 1e-12) {
                continue;
            }
            let k = section.modulus * area / length;
            let u_a = displacement.get(local * 3 + axis).copied().unwrap_or(0.0);
            let u_b = displacement.get(other * 3 + axis).copied().unwrap_or(0.0);
            let sigma = (k * (u_b - u_a) / area).abs();
            if sigma.is_finite() {
                local_peak = local_peak.max(sigma);
            }
        }
        peak = peak.max(local_peak);
        if let Some(slot) = stress.get_mut(local) {
            *slot = local_peak;
        }
    }
    if !(dmin.is_finite() && dmax.is_finite() && peak.is_finite() && tip.is_finite()) {
        return Err("the study did not produce a finite result");
    }
    Ok(StaticField {
        method: HEX_METHOD,
        displacement_min: dmin,
        displacement_max: dmax,
        tip,
        stress_min: -peak,
        stress_max: peak,
        displacement: samples,
        stress,
    })
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

    fn filled_box(n: usize) -> HexSection {
        let pitch = 10.0 / n as f64;
        HexSection { origin: [0.0, 0.0, 0.0], pitch: [pitch, pitch, pitch], nx: n, ny: n, nz: n, occupied: vec![true; n * n * n], modulus: 200_000.0 }
    }

    #[test]
    fn a_clamped_hex_grid_moves_under_a_face_load() {
        let section = filled_box(3);
        let field =
            solve_hex_grid(&section, &[HexSupport { face: HexFace::MinX, clamp: true }], &[HexLoad { face: HexFace::MaxX, force: [0.0, 0.0, 50.0] }])
                .expect("hex");
        assert_eq!(field.method, HEX_METHOD);
        assert!(field.displacement_max > 0.0, "{field:?}");
        assert!(field.stress_max > 0.0, "{field:?}");
        assert!(field.tip > 0.0, "{field:?}");
        assert_eq!(field.stress.len(), 27);
    }

    #[test]
    fn a_free_hex_grid_is_under_constrained_and_empty_volume_is_refused() {
        let section = filled_box(2);
        let err = solve_hex_grid(&section, &[], &[HexLoad { face: HexFace::MaxX, force: [1.0, 0.0, 0.0] }]).expect_err("free");
        assert!(err.contains("under-constrained"), "{err}");
        let empty = HexSection { occupied: vec![false; 8], ..filled_box(2) };
        let err =
            solve_hex_grid(&empty, &[HexSupport { face: HexFace::MinX, clamp: true }], &[HexLoad { face: HexFace::MaxX, force: [1.0, 0.0, 0.0] }])
                .expect_err("empty");
        assert!(err.contains("no usable volume") || err.contains("not supported"), "{err}");
        let occupied = fill_occupancy([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 2, 2, 2, &|_| false).expect_err("none");
        assert!(occupied.contains("no usable volume") || occupied.contains("not supported"), "{occupied}");
    }
}
