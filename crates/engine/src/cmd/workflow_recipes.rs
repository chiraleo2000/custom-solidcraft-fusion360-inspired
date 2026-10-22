//! End-to-end recipes for the standard workflows. Every step is a registered command.
//! An unsupported step must say "not supported yet" and leave the design unchanged.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcraft_doc::canvas::base64_encode;

use crate::{Session, find_command};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    assert!(find_command(id).is_some(), "{id} is not a registered command");
    let before = s.doc.to_json();
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not supported yet") {
                assert_eq!(s.doc.to_json(), before, "{id} changed the design after an unsupported step");
                assert_consistent(s);
            }
            panic!("{id} {p}: {msg}");
        }
    }
}

fn assert_consistent(s: &Session) {
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone().map(|e| format!("{}: {e}", r.name))).collect();
    assert!(errors.is_empty(), "{errors:?}");
}

fn rel(got: f64, want: f64) -> f64 {
    (got - want).abs() / want.abs().max(1.0)
}

fn body_volume(s: &mut Session, name: &str) -> f64 {
    let measured = run(s, "inspect.measure", json!({}));
    measured["bodies"]
        .as_array()
        .and_then(|bodies| bodies.iter().find(|body| body["name"] == name))
        .and_then(|body| body["volume_mm3"].as_f64())
        .unwrap_or(f64::NAN)
}

fn finite_deviation(report: &Value) {
    let max = report["max"].as_f64().unwrap_or(f64::NAN);
    let mean = report["mean"].as_f64().unwrap_or(f64::NAN);
    assert!(max.is_finite() && mean.is_finite() && max >= 0.0 && mean >= 0.0 && mean <= max + 1e-9, "{report}");
}

/// Req 13.1: sketch, extrude, cut, hole and mirror, then a parameter edit rebuilds dependents.
#[test]
fn req_13_1_parametric_part_rebuilds_when_a_dimension_changes() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "len", "expression": "60 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    let rect = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let bottom = rect["sides"]["bottom"].as_str().unwrap();
    let left = rect["sides"]["left"].as_str().unwrap();
    let corner = rect["corners"]["bottom_left"].as_str().unwrap();
    run(&mut s, "sketch.constraint.coincident", json!({"a": corner, "b": "origin"}));
    run(&mut s, "sketch.dimension", json!({"entities": [bottom], "value": 40}));
    run(&mut s, "sketch.dimension", json!({"entities": [left], "value": "len / 2"}));
    let sketch = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(sketch["dof"], 0, "{sketch}");
    assert_eq!(sketch["fully_constrained"], true, "{sketch}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Base", "distance": 10, "operation": "new", "body_name": "Block", "name": "BlockExtrude"}));
    assert!(rel(body_volume(&mut s, "Block"), 40.0 * 30.0 * 10.0) < 1e-6);

    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pocket"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [20, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Pocket", "distance": 5, "operation": "cut", "targets": ["Block"], "name": "PocketCut"}));
    let cut = body_volume(&mut s, "Block");
    assert!(rel(cut, 40.0 * 30.0 * 10.0 - 500.0) < 1e-6, "{cut}");

    run(&mut s, "solid.hole", json!({"position": [30, 5, 10], "diameter": 6, "name": "Hole"}));
    let drilled = body_volume(&mut s, "Block");
    assert!(rel(cut - drilled, PI * 9.0 * 10.0) < 1e-3, "{cut} -> {drilled}");
    run(&mut s, "solid.mirror", json!({"features": ["Hole"], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}, "name": "Twin"}));
    let mirrored = body_volume(&mut s, "Block");
    assert!(rel(drilled - mirrored, PI * 9.0 * 10.0) < 1e-3, "{drilled} -> {mirrored}");
    assert_eq!(run(&mut s, "inspect.measure", json!({}))["body_count"], 1);
    assert_consistent(&s);

    let changed = run(&mut s, "parameters.change", json!({"name": "len", "expression": "100 mm"}));
    assert!(changed["errors"].as_array().is_some_and(|e| e.is_empty()), "{changed}");
    assert!(changed["recomputed"].as_u64().unwrap_or(0) >= 1, "{changed}");
    let grown = body_volume(&mut s, "Block");
    // The extra 20 mm of length is clear of the pocket and both holes.
    assert!(rel(grown - mirrored, 40.0 * 20.0 * 10.0) < 1e-4, "{mirrored} -> {grown}");
    assert_consistent(&s);

    let edited = run(&mut s, "timeline.edit", json!({"feature": "PocketCut", "set": {"extent": {"distance": "3"}}}));
    assert!(edited["recomputed"].as_u64().unwrap_or(0) >= 1, "{edited}");
    let shallower = body_volume(&mut s, "Block");
    assert!(rel(shallower - grown, 10.0 * 10.0 * 2.0) < 1e-4, "{grown} -> {shallower}");
    assert_consistent(&s);
}

/// Req 13.2: a mesh built through the export/insert commands, a section, a rebuilt solid, finite deviation.
/// No command constructs a mesh body in memory, so the mesh is the part's own STL round-trip.
#[test]
fn req_13_2_mesh_section_rebuilds_a_solid_and_reports_deviation() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}));
    let path = std::env::temp_dir().join(format!("solidcraft-recipe-scan-{}.stl", std::process::id()));
    let path_text = path.to_string_lossy().to_string();
    run(&mut s, "file.save_mesh", json!({"path": path_text, "bodies": ["Box"]}));
    let inserted = run(&mut s, "file.insert_mesh", json!({"path": path_text}));
    let mesh = inserted["bodies"].as_array().and_then(|b| b.first()).and_then(Value::as_str).unwrap_or("").to_string();
    assert!(!mesh.is_empty(), "{inserted}");
    assert!(s.model.state().body(&mesh).is_some_and(|body| body.body.is_mesh()), "{mesh}");

    let section = run(&mut s, "mesh.section", json!({"body": mesh, "origin": [0, 0, 5], "normal": [0, 0, 1], "name": "Section"}));
    assert!(section["loops"].as_u64().unwrap_or(0) >= 1, "{section}");
    let sketch_name = section["sketch"].as_str().unwrap_or("Section");
    let info = run(&mut s, "sketch.inspect", json!({"sketch": sketch_name}));
    assert!(info["profiles"].as_array().is_some_and(|p| !p.is_empty()), "{info}");
    run(&mut s, "solid.extrude", json!({"sketch": sketch_name, "distance": 10, "operation": "new", "body_name": "Rebuild"}));
    assert!(!s.model.state().body("Rebuild").unwrap().body.is_mesh());
    assert!(body_volume(&mut s, "Rebuild").is_finite() && body_volume(&mut s, "Rebuild") > 1.0);

    let against_source = run(&mut s, "mesh.deviation", json!({"body": "Box", "mesh": mesh}));
    finite_deviation(&against_source);
    let against_rebuild = run(&mut s, "mesh.deviation", json!({"body": "Rebuild", "mesh": mesh}));
    finite_deviation(&against_rebuild);
    assert_consistent(&s);
    let _ = std::fs::remove_file(&path);
}

/// Req 13.3: a user parameter drives clearance, keep-tools combine preserves the tool, and nest lays parts out.
#[test]
fn req_13_3_clearance_follows_a_parameter_and_keep_tools_keeps_the_tool() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "1 mm"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "Part"}));
    run(&mut s, "dfam.clearance", json!({"body": "Part", "gap": "gap", "name": "Fit"}));
    let g1 = body_volume(&mut s, "Part");
    assert!(rel(g1, 18.0_f64.powi(3)) < 1e-3, "{g1}");
    let changed = run(&mut s, "parameters.change", json!({"name": "gap", "expression": "2 mm"}));
    assert!(changed["errors"].as_array().is_some_and(|e| e.is_empty()), "{changed}");
    assert!(changed["recomputed"].as_u64().unwrap_or(0) >= 1, "{changed}");
    let g2 = body_volume(&mut s, "Part");
    assert!(rel(g2, 16.0_f64.powi(3)) < 1e-3, "{g2}");
    assert!(g2 < g1, "{g1} -> {g2}");
    assert_consistent(&s);

    let mut kept = Session::default();
    run(&mut kept, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "A"}));
    run(&mut kept, "solid.box", json!({"corner": [10, 0, 0], "length": 20, "width": 20, "height": 20, "body_name": "B"}));
    run(&mut kept, "solid.combine", json!({"target": "A", "tools": ["B"], "operation": "cut", "keep_tools": true, "name": "Cavity"}));
    assert!(kept.model.state().body("B").is_some(), "the tool body is kept");
    assert!(rel(body_volume(&mut kept, "B"), 8000.0) < 1e-6);
    assert!(rel(body_volume(&mut kept, "A"), 4000.0) < 1e-4);
    assert_consistent(&kept);

    let mut nested = Session::default();
    run(&mut nested, "parameters.add", json!({"name": "gap", "expression": "2 mm"}));
    run(&mut nested, "solid.box", json!({"length": 10, "width": 10, "height": 4, "body_name": "A"}));
    run(&mut nested, "solid.box", json!({"length": 10, "width": 10, "height": 4, "body_name": "B"}));
    run(&mut nested, "dfam.nest", json!({"bodies": ["A", "B"], "width": "40", "depth": "40", "gap": "gap"}));
    let state = nested.model.state();
    let a = state.body("A").unwrap().mesh().bounds();
    let b = state.body("B").unwrap().mesh().bounds();
    let overlap_x = a.min.x < b.max.x && b.min.x < a.max.x;
    let overlap_y = a.min.y < b.max.y && b.min.y < a.max.y;
    assert!(!(overlap_x && overlap_y), "a {a:?} b {b:?}");
    assert_consistent(&nested);
}

fn occurrence(s: &Session, component: &str) -> u64 {
    let id = s.doc.find_component(component).unwrap_or(0);
    s.doc.occurrence_of(id).map(|o| o.id).unwrap_or(0)
}

/// Req 13.4: components, a grounded part, a limited joint, and an interference report.
#[test]
fn req_13_4_assembly_motion_respects_limits_and_interference_is_reported() {
    let mut s = Session::default();
    for (name, x) in [("Base", 0.0), ("Peg", 6.0)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let base = occurrence(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    let overlap = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(overlap["count"], 1, "{overlap}");
    let volume = overlap["interferences"][0]["volume_mm3"].as_f64().unwrap_or(f64::NAN);
    assert!(volume.is_finite() && rel(volume, 400.0) < 1e-3, "{overlap}");

    for (name, x) in [("Rail", 100.0), ("Slider", 120.0)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let rail = occurrence(&s, "Rail");
    run(&mut s, "occurrence.ground", json!({"occurrence": rail, "grounded": true}));
    let joint = run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"face": [105, 5, 10]}, "b": {"face": [125, 5, 0]}, "limits": [[0, 10]], "name": "Travel"}),
    );
    assert_eq!(joint["conflicts"].as_array().map(Vec::len), Some(0), "{joint}");
    let driven = run(&mut s, "joint.drive", json!({"joint": "Travel", "value": 25}));
    assert_eq!(driven["values"], json!([10.0]), "{driven}");
    let after = run(&mut s, "inspect.interference", json!({}));
    assert!(after["count"].as_u64().is_some(), "{after}");
    assert!(after["interferences"].as_array().is_some_and(|pairs| !pairs.is_empty()), "{after}");
    assert_consistent(&s);
}

fn png(w: u32, h: u32) -> String {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend(w.to_be_bytes());
    bytes.extend(h.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    base64_encode(&bytes)
}

/// Req 13.7: canvas calibrated at 1:1, a centerline, a fit-point spline, a fully defined
/// profile revolved 360°, then shelled and given a transparent appearance.
#[test]
fn req_13_7_calibrated_canvas_revolve_and_shell() {
    let mut s = Session::default();
    let canvas = run(&mut s, "canvas.insert", json!({"data": png(80, 40), "plane": "XZ", "width": 100, "name": "Reference"}));
    let id = canvas["canvas"].as_u64().unwrap();
    let calibrated = run(&mut s, "canvas.calibrate", json!({"canvas": id, "a": [0, 0], "b": [50, 0], "distance": 50}));
    assert!((calibrated["width"].as_f64().unwrap_or(0.0) - 100.0).abs() < 1e-9, "{calibrated}");

    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Profile"}));
    let axis = run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 12]]}));
    let axis_id = axis["curves"][0].as_str().unwrap().to_string();
    let axis_start = axis["created"]["points"][0].as_str().unwrap();
    run(&mut s, "sketch.centerline", json!({"curves": [axis_id], "value": true}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": axis_start, "b": "origin"}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": axis_id, "mode": "vertical"}));
    run(&mut s, "sketch.dimension", json!({"entities": [axis_id], "value": 12}));

    let rect = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 2], "p1": [15, 7]}));
    let corner = rect["corners"]["bottom_left"].as_str().unwrap();
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", corner], "type": "horizontal", "value": 10}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", corner], "type": "vertical", "value": 2}));
    run(&mut s, "sketch.dimension", json!({"entities": [rect["sides"]["bottom"].as_str().unwrap()], "value": 5}));
    run(&mut s, "sketch.dimension", json!({"entities": [rect["sides"]["left"].as_str().unwrap()], "value": 5}));

    let spline = run(&mut s, "sketch.spline.fit_point", json!({"points": [[30, 1], [34, 4], [38, 1]]}));
    let mut fixed = spline["created"]["points"].as_array().cloned().unwrap_or_default();
    fixed.push(spline["curves"][0].clone());
    run(&mut s, "sketch.constraint.fix", json!({"entities": fixed}));

    let sketch = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(sketch["dof"], 0, "{sketch}");
    assert_eq!(sketch["fully_constrained"], true, "{sketch}");
    assert!(sketch["profiles"].as_array().is_some_and(|p| p.len() == 1), "{sketch}");
    assert!(sketch["curves"].as_array().is_some_and(|curves| curves.iter().any(|c| c["type"] == "spline")), "{sketch}");
    run(&mut s, "sketch.finish", json!({}));

    run(
        &mut s,
        "solid.revolve",
        json!({"sketch": "Profile", "profiles": [0], "axis": axis_id, "angle": "360 deg", "body_name": "Vessel", "name": "Turn"}),
    );
    let spun = body_volume(&mut s, "Vessel");
    assert!(rel(spun, PI * (15.0_f64.powi(2) - 10.0_f64.powi(2)) * 5.0) < 1e-3, "{spun}");

    let faces = run(&mut s, "model.faces", json!({"body": "Vessel"}));
    let cap = faces["faces"]
        .as_array()
        .and_then(|all| all.iter().find(|face| face["plane_normal"].is_array()))
        .unwrap_or_else(|| panic!("no planar face to shell: {faces}"));
    let at = &cap["centroid"];
    run(&mut s, "solid.shell", json!({"body": "Vessel", "faces": [[at[0], at[1], at[2]]], "thickness": "1 mm", "name": "Wall"}));
    let hollow = body_volume(&mut s, "Vessel");
    assert!(hollow.is_finite() && hollow > 1.0 && hollow < spun, "{spun} -> {hollow}");

    run(&mut s, "appearance.assign", json!({"bodies": ["Vessel"], "color": "#7eb6d6", "opacity": 0.35}));
    let look = run(&mut s, "appearance.list", json!({"body": "Vessel"}));
    let opacity = look["body"]["appearance"]["opacity"].as_f64().unwrap_or(f64::NAN);
    assert!((opacity - 0.35).abs() < 1e-9, "{look}");
    assert_consistent(&s);
}

/// Every scene in `examples/demo/feature_tour.json` runs to completion. The README recording
/// plays these same scenes.
#[test]
fn feature_tour_scenes_cover_the_workspaces() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/feature_tour.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let tour: Value = serde_json::from_str(&text).unwrap();
    let scenes = tour["scenes"].as_array().expect("scenes");
    assert!(scenes.len() >= 12, "the tour should visit every major workspace");
    let mut areas = std::collections::BTreeSet::new();
    for scene in scenes {
        let title = scene["title"].as_str().unwrap_or("scene");
        let commands = scene["commands"].as_array().expect(title);
        assert!(!commands.is_empty(), "{title}");
        let mut session = Session::default();
        let out = session.run_script(&json!({"commands": commands})).unwrap_or_else(|e| panic!("{title}: {e}"));
        assert_eq!(out.len(), commands.len(), "{title}");
        assert_consistent(&session);
        for step in commands {
            let id = step["command"].as_str().unwrap_or("");
            assert!(find_command(id).is_some(), "{title} unknown {id}");
            areas.insert(id.split('.').next().unwrap_or("").to_string());
        }
    }
    for area in [
        "sketch",
        "solid",
        "parameters",
        "inspect",
        "surface",
        "sheet",
        "form",
        "mold",
        "cam",
        "sim",
        "drawing",
        "appearance",
        "view",
        "component",
        "ecad",
    ] {
        assert!(areas.contains(area), "tour missing {area}");
    }
}
