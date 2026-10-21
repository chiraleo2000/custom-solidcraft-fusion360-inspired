//! Parametric reconstruction of imported mechanical mesh bodies.

use serde_json::Value;
use solidcraft_doc::FeatureKind;
use solidcraft_doc::expr::Kind;

use super::{
    CommandSpec,
    features::{add_feature, check_expr},
};
use crate::{
    Result, Session,
    params::{bad, expr, str_, vec3},
};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("mesh.convert", "Convert Mesh", convert)
        .at("MESH", "MODIFY")
        .icon("import")
        .params("body: mesh body name; mode?: prismatic|faceted (prismatic merges coplanar facets, faceted keeps a face per triangle); tolerance?: length expression (default 0.05 mm, prismatic only); name?"),
    CommandSpec::new("mesh.plane_cut", "Plane Cut", plane_cut)
        .at("MESH", "PREPARE")
        .icon("split")
        .params("body: mesh body name; origin: [x,y,z]; normal: [x,y,z]; keep: [x,y,z] (a point on the side to keep)"),
    CommandSpec::new("mesh.repair", "Repair Mesh", repair).at("MESH", "PREPARE").icon("import").params("body: mesh body name"),
    CommandSpec::new("mesh.reduce", "Reduce Mesh", reduce)
        .at("MESH", "PREPARE")
        .icon("import")
        .params("body: mesh body name; ratio: 0…1 (fraction of detail to keep)"),
    CommandSpec::new("mesh.face_groups", "Face Groups", face_groups)
        .at("MESH", "PREPARE")
        .icon("import")
        .noundo()
        .params("body: mesh body name. Groups triangles by which axis their normal faces"),
    CommandSpec::new("mesh.section", "Mesh Section", section)
        .at("MESH", "CREATE")
        .icon("sketch")
        .params("body: mesh body name; origin: [x,y,z]; normal: [x,y,z]; name?: sketch name. Draws every closed cut loop as sketch lines"),
    CommandSpec::new("mesh.remesh", "Remesh", remesh)
        .at("MESH", "MODIFY")
        .icon("import")
        .params("body: mesh body name; target: triangle count"),
    CommandSpec::new("mesh.smooth", "Smooth Mesh", smooth)
        .at("MESH", "MODIFY")
        .icon("fillet")
        .params("body: mesh body name; iterations?: 1…50"),
    CommandSpec::new("mesh.separate", "Separate Mesh", separate).at("MESH", "MODIFY").icon("split").params("body: mesh body name"),
    CommandSpec::new("mesh.combine", "Combine Mesh", combine)
        .at("MESH", "MODIFY")
        .icon("combine")
        .params("a, b: mesh body names; operation?: join"),
    CommandSpec::new("mesh.merge_groups", "Merge Face Groups", merge_groups)
        .at("MESH", "MODIFY")
        .icon("stitch")
        .params("body: mesh body name. Welds the mesh the way a repair does"),
    CommandSpec::new("mesh.shell", "Shell Mesh", shell)
        .at("MESH", "MODIFY")
        .icon("shell")
        .params("body: mesh body name; thickness: expr"),
    CommandSpec::new("mesh.reverse", "Reverse Mesh", reverse).at("MESH", "MODIFY").icon("mirror").params("body: mesh body name"),
    CommandSpec::new("mesh.deviation", "Mesh Deviation", deviation)
        .noundo()
        .params("body: solid name; mesh: mesh body name → {max, mean} millimetres"),
];

fn convert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.convert";
    let body = str_(p, "body").filter(|b| !b.trim().is_empty()).ok_or_else(|| bad(cmd, "select a mesh body"))?;
    let state = s.model.state();
    let source = state.body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
    if !source.body.is_mesh() {
        return Err(bad(cmd, "select a mesh body"));
    }
    if s.doc.model_body_component(source) != s.active_component {
        return Err(bad(cmd, "activate the mesh body's component before converting it"));
    }
    let tolerance = expr(p, "tolerance").unwrap_or_else(|| "0.05 mm".into());
    check_expr(s, &tolerance, Kind::Length, cmd, "tolerance")?;
    let mode = str_(p, "mode").unwrap_or("prismatic").to_ascii_lowercase();
    if mode != "prismatic" && mode != "faceted" {
        return Err(bad(cmd, "mode must be prismatic or faceted"));
    }
    add_feature(s, p, FeatureKind::MeshConvert { body: body.into(), tolerance, mode })
}

fn plane_cut(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.plane_cut";
    let body = str_(p, "body").filter(|b| !b.trim().is_empty()).ok_or_else(|| bad(cmd, "select a mesh body"))?.to_string();
    let origin = p.get("origin").and_then(vec3).ok_or_else(|| bad(cmd, "`origin` must be [x,y,z]"))?;
    let normal = p.get("normal").and_then(vec3).ok_or_else(|| bad(cmd, "`normal` must be [x,y,z]"))?;
    let keep = p.get("keep").and_then(vec3).ok_or_else(|| bad(cmd, "`keep` must be [x,y,z]"))?;
    add_feature(s, p, FeatureKind::MeshCut { body, origin, normal, keep })
}

fn repair(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.repair";
    let body = str_(p, "body").filter(|b| !b.trim().is_empty()).ok_or_else(|| bad(cmd, "select a mesh body"))?.to_string();
    add_feature(s, p, FeatureKind::MeshRepair { body })
}

fn reduce(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.reduce";
    let body = str_(p, "body").filter(|b| !b.trim().is_empty()).ok_or_else(|| bad(cmd, "select a mesh body"))?.to_string();
    let ratio = expr(p, "ratio").unwrap_or_else(|| "0.5".into());
    check_expr(s, &ratio, Kind::Unitless, cmd, "ratio")?;
    add_feature(s, p, FeatureKind::MeshReduce { body, ratio })
}

fn face_groups(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.face_groups";
    let body = str_(p, "body").filter(|b| !b.trim().is_empty()).ok_or_else(|| bad(cmd, "select a mesh body"))?;
    let state = s.model.state();
    let source = state.body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
    let groups = solidcraft_kernel::mesh_face_groups(&source.body).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(serde_json::json!({
        "body": body,
        "groups": groups.iter().map(|(name, count)| serde_json::json!({"name": name, "triangles": count})).collect::<Vec<_>>(),
    }))
}

fn section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.section";
    let body = str_(p, "body").filter(|b| !b.trim().is_empty()).ok_or_else(|| bad(cmd, "select a mesh body"))?;
    let origin = p.get("origin").and_then(vec3).ok_or_else(|| bad(cmd, "`origin` must be [x,y,z]"))?;
    let normal = p.get("normal").and_then(vec3).and_then(|n| n.normalized()).ok_or_else(|| bad(cmd, "`normal` must be a direction"))?;
    let loops = {
        let state = s.model.state();
        let source = state.body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
        solidcraft_kernel::mesh_section(&source.body, origin, normal).map_err(|e| bad(cmd, e.to_string()))?
    };
    let name = str_(p, "name").unwrap_or("Mesh Section");
    let x = normal.any_perp();
    let y = normal.cross(x);
    let plane = solidcraft_geom::Plane { origin, x, y };
    s.execute(
        "sketch.create",
        &serde_json::json!({"plane": {"origin": [origin.x, origin.y, origin.z], "x_dir": [x.x, x.y, x.z], "y_dir": [y.x, y.y, y.z]}, "name": name}),
    )?;
    for ring in &loops {
        let chain: Vec<[f64; 2]> = ring
            .iter()
            .map(|q| {
                let p = plane.to_local(*q);
                [p.x, p.y]
            })
            .collect();
        if chain.len() >= 2 {
            s.execute("sketch.line", &serde_json::json!({"points": chain, "infer": false, "connect": false, "closed": true}))?;
        }
    }
    s.execute("sketch.finish", &serde_json::json!({}))?;
    let points = loops.iter().map(Vec::len).sum::<usize>();
    Ok(serde_json::json!({"sketch": name, "points": points, "loops": loops.len()}))
}

fn mesh_feature(s: &mut Session, p: &Value, cmd: &str, kind: impl FnOnce(String) -> Result<FeatureKind>) -> Result<Value> {
    let body = str_(p, "body").filter(|b| !b.trim().is_empty()).ok_or_else(|| bad(cmd, "select a mesh body"))?.to_string();
    add_feature(s, p, kind(body)?)
}

fn remesh(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.remesh";
    if let Some(Value::Number(n)) = p.get("target") {
        let Some(v) = n.as_f64() else {
            return Err(bad(cmd, "remesh target is out of range"));
        };
        if !v.is_finite() || v < 1.0 || v > solidcraft_kernel::MAX_TRIANGLES as f64 {
            return Err(bad(cmd, "remesh target is out of range"));
        }
    }
    let target = expr(p, "target").unwrap_or_else(|| "1000".into());
    check_expr(s, &target, Kind::Unitless, cmd, "target")?;
    mesh_feature(s, p, cmd, |body| Ok(FeatureKind::MeshRemesh { body, target }))
}

fn smooth(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.smooth";
    let iterations = p.get("iterations").and_then(Value::as_u64).unwrap_or(1);
    if iterations == 0 || iterations > 50 {
        return Err(bad(cmd, "iterations are 1…50"));
    }
    mesh_feature(s, p, cmd, |body| Ok(FeatureKind::MeshSmooth { body, iterations: iterations as u32 }))
}

fn separate(s: &mut Session, p: &Value) -> Result<Value> {
    mesh_feature(s, p, "mesh.separate", |body| Ok(FeatureKind::MeshSeparate { body }))
}

fn combine(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.combine";
    let a = str_(p, "a").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`a` is required"))?.to_string();
    let b = str_(p, "b").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`b` is required"))?.to_string();
    let operation = str_(p, "operation").unwrap_or("join").to_ascii_lowercase();
    if operation != "join" {
        return Err(bad(cmd, "not supported yet: a mesh combine that is not a join"));
    }
    add_feature(s, p, FeatureKind::MeshCombine { a, b, operation })
}

fn merge_groups(s: &mut Session, p: &Value) -> Result<Value> {
    mesh_feature(s, p, "mesh.merge_groups", |body| Ok(FeatureKind::MeshMergeGroups { body }))
}

fn shell(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.shell";
    let thickness = expr(p, "thickness").unwrap_or_else(|| "1 mm".into());
    check_expr(s, &thickness, Kind::Length, cmd, "thickness")?;
    mesh_feature(s, p, cmd, |body| Ok(FeatureKind::MeshShell { body, thickness }))
}

fn reverse(s: &mut Session, p: &Value) -> Result<Value> {
    mesh_feature(s, p, "mesh.reverse", |body| Ok(FeatureKind::MeshReverse { body }))
}

fn deviation(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.deviation";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?;
    let mesh = str_(p, "mesh").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`mesh` is required"))?;
    let state = s.model.state();
    let solid = state.body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
    let mesh_body = state.body(mesh).ok_or_else(|| bad(cmd, format!("no body `{mesh}`")))?;
    let (max, mean) = solidcraft_kernel::mesh_deviation(&solid.body, &mesh_body.body).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(serde_json::json!({"max": max, "mean": mean}))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use solidcraft_doc::{FeatureKind, MeshData};
    use solidcraft_geom::Seg2;

    use crate::{EngineError, Session, find_command};

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn with_scan() -> (Session, usize) {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}));
        let mesh = s.model.state().body("Box").unwrap().body.tessellate(0.2).unwrap();
        let count = mesh.triangles.len();
        let positions: Vec<f64> = mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
        let triangles: Vec<u32> = mesh.triangles.iter().flat_map(|t| *t).collect();
        s.doc_mut()
            .add_feature(
                FeatureKind::MeshImport { file: "scan".into(), meshes: vec![MeshData { name: "Scan".into(), positions, triangles, color: None }] },
                Some("Scan"),
            )
            .unwrap();
        s.refresh();
        (s, count)
    }

    #[test]
    fn section_groups_deviation_and_remesh_round_trip_a_box() {
        let (mut s, count) = with_scan();
        assert!(!find_command("mesh.deviation").unwrap().undoable);
        assert!(!find_command("mesh.face_groups").unwrap().undoable);
        let features = s.doc.features.len();
        let undo = s.undo.len();
        let groups = run(&mut s, "mesh.face_groups", json!({"body": "Scan"}));
        let grouped: u64 = groups["groups"].as_array().unwrap().iter().map(|g| g["triangles"].as_u64().unwrap()).sum();
        assert_eq!(grouped, count as u64, "{groups}");
        assert_eq!(s.doc.features.len(), features);
        assert_eq!(s.undo.len(), undo);
        let before = s.doc.to_json();
        let section = run(&mut s, "mesh.section", json!({"body": "Scan", "origin": [0, 0, 5], "normal": [0, 0, 1], "name": "Cut"}));
        assert!(section["loops"].as_u64().unwrap() >= 1, "{section}");
        let state = s.model.state();
        let sketch = state.sketches.iter().find(|k| k.name == "Cut").unwrap();
        assert!(!sketch.sketch.curves.is_empty());
        for ci in 0..sketch.sketch.curves.len() {
            for seg in sketch.sketch.segs(ci) {
                if let Seg2::Line { a, b } = seg {
                    for q in [a, b] {
                        let w = sketch.plane.to_world(q);
                        assert!((w.z - 5.0).abs() < 1e-4, "section point left the plane: {w:?}");
                    }
                }
            }
        }
        let undo_after_section = s.undo.len();
        let report = run(&mut s, "mesh.deviation", json!({"body": "Box", "mesh": "Scan"}));
        let (max, mean) = (report["max"].as_f64().unwrap(), report["mean"].as_f64().unwrap());
        assert!(max.is_finite() && mean.is_finite() && max >= 0.0 && mean >= 0.0 && mean <= max + 1e-9, "{report}");
        assert!(mean < 0.05 && max < 0.05, "{report}");
        assert_eq!(s.undo.len(), undo_after_section, "deviation is read-only");
        assert!(undo_after_section > undo, "the section is undoable");
        let missed = s.doc.to_json();
        assert!(s.execute("mesh.section", &json!({"body": "Scan", "origin": [0, 0, 80], "normal": [0, 0, 1]})).is_err());
        assert_eq!(s.doc.to_json(), missed);
        let target = (count / 2).max(1);
        run(&mut s, "mesh.remesh", json!({"body": "Scan", "target": target}));
        let left = s.model.state().body("Scan").unwrap().body.triangle_mesh().unwrap().1.len();
        assert!(left > 0 && left <= count, "{left} vs {count} target {target}");
        assert_ne!(s.doc.to_json(), before);
        let (mut fresh, tris) = with_scan();
        run(&mut fresh, "mesh.convert", json!({"body": "Scan", "mode": "faceted"}));
        let body = fresh.model.state().body("Scan").unwrap().body.clone();
        assert!(!body.is_mesh());
        assert_eq!(body.face_count(), tris);
    }

    #[test]
    fn a_cut_through_two_meshes_returns_two_loops() {
        let mut s = Session::default();
        let left = solidcraft_kernel::box_solid(solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let right = solidcraft_kernel::box_solid(solidcraft_geom::Vec3::new(30.0, 0.0, 0.0), solidcraft_geom::Vec3::new(40.0, 10.0, 10.0)).unwrap();
        let (la, ra) = (left.tessellate(0.2).unwrap(), right.tessellate(0.2).unwrap());
        let la_body = solidcraft_kernel::mesh_body(&la.positions, &la.triangles).unwrap();
        let ra_body = solidcraft_kernel::mesh_body(&ra.positions, &ra.triangles).unwrap();
        let both = solidcraft_kernel::mesh_combine(&la_body, &ra_body).unwrap();
        let (positions, triangles) = both.triangle_mesh().unwrap();
        s.doc_mut()
            .add_feature(
                FeatureKind::MeshImport {
                    file: "pair".into(),
                    meshes: vec![MeshData {
                        name: "Pair".into(),
                        positions: positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                        triangles: triangles.iter().flat_map(|t| *t).collect(),
                        color: None,
                    }],
                },
                Some("Pair"),
            )
            .unwrap();
        s.refresh();
        let section = run(&mut s, "mesh.section", json!({"body": "Pair", "origin": [0, 0, 5], "normal": [0, 0, 1], "name": "Both"}));
        assert!(section["loops"].as_u64().unwrap() >= 2, "{section}");
    }

    /// Feature: fusion360-feature-parity, Property 1: Hostile mesh parameters never panic
    #[test]
    fn property_1_hostile_mesh_commands_do_not_panic() {
        let ids = [
            "mesh.convert",
            "mesh.plane_cut",
            "mesh.repair",
            "mesh.reduce",
            "mesh.face_groups",
            "mesh.section",
            "mesh.remesh",
            "mesh.smooth",
            "mesh.separate",
            "mesh.combine",
            "mesh.merge_groups",
            "mesh.shell",
            "mesh.reverse",
            "mesh.deviation",
        ];
        let payloads = [
            json!(null),
            json!({}),
            json!({"body": ""}),
            json!({"body": "Missing"}),
            json!({"body": "Scan", "target": 0}),
            json!({"body": "Scan", "target": -3}),
            json!({"body": "Scan", "target": 1e20}),
            json!({"body": "Scan", "ratio": 0}),
            json!({"body": "Scan", "ratio": 4}),
            json!({"body": "Scan", "mode": "nurbs"}),
            json!({"body": "Scan", "origin": [0, 0, 0], "normal": [0, 0, 0], "keep": [1, 0, 0]}),
            json!({"body": "Scan", "iterations": 1000}),
            json!({"body": "Scan", "thickness": "nope"}),
            json!({"a": "Scan", "b": "Scan", "operation": "cut"}),
            json!({"body": "Box", "mesh": "Missing"}),
            json!({"body": "Scan", "tolerance": "nope"}),
        ];
        let base = {
            let mut s = Session::default();
            s.doc_mut()
                .add_feature(
                    FeatureKind::MeshImport {
                        file: "scan".into(),
                        meshes: vec![MeshData {
                            name: "Scan".into(),
                            positions: vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0, 2.0, 3.0, 8.0],
                            triangles: vec![0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3],
                            color: None,
                        }],
                    },
                    Some("Scan"),
                )
                .unwrap();
            s.refresh();
            s.doc.as_ref().clone()
        };
        let mut cases = 0usize;
        for id in ids {
            for payload in &payloads {
                let mut s = Session::new(base.clone());
                let before = s.doc.to_json();
                match s.execute(id, payload) {
                    Err(EngineError::Internal(cmd, msg)) => panic!("{cmd} {payload} panicked: {msg}"),
                    Err(_) => assert_eq!(s.doc.to_json(), before, "{id} changed the design on error"),
                    Ok(_) => {}
                }
                cases += 1;
            }
        }
        let (mut s, _) = with_scan();
        let before = s.doc.to_json();
        let err = s.execute("mesh.combine", &json!({"a": "Scan", "b": "Box", "operation": "cut"})).unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
        assert_eq!(s.doc.to_json(), before);
        let err = s.execute("mesh.remesh", &json!({"body": "Scan", "target": "30000000"})).unwrap_err();
        assert!(!matches!(err, EngineError::Internal(_, _)), "{err}");
        assert_eq!(s.doc.to_json(), before);
        assert!(cases >= 100, "{cases}");
    }
}
