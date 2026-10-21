//! Form: a 4×4 control grid, pulled, then thickened into a solid on the timeline.

use super::CommandSpec;
use super::features::{add_feature, check_expr};
use crate::params::{bad, expr, req_expr, str_, vec3};
use crate::{Result, Session};
use serde_json::{Value, json};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{FeatureKind, FormEdit, FormPull};
use solidcraft_geom::Vec3;
use solidcraft_kernel::{PointXform, bridge_deltas, point_edit_delta};

const MAX_CAGE_VERTS: usize = 20_000;
const MAX_SUBDIV_LEVEL: u32 = 6;
const MAX_EDITS: usize = 10_000;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("form.create", "Create Form", create).at("FORM", "CREATE").icon("box").params(
        "shape?: box|plane|cylinder|sphere|quadball|torus; length, width, thickness: expr; pulls?: [[index, x, y, z]] on the 4×4 grid; name?",
    ),
    CommandSpec::new("form.pull", "Pull Form", pull)
        .at("FORM", "MODIFY")
        .icon("move")
        .params("feature?: name (default: the latest form); index: 0…15; delta: [x,y,z]"),
    CommandSpec::new("form.edit_point", "Edit Form Point", edit_point)
        .at("FORM", "MODIFY")
        .icon("move")
        .params("feature?; index: 0…15; move?: [x,y,z] | rotate?: radians about Z | scale?: factor about the cage centre"),
    CommandSpec::new("form.insert_edge", "Insert Form Edge", insert_edge)
        .at("FORM", "MODIFY")
        .icon("line")
        .params("feature?; index: edge 0…23. Separates that edge's control points"),
    CommandSpec::new("form.insert_point", "Insert Form Point", insert_point).at("FORM", "MODIFY").icon("point").params("feature?; index: 0…15"),
    CommandSpec::new("form.subdivide", "Subdivide Form", subdivide)
        .at("FORM", "MODIFY")
        .icon("pattern_rect")
        .params("feature?; level: 1…6. Each level inserts edge and face midpoints, growing the cage from 4×4 toward at most 20000 points"),
    CommandSpec::new("form.bridge", "Bridge Form", bridge).at("FORM", "MODIFY").icon("loft").params("feature?; a, b: control indices 0…15"),
    CommandSpec::new("form.weld", "Weld Form", weld).at("FORM", "MODIFY").icon("combine").params("feature?; index: 0…15"),
    CommandSpec::new("form.crease", "Crease Form", crease).at("FORM", "MODIFY").icon("chamfer").params("feature?; index: edge 0…23"),
    CommandSpec::new("form.uncrease", "Uncrease Form", uncrease).at("FORM", "MODIFY").icon("fillet").params("feature?; index: edge 0…23"),
    CommandSpec::new("form.symmetry", "Form Symmetry", symmetry).at("FORM", "MODIFY").icon("mirror").params("feature?; plane: yz|xz|xy"),
    CommandSpec::new("form.extrude_face", "Extrude Form Face", extrude_face)
        .at("FORM", "MODIFY")
        .icon("extrude")
        .params("feature?; index: 0…15; delta: [x,y,z]"),
    CommandSpec::new("form.finish", "Finish Form", finish).at("FORM", "MODIFY").icon("finish").params("feature?; mode?: solid|surface"),
    CommandSpec::new("form.thicken", "Thicken Form", thicken_form).at("FORM", "MODIFY").icon("shell").params("feature?; thickness: expr"),
    CommandSpec::new("form.match", "Match Form Edge", match_edge)
        .at("FORM", "MODIFY")
        .icon("align")
        .params("feature?; edge: 0…23; target_edges: count to meet. The cage is refined until that edge is split into at least this many segments"),
];

fn pulls_of(p: &Value, cmd: &str) -> Result<Vec<FormPull>> {
    let rows = p.get("pulls").and_then(Value::as_array);
    if rows.is_some_and(|a| a.len() > 64) {
        return Err(bad(cmd, "a form keeps at most 64 pulls"));
    }
    let mut out = Vec::new();
    for row in rows.into_iter().flatten() {
        let Some(arr) = row.as_array() else { return Err(bad(cmd, "each pull is [index, x, y, z]")) };
        let index = arr.first().and_then(Value::as_u64).ok_or_else(|| bad(cmd, "a pull index is 0…15"))?;
        if index > 15 {
            return Err(bad(cmd, "a pull index is 0…15"));
        }
        let delta = vec3(&Value::Array(arr.iter().skip(1).cloned().collect())).ok_or_else(|| bad(cmd, "a pull delta is [x, y, z]"))?;
        out.push(FormPull { index: index as u32, delta });
    }
    Ok(out)
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.create";
    let shape = str_(p, "shape").unwrap_or("box").to_ascii_lowercase();
    if !matches!(shape.as_str(), "box" | "plane" | "cylinder" | "sphere" | "quadball" | "torus") {
        return Err(bad(cmd, "shape must be box, plane, cylinder, sphere, quadball or torus"));
    }
    let length = req_expr(cmd, p, "length")?;
    let width = req_expr(cmd, p, "width")?;
    let thickness = expr(p, "thickness").unwrap_or_else(|| "2 mm".into());
    for (name, e) in [("length", &length), ("width", &width), ("thickness", &thickness)] {
        check_expr(s, e, solidcraft_doc::expr::Kind::Length, cmd, name)?;
    }
    add_feature(
        s,
        p,
        FeatureKind::Form {
            shape,
            length,
            width,
            thickness,
            pulls: pulls_of(p, cmd)?,
            edits: Vec::new(),
            symmetry: None,
            creases: Vec::new(),
            finished: None,
        },
    )
}

fn pull(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.pull";
    let index = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`index` is 0…15"))?;
    if index > 15 {
        return Err(bad(cmd, "`index` is 0…15"));
    }
    let delta = p.get("delta").and_then(vec3).ok_or_else(|| bad(cmd, "`delta` must be [x, y, z]"))?;
    let name = str_(p, "feature").map(str::to_string);
    let id = s
        .doc
        .features
        .iter()
        .rev()
        .find(|f| name.as_ref().is_none_or(|n| f.name == *n || f.id.to_string() == *n) && matches!(f.kind, FeatureKind::Form { .. }))
        .map(|f| f.id)
        .ok_or_else(|| bad(cmd, "no form feature to pull"))?;
    if let Some(f) = s.doc_mut().feature_mut(id)
        && let FeatureKind::Form { pulls, .. } = &mut f.kind
    {
        if let Some(existing) = pulls.iter_mut().find(|q| q.index == index as u32) {
            existing.delta += delta;
        } else {
            pulls.push(FormPull { index: index as u32, delta });
        }
    }
    s.refresh();
    let err = s.model.result(id).and_then(|r| r.error.clone());
    if let Some(e) = err {
        return Err(model_err(cmd, e));
    }
    Ok(json!({"feature": id}))
}

fn latest_form(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    let name = str_(p, "feature").map(str::to_string);
    s.doc
        .features
        .iter()
        .rev()
        .find(|f| name.as_ref().is_none_or(|n| f.name == *n || f.id.to_string() == *n) && matches!(f.kind, FeatureKind::Form { .. }))
        .map(|f| f.id)
        .ok_or_else(|| bad(cmd, "no form feature"))
}

fn model_err(cmd: &str, err: impl Into<String>) -> crate::EngineError {
    let err = err.into();
    if let Some(i) = err.find("not supported yet") { bad(cmd, err[i..].to_string()) } else { bad(cmd, err) }
}

struct CagePreview {
    shape: String,
    length: f64,
    width: f64,
    pulls: Vec<(u32, Vec3)>,
}

/// Pulls the evaluator will apply, including the fixed offsets it still uses for older edit
/// kinds, so a rotate or a bridge is computed from the cage those edits already produced.
fn preview(s: &Session, id: u64, cmd: &str) -> Result<CagePreview> {
    let (shape, length_e, width_e, pulls) = {
        let Some(f) = s.doc.feature(id) else { return Err(bad(cmd, "no form feature")) };
        let FeatureKind::Form { shape, length, width, pulls, edits, .. } = &f.kind else {
            return Err(bad(cmd, "no form feature"));
        };
        let mut acc = Vec::new();
        for pull in pulls {
            acc.push((pull.index, pull.delta));
        }
        for edit in edits {
            match edit {
                FormEdit::MovePoint { index, delta } | FormEdit::ExtrudeFace { index, delta } => acc.push((*index, *delta)),
                FormEdit::InsertPoint { index } => acc.push((*index, Vec3::new(0.0, 0.0, 0.25))),
                FormEdit::Bridge { a, b } => {
                    acc.push((*a, Vec3::new(0.15, 0.0, 0.15)));
                    acc.push((*b, Vec3::new(-0.15, 0.0, 0.15)));
                }
                FormEdit::Weld { index } => acc.push((*index, Vec3::new(0.0, 0.0, -0.05))),
                FormEdit::InsertEdge { .. } | FormEdit::Subdivide { .. } | FormEdit::Crease { .. } => {}
            }
        }
        (shape.clone(), length.clone(), width.clone(), acc)
    };
    if pulls.len() > MAX_CAGE_VERTS {
        return Err(bad(cmd, "not supported yet: cage too large"));
    }
    if pulls.iter().any(|(_, delta)| !delta.is_finite()) {
        return Err(bad(cmd, "the edit must be finite"));
    }
    let length = s.doc.eval(&length_e, Kind::Length).map_err(|e| bad(cmd, e.to_string()))?;
    let width = s.doc.eval(&width_e, Kind::Length).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(CagePreview { shape, length, width, pulls })
}

fn push_edits(s: &mut Session, cmd: &str, id: u64, extra: Vec<FormEdit>) -> Result<Value> {
    if extra.is_empty() {
        return Ok(json!({"feature": id}));
    }
    if let Some(f) = s.doc_mut().feature_mut(id)
        && let FeatureKind::Form { edits, .. } = &mut f.kind
    {
        if edits.len().saturating_add(extra.len()) > MAX_EDITS {
            return Err(bad(cmd, "a form keeps at most 10000 edits"));
        }
        edits.extend(extra);
    } else {
        return Err(bad(cmd, "no form feature"));
    }
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(model_err(cmd, e));
    }
    Ok(json!({"feature": id}))
}

fn push_edit(s: &mut Session, cmd: &str, id: u64, edit: FormEdit) -> Result<Value> {
    push_edits(s, cmd, id, vec![edit])
}

fn edit_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.edit_point";
    let index = p.get("index").and_then(Value::as_u64).filter(|i| *i <= 15).ok_or_else(|| bad(cmd, "`index` is 0…15"))? as u32;
    let id = latest_form(s, p, cmd)?;
    let delta = if p.get("move").is_some() {
        p.get("move").and_then(vec3).ok_or_else(|| bad(cmd, "the edit must be finite"))?
    } else if p.get("rotate").is_some() {
        let turns = p.get("rotate").and_then(Value::as_f64).filter(|n| n.is_finite()).ok_or_else(|| bad(cmd, "the edit must be finite"))?;
        let cage = preview(s, id, cmd)?;
        point_edit_delta(&cage.shape, cage.length, cage.width, &cage.pulls, index, &PointXform::RotateZ(turns))
            .map_err(|e| model_err(cmd, e.to_string()))?
    } else if p.get("scale").is_some() {
        let scale =
            p.get("scale").and_then(Value::as_f64).filter(|n| n.is_finite() && n.abs() < 1e6).ok_or_else(|| bad(cmd, "the edit must be finite"))?;
        let cage = preview(s, id, cmd)?;
        point_edit_delta(&cage.shape, cage.length, cage.width, &cage.pulls, index, &PointXform::Scale(scale))
            .map_err(|e| model_err(cmd, e.to_string()))?
    } else {
        return Err(bad(cmd, "give `move`, `rotate` or `scale`"));
    };
    if !delta.is_finite() {
        return Err(bad(cmd, "the edit must be finite"));
    }
    push_edit(s, cmd, id, FormEdit::MovePoint { index, delta })
}

fn insert_edge(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.insert_edge";
    let index = p.get("index").and_then(Value::as_u64).filter(|i| *i < 24).ok_or_else(|| bad(cmd, "`index` is an edge 0…23"))? as u32;
    let id = latest_form(s, p, cmd)?;
    push_edit(s, cmd, id, FormEdit::InsertEdge { index })
}

fn insert_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.insert_point";
    let _index = p.get("index").and_then(Value::as_u64).filter(|i| *i <= 15).ok_or_else(|| bad(cmd, "`index` is 0…15"))?;
    let _id = latest_form(s, p, cmd)?;
    Err(bad(cmd, "not supported yet: inserting a control point into the quad cage"))
}

fn form_level(s: &Session, id: u64) -> u32 {
    let Some(f) = s.doc.feature(id) else { return 0 };
    let FeatureKind::Form { edits, .. } = &f.kind else { return 0 };
    edits.iter().fold(0u32, |n, e| match e {
        FormEdit::Subdivide { level } => n.saturating_add(*level),
        _ => n,
    })
}

fn subdivide(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.subdivide";
    let level = p.get("level").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`level` is 1…6"))?;
    if level == 0 || level > u64::from(MAX_SUBDIV_LEVEL) {
        return Err(bad(cmd, "subdivision level is at most 6"));
    }
    let id = latest_form(s, p, cmd)?;
    let total = form_level(s, id).saturating_add(level as u32);
    if total > MAX_SUBDIV_LEVEL {
        return Err(bad(cmd, "subdivision level is at most 6"));
    }
    let points = solidcraft_kernel::cage_points(total).map_err(|_| bad(cmd, "not supported yet: cage too large"))?;
    if points > MAX_CAGE_VERTS {
        return Err(bad(cmd, "not supported yet: cage too large"));
    }
    let mut out = push_edit(s, cmd, id, FormEdit::Subdivide { level: level as u32 })?;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("points".into(), json!(points));
    }
    Ok(out)
}

fn bridge(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.bridge";
    let a = p.get("a").and_then(Value::as_u64).filter(|i| *i <= 15).ok_or_else(|| bad(cmd, "`a` is 0…15"))? as u32;
    let b = p.get("b").and_then(Value::as_u64).filter(|i| *i <= 15).ok_or_else(|| bad(cmd, "`b` is 0…15"))? as u32;
    if a == b {
        return Err(bad(cmd, "not supported yet: bridging a control point to itself"));
    }
    let id = latest_form(s, p, cmd)?;
    let cage = preview(s, id, cmd)?;
    let (da, db) = bridge_deltas(&cage.shape, cage.length, cage.width, &cage.pulls, a, b).map_err(|e| model_err(cmd, e.to_string()))?;
    push_edits(s, cmd, id, vec![FormEdit::MovePoint { index: a, delta: da }, FormEdit::MovePoint { index: b, delta: db }])
}

fn weld(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.weld";
    let _index = p.get("index").and_then(Value::as_u64).filter(|i| *i <= 15).ok_or_else(|| bad(cmd, "`index` is 0…15"))?;
    let _id = latest_form(s, p, cmd)?;
    Err(bad(cmd, "not supported yet: welding a form control point"))
}

fn crease(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.crease";
    let index = p.get("index").and_then(Value::as_u64).filter(|i| *i < 24).ok_or_else(|| bad(cmd, "`index` is an edge 0…23"))? as u32;
    let id = latest_form(s, p, cmd)?;
    push_edit(s, cmd, id, FormEdit::Crease { index })
}

fn uncrease(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.uncrease";
    let index = p.get("index").and_then(Value::as_u64).filter(|i| *i < 24).ok_or_else(|| bad(cmd, "`index` is an edge 0…23"))? as u32;
    let id = latest_form(s, p, cmd)?;
    let doc = s.doc_mut();
    let Some(f) = doc.feature_mut(id) else { return Err(bad(cmd, "no form feature")) };
    if let FeatureKind::Form { edits, creases, .. } = &mut f.kind {
        edits.retain(|e| !matches!(e, FormEdit::Crease { index: i } if *i == index));
        creases.retain(|i| *i != index);
    }
    s.refresh();
    Ok(json!({"feature": id}))
}

fn symmetry(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.symmetry";
    let plane = str_(p, "plane").unwrap_or("yz").to_ascii_lowercase();
    if !matches!(plane.as_str(), "yz" | "xz" | "xy") {
        return Err(bad(cmd, "plane must be yz, xz or xy"));
    }
    let id = latest_form(s, p, cmd)?;
    if let Some(f) = s.doc_mut().feature_mut(id)
        && let FeatureKind::Form { symmetry, .. } = &mut f.kind
    {
        *symmetry = Some(plane);
    }
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(model_err(cmd, e));
    }
    Ok(json!({"feature": id}))
}

fn extrude_face(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.extrude_face";
    let index = p.get("index").and_then(Value::as_u64).filter(|i| *i <= 15).ok_or_else(|| bad(cmd, "`index` is 0…15"))? as u32;
    let delta = p.get("delta").and_then(vec3).filter(|d| d.is_finite()).ok_or_else(|| bad(cmd, "`delta` must be [x,y,z]"))?;
    let id = latest_form(s, p, cmd)?;
    push_edit(s, cmd, id, FormEdit::ExtrudeFace { index, delta })
}

fn finish(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.finish";
    let mode = str_(p, "mode").unwrap_or("solid").to_ascii_lowercase();
    if !matches!(mode.as_str(), "solid" | "surface") {
        return Err(bad(cmd, "mode must be solid or surface"));
    }
    let id = latest_form(s, p, cmd)?;
    if let Some(f) = s.doc_mut().feature_mut(id)
        && let FeatureKind::Form { finished, .. } = &mut f.kind
    {
        *finished = Some(mode);
    }
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(model_err(cmd, e));
    }
    let state = s.model.state();
    let bodies: Vec<_> = state.bodies.iter().filter(|b| b.feature == id).collect();
    if bodies.is_empty() {
        return Err(bad(cmd, "not supported yet: finishing this form"));
    }
    let surface = bodies.iter().any(|b| b.body.is_surface());
    let solid = bodies.iter().any(|b| !b.body.is_surface() && !b.body.is_mesh());
    if !surface && !solid {
        return Err(bad(cmd, "not supported yet: finishing this form"));
    }
    Ok(json!({"feature": id, "surface": surface}))
}

fn thicken_form(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.thicken";
    let thickness = crate::params::req_expr(cmd, p, "thickness")?;
    super::features::check_expr(s, &thickness, solidcraft_doc::expr::Kind::Length, cmd, "thickness")?;
    let id = latest_form(s, p, cmd)?;
    if let Some(f) = s.doc_mut().feature_mut(id)
        && let FeatureKind::Form { thickness: slot, finished, .. } = &mut f.kind
    {
        *slot = thickness;
        *finished = Some("solid".into());
    }
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(model_err(cmd, e));
    }
    Ok(json!({"feature": id}))
}

fn match_edge(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.match";
    let edge = p.get("edge").and_then(Value::as_u64).filter(|i| *i < 24).ok_or_else(|| bad(cmd, "`edge` is 0…23"))? as u32;
    let targets = p.get("target_edges").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`target_edges` is a count"))?;
    if targets == 0 || targets > 64 {
        return Err(bad(cmd, "`target_edges` is 1…64"));
    }
    let id = latest_form(s, p, cmd)?;
    if targets == 1 {
        return push_edit(s, cmd, id, FormEdit::InsertEdge { index: edge });
    }
    let mut pieces = 1u32;
    let mut want = 0u32;
    while pieces < targets as u32 {
        want += 1;
        if want > MAX_SUBDIV_LEVEL {
            return Err(bad(cmd, "subdivision level is at most 6"));
        }
        pieces = pieces.saturating_mul(2);
    }
    let have = form_level(s, id);
    if have >= want {
        let points = solidcraft_kernel::cage_points(have).map_err(|_| bad(cmd, "not supported yet: cage too large"))?;
        return Ok(json!({"feature": id, "points": points, "edge": edge}));
    }
    let add = want - have;
    let points = solidcraft_kernel::cage_points(have + add).map_err(|_| bad(cmd, "not supported yet: cage too large"))?;
    let mut out = push_edit(s, cmd, id, FormEdit::Subdivide { level: add })?;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("points".into(), json!(points));
        obj.insert("edge".into(), json!(edge));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{MAX_CAGE_VERTS, MAX_EDITS, MAX_SUBDIV_LEVEL};
    use crate::Session;
    use solidcraft_doc::{FeatureKind, FormEdit};

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn area(s: &Session) -> f64 {
        let state = s.model.state();
        state.bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).map(|m| m.area).unwrap_or(0.0)).sum()
    }

    fn volume_of(s: &Session, name: &str) -> f64 {
        let state = s.model.state();
        solidcraft_kernel::measure(&state.body(name).unwrap().body).unwrap().volume
    }

    fn body_named(s: &Session, feature: &str) -> String {
        let id = s.doc.features.iter().find(|f| f.name == feature).unwrap().id;
        s.model.state().bodies.iter().find(|b| b.feature == id).unwrap().name.clone()
    }

    /// Feature: fusion360-feature-parity, Property 9: Cage edits re-evaluate deterministically and change the body
    /// Feature: fusion360-feature-parity, Property 10: Symmetric sculpting yields a symmetric body
    #[test]
    fn cage_edits_symmetry_and_creases_replay() {
        let mut s = Session::default();
        run(&mut s, "form.create", json!({"shape": "box", "length": 30, "width": 18, "thickness": 0, "name": "Cage"}));
        let before = area(&s);
        run(&mut s, "form.edit_point", json!({"feature": "Cage", "index": 1, "move": [0, 0, 5]}));
        assert!((area(&s) - before).abs() > 1e-3);
        run(&mut s, "form.insert_edge", json!({"feature": "Cage", "index": 4}));
        run(&mut s, "form.crease", json!({"feature": "Cage", "index": 1}));
        run(&mut s, "form.symmetry", json!({"feature": "Cage", "plane": "yz"}));
        let sub = run(&mut s, "form.subdivide", json!({"feature": "Cage", "level": 1}));
        assert_eq!(sub["points"], 49);
        let moved = area(&s);
        let saved = s.doc.to_json();
        let again = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert!((area(&again) - moved).abs() < 1e-3, "{} vs {moved}", area(&again));
        let state = again.model.state();
        let body = state.bodies.iter().find(|b| b.name == "Body1").unwrap();
        let m = solidcraft_kernel::measure(&body.body).unwrap();
        assert!(m.centroid.x.abs() < 0.25, "yz symmetry centroid x {}", m.centroid.x);
        run(&mut s, "form.edit_point", json!({"feature": "Cage", "index": 0, "rotate": std::f64::consts::FRAC_PI_2}));
        let FeatureKind::Form { edits, .. } = &s.doc.features.iter().find(|f| f.name == "Cage").unwrap().kind else { panic!("form") };
        let delta = edits.iter().rev().find_map(|e| match e {
            FormEdit::MovePoint { index: 0, delta } => Some(*delta),
            _ => None,
        });
        let delta = delta.unwrap();
        assert!(delta.x.abs() > 2.0 && delta.z.abs() < 1e-6, "rotate should orbit the cage, got {delta:?}");
    }

    /// Feature: fusion360-feature-parity, Property 11: A finished form is a usable solid or surface
    #[test]
    fn finished_form_is_used_by_a_later_solid_and_surface_command() {
        let mut s = Session::default();
        run(&mut s, "form.create", json!({"shape": "box", "length": 40, "width": 30, "thickness": 2, "name": "Panel"}));
        let name = body_named(&s, "Panel");
        run(&mut s, "form.finish", json!({"feature": "Panel", "mode": "solid"}));
        let state = s.model.state();
        let body = state.body(&name).unwrap();
        assert!(!body.body.is_surface(), "a flat form finishes to a solid");
        let solid = volume_of(&s, &name);
        assert!(solid > 2000.0, "thickened slab volume {solid}");
        run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 30, "base": [20, 15, -2], "operation": "cut", "targets": [name]}));
        assert!(volume_of(&s, &name) < solid - 10.0);

        let mut skin = Session::default();
        run(&mut skin, "form.create", json!({"shape": "box", "length": 20, "width": 12, "thickness": 0, "name": "Sheet"}));
        run(&mut skin, "form.edit_point", json!({"feature": "Sheet", "index": 5, "move": [0, 0, 4]}));
        let sheet = body_named(&skin, "Sheet");
        let finished = run(&mut skin, "form.finish", json!({"feature": "Sheet", "mode": "surface"}));
        assert_eq!(finished["surface"], true);
        assert!(skin.model.state().body(&sheet).unwrap().body.is_surface());
        run(&mut skin, "surface.reverse", json!({"body": sheet}));
        assert!(skin.model.results.iter().all(|r| r.error.is_none()));
    }

    /// Feature: fusion360-feature-parity, Property 1: Hostile form input leaves the design unchanged
    #[test]
    fn hostile_form_inputs_leave_the_document_unchanged() {
        assert_eq!(MAX_CAGE_VERTS, solidcraft_kernel::MAX_CAGE_VERTS);
        assert_eq!(MAX_SUBDIV_LEVEL, solidcraft_kernel::MAX_SUBDIV_LEVEL);
        assert_eq!(MAX_EDITS, 10_000);

        let mut empty = Session::default();
        let untouched = empty.doc.to_json();
        for (id, params) in [
            ("form.edit_point", json!({"index": 1, "move": [0, 0, 1]})),
            ("form.finish", json!({"mode": "solid"})),
            ("form.crease", json!({"index": 1})),
            ("form.symmetry", json!({"plane": "yz"})),
            ("form.bridge", json!({"a": 0, "b": 3})),
            ("form.subdivide", json!({"level": 1})),
            ("form.insert_edge", json!({"index": 0})),
        ] {
            let err = empty.execute(id, &params).unwrap_err();
            assert!(err.to_string().contains("no form"), "{id}: {err}");
            assert_eq!(empty.doc.to_json(), untouched, "{id}");
        }
        run(&mut empty, "solid.box", json!({"length": 8, "width": 8, "height": 8, "body_name": "Box"}));
        let boxed = empty.doc.to_json();
        assert!(empty.execute("form.finish", &json!({"feature": "Box"})).is_err());
        assert_eq!(empty.doc.to_json(), boxed);

        let mut s = Session::default();
        run(&mut s, "form.create", json!({"shape": "box", "length": 20, "width": 12, "thickness": 2, "name": "Panel"}));
        let saved = s.doc.to_json();
        let nan = json!({"feature": "Panel", "index": 1, "move": [f64::NAN, 0.0, 1.0]});
        assert!(s.execute("form.edit_point", &nan).is_err());
        assert_eq!(s.doc.to_json(), saved);
        assert!(s.execute("form.edit_point", &json!({"feature": "Panel", "index": 1, "rotate": f64::NAN})).is_err());
        assert_eq!(s.doc.to_json(), saved);
        assert!(s.execute("form.edit_point", &json!({"feature": "Panel", "index": 1, "scale": f64::INFINITY})).is_err());
        assert_eq!(s.doc.to_json(), saved);

        let mut pulls = Vec::new();
        for i in 0..65 {
            pulls.push(json!([i % 16, 0.0, 0.0, 0.1]));
        }
        assert!(s.execute("form.create", &json!({"length": 10, "width": 10, "thickness": 1, "pulls": pulls})).is_err());
        assert_eq!(s.doc.to_json(), saved);

        assert!(s.execute("form.subdivide", &json!({"feature": "Panel", "level": 7})).unwrap_err().to_string().contains("at most 6"));
        assert_eq!(s.doc.to_json(), saved);
        let too_big = s.execute("form.subdivide", &json!({"feature": "Panel", "level": 6})).unwrap_err();
        assert!(too_big.to_string().contains("not supported yet"), "{too_big}");
        assert_eq!(s.doc.to_json(), saved);

        let interior = s.execute("form.bridge", &json!({"feature": "Panel", "a": 5, "b": 6})).unwrap_err();
        assert!(interior.to_string().contains("not supported yet"), "{interior}");
        assert_eq!(s.doc.to_json(), saved);
        run(&mut s, "form.create", json!({"shape": "sphere", "length": 20, "width": 12, "thickness": 0, "name": "Orb"}));
        let with_orb = s.doc.to_json();
        let pole = s.execute("form.insert_edge", &json!({"feature": "Orb", "index": 0})).unwrap_err();
        assert!(pole.to_string().contains("not supported yet"), "{pole}");
        assert_eq!(s.doc.to_json(), with_orb);
        let weld = s.execute("form.weld", &json!({"feature": "Panel", "index": 2})).unwrap_err();
        assert!(weld.to_string().contains("not supported yet"), "{weld}");
        assert_eq!(s.doc.to_json(), with_orb);

        let id = s.doc.features.iter().find(|f| f.name == "Panel").unwrap().id;
        {
            let doc = s.doc_mut();
            let feature = doc.feature_mut(id).unwrap();
            if let FeatureKind::Form { edits, .. } = &mut feature.kind {
                edits.resize(MAX_EDITS, FormEdit::Crease { index: 0 });
            }
        }
        let stuffed = s.doc.to_json();
        let over = s.execute("form.crease", &json!({"feature": "Panel", "index": 2})).unwrap_err();
        assert!(over.to_string().contains("10000"), "{over}");
        assert_eq!(s.doc.to_json(), stuffed);
    }

    /// Feature: fusion360-feature-parity, Property 21: An old form with only pulls loads
    #[test]
    fn an_old_form_without_cage_fields_loads() {
        let old = r#"{
            "format": "solidcraft/11",
            "name": "OldForm",
            "units": "mm",
            "params": [],
            "next_id": 2,
            "features": [{
                "id": 1,
                "name": "Form1",
                "suppressed": false,
                "type": "form",
                "shape": "cylinder",
                "length": "20 mm",
                "width": "16 mm",
                "thickness": "0 mm",
                "pulls": [{"index": 6, "delta": [0.0, 1.5, 2.0]}]
            }]
        }"#;
        let doc = solidcraft_doc::Document::from_json(old).unwrap();
        let FeatureKind::Form { edits, symmetry, creases, finished, pulls, .. } = &doc.features[0].kind else { panic!("form") };
        assert!(edits.is_empty() && symmetry.is_none() && creases.is_empty() && finished.is_none());
        assert_eq!(pulls.len(), 1);
        let s = Session::new(doc);
        assert!(
            s.model.results.iter().all(|r| r.error.is_none()),
            "{:?}",
            s.model.results.iter().filter_map(|r| r.error.clone()).collect::<Vec<_>>()
        );
        assert!(!s.model.state().bodies.is_empty());
        let again = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(again, *s.doc);
    }
}
