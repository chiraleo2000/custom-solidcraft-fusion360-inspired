//! Local PBR images and storyboard playback. Real placements stay where they are.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn two_boxes(s: &mut Session) -> (u64, u64) {
    let mut ids = [0, 0];
    for (i, (name, x)) in [("A", 0.0), ("B", 40.0)].into_iter().enumerate() {
        run(s, "component.activate", json!({"component": "root"}));
        let made = run(s, "component.create", json!({"name": name}));
        ids[i] = made["occurrence"].as_u64().expect("occurrence");
        run(s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(s, "component.activate", json!({"component": "root"}));
    (ids[0], ids[1])
}

fn pose_translate(play: &Value, occurrence: u64) -> [f64; 3] {
    let pose = play["poses"].as_array().and_then(|poses| poses.iter().find(|pose| pose["occurrence"] == occurrence)).expect("pose");
    let t = pose["translate"].as_array().expect("translate");
    [t[0].as_f64().unwrap_or(f64::NAN), t[1].as_f64().unwrap_or(f64::NAN), t[2].as_f64().unwrap_or(f64::NAN)]
}

/// Feature: fusion360-feature-parity, Property 24: Render and animation leave geometry and placements unchanged
#[test]
fn property_24_storyboard_interpolates_without_moving_placements() {
    let mut empty = Session::default();
    for id in ["anim.storyboard", "anim.key", "anim.play", "anim.publish"] {
        let err = empty.execute(id, &json!({"path": "unused.png", "time": 0, "occurrence": 1})).unwrap_err();
        assert!(err.to_string().contains("no components"), "{id} {err}");
        assert!(empty.doc.occurrences.is_empty());
        assert!(empty.doc.components.is_empty());
    }

    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Arm"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 8, "body_name": "Arm"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let occurrence = s.doc.occurrences[0].id;
    let placements = s.doc.occurrences.clone();
    let features = s.doc.features.clone();
    let pending = s.pending_moves.clone();
    run(&mut s, "anim.storyboard", json!({"name": "Lift"}));
    run(&mut s, "anim.key", json!({"story": "Lift", "time": 0, "occurrence": occurrence, "translate": [0, 0, 0]}));
    run(&mut s, "anim.key", json!({"story": "Lift", "time": 1, "occurrence": occurrence, "translate": [0, 0, 10]}));
    assert_eq!(s.doc.occurrences, placements);
    for i in 0..100 {
        let t = i as f64 / 99.0;
        let play = run(&mut s, "anim.play", json!({"story": "Lift", "time": t}));
        let got = pose_translate(&play, occurrence);
        assert!((got[2] - 10.0 * t).abs() < 1e-9, "{i} {got:?}");
        assert!(got[0].abs() < 1e-12 && got[1].abs() < 1e-12, "{got:?}");
        assert_eq!(s.doc.occurrences, placements);
        assert_eq!(s.pending_moves, pending);
        assert_eq!(s.doc.features, features);
    }
}

#[test]
fn metalness_and_roughness_change_the_png_and_leave_the_solid() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 16, "body_name": "Block"}));
    let features = s.doc.features.len();
    let positions = s.model.state().bodies[0].mesh().positions.clone();
    let dir = std::env::temp_dir();
    let metal = dir.join(format!("solidcraft-metal-{}.png", std::process::id()));
    let rough = dir.join(format!("solidcraft-rough-{}.png", std::process::id()));
    let brass = dir.join(format!("solidcraft-brass-{}.png", std::process::id()));
    let plastic = dir.join(format!("solidcraft-plastic-{}.png", std::process::id()));
    run(&mut s, "appearance.assign", json!({"bodies": ["Block"], "color": "#808080", "metalness": 0.95, "roughness": 0.15}));
    let shot = run(&mut s, "render.image", json!({"path": metal.to_string_lossy()}));
    assert_eq!(shot["varied"], true, "{shot}");
    assert_eq!(shot["fallback"], false, "{shot}");
    run(&mut s, "appearance.assign", json!({"bodies": ["Block"], "color": "#808080", "metalness": 0.0, "roughness": 0.95}));
    run(&mut s, "render.image", json!({"path": rough.to_string_lossy()}));
    let metal_bytes = std::fs::read(&metal).unwrap();
    let rough_bytes = std::fs::read(&rough).unwrap();
    assert!(metal_bytes.starts_with(b"\x89PNG") && rough_bytes.starts_with(b"\x89PNG"));
    assert_ne!(metal_bytes, rough_bytes, "metalness and roughness did not change the image");
    run(&mut s, "appearance.assign", json!({"bodies": ["Block"], "appearance": "Brass - Polished"}));
    run(&mut s, "render.image", json!({"path": brass.to_string_lossy()}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Block"], "appearance": "ABS - Red"}));
    run(&mut s, "render.image", json!({"path": plastic.to_string_lossy()}));
    assert_ne!(std::fs::read(&brass).unwrap(), std::fs::read(&plastic).unwrap());
    assert_eq!(s.doc.features.len(), features);
    assert_eq!(s.model.state().bodies[0].mesh().positions, positions);
    assert!(s.doc.occurrences.is_empty());
    let _ = std::fs::remove_file(&metal);
    let _ = std::fs::remove_file(&rough);
    let _ = std::fs::remove_file(&brass);
    let _ = std::fs::remove_file(&plastic);
}

#[test]
fn a_missing_environment_falls_back_and_a_cloud_renderer_is_refused() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 12, "width": 12, "height": 12}));
    let features = s.doc.features.clone();
    let occurrences = s.doc.occurrences.clone();
    let missing = run(&mut s, "render.environment", json!({"name": "night-hdri"}));
    assert_eq!(missing["fallback"], true, "{missing}");
    assert!(missing["missing"].as_array().is_some_and(|names| names.iter().any(|name| name == "night-hdri")), "{missing}");
    assert!((missing["color"][0].as_f64().unwrap_or(0.0) - 1.0).abs() < 1e-9, "{missing}");
    let warm = run(&mut s, "render.environment", json!({"name": "warm"}));
    assert_eq!(warm["fallback"], false, "{warm}");
    assert!(warm["color"][0].as_f64().unwrap_or(0.0) > warm["color"][2].as_f64().unwrap_or(1.0), "{warm}");
    assert_eq!(s.doc.features, features);
    assert_eq!(s.doc.occurrences, occurrences);
    let err = s.execute("render.image", &json!({"cloud": true, "path": "cloud.png"})).unwrap_err();
    assert!(err.to_string().contains("not supported yet"), "{err}");
    assert!(err.to_string().contains("cloud"), "{err}");
    assert_eq!(s.doc.features, features);
    assert_eq!(s.doc.occurrences, occurrences);
    let shot = std::env::temp_dir().join(format!("solidcraft-missing-look-{}.png", std::process::id()));
    let rendered = run(&mut s, "render.image", json!({"path": shot.to_string_lossy(), "appearance": "Unobtainium", "environment": "no-such-light"}));
    assert_eq!(rendered["fallback"], true, "{rendered}");
    assert!(rendered["missing"].as_array().is_some_and(|names| names.len() == 2), "{rendered}");
    assert!(std::fs::read(&shot).unwrap().starts_with(b"\x89PNG"));
    assert_eq!(s.doc.features, features);
    let _ = std::fs::remove_file(&shot);
}

#[test]
fn explode_and_auto_steps_interpolate_and_publish_without_moving_placements() {
    let mut s = Session::default();
    let (a, b) = two_boxes(&mut s);
    let placements = s.doc.occurrences.clone();
    let pending = s.pending_moves.clone();
    run(&mut s, "explode.create", json!({"name": "Apart", "scale": 1}));
    let exploded = s.doc.assembly.exploded.clone();
    let made = run(&mut s, "anim.storyboard", json!({"name": "Apart"}));
    assert_eq!(made["source"], "explode", "{made}");
    assert!(made["keys"].as_u64().unwrap_or(0) >= 4, "{made}");
    assert_eq!(s.doc.occurrences, placements);
    assert_eq!(s.doc.assembly.exploded, exploded);
    assert_eq!(s.pending_moves, pending);
    let end = s.doc.stories[0].keys.iter().find(|key| key.occurrence == a && (key.time - 1.0).abs() < 1e-9).cloned().expect("end key");
    let start = run(&mut s, "anim.play", json!({"time": 0}));
    let mid = run(&mut s, "anim.play", json!({"time": 0.5}));
    let full = run(&mut s, "anim.play", json!({"time": 1}));
    let at = |play: &Value, id| pose_translate(play, id);
    assert!(at(&start, a).iter().all(|n| n.abs() < 1e-9), "{start}");
    let half = at(&mid, a);
    let done = at(&full, a);
    assert!((half[0] - end.translate[0] * 0.5).abs() < 1e-6, "{half:?} vs {:?}", end.translate);
    assert!((done[0] - end.translate[0]).abs() < 1e-6 && (done[2] - end.translate[2]).abs() < 1e-6, "{done:?}");
    assert!(done[0].abs() > 1.0, "explode did not separate A: {done:?}");
    assert_ne!(at(&full, b)[0], at(&full, a)[0]);
    assert_eq!(s.doc.occurrences, placements);

    let dir = std::env::temp_dir();
    let frame = dir.join(format!("solidcraft-story-{}.png", std::process::id()));
    let published = run(&mut s, "anim.publish", json!({"path": frame.to_string_lossy(), "frames": 2}));
    assert_eq!(published["frames"], 2, "{published}");
    let paths = published["paths"].as_array().expect("paths");
    assert_eq!(paths.len(), 2);
    let first = std::fs::read(paths[0].as_str().unwrap()).unwrap();
    let second = std::fs::read(paths[1].as_str().unwrap()).unwrap();
    assert!(first.starts_with(b"\x89PNG") && second.starts_with(b"\x89PNG"));
    assert_ne!(first, second, "the published frames are the same picture");
    assert_eq!(s.doc.occurrences, placements);
    assert_eq!(s.doc.assembly.exploded, exploded);
    assert_eq!(s.pending_moves, pending);
    for path in paths {
        let _ = std::fs::remove_file(path.as_str().unwrap_or_default());
    }

    let mut auto = Session::default();
    two_boxes(&mut auto);
    let placements = auto.doc.occurrences.clone();
    let views = auto.doc.assembly.exploded.clone();
    let made = run(&mut auto, "anim.storyboard", json!({"auto": true, "scale": 1}));
    assert_eq!(made["source"], "auto", "{made}");
    assert!(made["keys"].as_u64().unwrap_or(0) >= 4, "{made}");
    assert_eq!(auto.doc.assembly.exploded, views);
    assert_eq!(auto.doc.occurrences, placements);
    let id = auto.doc.occurrences[0].id;
    let end = auto.doc.stories[0].keys.iter().find(|key| key.occurrence == id && (key.time - 1.0).abs() < 1e-9).cloned().expect("auto end");
    let mid = run(&mut auto, "anim.play", json!({"time": 0.5}));
    let half = pose_translate(&mid, id);
    assert!((half[0] - end.translate[0] * 0.5).abs() < 1e-6, "{half:?} {:?}", end.translate);
    assert_eq!(auto.doc.occurrences, placements);
}

#[test]
fn a_motion_study_is_reused_as_preview_keys() {
    let mut s = Session::default();
    for (name, x) in [("A", 0.0), ("B", 20.0)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let a = s.doc.occurrences.iter().find(|o| o.name.starts_with('A')).map(|o| o.id).expect("A");
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "motion.study", json!({"name": "Push", "steps": 100, "keys": [{"joint": "Rail", "points": [[0, 10], [100, -5]]}]}));
    let placements = s.doc.occurrences.clone();
    let joints = s.doc.assembly.joints.clone();
    let studies = s.doc.assembly.studies.clone();
    let made = run(&mut s, "anim.storyboard", json!({"name": "Push"}));
    assert_eq!(made["source"], "motion", "{made}");
    assert!(made["keys"].as_u64().unwrap_or(0) >= 2, "{made}");
    assert_eq!(s.doc.occurrences, placements);
    assert_eq!(s.doc.assembly.joints, joints);
    assert_eq!(s.doc.assembly.studies, studies);
    let play = run(&mut s, "anim.play", json!({"time": 50}));
    let moved = play["poses"].as_array().expect("poses").iter().any(|pose| (pose["translate"][0].as_f64().unwrap_or(0.0) + 7.5).abs() < 1e-3);
    assert!(moved, "{play}");
    assert_eq!(s.doc.occurrences, placements);
    assert_eq!(s.doc.assembly.joints, joints);
}

#[test]
fn a_bad_step_a_video_path_and_a_rotation_do_not_move_the_assembly() {
    let mut s = Session::default();
    let (a, _) = two_boxes(&mut s);
    let placements = s.doc.occurrences.clone();
    let pending = s.pending_moves.clone();
    run(&mut s, "anim.storyboard", json!({"name": "Turn"}));
    let missing = s.execute("anim.key", &json!({"time": 1, "occurrence": 999, "translate": [0, 0, 1]})).unwrap_err();
    assert!(missing.to_string().contains("no occurrence"), "{missing}");
    let bad_time = s.execute("anim.key", &json!({"time": "soon", "occurrence": a})).unwrap_err();
    assert!(bad_time.to_string().contains("time"), "{bad_time}");
    assert_eq!(s.doc.occurrences, placements);
    let quarter = std::f64::consts::FRAC_PI_4;
    run(&mut s, "anim.key", json!({"time": 0, "occurrence": a, "rotate": [0, 0, 0, 1]}));
    run(&mut s, "anim.key", json!({"time": 1, "occurrence": a, "rotate": [0, 0, quarter.sin(), quarter.cos()]}));
    let mid = run(&mut s, "anim.play", json!({"time": 0.5}));
    let rotate = mid["poses"].as_array().and_then(|poses| poses.iter().find(|pose| pose["occurrence"] == a)).expect("pose")["rotate"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let z = rotate.get(2).and_then(Value::as_f64).unwrap_or(0.0);
    let w = rotate.get(3).and_then(Value::as_f64).unwrap_or(0.0);
    assert!((z - (quarter / 2.0).sin()).abs() < 1e-3, "{rotate:?}");
    assert!((w - (quarter / 2.0).cos()).abs() < 1e-3, "{rotate:?}");
    assert_eq!(s.doc.occurrences, placements);
    assert_eq!(s.pending_moves, pending);
    let video = s.execute("anim.publish", &json!({"path": "story.mp4"})).unwrap_err();
    assert!(video.to_string().contains("not supported yet"), "{video}");
    assert_eq!(s.doc.occurrences, placements);
    let mut lone = Session::default();
    run(&mut lone, "component.create", json!({"name": "Only"}));
    run(&mut lone, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let alone = lone.doc.occurrences.clone();
    let err = lone.execute("anim.storyboard", &json!({"auto": true})).unwrap_err();
    assert!(err.to_string().contains("two or more"), "{err}");
    assert_eq!(lone.doc.occurrences, alone);
    assert!(lone.doc.stories.is_empty());
    assert!(lone.doc.assembly.exploded.is_empty());
}
