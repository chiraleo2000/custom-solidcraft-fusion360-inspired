//! Machining toolpaths: a setup's work coordinates, tool-radius compensation, depth steps,
//! a stepped surface path, hole recognition and a SolidCraft G-code post.

use serde_json::{Value, json};
use solidcraft_doc::CamJob;
use solidcraft_geom::toolpath::{
    WorkCoords, ball_center, contour_passes, depth_levels, inset_passes_around, island_keepouts, offset_closed, polygon_area, stepover_stations,
    upward_hit,
};
use solidcraft_geom::{Plane, Vec2, Vec3};

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("cam.contour", "2D Contour", contour)
    .at("UTILITIES", "MAKE")
    .icon("extrude")
    .params("sketch; tool_diameter; depth; stepdown?; name?. The tool centre is a radius outside the outer profile and a radius inside each island, cut in depth steps")];

/// A positive finite parameter, or `None` when the key is absent.
pub(crate) fn opt_positive(cmd: &str, p: &Value, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let n = v.as_f64().ok_or_else(|| bad(cmd, format!("`{key}` must be a positive number")))?;
            if !(n.is_finite() && n.abs() < 1e9) {
                return Err(bad(cmd, format!("`{key}` must be finite")));
            }
            if n <= 0.0 {
                return Err(bad(cmd, format!("`{key}` must be positive")));
            }
            Ok(Some(n))
        }
    }
}

fn req_positive(cmd: &str, p: &Value, key: &str) -> Result<f64> {
    opt_positive(cmd, p, key)?.ok_or_else(|| bad(cmd, format!("`{key}` is required")))
}

pub(crate) fn cut_levels(cmd: &str, depth: f64, stepdown: Option<f64>, fallback: f64) -> Result<Vec<f64>> {
    let step = stepdown.unwrap_or(fallback);
    if !(step.is_finite() && step > 0.0 && step < 1e9) {
        return Err(bad(cmd, "`stepdown` must be positive"));
    }
    depth_levels(depth, step).map_err(|e| bad(cmd, e))
}

pub(crate) fn wcs_of(s: &Session, cmd: &str) -> Result<WorkCoords> {
    let Some(setup) = s.doc.cam_setups.last() else {
        return Ok(WorkCoords::model());
    };
    WorkCoords::from_xz(
        Vec3::new(setup.origin[0], setup.origin[1], setup.origin[2]),
        Vec3::new(setup.x_axis[0], setup.x_axis[1], setup.x_axis[2]),
        Vec3::new(setup.z_axis[0], setup.z_axis[1], setup.z_axis[2]),
    )
    .map_err(|e| bad(cmd, e))
}

pub(crate) fn sketch_profile(s: &Session, sketch: &str, cmd: &str) -> Result<(Plane, Vec<Vec2>, Vec<Vec<Vec2>>)> {
    let state = s.model.state();
    let solved = state.sketches.iter().find(|item| item.name == sketch).ok_or_else(|| bad(cmd, format!("no sketch `{sketch}`")))?;
    let profile = solved.profiles.first().ok_or_else(|| bad(cmd, "the sketch needs one closed profile"))?;
    if profile.region.holes.len() > 64 {
        return Err(bad(cmd, "too many islands"));
    }
    let poly = profile.region.outer.polyline(0.2);
    if poly.len() < 3 || poly.len() > 8_000 {
        return Err(bad(cmd, "the sketch needs one closed profile"));
    }
    let mut holes = Vec::with_capacity(profile.region.holes.len());
    for hole in &profile.region.holes {
        let loop_ = hole.polyline(0.2);
        if loop_.len() < 3 || loop_.len() > 8_000 {
            return Err(bad(cmd, "an island needs a closed profile"));
        }
        holes.push(loop_);
    }
    Ok((solved.plane, poly, holes))
}

pub(crate) fn to_machine_xy(plane: Plane, wcs: WorkCoords, pts: &[Vec2]) -> Vec<Vec2> {
    pts.iter()
        .map(|q| {
            let local = wcs.to_local(plane.to_world(*q));
            Vec2::new(local.x, local.y)
        })
        .collect()
}

pub(crate) fn remember_job(
    s: &mut Session,
    cmd: &str,
    name: &str,
    sketch: &str,
    kind: &str,
    tool: f64,
    depth: f64,
    area: f64,
    gcode: String,
) -> Result<()> {
    if s.doc.cam.len() >= 100 {
        return Err(bad(cmd, "too many toolpaths"));
    }
    if gcode.len() > 2_000_000 || name.len() > 200 {
        return Err(bad(cmd, "the toolpath is too long"));
    }
    let setup = s.doc.cam_setups.last().map(|c| c.name.clone()).unwrap_or_default();
    s.doc_mut().cam.push(CamJob {
        name: name.to_string(),
        sketch: sketch.to_string(),
        tool_diameter: tool,
        depth,
        gcode,
        kind: kind.to_string(),
        area,
        setup,
    });
    Ok(())
}

/// Closed passes, one plunge per depth. Coordinates are already in work coordinates.
pub(crate) fn passes_gcode(passes: &[Vec<Vec2>], levels: &[f64]) -> std::result::Result<String, &'static str> {
    if passes.is_empty() || levels.is_empty() {
        return Err("the toolpath is empty");
    }
    let mut g = String::from("G21\nG90\nG0 Z5.000\n");
    for level in levels {
        if !level.is_finite() || *level <= 0.0 {
            return Err("a depth step is not usable");
        }
        for pass in passes {
            let Some(first) = pass.first() else { continue };
            if !first.is_finite() {
                return Err("a toolpath point is not finite");
            }
            g.push_str(&format!("G0 X{:.3} Y{:.3}\nG1 Z-{:.3} F200\n", first.x, first.y, level));
            for q in pass.iter().skip(1) {
                if !q.is_finite() {
                    return Err("a toolpath point is not finite");
                }
                g.push_str(&format!("G1 X{:.3} Y{:.3}\n", q.x, q.y));
            }
            g.push_str(&format!("G1 X{:.3} Y{:.3}\n", first.x, first.y));
            if g.len() > 2_000_000 {
                return Err("the toolpath is too long");
            }
        }
    }
    g.push_str("G0 Z5.000\n");
    Ok(g)
}

pub(crate) fn drill_gcode(x: f64, y: f64, levels: &[f64]) -> std::result::Result<String, &'static str> {
    drill_gcode_axis(Vec3::new(x, y, 0.0), Vec3::new(0.0, 0.0, -1.0), levels)
}

/// Peck along a known hole axis in work coordinates. Entry is the open end; `into` points into the hole.
pub(crate) fn drill_gcode_axis(entry: Vec3, into: Vec3, levels: &[f64]) -> std::result::Result<String, &'static str> {
    if !(entry.is_finite() && into.is_finite()) || levels.is_empty() {
        return Err("the drill axis is not usable");
    }
    let Some(into) = into.normalized() else {
        return Err("the drill axis is not usable");
    };
    let retract = entry - into * 5.0;
    if !retract.is_finite() {
        return Err("the drill axis is not usable");
    }
    let mut g = String::from("G21\nG90\n");
    g.push_str(&format!("G0 X{:.3} Y{:.3} Z{:.3}\n", retract.x, retract.y, retract.z));
    for level in levels {
        if !level.is_finite() || *level <= 0.0 {
            return Err("a depth step is not usable");
        }
        let tip = entry + into * *level;
        if !tip.is_finite() {
            return Err("a depth step is not usable");
        }
        g.push_str(&format!("G1 X{:.3} Y{:.3} Z{:.3} F120\nG0 X{:.3} Y{:.3} Z{:.3}\n", tip.x, tip.y, tip.z, retract.x, retract.y, retract.z));
        if g.len() > 2_000_000 {
            return Err("the toolpath is too long");
        }
    }
    Ok(g)
}

fn job_name(p: &Value, default: &str, cmd: &str) -> Result<String> {
    let name = str_(p, "name").unwrap_or(default);
    if name.is_empty() || name.len() > 200 {
        return Err(bad(cmd, "the toolpath name is not usable"));
    }
    Ok(name.to_string())
}

fn contour(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.contour";
    let sketch = str_(p, "sketch").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`sketch` is required"))?.to_string();
    let tool = req_positive(cmd, p, "tool_diameter")?;
    let depth = req_positive(cmd, p, "depth")?;
    let stepdown = opt_positive(cmd, p, "stepdown")?;
    let name = job_name(p, "Contour", cmd)?;
    let (plane, poly, holes) = sketch_profile(s, &sketch, cmd)?;
    let radius = tool * 0.5;
    let passes = contour_passes(&poly, &holes, radius).map_err(|e| bad(cmd, e))?;
    let outer_band = offset_closed(&poly, tool).unwrap_or_else(|_| passes.first().cloned().unwrap_or_default());
    let mut area = (polygon_area(&outer_band) - polygon_area(&poly)).abs();
    for (hole, pass) in holes.iter().zip(passes.iter().skip(1)) {
        area += (polygon_area(hole) - polygon_area(pass)).abs();
    }
    if !(area.is_finite() && area > 0.0) {
        return Err(bad(cmd, "the contour has no area"));
    }
    let levels = cut_levels(cmd, depth, stepdown, tool)?;
    let wcs = wcs_of(s, cmd)?;
    let machine: Vec<Vec<Vec2>> = passes.iter().map(|loop_| to_machine_xy(plane, wcs, loop_)).collect();
    let gcode = passes_gcode(&machine, &levels).map_err(|e| bad(cmd, e))?;
    remember_job(s, cmd, &name, &sketch, "contour", tool, depth, area, gcode.clone())?;
    s.refresh();
    Ok(json!({"name": name, "gcode": gcode, "steps": levels.len(), "area": area, "islands": holes.len()}))
}

pub(crate) fn pocket(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.pocket";
    let sketch = str_(p, "sketch").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`sketch` is required"))?.to_string();
    let tool = req_positive(cmd, p, "tool_diameter")?;
    let depth = req_positive(cmd, p, "depth")?;
    let stepdown = opt_positive(cmd, p, "stepdown")?;
    let stepover = opt_positive(cmd, p, "stepover")?.unwrap_or(tool * 0.5);
    let name = job_name(p, "Pocket", cmd)?;
    let (plane, poly, holes) = sketch_profile(s, &sketch, cmd)?;
    let radius = tool * 0.5;
    let mut passes = inset_passes_around(&poly, &holes, radius, stepover).map_err(|e| bad(cmd, e))?;
    let keepouts = island_keepouts(&holes, radius).map_err(|e| bad(cmd, e))?;
    // Wall pass around each island (tool stays a radius outside the island).
    for keep in &keepouts {
        passes.push(keep.clone());
    }
    let mut area = passes.first().map(|loop_| polygon_area(loop_)).unwrap_or(0.0);
    for keep in &keepouts {
        area = (area - polygon_area(keep)).max(0.0);
    }
    if !(area.is_finite() && area > 0.0) {
        return Err(bad(cmd, "the pocket has no area"));
    }
    let levels = cut_levels(cmd, depth, stepdown, tool)?;
    let wcs = wcs_of(s, cmd)?;
    let machine: Vec<Vec<Vec2>> = passes.iter().map(|loop_| to_machine_xy(plane, wcs, loop_)).collect();
    let gcode = passes_gcode(&machine, &levels).map_err(|e| bad(cmd, e))?;
    remember_job(s, cmd, &name, &sketch, "pocket", tool, depth, area, gcode)?;
    Ok(json!({"area": area, "steps": levels.len(), "passes": machine.len(), "islands": holes.len()}))
}

pub(crate) fn drill(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.drill";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    let solid = s.model.state().body(&body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?.body.clone();
    let diameter = req_positive(cmd, p, "diameter")?;
    let depth = req_positive(cmd, p, "depth")?;
    let stepdown = opt_positive(cmd, p, "stepdown")?;
    let at = match p.get("at") {
        None => {
            let measured = solidcraft_kernel::measure(&solid).map_err(|e| bad(cmd, e.to_string()))?;
            let c = (measured.bbox.min + measured.bbox.max) * 0.5;
            Vec2::new(c.x, c.y)
        }
        Some(v) => crate::params::vec2(v).ok_or_else(|| bad(cmd, "`at` must be [x, y]"))?,
    };
    let levels = cut_levels(cmd, depth, stepdown, diameter)?;
    let local = wcs_of(s, cmd)?.to_local(Vec3::new(at.x, at.y, 0.0));
    let gcode = drill_gcode(local.x, local.y, &levels).map_err(|e| bad(cmd, e))?;
    let area = std::f64::consts::PI * diameter * diameter * 0.25;
    remember_job(s, cmd, "Drill", "", "drill", diameter, depth, area, gcode)?;
    Ok(json!({"area": area, "at": [at.x, at.y], "steps": levels.len()}))
}

pub(crate) fn not_yet(_s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    if let Some(v) = p.get("tool_diameter") {
        let n = v.as_f64().ok_or_else(|| bad(cmd, "`tool_diameter` must be a positive number"))?;
        if !(n.is_finite() && n > 0.0 && n < 1e9) {
            return Err(bad(cmd, "`tool_diameter` must be positive"));
        }
    }
    if let Some(v) = p.get("stepover") {
        let n = v.as_f64().ok_or_else(|| bad(cmd, "`stepover` must be a positive number"))?;
        if !(n.is_finite() && n > 0.0 && n < 1e9) {
            return Err(bad(cmd, "`stepover` must be positive"));
        }
    }
    Err(bad(cmd, format!("not supported yet: {cmd}")))
}

fn surface_mesh(s: &Session, p: &Value, cmd: &str) -> Result<(solidcraft_geom::Mesh, f64, f64)> {
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    let solid = s.model.state().body(&body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?.body.clone();
    let tool = req_positive(cmd, p, "tool_diameter")?;
    let radius = tool * 0.5;
    let tol = (solid.size() * 0.01).clamp(0.15, 1.0);
    let mesh = solid.tessellate(tol).map_err(|e| bad(cmd, e.to_string()))?;
    if mesh.triangles.len() > 80_000 {
        return Err(bad(cmd, "the body is too detailed for a toolpath"));
    }
    if mesh.bounds().is_empty() {
        return Err(bad(cmd, "the body has no surface"));
    }
    Ok((mesh, tool, radius))
}

fn push_surface_samples(
    s: &mut Session,
    cmd: &str,
    kind: &str,
    tool: f64,
    radius: f64,
    stepover: f64,
    samples: Vec<Vec3>,
    rows: Value,
) -> Result<Value> {
    if samples.len() < 2 {
        return Err(bad(cmd, "the toolpath missed the body"));
    }
    let wcs = wcs_of(s, cmd)?;
    let mut gcode = String::from("G21\nG90\n");
    for centre in &samples {
        let local = wcs.to_local(*centre);
        if !local.is_finite() {
            return Err(bad(cmd, "a toolpath point is not finite"));
        }
        gcode.push_str(&format!("G1 X{:.3} Y{:.3} Z{:.3}\n", local.x, local.y, local.z));
        if gcode.len() > 2_000_000 {
            return Err(bad(cmd, "the toolpath is too long"));
        }
    }
    remember_job(s, cmd, kind, "", kind, tool, 0.0, 0.0, gcode)?;
    let mut out = json!({
        "samples": samples.iter().map(|c| [c.x, c.y, c.z]).collect::<Vec<_>>(),
        "stepover": stepover,
        "rows": rows,
    });
    if let (Some(obj), Ok(cusp)) = (out.as_object_mut(), solidcraft_geom::toolpath::cusp_height(radius, stepover)) {
        obj.insert("cusp".into(), json!(cusp));
    }
    Ok(out)
}

fn accept_ball(mesh: &solidcraft_geom::Mesh, point: Vec3, normal: Vec3, radius: f64) -> Option<Vec3> {
    let centre = ball_center(point, normal, radius)?;
    if centre.z + 1e-6 < point.z || mesh.contains(centre) {
        return None;
    }
    Some(centre)
}

pub(crate) fn surface(s: &mut Session, p: &Value, cmd: &str, kind: &str, scallop: bool) -> Result<Value> {
    let (mesh, tool, radius) = surface_mesh(s, p, cmd)?;
    let stepover = if scallop {
        match opt_positive(cmd, p, "stepover")? {
            Some(step) => step,
            None => {
                let cusp = opt_positive(cmd, p, "cusp")?.unwrap_or(0.05);
                solidcraft_geom::toolpath::scallop_stepover(radius, cusp).map_err(|e| bad(cmd, e))?
            }
        }
    } else {
        opt_positive(cmd, p, "stepover")?.unwrap_or(radius)
    };
    let bounds = mesh.bounds();
    let xs = stepover_stations(bounds.min.x, bounds.max.x, stepover, 80).map_err(|e| bad(cmd, e))?;
    let ys = stepover_stations(bounds.min.y, bounds.max.y, stepover, 80).map_err(|e| bad(cmd, e))?;
    if xs.len().saturating_mul(ys.len()) > 2_500 {
        return Err(bad(cmd, "the stepover is too fine"));
    }
    let mut samples = Vec::new();
    for (row, y) in ys.iter().enumerate() {
        let forward = row % 2 == 0;
        let n = xs.len();
        for k in 0..n {
            let Some(x) = xs.get(if forward { k } else { n - 1 - k }).copied() else { continue };
            let Some((point, normal)) = upward_hit(&mesh, x, *y) else { continue };
            let Some(centre) = accept_ball(&mesh, point, normal, radius) else { continue };
            samples.push(centre);
            if samples.len() > 2_500 {
                return Err(bad(cmd, "the toolpath is too long"));
            }
        }
    }
    push_surface_samples(s, cmd, kind, tool, radius, stepover, samples, json!(ys))
}

/// Constant-Z passes on upward faces: each Z band keeps hits within half a stepover of that plane.
pub(crate) fn horizontal(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.horizontal";
    let (mesh, tool, radius) = surface_mesh(s, p, cmd)?;
    let stepover = opt_positive(cmd, p, "stepover")?.unwrap_or(radius);
    let bounds = mesh.bounds();
    let zs = stepover_stations(bounds.min.z, bounds.max.z, stepover, 80).map_err(|e| bad(cmd, e))?;
    let xs = stepover_stations(bounds.min.x, bounds.max.x, stepover, 80).map_err(|e| bad(cmd, e))?;
    let ys = stepover_stations(bounds.min.y, bounds.max.y, stepover, 80).map_err(|e| bad(cmd, e))?;
    if xs.len().saturating_mul(ys.len()).saturating_mul(zs.len()) > 40_000 {
        return Err(bad(cmd, "the stepover is too fine"));
    }
    let half = stepover * 0.5;
    let mut samples = Vec::new();
    for (band, z) in zs.iter().enumerate().rev() {
        let forward = band % 2 == 0;
        let n = xs.len();
        for (row, y) in ys.iter().enumerate() {
            let row_fwd = forward == (row % 2 == 0);
            for k in 0..n {
                let Some(x) = xs.get(if row_fwd { k } else { n - 1 - k }).copied() else { continue };
                let Some((point, normal)) = upward_hit(&mesh, x, *y) else { continue };
                if (point.z - *z).abs() > half + 1e-6 {
                    continue;
                }
                let Some(centre) = accept_ball(&mesh, point, normal, radius) else { continue };
                samples.push(centre);
                if samples.len() > 2_500 {
                    return Err(bad(cmd, "the toolpath is too long"));
                }
            }
        }
    }
    push_surface_samples(s, cmd, "horizontal", tool, radius, stepover, samples, json!(zs))
}

/// Radial spokes from the body centre, projected onto the upper surface.
pub(crate) fn radial(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.radial";
    let (mesh, tool, radius) = surface_mesh(s, p, cmd)?;
    let stepover = opt_positive(cmd, p, "stepover")?.unwrap_or(radius);
    let bounds = mesh.bounds();
    let cx = (bounds.min.x + bounds.max.x) * 0.5;
    let cy = (bounds.min.y + bounds.max.y) * 0.5;
    let reach = ((bounds.max.x - bounds.min.x).hypot(bounds.max.y - bounds.min.y) * 0.5).max(stepover);
    let spokes = ((std::f64::consts::TAU * reach) / stepover).ceil().clamp(4.0, 72.0) as usize;
    let rings = stepover_stations(0.0, reach, stepover, 80).map_err(|e| bad(cmd, e))?;
    if spokes.saturating_mul(rings.len()) > 2_500 {
        return Err(bad(cmd, "the stepover is too fine"));
    }
    let mut samples = Vec::new();
    for s_i in 0..spokes {
        let ang = (s_i as f64) * std::f64::consts::TAU / spokes as f64;
        let (c, sn) = (ang.cos(), ang.sin());
        let outward = s_i % 2 == 0;
        let n = rings.len();
        for k in 0..n {
            let Some(r) = rings.get(if outward { k } else { n - 1 - k }).copied() else { continue };
            let Some((point, normal)) = upward_hit(&mesh, cx + r * c, cy + r * sn) else { continue };
            let Some(centre) = accept_ball(&mesh, point, normal, radius) else { continue };
            samples.push(centre);
            if samples.len() > 2_500 {
                return Err(bad(cmd, "the toolpath is too long"));
            }
        }
    }
    push_surface_samples(s, cmd, "radial", tool, radius, stepover, samples, json!({"spokes": spokes, "rings": rings.len()}))
}

/// Archimedean spiral in XY, projected onto the upper surface.
pub(crate) fn spiral(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.spiral";
    let (mesh, tool, radius) = surface_mesh(s, p, cmd)?;
    let stepover = opt_positive(cmd, p, "stepover")?.unwrap_or(radius);
    let bounds = mesh.bounds();
    let cx = (bounds.min.x + bounds.max.x) * 0.5;
    let cy = (bounds.min.y + bounds.max.y) * 0.5;
    let reach = ((bounds.max.x - bounds.min.x).hypot(bounds.max.y - bounds.min.y) * 0.5).max(stepover);
    let turns = (reach / stepover).ceil().clamp(1.0, 40.0);
    let steps = ((turns * 24.0).ceil() as usize).clamp(24, 2_500);
    let mut samples = Vec::new();
    for i in 0..steps {
        let t = i as f64 / (steps - 1).max(1) as f64;
        let r = reach * t;
        let ang = t * turns * std::f64::consts::TAU;
        let Some((point, normal)) = upward_hit(&mesh, cx + r * ang.cos(), cy + r * ang.sin()) else { continue };
        let Some(centre) = accept_ball(&mesh, point, normal, radius) else { continue };
        samples.push(centre);
        if samples.len() > 2_500 {
            return Err(bad(cmd, "the toolpath is too long"));
        }
    }
    push_surface_samples(s, cmd, "spiral", tool, radius, stepover, samples, json!({"turns": turns, "steps": steps}))
}

struct FoundHole {
    a: Vec3,
    b: Vec3,
    axis: Vec3,
    radius: f64,
    area: f64,
}

fn coaxial(a: &FoundHole, b: &FoundHole) -> bool {
    if a.axis.dot(b.axis).abs() < 0.999 {
        return false;
    }
    if (a.radius - b.radius).abs() > 0.05_f64.max(a.radius * 0.02) {
        return false;
    }
    let off = b.a - a.a;
    let radial = off - a.axis * off.dot(a.axis);
    radial.len() <= 0.2_f64.max(a.radius * 0.05)
}

fn absorb(a: &mut FoundHole, b: &FoundHole) {
    let origin = a.a;
    let t_b = (a.b - origin).dot(a.axis);
    let mut lo = 0.0_f64.min(t_b);
    let mut hi = 0.0_f64.max(t_b);
    for p in [b.a, b.b] {
        let t = (p - origin).dot(a.axis);
        lo = lo.min(t);
        hi = hi.max(t);
    }
    a.a = origin + a.axis * lo;
    a.b = origin + a.axis * hi;
    a.area += b.area;
}

pub(crate) fn holes(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.recognize_holes";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    let solid = s.model.state().body(&body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?.body.clone();
    let count = solid.face_count();
    if count > 5_000 {
        return Err(bad(cmd, "too many faces"));
    }
    let tol = (solid.size() * 1e-3).max(0.05);
    let infos = solid.faces(tol).map_err(|e| bad(cmd, e.to_string()))?;
    let mut found = Vec::new();
    let mut cylinders = 0usize;
    for info in &infos {
        let Some(solidcraft_kernel::FaceSurface::Cylinder { origin, axis, radius }) = solidcraft_kernel::face_surface(&solid, info.index) else {
            continue;
        };
        cylinders += 1;
        if cylinders > 200 {
            return Err(bad(cmd, "too many cylindrical faces"));
        }
        if !(radius.is_finite() && radius > 0.2 && radius < 1e5) {
            continue;
        }
        let Some(axis_u) = axis.normalized() else { continue };
        let sample = origin + axis_u.any_perp() * radius;
        if !sample.is_finite() {
            continue;
        }
        let Some(cyl) = solidcraft_kernel::cylinder_face_at(&solid, sample) else { continue };
        if !cyl.internal || (cyl.radius - radius).abs() > 0.5_f64.max(radius * 0.15) {
            continue;
        }
        let Some(fitted) = cyl.axis.normalized() else { continue };
        let axis = if fitted.z < 0.0 { -fitted } else { fitted };
        let hole = FoundHole { a: cyl.axis_point + cyl.axis * cyl.start, b: cyl.axis_point + cyl.axis * cyl.end, axis, radius, area: info.area };
        if !(hole.a.is_finite() && hole.b.is_finite()) {
            continue;
        }
        if let Some(slot) = found.iter_mut().find(|have| coaxial(have, &hole)) {
            absorb(slot, &hole);
        } else if found.len() >= 64 {
            return Err(bad(cmd, "too many holes"));
        } else {
            found.push(hole);
        }
    }
    let wcs = wcs_of(s, cmd)?;
    let mut jobs = Vec::new();
    let mut listed = Vec::new();
    let mut drill_n = 0usize;
    for hole in &found {
        let depth = hole.a.dist(hole.b);
        if !(depth > 0.05 && depth < 1e5) {
            continue;
        }
        let full = std::f64::consts::TAU * hole.radius * depth;
        let closed = full > 1e-6 && hole.area >= full * 0.45;
        if !closed {
            continue;
        }
        let local_a = wcs.to_local(hole.a);
        let local_b = wcs.to_local(hole.b);
        if !(local_a.is_finite() && local_b.is_finite()) {
            continue;
        }
        // Open end is the higher work-Z tip; cut into the hole along the cylinder axis.
        let (entry_w, exit_w) = if local_a.z >= local_b.z { (hole.a, hole.b) } else { (hole.b, hole.a) };
        let Some(into_w) = (exit_w - entry_w).normalized() else { continue };
        let entry = wcs.to_local(entry_w);
        let into = wcs.to_local(entry_w + into_w) - entry;
        let Some(into) = into.normalized() else { continue };
        let diameter = hole.radius * 2.0;
        let levels = cut_levels(cmd, depth, None, diameter)?;
        let vertical = into.x.abs() < 0.02 && into.y.abs() < 0.02 && into.z < -0.98;
        let gcode = if vertical {
            drill_gcode(entry.x, entry.y, &levels).map_err(|e| bad(cmd, e))?
        } else {
            drill_gcode_axis(entry, into, &levels).map_err(|e| bad(cmd, e))?
        };
        drill_n += 1;
        let area = std::f64::consts::PI * hole.radius * hole.radius;
        jobs.push((format!("Hole {drill_n}"), diameter, depth, area, gcode));
        listed.push(json!({
            "diameter": diameter,
            "depth": depth,
            "at": [entry_w.x, entry_w.y, entry_w.z],
            "axis": [into_w.x, into_w.y, into_w.z],
            "drill": true,
        }));
    }
    if s.doc.cam.len() + jobs.len() > 100 {
        return Err(bad(cmd, "too many toolpaths"));
    }
    let count = jobs.len();
    for (name, diameter, depth, area, gcode) in jobs {
        remember_job(s, cmd, &name, "", "drill", diameter, depth, area, gcode)?;
    }
    Ok(json!({"count": count, "holes": listed}))
}

pub(crate) fn post(s: &Session, p: &Value) -> Result<Value> {
    let cmd = "cam.post";
    let units = str_(p, "units").unwrap_or("mm");
    let unit_code = match units {
        "mm" => "G21",
        "inch" => "G20",
        _ => return Err(bad(cmd, "`units` must be mm or inch")),
    };
    let work = str_(p, "work").unwrap_or("G54");
    if !matches!(work, "G54" | "G55" | "G56" | "G57" | "G58" | "G59") {
        return Err(bad(cmd, "`work` must be G54, G55, G56, G57, G58 or G59"));
    }
    let safe = match p.get("safe_z") {
        None => None,
        Some(v) => {
            let z = v.as_f64().ok_or_else(|| bad(cmd, "`safe_z` must be a number"))?;
            if !(z.is_finite() && z.abs() < 1e9) {
                return Err(bad(cmd, "`safe_z` must be finite"));
            }
            if z < 0.0 {
                return Err(bad(cmd, "`safe_z` must be at or above the origin"));
            }
            Some(z)
        }
    };
    let name = str_(p, "name").unwrap_or("SolidCraft");
    if name.is_empty() || name.len() > 80 || name.chars().any(|c| c.is_control() || c == '(' || c == ')') {
        return Err(bad(cmd, "the program name is not usable"));
    }
    let mut program = format!("({name})\nN10 {unit_code}\nN20 {work}\n");
    let mut line = 30;
    if let Some(z) = safe {
        program.push_str(&format!("N{line} G0 Z{z:.3}\n"));
        line += 10;
    }
    if let Some(setup) = s.doc.cam_setups.last() {
        program.push_str(&format!("N{line} (WCS {:.3} {:.3} {:.3})\n", setup.origin[0], setup.origin[1], setup.origin[2]));
        line += 10;
    }
    for job in &s.doc.cam {
        for row in job.gcode.lines() {
            let row = row.trim();
            if row.is_empty() {
                continue;
            }
            program.push_str(&format!("N{line} {row}\n"));
            line += 10;
            if line > 100_000 {
                return Err(bad(cmd, "the program is too long"));
            }
        }
    }
    program.push_str(&format!("N{line} M30\n"));
    Ok(json!({"gcode": program, "units": units, "work": work}))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn rectangle(s: &mut Session, name: &str, p0: [f64; 2], p1: [f64; 2]) {
        run(s, "sketch.create", json!({"plane": "XY", "name": name}));
        run(s, "sketch.rectangle.two_point", json!({"p0": p0, "p1": p1}));
        run(s, "sketch.finish", json!({}));
    }

    #[test]
    fn a_setup_records_wcs_stock_and_bodies_and_a_bad_one_does_not() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Plate"}));
        let before = s.doc.to_json();
        assert!(s.execute("cam.setup", &json!({"stock": [10.0, 0.0, 10.0]})).is_err());
        assert!(s.execute("cam.setup", &json!({"stock": [10.0, 1.0e12, 10.0]})).is_err());
        assert!(s.execute("cam.setup", &json!({"stock": [10.0, 10.0, 10.0], "bodies": ["Missing"]})).is_err());
        assert!(s.execute("cam.setup", &json!({"stock": [10.0, 10.0, 10.0], "x_axis": [0.0, 0.0, 1.0], "z_axis": [0.0, 0.0, 2.0]})).is_err());
        assert_eq!(s.doc.to_json(), before);
        run(
            &mut s,
            "cam.setup",
            json!({
                "name": "Op1",
                "origin": [5.0, 0.0, 1.0],
                "x_axis": [0.0, 2.0, 0.0],
                "z_axis": [0.0, 0.0, 3.0],
                "stock": [40.0, 30.0, 20.0],
                "bodies": ["Plate"],
            }),
        );
        let setup = s.doc.cam_setups.last().unwrap();
        assert_eq!(setup.name, "Op1");
        assert_eq!(setup.origin, [5.0, 0.0, 1.0]);
        assert_eq!(setup.stock, [40.0, 30.0, 20.0]);
        assert_eq!(setup.bodies, vec!["Plate".to_string()]);
        assert!((setup.x_axis[1] - 1.0).abs() < 1e-12 && setup.x_axis[0].abs() < 1e-12, "{:?}", setup.x_axis);
        assert!((setup.z_axis[2] - 1.0).abs() < 1e-12, "{:?}", setup.z_axis);
        let loaded = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(loaded.cam_setups[0].x_axis, setup.x_axis);
        assert_eq!(loaded.cam_setups[0].bodies, setup.bodies);
        let legacy: solidcraft_doc::CamSetup =
            serde_json::from_str(r#"{"name":"S","origin":[0.0,0.0,0.0],"stock":[1.0,2.0,3.0],"bodies":[],"tools":[]}"#).unwrap();
        assert_eq!(legacy.x_axis, [1.0, 0.0, 0.0]);
        assert_eq!(legacy.z_axis, [0.0, 0.0, 1.0]);
        let job: CamJob = serde_json::from_value(json!({"name": "C", "sketch": "S", "tool_diameter": 1.0, "depth": 2.0, "gcode": "G1"})).unwrap();
        assert!(job.setup.is_empty());
    }

    #[test]
    fn contour_and_pocket_compensate_the_tool_and_step_the_depth() {
        let mut s = Session::default();
        rectangle(&mut s, "Pad", [0.0, 0.0], [20.0, 10.0]);
        let path = run(&mut s, "cam.contour", json!({"sketch": "Pad", "tool_diameter": 2.0, "depth": 3.0, "stepdown": 1.0, "name": "Outline"}));
        let gcode = path["gcode"].as_str().unwrap();
        assert!(gcode.contains("G21") && gcode.contains("G1"), "{gcode}");
        assert!(gcode.contains("X-1.000") && gcode.contains("Y-1.000"), "tool centre should sit a radius outside: {gcode}");
        assert!(gcode.contains("Z-1.000") && gcode.contains("Z-2.000") && gcode.contains("Z-3.000"), "{gcode}");
        assert_eq!(path["steps"], 3);
        let jobs = s.doc.cam.len();
        let before = s.doc.to_json();
        assert!(s.execute("cam.contour", &json!({"sketch": "Pad", "tool_diameter": -2.0, "depth": 3.0})).is_err());
        assert!(s.execute("cam.contour", &json!({"sketch": "Pad", "tool_diameter": 1.0e20, "depth": 3.0})).is_err());
        assert_eq!(s.doc.cam.len(), jobs);
        assert_eq!(s.doc.to_json(), before);

        rectangle(&mut s, "Pocket", [0.0, 0.0], [20.0, 20.0]);
        run(&mut s, "cam.setup", json!({"stock": [50.0, 50.0, 20.0], "origin": [10.0, 0.0, 0.0]}));
        let pocket = run(&mut s, "cam.pocket", json!({"sketch": "Pocket", "tool_diameter": 2.0, "depth": 10.0}));
        assert!((pocket["area"].as_f64().unwrap() - 18.0 * 18.0).abs() < 1e-6, "{pocket}");
        assert!(pocket["steps"].as_u64().unwrap() > 1, "{pocket}");
        let stored = s.doc.cam.iter().find(|c| c.kind == "pocket").unwrap();
        assert!(stored.gcode.contains("X-9.000"), "work origin shifts the toolpath: {}", stored.gcode);
        assert_eq!(stored.setup, "Setup");
        let sim = run(&mut s, "cam.simulate", json!({}));
        let removed = sim["removed"].as_f64().unwrap();
        let stock = 50.0 * 50.0 * 20.0;
        let pocket_only = 18.0 * 18.0 * 10.0;
        // The earlier contour is in the same document, so the measured removal includes it.
        assert!(removed + 1e-6 >= pocket_only, "{sim}");
        assert!((sim["stock"].as_f64().unwrap() - stock).abs() < 1e-6, "{sim}");
        assert!((sim["remaining"].as_f64().unwrap() - (stock - removed)).abs() < 1e-3, "{sim}");
        assert_eq!(sim["method"], "toolpath");
        let posted = run(&mut s, "cam.post", json!({}));
        let program = posted["gcode"].as_str().unwrap();
        assert!(program.contains("G21") && program.contains("G54") && program.contains("M30"), "{program}");
        let same = s.doc.to_json();
        assert!(s.execute("cam.post", &json!({"work": "G99"})).is_err());
        assert!(s.execute("cam.post", &json!({"units": "feet"})).is_err());
        assert_eq!(s.doc.to_json(), same);
        let g55 = run(&mut s, "cam.post", json!({"work": "G55", "units": "mm"}));
        assert!(g55["gcode"].as_str().unwrap().contains("G55"));
    }

    #[test]
    fn a_drill_pecks_and_a_cylinder_hole_is_proposed() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 20, "body_name": "Plate"}));
        let drilled = run(&mut s, "cam.drill", json!({"body": "Plate", "diameter": 4.0, "depth": 10.0}));
        assert!(drilled["steps"].as_u64().unwrap() > 1, "{drilled}");
        let gcode = s.doc.cam.last().unwrap().gcode.clone();
        assert!(gcode.contains("Z-4.000") && gcode.contains("Z-8.000") && gcode.contains("Z-10.000"), "{gcode}");
        let before = s.doc.to_json();
        assert!(s.execute("cam.drill", &json!({"body": "Plate", "diameter": 0.0, "depth": 5.0})).is_err());
        assert_eq!(s.doc.to_json(), before);

        run(&mut s, "solid.cylinder", json!({"base": [10.0, 10.0, -1.0], "radius": 4.0, "height": 22.0, "operation": "cut", "targets": ["Plate"]}));
        let jobs = s.doc.cam.len();
        let recognized = run(&mut s, "cam.recognize_holes", json!({"body": "Plate"}));
        assert!(recognized["count"].as_u64().unwrap() >= 1, "{recognized}");
        let hole = recognized["holes"].as_array().unwrap().iter().find(|h| h["drill"] == true).unwrap();
        assert!((hole["diameter"].as_f64().unwrap() - 8.0).abs() < 0.5, "{hole}");
        let at = hole["at"].as_array().unwrap();
        assert!((at[0].as_f64().unwrap() - 10.0).abs() < 1.5 && (at[1].as_f64().unwrap() - 10.0).abs() < 1.5, "{hole}");
        assert!(s.doc.cam.len() > jobs);
        assert!(s.model.state().body("Plate").is_some());

        run(&mut s, "solid.cylinder", json!({"radius": 5.0, "height": 12.0, "body_name": "Peg"}));
        let peg_jobs = s.doc.cam.len();
        let peg = run(&mut s, "cam.recognize_holes", json!({"body": "Peg"}));
        assert_eq!(peg["count"], 0, "{peg}");
        assert_eq!(s.doc.cam.len(), peg_jobs);
    }

    #[test]
    fn a_surface_path_uses_the_stepover_and_other_strategies_do_not_write() {
        let mut s = Session::default();
        run(&mut s, "solid.sphere", json!({"radius": 10, "body_name": "Ball"}));
        let before = s.doc.to_json();
        let err = s.execute("cam.pencil", &json!({"body": "Ball", "tool_diameter": 4.0})).unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
        assert!(s.execute("cam.flow", &json!({"body": "Ball", "tool_diameter": -1.0})).is_err());
        assert!(s.execute("cam.parallel", &json!({"body": "Ball", "tool_diameter": 4.0, "stepover": 0.0})).is_err());
        assert_eq!(s.doc.to_json(), before);

        let path = run(&mut s, "cam.parallel", json!({"body": "Ball", "tool_diameter": 4.0, "stepover": 4.0}));
        assert!((path["stepover"].as_f64().unwrap() - 4.0).abs() < 1e-9, "{path}");
        let mut rows = Vec::new();
        for row in path["rows"].as_array().unwrap() {
            rows.push(row.as_f64().unwrap());
        }
        assert!(rows.len() > 1, "{rows:?}");
        let mut saw_step = false;
        for pair in rows.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(gap <= 4.0 + 1e-6, "{gap} in {rows:?}");
            if (gap - 4.0).abs() < 1e-6 {
                saw_step = true;
            }
        }
        assert!(saw_step, "{rows:?}");
        for sample in path["samples"].as_array().unwrap() {
            let (x, y, z) = (sample[0].as_f64().unwrap(), sample[1].as_f64().unwrap(), sample[2].as_f64().unwrap());
            let radius = (x * x + y * y + z * z).sqrt();
            assert!(radius >= 8.0, "tool centre inside the offset sphere: {radius}");
        }
        let scallop = run(&mut s, "cam.scallop", json!({"body": "Ball", "tool_diameter": 4.0, "cusp": 0.2}));
        let want = solidcraft_geom::toolpath::scallop_stepover(2.0, 0.2).unwrap();
        assert!((scallop["stepover"].as_f64().unwrap() - want).abs() < 1e-6, "{scallop} vs {want}");
        for sample in scallop["samples"].as_array().unwrap() {
            let (x, y, z) = (sample[0].as_f64().unwrap(), sample[1].as_f64().unwrap(), sample[2].as_f64().unwrap());
            assert!((x * x + y * y + z * z).sqrt() >= 8.0, "{sample}");
        }

        let err3 = s.execute("cam.contour_3d", &json!({"body": "Ball", "tool_diameter": 4.0})).unwrap_err();
        assert!(err3.to_string().contains("not supported yet"), "{err3}");
        let radial = run(&mut s, "cam.radial", json!({"body": "Ball", "tool_diameter": 4.0, "stepover": 4.0}));
        assert!(radial["samples"].as_array().unwrap().len() >= 2, "{radial}");
        let spiral = run(&mut s, "cam.spiral", json!({"body": "Ball", "tool_diameter": 4.0, "stepover": 4.0}));
        assert!(spiral["samples"].as_array().unwrap().len() >= 2, "{spiral}");
        run(&mut s, "solid.box", json!({"length": 30, "width": 30, "height": 10, "body_name": "Block"}));
        let horiz = run(&mut s, "cam.horizontal", json!({"body": "Block", "tool_diameter": 4.0, "stepover": 4.0}));
        assert!(horiz["samples"].as_array().unwrap().len() >= 2, "{horiz}");
    }

    #[test]
    fn contour_and_pocket_cut_around_sketch_islands() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0.0, 0.0], "p1": [40.0, 40.0]}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [12.0, 12.0], "p1": [28.0, 28.0]}));
        run(&mut s, "sketch.finish", json!({}));

        let contour = run(&mut s, "cam.contour", json!({"sketch": "Frame", "tool_diameter": 2.0, "depth": 2.0, "stepdown": 2.0}));
        assert!(contour["islands"].as_u64().unwrap() >= 1, "{contour}");
        let gcode = contour["gcode"].as_str().unwrap();
        assert!(gcode.contains("G1") && gcode.contains("Z-2.000"), "{gcode}");

        let pocket = run(&mut s, "cam.pocket", json!({"sketch": "Frame", "tool_diameter": 2.0, "depth": 4.0, "stepover": 2.0}));
        assert!(pocket["islands"].as_u64().unwrap() >= 1, "{pocket}");
        assert!(pocket["passes"].as_u64().unwrap() >= 1, "{pocket}");
        assert!(pocket["area"].as_f64().unwrap() > 0.0, "{pocket}");
    }

    #[test]
    fn hole_recognition_drills_along_a_known_axis() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 20, "body_name": "Plate"}));
        run(
            &mut s,
            "cam.setup",
            json!({
                "name": "Tilt",
                "origin": [0.0, 0.0, 0.0],
                "x_axis": [1.0, 0.0, 0.0],
                "z_axis": [0.0, 1.0, 0.0],
                "stock": [50.0, 50.0, 50.0],
                "bodies": ["Plate"],
            }),
        );
        run(&mut s, "solid.cylinder", json!({"base": [10.0, 10.0, -1.0], "radius": 4.0, "height": 22.0, "operation": "cut", "targets": ["Plate"]}));
        let recognized = run(&mut s, "cam.recognize_holes", json!({"body": "Plate"}));
        assert!(recognized["count"].as_u64().unwrap() >= 1, "{recognized}");
        let hole = recognized["holes"].as_array().unwrap().iter().find(|h| h["drill"] == true).unwrap();
        let axis = hole["axis"].as_array().unwrap();
        assert!(axis[2].as_f64().unwrap().abs() > 0.9, "cylinder along model Z: {hole}");
        let job = s.doc.cam.iter().find(|c| c.kind == "drill" && c.name.starts_with("Hole")).unwrap();
        assert!(job.gcode.contains("G1 X") && job.gcode.contains("Y"), "{}", job.gcode);
    }
}
