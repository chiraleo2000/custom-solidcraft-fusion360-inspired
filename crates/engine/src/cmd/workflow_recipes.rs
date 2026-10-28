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

/// Day 20 / DfAM: a print-in-place hinge leaf gets a parametric clearance gap that follows edits.
#[test]
fn wf_day20_hinge_clearance_follows_the_gap_parameter() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "clearance", "expression": "0.4 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 6, "body_name": "Leaf"}));
    run(&mut s, "dfam.clearance", json!({"body": "Leaf", "gap": "clearance", "name": "PrintGap"}));
    let after_gap = body_volume(&mut s, "Leaf");
    assert!(rel(after_gap, 39.2 * 19.2 * 5.2) < 2e-2, "{after_gap}");
    let changed = run(&mut s, "parameters.change", json!({"name": "clearance", "expression": "0.8 mm"}));
    assert!(changed["errors"].as_array().is_some_and(|e| e.is_empty()), "{changed}");
    let tighter = body_volume(&mut s, "Leaf");
    assert!(tighter < after_gap, "{after_gap} -> {tighter}");
    assert!(rel(tighter, 38.4 * 18.4 * 4.4) < 2e-2, "{tighter}");
    assert_consistent(&s);
}

/// Day 21 / Req 13.8: prototype → mold box → keep-tools cavity → mid-height split → pins → spout.
#[test]
fn wf_day21_mold_box_cavity_split_pins_and_spout() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 24, "width": 18, "height": 10, "body_name": "Proto"}));
    run(&mut s, "mold.box", json!({"body": "Proto", "margin": "5", "body_name": "Mold", "name": "Block"}));
    // Explicit keep-tools combine (Req 13.3 / 14.2) beside the mold.cavity convenience.
    run(&mut s, "solid.box", json!({"length": 24, "width": 18, "height": 10, "body_name": "Tool"}));
    run(&mut s, "solid.combine", json!({"target": "Mold", "tools": ["Tool"], "operation": "cut", "keep_tools": true, "name": "KeepCut"}));
    assert!(s.model.state().body("Tool").is_some(), "keep_tools preserves the tool");
    // Dedicated cavity path on a fresh mold also keeps the prototype.
    let mut cavity = Session::default();
    run(&mut cavity, "solid.box", json!({"length": 24, "width": 18, "height": 10, "body_name": "Proto"}));
    run(&mut cavity, "mold.box", json!({"body": "Proto", "margin": "5", "body_name": "Mold"}));
    run(&mut cavity, "mold.cavity", json!({"mold": "Mold", "tool": "Proto"}));
    assert!(cavity.model.state().body("Proto").is_some());
    let split = run(&mut cavity, "mold.split", json!({"body": "Mold", "body_names": ["Lower", "Upper"]}));
    let lower = split["parting"]["lower"].as_str().unwrap_or("Lower").to_string();
    let upper = split["parting"]["upper"].as_str().unwrap_or("Upper").to_string();
    assert!(cavity.model.state().body(&lower).is_some() && cavity.model.state().body(&upper).is_some(), "{split}");
    run(&mut cavity, "mold.alignment_pins", json!({"upper": upper, "lower": lower.clone(), "diameter": "3", "depth": "3"}));
    run(&mut cavity, "mold.pour_spout", json!({"body": lower, "width": "5", "depth": "6"}));
    assert!(cavity.model.state().body("Proto").is_some());
    assert_consistent(&cavity);
    assert_consistent(&s);
}

/// Day 24: sheet flange → edge flange → unfold → flat pattern DXF with bend layers.
#[test]
fn wf_day24_sheet_flange_unfold_and_flat_dxf() {
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "Steel", "thickness": "1 mm", "bend_radius": "1 mm", "active": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [80, 50]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base", "rule": "Steel", "body_name": "Bracket"}));
    let folded = body_volume(&mut s, "Bracket");
    assert!(folded.is_finite() && folded > 100.0, "{folded}");
    run(&mut s, "sheet.flange", json!({"edges": [[40, 0, 1.0]], "height": "20 mm", "angle": "90 deg", "body": "Bracket"}));
    let with_flange = body_volume(&mut s, "Bracket");
    assert!(with_flange > folded, "{folded} -> {with_flange}");
    let flat = run(&mut s, "sheet.flat_pattern", json!({"body": "Bracket"}));
    assert!(flat["bends"].as_array().is_some_and(|b| !b.is_empty()) || flat["bend_lines"].as_array().is_some_and(|b| !b.is_empty()), "{flat}");
    let dxf = run(&mut s, "sheet.export_dxf", json!({"body": "Bracket"}));
    let text = dxf["dxf"].as_str().unwrap_or("");
    assert!(text.contains("OUTLINE"), "{dxf}");
    assert!(text.contains("BEND_UP") || text.contains("BEND_DOWN") || text.contains("BEND"), "{dxf}");
    let layers = dxf["layers"].as_array().cloned().unwrap_or_default();
    assert!(layers.iter().any(|l| l == "OUTLINE"), "layers {layers:?}");
    assert!(layers.iter().any(|l| l == "BEND_UP" || l == "BEND_DOWN"), "layers {layers:?}");
    run(&mut s, "sheet.unfold", json!({"body": "Bracket"}));
    run(&mut s, "sheet.refold", json!({"body": "Bracket"}));
    assert_consistent(&s);
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

    run(&mut s, "appearance.assign", json!({"bodies": ["Vessel"], "appearance": "Glass - Clear"}));
    let look = run(&mut s, "appearance.list", json!({"body": "Vessel"}));
    let opacity = look["body"]["appearance"]["opacity"].as_f64().unwrap_or(f64::NAN);
    assert!(opacity < 0.5, "glass must be transparent: {look}");
    assert_eq!(look["body"]["appearance"]["name"], "Glass - Clear");
    assert_consistent(&s);
}

/// Year 1 / PDF Day 25: open surface create, patch, trim/untrim, stitch gaps, thicken volume.
#[test]
fn wf_day25_surface() {
    let mut s = Session::default();

    // Open extrude of a closed rectangle: four walls, no caps, zero volume.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Wall"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "Wall", "distance": 8, "body_name": "Tube"}));
    let tube = s.world_state().body("Tube").expect("Tube").body.clone();
    assert!(tube.is_surface());
    assert_eq!(tube.face_count(), 4, "four walls and no caps");
    let tube_m = solidcraft_kernel::measure(&tube).unwrap();
    assert!(tube_m.volume.abs() < 1e-3, "{}", tube_m.volume);
    assert!((tube_m.area - 480.0).abs() < 2.0, "{}", tube_m.area);

    // Open revolve of a line about Y.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Spoke"}));
    run(&mut s, "sketch.line", json!({"points": [[15, 0], [15, 6]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.revolve", json!({"sketch": "Spoke", "axis": "y", "angle": "90 deg", "body_name": "Band"}));
    let band = s.world_state().body("Band").expect("Band").body.clone();
    assert!(band.is_surface());
    let band_area = solidcraft_kernel::measure(&band).unwrap().area;
    let want_band = std::f64::consts::FRAC_PI_2 * 15.0 * 6.0;
    assert!(rel(band_area, want_band) < 0.05, "{band_area} vs {want_band}");

    // Open sweep and loft.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Prof"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [6, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
    let path = run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 10]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.sweep", json!({"sketch": "Prof", "path_sketch": "Path", "path": [path], "body_name": "Ribbon"}));
    assert!(s.world_state().body("Ribbon").expect("Ribbon").body.is_surface());

    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "LoftA"}));
    run(&mut s, "sketch.line", json!({"points": [[40, 0], [50, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "LoftB"}));
    run(&mut s, "sketch.line", json!({"points": [[40, 8], [50, 8]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.loft", json!({"sections": [{"sketch": "LoftA"}, {"sketch": "LoftB"}], "body_name": "Ruled"}));
    let ruled = s.world_state().body("Ruled").expect("Ruled").body.clone();
    assert!(ruled.is_surface());
    assert!((solidcraft_kernel::measure(&ruled).unwrap().area - 80.0).abs() < 2.0);

    // Patch → curve trim → untrim restores area; second patch → stitch reports gaps; thicken volume.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pad"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.patch", json!({"sketch": "Pad", "body_name": "Sheet"}));
    let sheet0 = solidcraft_kernel::measure(&s.world_state().body("Sheet").unwrap().body).unwrap().area;
    assert!((sheet0 - 100.0).abs() < 1e-3, "{sheet0}");

    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cut"}));
    let curve = run(&mut s, "sketch.line", json!({"points": [[4, 0], [4, 10]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.trim", json!({"body": "Sheet", "sketch": "Cut", "curve": curve, "keep": [8, 5, 0]}));
    let trimmed = solidcraft_kernel::measure(&s.world_state().body("Sheet").unwrap().body).unwrap().area;
    assert!((trimmed - 60.0).abs() < 1.0, "{trimmed}");
    run(&mut s, "surface.untrim", json!({"body": "Sheet"}));
    let restored = solidcraft_kernel::measure(&s.world_state().body("Sheet").unwrap().body).unwrap().area;
    assert!((restored - 100.0).abs() < 1e-3, "{restored}");

    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Other"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, 0], "p1": [40, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.patch", json!({"sketch": "Other", "body_name": "Sheet2"}));
    // Thicken before stitch: stitch consumes Sheet2 into Sheet.
    run(&mut s, "surface.thicken", json!({"bodies": ["Sheet2"], "thickness": "2 mm", "body_name": "Slab"}));
    let slab_vol = solidcraft_kernel::measure(&s.world_state().body("Slab").unwrap().body).unwrap().volume.abs();
    assert!((slab_vol - 200.0).abs() < 1e-2, "{slab_vol}");
    let stitched = run(&mut s, "surface.stitch", json!({"bodies": ["Sheet", "Sheet2"], "tolerance": "0.01 mm"}));
    assert_eq!(stitched["closed"], json!(false), "{stitched}");
    assert!(stitched["gaps"].as_array().is_some_and(|g| !g.is_empty()), "{stitched}");
    assert!(stitched["gap_edges"].as_array().is_some_and(|g| !g.is_empty()), "{stitched}");

    // Mixed sketch + hole and a guided lift stay on the timeline.
    run(&mut s, "surface.patch", json!({"sketch": "Pad", "edges": [[2, 2, 0], [4, 2, 0], [4, 4, 0], [2, 4, 0]], "body_name": "WithHole"}));
    let hole_area = solidcraft_kernel::measure(&s.world_state().body("WithHole").unwrap().body).unwrap().area;
    assert!((hole_area - 96.0).abs() < 1e-2, "{hole_area}");
    run(&mut s, "surface.patch", json!({"sketch": "Pad", "guide": [[0, 5, 0], [5, 5, 3], [10, 5, 0]], "body_name": "Tent"}));
    assert!(s.world_state().body("Tent").unwrap().body.tessellate(0.2).unwrap().bounds().max.z > 1.0);

    assert_consistent(&s);
}

/// Year 2 / PDF Day 29: associative drawing views, BOM/balloons, title block, standards, export.
#[test]
fn wf_day29_drawing() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "height", "expression": "12 mm", "unit": "mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": "height", "name": "Block", "body_name": "Block"}));
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "component.create", json!({"name": "Lid"}));
    run(&mut s, "drawing.create", json!({"name": "Sheet1"}));
    run(&mut s, "drawing.standard", json!({"standard": "third_angle", "arrows": "filled"}));
    run(&mut s, "drawing.base_view", json!({"direction": [0, 0, -1], "origin": [0, 0], "hidden": true}));
    run(&mut s, "drawing.project", json!({"from": 0, "side": "right", "origin": [60, 0]}));
    run(&mut s, "drawing.section", json!({"direction": [1, 0, 0], "origin": [0, 50]}));
    run(&mut s, "drawing.detail", json!({"from": 0, "scale": 2, "center": [0, 0], "radius": 15, "origin": [120, 0]}));
    run(&mut s, "drawing.dimension", json!({"body": "Block", "axis": "z", "at": [0, -20]}));
    run(&mut s, "drawing.center_mark", json!({"body": "Block"}));
    let list = run(&mut s, "drawing.parts_list", json!({}));
    assert_eq!(list["rows"], 2, "{list}");
    let balloon = run(&mut s, "drawing.balloon", json!({"component": "Lid", "at": [30, 30]}));
    assert_eq!(balloon["lost"], false);
    assert!(balloon["item"].as_u64().unwrap_or(0) >= 1, "{balloon}");
    run(
        &mut s,
        "drawing.title_block",
        json!({"title": "Day29 Bracket", "size": "A4", "template": "iso", "revision": "A", "drawn": "SolidCraft", "date": "2026-10-28"}),
    );
    run(&mut s, "parameters.change", json!({"name": "height", "expression": "18 mm"}));
    let note = s.doc.drawings[0].notes.iter().find(|n| n.kind == "dimension").expect("dimension");
    assert!((note.measured - 18.0).abs() < 0.25, "{note:?}");
    assert!(!note.lost);
    assert!(!s.doc.drawings[0].views[0].curves.is_empty());
    assert!(s.doc.drawings[0].views[2].curves.iter().any(|c| c.section));
    assert!(!s.doc.drawings[0].views[3].curves.is_empty());

    let dir = std::env::temp_dir();
    let dxf_path = dir.join(format!("solidcraft-day29-{}.dxf", std::process::id()));
    let pdf_path = dir.join(format!("solidcraft-day29-{}.pdf", std::process::id()));
    let svg = run(&mut s, "drawing.export_svg", json!({}));
    assert!(svg["svg"].as_str().unwrap_or("").contains("Day29 Bracket"), "{svg}");
    assert!(svg["lost"].as_array().unwrap().is_empty(), "{svg}");
    run(&mut s, "drawing.export_dxf", json!({"path": dxf_path.to_string_lossy()}));
    run(&mut s, "drawing.export_pdf", json!({"path": pdf_path.to_string_lossy()}));
    let pdf = std::fs::read_to_string(&pdf_path).unwrap();
    assert!(pdf.contains("%PDF-1.4") && pdf.contains("/Title (Day29 Bracket)"), "{pdf}");

    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.delete", json!({"component": "Lid"}));
    let balloon_note = s.doc.drawings[0].notes.iter().find(|n| n.kind == "balloon").expect("balloon");
    assert!(balloon_note.lost, "{balloon_note:?}");
    let lost = run(&mut s, "drawing.export_svg", json!({}));
    assert!(lost["lost"].as_array().unwrap().iter().any(|n| n == "Lid"), "{lost}");
    assert_consistent(&s);
    let _ = std::fs::remove_file(dxf_path);
    let _ = std::fs::remove_file(pdf_path);
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
