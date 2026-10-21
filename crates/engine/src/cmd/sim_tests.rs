//! Static studies, generative candidates, and the cases that must refuse a result.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn rel(got: f64, want: f64) -> f64 {
    (got - want).abs() / want.abs().max(1e-12)
}

fn beam(s: &mut Session) {
    run(s, "solid.box", json!({"length": 100, "width": 10, "height": 10, "body_name": "Beam"}));
}

fn features(s: &Session) -> usize {
    s.doc.features.len()
}

/// Feature: fusion360-feature-parity, Property 24: Analysis leaves geometry unchanged and checks are sound
#[test]
fn property_24_static_solve_leaves_geometry_and_matches_beam_theory() {
    let mut s = Session::default();
    beam(&mut s);
    let before = s.doc.features.clone();
    let bodies = s.model.state().bodies.len();
    let inertia = 10.0 * 1000.0 / 12.0;
    let mut cases = 0usize;
    for i in 1..=100 {
        let force = i as f64 * 5.0;
        let solved = run(&mut s, "sim.static", json!({"name": "S", "body": "Beam", "force": force, "modulus": 200000}));
        let tip = force * 1_000_000.0 / (3.0 * 200_000.0 * inertia);
        let peak = force * 100.0 * 5.0 / inertia;
        assert!(rel(solved["tip_mm"].as_f64().unwrap(), tip) < 1e-6, "{i} {solved}");
        assert!(rel(solved["displacement_max"].as_f64().unwrap(), tip) < 1e-6, "{i}");
        assert!(solved["displacement_min"].as_f64().unwrap().abs() < 1e-8, "{i}");
        assert!(rel(solved["stress_max"].as_f64().unwrap(), peak) < 1e-6, "{i} {solved}");
        assert!(rel(solved["stress_min"].as_f64().unwrap(), -peak) < 1e-6, "{i}");
        assert_eq!(solved["method"], "solidcraft-beam-stiffness");
        let colours = solved["colours"].as_array().unwrap();
        assert!(colours.len() > 2, "{solved}");
        let root = colours.first().unwrap();
        let end = colours.last().unwrap();
        assert!(root[0].as_u64().unwrap() > end[0].as_u64().unwrap(), "root should be redder {colours:?}");
        assert!(end[2].as_u64().unwrap() > root[2].as_u64().unwrap(), "tip should be bluer {colours:?}");
        assert_eq!(s.doc.features, before);
        assert_eq!(s.model.state().bodies.len(), bodies);
        let study = s.doc.studies.iter().find(|study| study.name == "S").unwrap();
        assert!(!study.stale);
        assert_eq!(study.constraints.len(), 1);
        assert_eq!(study.loads.len(), 1);
        cases += 1;
    }
    assert_eq!(cases, 100);
    let samples: Vec<f64> = (0..5).map(|i| i as f64 * 15.0).collect();
    let png = solidcraft_render::result_ramp_png(&samples).expect("ramp");
    assert!(png.starts_with(b"\x89PNG"));
    let path = std::env::temp_dir().join("solidcraft-sim-ramp.png");
    std::fs::write(&path, &png).unwrap();
    let stored = std::fs::read(&path).unwrap();
    assert!(stored.starts_with(b"\x89PNG"));
}

#[test]
fn changing_a_load_marks_the_study_stale_and_a_resolve_updates_it() {
    let mut s = Session::default();
    beam(&mut s);
    run(&mut s, "sim.static", json!({"name": "S", "body": "Beam", "force": 100, "modulus": 200000, "material": "Steel"}));
    let tip = s.doc.studies[0].tip;
    let edited = run(&mut s, "sim.study", json!({"name": "S", "body": "Beam", "force": 200}));
    assert_eq!(edited["stale"], true);
    let study = &s.doc.studies[0];
    assert!(study.stale);
    assert_eq!(study.tip, tip);
    assert_eq!(study.material, "Steel");
    let solved = run(&mut s, "sim.solve", json!({"name": "S"}));
    assert_eq!(solved["method"], "solidcraft-beam-stiffness");
    assert!(!s.doc.studies[0].stale);
    assert!(rel(solved["tip_mm"].as_f64().unwrap(), tip.unwrap() * 2.0) < 1e-6, "{solved}");
}

#[test]
fn under_constrained_unsupported_and_non_box_studies_do_not_invent_stress() {
    let mut s = Session::default();
    beam(&mut s);
    run(&mut s, "sim.study", json!({"name": "Free", "body": "Beam", "kind": "static"}));
    let before = s.doc.to_json();
    let err = s.execute("sim.solve", &json!({"name": "Free"})).unwrap_err();
    assert!(err.to_string().contains("under-constrained"), "{err}");
    assert_eq!(s.doc.to_json(), before);
    assert!(s.doc.studies[0].stress_max.is_none());
    assert!(s.doc.studies[0].method.is_empty());
    run(
        &mut s,
        "sim.study",
        json!({"name": "Axial", "body": "Beam", "constraints": [{"kind": "fixed", "target": "min_x"}], "loads": [{"magnitude": 10, "direction": [1, 0, 0]}]}),
    );
    let err = s.execute("sim.solve", &json!({"name": "Axial"})).unwrap_err();
    assert!(err.to_string().contains("not supported yet"), "{err}");
    assert!(s.doc.studies.iter().find(|study| study.name == "Axial").unwrap().stress_max.is_none());
    run(&mut s, "sim.thermal", json!({"body": "Beam", "name": "Heat"}));
    let err = s.execute("sim.solve", &json!({"name": "Heat"})).unwrap_err();
    assert!(err.to_string().contains("not supported yet"), "{err}");
    run(&mut s, "solid.sphere", json!({"radius": 8, "body_name": "Ball"}));
    let before = s.doc.to_json();
    let err = s.execute("sim.static", &json!({"body": "Ball", "force": 10})).unwrap_err();
    assert!(err.to_string().contains("not supported yet"), "{err}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn hostile_sim_and_generative_inputs_leave_the_design_unchanged() {
    let mut s = Session::default();
    beam(&mut s);
    run(&mut s, "sim.static", json!({"name": "S", "body": "Beam", "force": 50}));
    let before = s.doc.to_json();
    let feature_count = features(&s);
    assert!(s.execute("sim.static", &json!({"body": "Beam", "force": f64::NAN})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert!(s.execute("sim.static", &json!({"body": "Beam", "modulus": f64::INFINITY})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert!(s.execute("sim.solve", &json!({"name": "S", "elements": 1000})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert!(!s.doc.studies[0].stale);
    assert!(s.doc.studies[0].tip.is_some());
    let mut constraints = Vec::new();
    for _ in 0..40 {
        constraints.push(json!({"kind": "fixed", "target": "min_x"}));
    }
    assert!(s.execute("sim.study", &json!({"name": "Huge", "body": "Beam", "constraints": constraints})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(features(&s), feature_count);
    run(&mut s, "generative.study", json!({"name": "G", "preserve": ["Beam"], "objective": "mass", "loads": [{"magnitude": 20}]}));
    let before = s.doc.to_json();
    assert!(s.execute("generative.study", &json!({"name": "G", "loads": [{"magnitude": f64::NAN}]})).is_err());
    assert_eq!(s.doc.to_json(), before);
    let mut names = Vec::new();
    for i in 0..40 {
        names.push(format!("B{i}"));
    }
    assert!(s.execute("generative.study", &json!({"preserve": names})).is_err());
    assert_eq!(s.doc.to_json(), before);
    let generated = run(&mut s, "generative.solve", json!({"name": "G"}));
    assert_eq!(generated["method"], "solidcraft-section-reduction");
    assert_eq!(s.doc.features.len(), feature_count);
    assert!(s.model.state().body("Candidate").is_none());
    run(&mut s, "generative.promote", json!({"name": "G"}));
    assert!(s.model.state().body("Candidate").is_some());
    assert!(s.doc.features.len() == feature_count + 1);
    let again = run(&mut s, "generative.promote", json!({"name": "G"}));
    assert_eq!(again["body"], "Candidate");
    assert_eq!(s.doc.features.len(), feature_count + 1);
}

#[test]
fn an_obstacle_is_kept_out_of_the_candidate_and_old_studies_still_load() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Keep"}));
    run(&mut s, "solid.box", json!({"length": 12, "width": 20, "height": 10, "corner": [28, 0, 0], "body_name": "Block"}));
    let features = s.doc.features.clone();
    run(
        &mut s,
        "generative.study",
        json!({"name": "G", "preserve": ["Keep"], "obstacle": ["Block"], "objective": "stiffness", "loads": [{"kind": "force", "magnitude": 30, "target": "max_x"}]}),
    );
    let generated = run(&mut s, "generative.solve", json!({"name": "G"}));
    assert_eq!(generated["method"], "solidcraft-section-reduction");
    let end = generated["origin"][0].as_f64().unwrap() + generated["length"].as_f64().unwrap();
    assert!(end <= 28.0 + 1e-6, "{generated}");
    assert_eq!(s.doc.features, features);
    assert_eq!(s.doc.generative[0].loads.len(), 1);
    run(&mut s, "generative.study", json!({"name": "G", "objective": "mass"}));
    assert!(s.doc.generative[0].stale);
    let err = s.execute("generative.promote", &json!({"name": "G"})).unwrap_err();
    assert!(err.to_string().contains("stale"), "{err}");
    assert_eq!(s.doc.features, features);

    let old = r#"{"name":"S","kind":"static","body":"B","force":1.0,"modulus":2.0,"tip":null,"method":"","stale":true}"#;
    let study: solidcraft_doc::Study = serde_json::from_str(old).unwrap();
    assert!(study.material.is_empty());
    assert!(study.constraints.is_empty());
    assert!(study.loads.is_empty());
    assert!(study.stress_max.is_none());
    let again: solidcraft_doc::Study = serde_json::from_str(&serde_json::to_string(&study).unwrap()).unwrap();
    assert_eq!(study, again);
    let old_gen = r#"{"name":"G","preserve":["Keep"],"obstacle":[],"objective":"mass","method":"","candidate":null}"#;
    let generative: solidcraft_doc::GenerativeStudy = serde_json::from_str(old_gen).unwrap();
    assert!(generative.loads.is_empty());
    assert_eq!(generative.length, 0.0);
    assert!(!generative.stale);
}

#[test]
fn a_pinned_only_end_is_under_constrained() {
    let mut s = Session::default();
    beam(&mut s);
    let before = s.doc.to_json();
    let err = s
        .execute(
            "sim.static",
            &json!({"body": "Beam", "constraints": [{"kind": "pin", "target": "min_x"}], "loads": [{"magnitude": 25, "target": "max_x"}]}),
        )
        .unwrap_err();
    assert!(err.to_string().contains("under-constrained"), "{err}");
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(features(&s), 1);
}
