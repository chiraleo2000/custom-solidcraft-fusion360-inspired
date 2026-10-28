//! Year-1 solid / sketch / assembly / params polish for PDF Days 1–15, 13, 19 and 23.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// Day 5: tapered one-side, symmetric, and two-sides extrudes keep finite volume.
#[test]
fn extrude_taper_one_side_and_symmetric() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Boss"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Boss", "distance": 20, "taper": "-5 deg", "body_name": "Tray"}));
    let tapered = volume(&mut s);
    assert!(tapered.is_finite() && tapered > 1.0 && tapered < 20.0 * 20.0 * 20.0, "{tapered}");

    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Sym"}));
    run(&mut s, "sketch.circle.center", json!({"center": [40, 0], "radius": 8}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Sym", "distance": 6, "direction": "symmetric", "taper": "8 deg", "operation": "new", "body_name": "Pillar"}),
    );
    let pillar = run(&mut s, "inspect.measure", json!({}))["bodies"]
        .as_array()
        .and_then(|bodies| bodies.iter().find(|b| b["name"] == "Pillar"))
        .and_then(|b| b["volume_mm3"].as_f64())
        .unwrap_or(f64::NAN);
    // Mid radius 8, ends grow with +8° over 6 mm each way → larger than a plain cylinder of r=8, h=12.
    let plain = PI * 64.0 * 12.0;
    assert!(pillar.is_finite() && pillar > plain, "{pillar} vs {plain}");

    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Two"}));
    run(&mut s, "sketch.circle.center", json!({"center": [80, 0], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({
            "sketch": "Two",
            "distance": 4,
            "distance2": 8,
            "taper": "5 deg",
            "operation": "new",
            "body_name": "Wedge"
        }),
    );
    let wedge = run(&mut s, "inspect.measure", json!({}))["bodies"]
        .as_array()
        .and_then(|bodies| bodies.iter().find(|b| b["name"] == "Wedge"))
        .and_then(|b| b["volume_mm3"].as_f64())
        .unwrap_or(f64::NAN);
    assert!(wedge.is_finite() && wedge > PI * 25.0 * 12.0, "{wedge}");

    let features = s.doc.features.len();
    let err = s.execute("solid.extrude", &json!({"sketch": "Boss", "distance": 5, "taper": "5 deg", "start_offset": "1 mm"})).unwrap_err();
    assert!(err.to_string().contains("not supported yet"), "{err}");
    assert_eq!(s.doc.features.len(), features);
}

/// Day 10: pipe cut removes a clearance tunnel through a plate.
#[test]
fn pipe_cut_clears_a_tunnel() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 8, "corner": [0, 0, 0], "body_name": "Plate"}));
    let before = volume(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
    let line = run(&mut s, "sketch.line", json!({"points": [[5, 10], [35, 10]]}));
    let path = line["curves"][0].clone();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.pipe", json!({"path_sketch": "Path", "path": [path], "diameter": 4, "operation": "cut", "targets": ["Plate"]}));
    let after = volume(&mut s);
    let expect = before - PI * 4.0 * 30.0;
    assert!(rel(after, expect) < 5e-2, "{before} -> {after}, expect ~{expect}");
}

/// Day 4 / 15: loft rails that miss refuse; loft-to-point refuses rails; loft-to-point succeeds alone.
#[test]
fn loft_rails_harden_misses_and_point_sections() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Bottom"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-8, -4], "p1": [8, 4]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 30], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Top"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-8, -4], "p1": [8, 4]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Guide"}));
    let miss = run(&mut s, "sketch.line", json!({"points": [[40, 0], [40, 30]]}));
    run(&mut s, "sketch.finish", json!({}));
    let features = s.doc.features.len();
    let err = s
        .execute(
            "solid.loft",
            &json!({"sections": [{"sketch": "Bottom"}, {"sketch": "Top"}], "rails": {"sketch": "Guide", "curves": [miss["curves"][0].clone()]}}),
        )
        .unwrap_err();
    assert!(err.to_string().contains("misses"), "{err}");
    assert_eq!(s.doc.features.len(), features);

    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 50], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Apex"}));
    let pt = run(&mut s, "sketch.point", json!({"point": [0, 0]}));
    let point = pt["point"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    // Apex sketch is a new timeline feature; refuse rails without adding a loft.
    let features_after_apex = s.doc.features.len();
    let err = s
        .execute(
            "solid.loft",
            &json!({
                "sections": [{"sketch": "Bottom"}, {"sketch": "Apex", "point": point}],
                "rails": {"sketch": "Guide", "curves": [miss["curves"][0].clone()]}
            }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("not supported yet"), "{err}");
    assert_eq!(s.doc.features.len(), features_after_apex);

    // Day 15: loft to a point without rails builds a finite pyramid.
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "Bottom"}, {"sketch": "Apex", "point": point}], "body_name": "Pyramid"}));
    let pyramid = run(&mut s, "inspect.measure", json!({}))["bodies"]
        .as_array()
        .and_then(|bodies| bodies.iter().find(|b| b["name"] == "Pyramid"))
        .and_then(|b| b["volume_mm3"].as_f64())
        .unwrap_or(f64::NAN);
    // Base 16×8, height 50 → pyramid volume = (1/3)·base·h = 2133⅓.
    assert!(pyramid.is_finite() && rel(pyramid, 16.0 * 8.0 * 50.0 / 3.0) < 5e-2, "{pyramid}");
}

/// Day 6: tapped countersink hole is one feature and removes material.
#[test]
fn tapped_countersink_hole_is_discoverable() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 30, "width": 30, "height": 12, "body_name": "Block"}));
    let before = volume(&mut s);
    run(
        &mut s,
        "solid.hole",
        json!({
            "position": [15, 15, 12],
            "type": "countersink",
            "thread": "M6",
            "cs_diameter": 12,
            "cs_angle": "90 deg",
            "depth": 12
        }),
    );
    let after = volume(&mut s);
    assert!(after < before - 50.0, "{before} -> {after}");
    assert!(
        s.doc.features.iter().any(|f| matches!(
            &f.kind,
            solidcraft_doc::FeatureKind::Hole { hole: solidcraft_doc::HoleKind::Countersink { .. }, thread: Some(_), .. }
        ))
    );
}

/// Day 8: rib and web still fill thin-wall stories.
#[test]
fn rib_and_web_join_an_l_bracket() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 5, "corner": [0, 0, 0], "body_name": "L"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 20, "height": 40, "corner": [0, 0, 0], "operation": "join"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 10, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Rib"}));
    run(&mut s, "sketch.line", json!({"points": [[5, 25], [25, 5]], "ids": ["l1"]}));
    run(&mut s, "sketch.finish", json!({}));
    let before = volume(&mut s);
    run(&mut s, "solid.rib", json!({"sketch": "Rib", "curves": ["l1"], "thickness": 3}));
    let after_rib = volume(&mut s);
    assert!(after_rib > before + 100.0, "{before} -> {after_rib}");

    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 5, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Web"}));
    run(&mut s, "sketch.line", json!({"points": [[8, 8], [28, 20]], "ids": ["w1"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.web", json!({"sketch": "Web", "curves": ["w1"], "thickness": 2}));
    assert!(volume(&mut s) > after_rib + 20.0, "{after_rib} -> {}", volume(&mut s));
}

/// Day 19: named user parameters drive a box; edit rebuilds.
#[test]
fn day19_user_parameters_rebuild_a_box() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "length", "expression": "40 mm"}));
    run(&mut s, "parameters.add", json!({"name": "width", "expression": "length / 2"}));
    run(&mut s, "parameters.add", json!({"name": "height", "expression": "10 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    let rect = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.dimension", json!({"entities": [rect["sides"]["bottom"]], "value": "length"}));
    run(&mut s, "sketch.dimension", json!({"entities": [rect["sides"]["left"]], "value": "width"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Base", "distance": "height", "body_name": "Box"}));
    assert!(rel(volume(&mut s), 40.0 * 20.0 * 10.0) < 1e-6);
    let changed = run(&mut s, "parameters.change", json!({"name": "length", "expression": "60 mm"}));
    assert!(changed["errors"].as_array().is_some_and(|e| e.is_empty()), "{changed}");
    assert!(rel(volume(&mut s), 60.0 * 30.0 * 10.0) < 1e-4, "{}", volume(&mut s));
}

/// Day 13: new component + activate keeps authorship in the child.
#[test]
fn new_component_activate_authors_in_the_child() {
    let mut s = Session::default();
    let c = run(&mut s, "component.create", json!({"name": "Lid", "activate": true}));
    let id = c["component"].as_u64().unwrap();
    assert_eq!(s.active_component, id);
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 4, "body_name": "LidBody"}));
    // `model.state()` returns an Arc; keep it alive while we borrow the body.
    let authored = {
        let state = s.model.state();
        let body = state.body("LidBody").expect("body");
        s.doc.model_body_component(body)
    };
    assert_eq!(authored, id);
}

/// Day 23 smoke: as-built rigid joint between two grounded-ready boxes.
#[test]
fn as_built_joint_between_two_components() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "A", "activate": true}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "ABody"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "B", "activate": true}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "BBody"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let joint = run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "A", "b": "B", "name": "Lock"}));
    assert_eq!(joint["conflicts"].as_array().map(Vec::len), Some(0), "{joint}");
}

/// Day 23: limited slider, Drive Joints clamp, Interference + Measure stay discoverable.
#[test]
fn day23_drive_limits_interference_and_measure() {
    let mut s = Session::default();
    for (name, x) in [("Rail", 0.0), ("Slider", 20.0)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name, "activate": true}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let rail = s.doc.find_component("Rail").unwrap();
    let occ = s.doc.occurrence_of(rail).map(|o| o.id).unwrap();
    run(&mut s, "occurrence.ground", json!({"occurrence": occ, "grounded": true}));
    let joint = run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "limits": [[0, 8]], "name": "Travel"}),
    );
    assert_eq!(joint["conflicts"].as_array().map(Vec::len), Some(0), "{joint}");
    let driven = run(&mut s, "joint.drive", json!({"joint": "Travel", "value": 40}));
    assert_eq!(driven["values"], json!([8.0]), "{driven}");
    let clash = run(&mut s, "inspect.interference", json!({}));
    assert!(clash["count"].as_u64().is_some(), "{clash}");
    let measured = run(&mut s, "inspect.measure", json!({}));
    assert!(measured["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN).is_finite(), "{measured}");

    // Separate overlapping boxes report a finite interference volume.
    let mut clash_sess = Session::default();
    for (name, x) in [("Left", 0.0), ("Right", 6.0)] {
        run(&mut clash_sess, "component.activate", json!({"component": "root"}));
        run(&mut clash_sess, "component.create", json!({"name": name, "activate": true}));
        run(&mut clash_sess, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let hit = run(&mut clash_sess, "inspect.interference", json!({}));
    assert_eq!(hit["count"], 1, "{hit}");
    let vol = hit["interferences"][0]["volume_mm3"].as_f64().unwrap_or(f64::NAN);
    assert!(vol.is_finite() && rel(vol, 400.0) < 1e-3, "{hit}");
}

/// Days 14 / 16–17: AutoConstrain reduces freedom on a free rectangle.
#[test]
fn auto_constrain_reduces_sketch_freedom() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Free"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 20]}));
    let before = run(&mut s, "sketch.inspect", json!({}));
    let dof0 = before["dof"].as_u64().unwrap_or(999);
    assert!(dof0 > 0, "{before}");
    let out = run(&mut s, "sketch.auto_constrain", json!({}));
    assert!(!out["added"].as_array().unwrap().is_empty(), "{out}");
    let after = run(&mut s, "sketch.inspect", json!({}));
    assert!(after["dof"].as_u64().unwrap_or(999) < dof0, "{before} -> {after}");
}

fn png_stub(w: u32, h: u32) -> String {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend(w.to_be_bytes());
    bytes.extend(h.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    solidcraft_doc::canvas::base64_encode(&bytes)
}

/// Day 2: canvas calibrates to 1:1; glass appearance is transparent.
#[test]
fn canvas_one_to_one_and_glass_appearance() {
    let mut s = Session::default();
    let canvas = run(&mut s, "canvas.insert", json!({"data": png_stub(80, 40), "plane": "XZ", "width": 80, "name": "Ref"}));
    let id = canvas["canvas"].clone();
    let calibrated = run(&mut s, "canvas.calibrate", json!({"canvas": id, "a": [0, 0], "b": [40, 0], "distance": 40}));
    assert!((calibrated["width"].as_f64().unwrap_or(0.0) - 80.0).abs() < 1e-9, "{calibrated}");

    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Bottle"}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Bottle"], "appearance": "Glass - Clear"}));
    let look = run(&mut s, "appearance.list", json!({"body": "Bottle"}));
    let opacity = look["body"]["appearance"]["opacity"].as_f64().unwrap_or(f64::NAN);
    assert!(opacity < 0.5, "glass must be transparent: {look}");
    assert_eq!(look["body"]["appearance"]["name"], "Glass - Clear");
}
