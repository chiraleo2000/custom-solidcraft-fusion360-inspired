use std::f64::consts::PI;

use serde_json::json;

use crate::{Sel, Session};

fn run(s: &mut Session, id: &str, p: serde_json::Value) -> serde_json::Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

fn area(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["area_mm2"].as_f64().unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

#[test]
fn through_cut_keeps_a_blind_pocket_and_the_other_body() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 90, "width": 50, "height": 8, "corner": [-45, -25, 0], "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [200, 0, 0], "body_name": "Other"}));
    run(&mut s, "solid.cylinder", json!({"radius": 11, "height": 6, "operation": "cut", "targets": ["Plate"]}));
    let pocketed = volume(&mut s);
    let expect_pocket = 90.0 * 50.0 * 8.0 + 1000.0 - PI * 121.0 * 6.0;
    assert!(rel(pocketed, expect_pocket) < 5e-3, "{pocketed} vs {expect_pocket}");
    run(&mut s, "solid.cylinder", json!({"radius": 9.5, "height": 10, "base": [0, 0, -1], "operation": "cut", "targets": ["Plate"]}));
    let cut = volume(&mut s);
    let expect = expect_pocket - PI * 9.5 * 9.5 * 2.0;
    assert!(rel(cut, expect) < 5e-3, "{cut} vs {expect}");
    let other = run(&mut s, "inspect.measure", json!({}))["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "Other").unwrap()["volume_mm3"]
        .as_f64()
        .unwrap();
    assert!(rel(other, 1000.0) < 1e-6, "{other}");
    run(&mut s, "edit.undo", json!({}));
    assert!(rel(volume(&mut s), pocketed) < 1e-6);
}

#[test]
fn form_edits_are_deterministic_and_a_move_changes_the_body() {
    let mut s = Session::default();
    for shape in ["box", "plane", "cylinder", "sphere", "quadball", "torus"] {
        run(&mut s, "form.create", json!({"shape": shape, "length": 20, "width": 12, "thickness": 0, "name": shape}));
    }
    let forms: Vec<String> =
        s.doc.features.iter().filter(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Form { .. })).map(|f| f.name.clone()).collect();
    assert_eq!(forms, ["box", "plane", "cylinder", "sphere", "quadball", "torus"]);
    let before = area(&mut s);
    run(&mut s, "form.edit_point", json!({"feature": "box", "index": 5, "move": [0, 0, 4]}));
    let moved = area(&mut s);
    assert!((moved - before).abs() > 1e-3, "{moved} vs {before}");
    let saved = s.doc.to_json();
    let mut again = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert!(rel(area(&mut again), moved) < 1e-3, "{} vs {moved}", area(&mut again));
    assert!(s.execute("form.subdivide", &json!({"feature": "box", "level": 7})).is_err());
    assert_eq!(s.doc.to_json(), saved);
    let matched = run(&mut s, "form.match", json!({"feature": "plane", "edge": 0, "target_edges": 2}));
    assert_eq!(matched["points"], 49, "{matched}");
    let sub = run(&mut s, "form.subdivide", json!({"feature": "box", "level": 1}));
    assert_eq!(sub["points"], 49, "{sub}");
    run(&mut s, "form.symmetry", json!({"feature": "box", "plane": "yz"}));
    run(&mut s, "form.crease", json!({"feature": "box", "index": 1}));
    let finished = run(&mut s, "form.finish", json!({"feature": "sphere", "mode": "surface"}));
    assert!(finished.get("feature").is_some(), "{finished}");
}

#[test]
fn a_box_edge_loop_has_four_edges_and_a_face_filter_rejects_edges() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let looped = run(&mut s, "select.loop", json!({"edge": 0}));
    assert_eq!(looped["count"], 4, "{looped}");
    assert_eq!(s.selection.iter().filter(|item| matches!(item, Sel::Edge { .. })).count(), 4);
    run(&mut s, "ui.select_filter", json!({"kinds": ["face"]}));
    assert!(!s.allows_pick("edge"));
    assert!(s.allows_pick("face"));
    run(&mut s, "ui.select_filter", json!({"kinds": []}));
    assert!(s.allows_pick("edge"));
}

#[test]
fn visual_style_and_repeat_do_not_touch_geometry_on_a_bad_repeat() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 8, "width": 8, "height": 8, "body_name": "Box"}));
    let features = s.doc.features.len();
    run(&mut s, "view.visual_style", json!({"style": "wireframe"}));
    assert_eq!(s.visual_style, 2);
    assert_eq!(s.doc.features.len(), features);
    assert!(s.execute("view.visual_style", &json!({"style": "nope"})).is_err());
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [4, 4]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 3}));
    let sketch = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. })).map(|f| f.id).unwrap();
    run(&mut s, "timeline.delete", json!({"features": [sketch.to_string()]}));
    s.last_command = Some(("solid.extrude".into(), json!({"distance": 3})));
    let before = s.doc.to_json();
    assert!(s.execute("ui.repeat_last", &json!({})).is_err());
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn a_version_note_round_trips_and_a_long_note_is_refused() {
    let mut s = Session::default();
    run(&mut s, "file.version_note", json!({"text": "first cut"}));
    let loaded = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(loaded.version_note, "first cut");
    let mut cold = Session::new(loaded);
    let history = run(&mut cold, "file.history", json!({}));
    assert_eq!(history["notes"][0], "first cut");
    assert!(s.execute("file.version_note", &json!({"text": "x".repeat(4001)})).is_err());
    assert_eq!(s.doc.version_note, "first cut");
}

#[test]
fn mesh_deviation_of_a_rebuild_is_about_zero() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}));
    let mesh = s.model.state().body("Box").unwrap().body.tessellate(0.2).unwrap();
    let positions: Vec<f64> = mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
    let triangles: Vec<u32> = mesh.triangles.iter().flat_map(|t| *t).collect();
    // Insert the tessellation as a mesh body through the document feature used by file import.
    s.doc_mut()
        .add_feature(
            solidcraft_doc::FeatureKind::MeshImport {
                file: "box".into(),
                meshes: vec![solidcraft_doc::MeshData { name: "Scan".into(), positions, triangles, color: None }],
            },
            Some("Scan"),
        )
        .unwrap();
    s.refresh();
    let report = run(&mut s, "mesh.deviation", json!({"body": "Box", "mesh": "Scan"}));
    let mean = report["mean"].as_f64().unwrap();
    assert!(mean < 0.5, "{report}");
    assert!(report["max"].as_f64().unwrap() >= 0.0);
}

#[test]
fn a_cantilever_tip_matches_beam_theory() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 10, "height": 10, "body_name": "Beam"}));
    let features = s.doc.features.len();
    let solved = run(&mut s, "sim.static", json!({"body": "Beam", "force": 100, "modulus": 200000}));
    let tip = solved["tip_mm"].as_f64().unwrap();
    let inertia = 10.0 * 1000.0 / 12.0;
    let want = 100.0 * 1_000_000.0 / (3.0 * 200_000.0 * inertia);
    assert!(rel(tip, want) < 1e-6, "{tip} vs {want}");
    let peak = 100.0 * 100.0 * 5.0 / inertia;
    assert!(rel(solved["stress_max"].as_f64().unwrap(), peak) < 1e-6, "{solved}");
    assert!(rel(solved["stress_min"].as_f64().unwrap(), -peak) < 1e-6, "{solved}");
    assert_eq!(solved["method"], "solidcraft-beam-stiffness");
    assert_eq!(s.doc.features.len(), features);
    run(&mut s, "sim.modal", json!({"body": "Beam", "name": "Modes"}));
    let mode = s.execute("sim.solve", &json!({"name": "Modes"})).unwrap_err();
    assert!(mode.to_string().contains("not supported yet"), "{mode}");
    assert!(s.doc.studies.iter().any(|study| study.name == "Modes" && study.tip.is_none() && study.stress_max.is_none()));
    run(&mut s, "sim.buckling", json!({"body": "Beam", "name": "Buckle"}));
    let buckled = s.execute("sim.solve", &json!({"name": "Buckle"})).unwrap_err();
    assert!(buckled.to_string().contains("not supported yet"), "{buckled}");
    assert_eq!(s.doc.features.len(), features);
}

#[test]
fn a_pocket_reports_removed_stock_and_a_short_is_only_a_report() {
    let mut s = Session::default();
    run(&mut s, "cam.setup", json!({"stock": [50, 50, 20]}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pocket"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "cam.pocket", json!({"sketch": "Pocket", "tool_diameter": 2, "depth": 10}));
    let sim = run(&mut s, "cam.simulate", json!({}));
    assert!(rel(sim["removed"].as_f64().unwrap(), 18.0 * 18.0 * 10.0) < 1e-6, "{sim}");
    let occurrences = s.doc.occurrences.clone();
    let story = s.execute("anim.storyboard", &json!({"name": "Explode"})).unwrap_err();
    assert!(story.to_string().contains("no components"), "{story}");
    let keyed = s.execute("anim.key", &json!({"time": 1, "occurrence": 0, "translate": [0, 0, 10]})).unwrap_err();
    assert!(keyed.to_string().contains("no components"), "{keyed}");
    assert_eq!(s.doc.occurrences, occurrences);
    run(&mut s, "ecad.schematic", json!({"parts": ["U1"]}));
    run(&mut s, "ecad.wire", json!({"a": "vcc", "b": "mid"}));
    run(&mut s, "ecad.wire", json!({"a": "mid", "b": "gnd"}));
    let before = s.doc.to_json();
    let erc = run(&mut s, "ecad.erc", json!({}));
    assert_eq!(erc["shorts"], 1, "{erc}");
    assert_eq!(erc["unconnected"], 1, "{erc}");
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "component.create", json!({"name": "Lid"}));
    let list = run(&mut s, "drawing.parts_list", json!({}));
    assert_eq!(list["rows"], 1);
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 8, "body_name": "Keep"}));
    run(&mut s, "generative.study", json!({"preserve": ["Keep"], "objective": "mass"}));
    let before = s.doc.features.len();
    let generated = run(&mut s, "generative.solve", json!({}));
    assert_eq!(generated["method"], "solidcraft-structured-lattice");
    assert!(generated["cells"].as_u64().unwrap() > 1, "{generated}");
    assert!(generated["width"].as_f64().unwrap() > 0.0, "{generated}");
    assert_eq!(s.doc.features.len(), before);
    assert!(s.model.state().body("Candidate").is_none());
    run(&mut s, "generative.promote", json!({}));
    run(&mut s, "solid.fillet", json!({"edges": [[0, 0, 4]], "radius": 1, "body": "Candidate"}));
    assert!(volume(&mut s) > 0.0);
}

#[test]
fn adaptive_stays_outside_and_a_board_pushes_to_3d() {
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"radius": 10, "body_name": "Ball"}));
    run(&mut s, "cam.setup", json!({"stock": [80, 80, 80]}));
    let path = run(&mut s, "cam.adaptive", json!({"body": "Ball", "tool_diameter": 4}));
    assert!(path["samples"].as_array().unwrap().len() > 8, "{path}");
    let swept = run(&mut s, "cam.simulate", json!({}));
    assert!(swept["removed"].as_f64().unwrap() > 1.0, "{swept}");
    assert_eq!(swept["method"], "toolpath");
    for sample in path["samples"].as_array().unwrap() {
        let (x, y, z) = (sample[0].as_f64().unwrap(), sample[1].as_f64().unwrap(), sample[2].as_f64().unwrap());
        let radius = (x * x + y * y + z * z).sqrt();
        assert!(radius >= 8.0, "tool centre inside the offset sphere: {radius}");
    }
    run(&mut s, "ecad.board", json!({"width": 30, "height": 20}));
    run(&mut s, "ecad.trace", json!({"x0": 0, "y0": 0, "x1": 5, "y1": 0, "width": 0.2}));
    run(&mut s, "ecad.trace", json!({"x0": 0.1, "y0": 0, "x1": 5, "y1": 0, "width": 0.2}));
    let copper = s.doc.to_json();
    let drc = run(&mut s, "ecad.drc", json!({"clearance": 0.2}));
    assert!(drc["violations"].as_u64().unwrap() >= 1, "{drc}");
    assert_eq!(s.doc.to_json(), copper);
    run(&mut s, "ecad.push_3d", json!({}));
    assert!(s.model.state().body("PCB").is_some());
    let dir = std::env::temp_dir();
    let dxf = dir.join("solidcraft-sheet.dxf");
    let frame = dir.join("solidcraft-frame.png");
    let gerber = dir.join("solidcraft-board.gbr");
    run(&mut s, "drawing.create", json!({"name": "Sheet"}));
    run(&mut s, "drawing.export_dxf", json!({"path": dxf.to_string_lossy()}));
    let text = std::fs::read_to_string(&dxf).unwrap();
    assert!(text.matches("LINE").count() > 4, "{text}");
    let shaded = dir.join("solidcraft-shaded.png");
    let shot = run(&mut s, "render.image", json!({"path": shaded.to_string_lossy()}));
    assert_eq!(shot["varied"], true, "{shot}");
    let png = std::fs::read(&shaded).unwrap();
    assert!(png.starts_with(b"\x89PNG"), "not a png");
    let places = s.doc.occurrences.clone();
    let story = run(&mut s, "anim.storyboard", json!({}));
    assert_eq!(story["source"], "empty", "{story}");
    run(&mut s, "anim.publish", json!({"path": frame.to_string_lossy()}));
    assert_eq!(s.doc.occurrences, places);
    assert!(std::fs::metadata(&frame).unwrap().len() > 8);
    run(&mut s, "ecad.export", json!({"path": gerber.to_string_lossy()}));
    let gerber_text = std::fs::read_to_string(&gerber).unwrap();
    assert!(gerber_text.contains("M02"), "{gerber_text}");
}

#[test]
fn a_planar_patch_offsets_and_remesh_does_not_add_triangles() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pad"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.patch", json!({"sketch": "Pad", "body_name": "Sheet"}));
    let before = s.model.state().body("Sheet").unwrap().body.tessellate(0.2).unwrap().measure().centroid.z;
    run(&mut s, "surface.offset", json!({"body": "Sheet", "distance": "2 mm"}));
    let after = s.model.state().body("Sheet").unwrap().body.tessellate(0.2).unwrap().measure().centroid.z;
    assert!((after - before).abs() > 1.0, "{before} -> {after}");
    let mesh = s.model.state().body("Sheet").unwrap().body.tessellate(0.5).unwrap();
    let count = mesh.triangles.len();
    let positions: Vec<f64> = mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
    let triangles: Vec<u32> = mesh.triangles.iter().flat_map(|t| *t).collect();
    s.doc_mut()
        .add_feature(
            solidcraft_doc::FeatureKind::MeshImport {
                file: "sheet".into(),
                meshes: vec![solidcraft_doc::MeshData { name: "Cloth".into(), positions, triangles, color: None }],
            },
            Some("Cloth"),
        )
        .unwrap();
    s.refresh();
    let target = (count / 2).max(1);
    run(&mut s, "mesh.remesh", json!({"body": "Cloth", "target": target}));
    let left = s.model.state().body("Cloth").unwrap().body.triangle_mesh().map(|(_, t)| t.len()).unwrap_or(0);
    assert!(left <= count, "{left} vs {count}");
}
