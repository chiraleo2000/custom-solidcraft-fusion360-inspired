//! Schematic, PCB, and the checks that must not edit geometry.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use solidcraft_doc::{Board, Document, Schematic};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn rel(got: f64, want: f64) -> f64 {
    (got - want).abs() / want.abs().max(1e-12)
}

fn body_volume(s: &mut Session, name: &str) -> f64 {
    let measured = run(s, "inspect.measure", json!({}));
    measured["bodies"]
        .as_array()
        .and_then(|bodies| bodies.iter().find(|body| body["name"] == name))
        .and_then(|body| body["volume_mm3"].as_f64())
        .unwrap_or(f64::NAN)
}

/// Feature: fusion360-feature-parity, Property 24: ERC and DRC report the rule hits and leave geometry unchanged
#[test]
fn property_24_erc_and_drc_match_the_rules_and_leave_geometry() {
    for i in 1..=100 {
        let mut s = Session::default();
        run(&mut s, "ecad.schematic", json!({"parts": ["U"]}));
        assert_eq!(s.doc.schematic.as_ref().map(|sc| sc.components.len()), Some(1));
        run(&mut s, "ecad.wire", json!({"a": format!("s{i}"), "b": format!("t{i}")}));
        if i % 2 == 0 {
            run(&mut s, "ecad.wire", json!({"a": "vcc", "b": "gnd"}));
        }
        if i % 5 == 0 {
            run(&mut s, "ecad.wire", json!({"a": "U", "b": format!("sig{i}")}));
        }
        let before = s.doc.to_json();
        let erc = run(&mut s, "ecad.erc", json!({}));
        let shorts = u64::from(i % 2 == 0);
        let unconnected = u64::from(i % 5 != 0);
        assert_eq!(erc["shorts"].as_u64(), Some(shorts), "{i} {erc}");
        assert_eq!(erc["unconnected"].as_u64(), Some(unconnected), "{i} {erc}");
        assert_eq!(erc["open_pins"].as_u64(), Some(0), "{i} {erc}");
        assert_eq!(s.doc.to_json(), before, "{i}");
        assert!(s.doc.features.is_empty(), "{i}");
        assert!(s.model.state().bodies.is_empty(), "{i}");
        let nets = s.doc.schematic.as_ref().map(|sc| sc.nets.len()).unwrap_or(0);
        assert!(nets >= 1, "{i}");
        if i % 2 == 0 {
            assert!(s.doc.schematic.as_ref().is_some_and(|sc| sc.nets.iter().any(|net| net.name == "vcc")), "{i}");
        }
    }
    for i in 1..=100 {
        let mut s = Session::default();
        run(&mut s, "ecad.board", json!({"width": 80, "height": 50}));
        let sep = if i % 2 == 0 { 0.30 } else { 0.80 };
        run(&mut s, "ecad.trace", json!({"x0": 5, "y0": 10, "x1": 25, "y1": 10, "width": 0.2}));
        run(&mut s, "ecad.trace", json!({"x0": 5, "y0": 10.0 + sep, "x1": 25, "y1": 10.0 + sep, "width": 0.2}));
        if i % 4 == 0 {
            run(&mut s, "ecad.trace", json!({"x0": 5, "y0": 35, "x1": 15, "y1": 35, "width": 0.05}));
        }
        let before = s.doc.to_json();
        let drc = run(&mut s, "ecad.drc", json!({"clearance": 0.2, "min_width": 0.15}));
        let clearance = u64::from(i % 2 == 0);
        let width = u64::from(i % 4 == 0);
        assert_eq!(drc["clearance"].as_u64(), Some(clearance), "{i} {drc}");
        assert_eq!(drc["width"].as_u64(), Some(width), "{i} {drc}");
        assert_eq!(drc["violations"].as_u64(), Some(clearance + width), "{i} {drc}");
        assert_eq!(s.doc.to_json(), before, "{i}");
        assert!(s.doc.features.is_empty(), "{i}");
        assert!(s.model.state().bodies.is_empty(), "{i}");
    }
}

#[test]
fn pins_nets_and_clearance_are_exact_and_do_not_edit_the_solid() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 8, "body_name": "Keep"}));
    let volume = body_volume(&mut s, "Keep");
    let features = s.doc.features.len();
    run(
        &mut s,
        "ecad.schematic",
        json!({"parts": [{"refdes": "U1", "pins": ["1", "2"], "x": 4, "y": 6}, {"refdes": "U2", "pins": ["1"]}], "nets": [{"name": "VCC", "nodes": ["U1.1", "U2.1"]}]}),
    );
    let before = s.doc.to_json();
    let erc = run(&mut s, "ecad.erc", json!({}));
    assert_eq!(erc["shorts"].as_u64(), Some(0), "{erc}");
    assert_eq!(erc["unconnected"].as_u64(), Some(0), "{erc}");
    assert_eq!(erc["open_pins"].as_u64(), Some(1), "{erc}");
    assert_eq!(s.doc.schematic.as_ref().and_then(|sc| sc.nets.first()).map(|net| net.name.as_str()), Some("VCC"));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "ecad.wire", json!({"a": "vcc", "b": "U1.1"}));
    run(&mut s, "ecad.wire", json!({"a": "gnd", "b": "U1.1"}));
    let before = s.doc.to_json();
    let erc = run(&mut s, "ecad.erc", json!({}));
    assert_eq!(erc["shorts"].as_u64(), Some(1), "{erc}");
    assert_eq!(erc["open_pins"].as_u64(), Some(1), "{erc}");
    assert_eq!(s.doc.to_json(), before);
    let mut letters = Session::default();
    run(&mut letters, "ecad.schematic", json!({"parts": ["U"]}));
    run(&mut letters, "ecad.wire", json!({"a": "VCC", "b": "GND"}));
    let erc = run(&mut letters, "ecad.erc", json!({}));
    assert_eq!(erc["shorts"].as_u64(), Some(1), "{erc}");
    assert_eq!(erc["unconnected"].as_u64(), Some(1), "{erc}");

    run(&mut s, "ecad.board", json!({"width": 40, "height": 30}));
    run(&mut s, "ecad.trace", json!({"x0": 0, "y0": 0, "x1": 10, "y1": 0, "width": 0.4, "net": "vcc"}));
    run(&mut s, "ecad.trace", json!({"x0": 0, "y0": 0.1, "x1": 10, "y1": 0, "width": 0.4, "net": "vcc"}));
    let before = s.doc.to_json();
    let drc = run(&mut s, "ecad.drc", json!({"clearance": 0.2, "min_width": 0.15}));
    assert_eq!(drc["clearance"].as_u64(), Some(0), "{drc}");
    assert_eq!(drc["width"].as_u64(), Some(0), "{drc}");
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "ecad.trace", json!({"x0": 0, "y0": 0, "x1": 10, "y1": 0, "width": 0.4, "net": "gnd"}));
    let drc = run(&mut s, "ecad.drc", json!({"clearance": 0.2, "min_width": 0.15}));
    assert_eq!(drc["clearance"].as_u64(), Some(2), "{drc}");
    run(&mut s, "ecad.trace", json!({"x0": 0, "y0": 20, "x1": 5, "y1": 20, "width": 0.05, "net": "sig"}));
    let drc = run(&mut s, "ecad.drc", json!({"clearance": 0.2, "min_width": 0.15}));
    assert_eq!(drc["width"].as_u64(), Some(1), "{drc}");
    assert_eq!(drc["clearance"].as_u64(), Some(2), "{drc}");
    assert_eq!(drc["violations"].as_u64(), Some(3), "{drc}");

    let mut gap = Session::default();
    run(&mut gap, "ecad.board", json!({"width": 40, "height": 20}));
    run(&mut gap, "ecad.trace", json!({"x0": 0, "y0": 0, "x1": 10, "y1": 0, "width": 0.2, "net": "a"}));
    run(&mut gap, "ecad.trace", json!({"x0": 0, "y0": 0.4, "x1": 10, "y1": 0.4, "width": 0.2, "net": "b"}));
    let exact = run(&mut gap, "ecad.drc", json!({"clearance": 0.2, "min_width": 0.15}));
    assert_eq!(exact["clearance"].as_u64(), Some(0), "{exact}");
    let mut cross = Session::default();
    run(&mut cross, "ecad.board", json!({"width": 20, "height": 20}));
    run(&mut cross, "ecad.trace", json!({"x0": 0, "y0": 0, "x1": 10, "y1": 10, "width": 0.2, "net": "a"}));
    run(&mut cross, "ecad.trace", json!({"x0": 0, "y0": 10, "x1": 10, "y1": 0, "width": 0.2, "net": "b"}));
    let crossed = run(&mut cross, "ecad.drc", json!({"clearance": 0.2}));
    assert_eq!(crossed["clearance"].as_u64(), Some(1), "{crossed}");
    assert_eq!(s.doc.features.len(), features);
    assert!(rel(body_volume(&mut s, "Keep"), volume) < 1e-6, "{volume}");
}

#[test]
fn missing_schematic_and_board_fields_still_load() {
    let schematic: Schematic = serde_json::from_str(r#"{"parts":["U1"],"wires":[["vcc","gnd"]]}"#).unwrap();
    assert!(schematic.components.is_empty());
    assert!(schematic.nets.is_empty());
    assert_eq!(schematic.board_width, 0.0);
    assert!(schematic.traces.is_empty());
    let board: Board = serde_json::from_str(r#"{"width":12.0,"height":8.0}"#).unwrap();
    assert!(board.traces.is_empty());

    let mut value: Value = serde_json::from_str(&Document::new("Old").to_json()).unwrap();
    value["schematic"] = json!({"parts": ["U1"], "wires": [["vcc", "gnd"]], "board_width": 30, "board_height": 20, "traces": [[0, 0, 5, 0, 0.2], [0.1, 0, 5, 0, 0.2]]});
    assert!(value.get("board").is_none());
    let mut s = Session { doc: Document::from_json(&value.to_string()).unwrap().into(), ..Session::default() };
    assert!(s.doc.board.is_none());
    assert!(s.doc.schematic.as_ref().is_some_and(|sc| sc.components.is_empty() && sc.nets.is_empty()));
    let before = s.doc.to_json();
    let erc = run(&mut s, "ecad.erc", json!({}));
    assert_eq!(erc["shorts"].as_u64(), Some(1), "{erc}");
    assert_eq!(erc["unconnected"].as_u64(), Some(1), "{erc}");
    assert_eq!(s.doc.to_json(), before);
    let drc = run(&mut s, "ecad.drc", json!({"clearance": 0.2}));
    assert!(drc["violations"].as_u64().unwrap_or(0) >= 1, "{drc}");
    assert!(s.doc.board.is_none());
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn hostile_ecad_inputs_leave_the_design_unchanged() {
    let mut empty = Session::default();
    assert!(empty.execute("ecad.board", &json!({"width": -1, "height": 2})).is_err());
    assert!(empty.execute("ecad.trace", &json!({"x0": 0, "y0": 0, "x1": 1, "y1": 0, "width": 0})).is_err());
    assert!(empty.execute("ecad.push_3d", &json!({})).is_err());
    assert!(empty.execute("ecad.push_3d", &json!({"autoroute": true})).is_err());
    assert!(empty.doc.schematic.is_none());
    assert!(empty.doc.board.is_none());
    assert!(empty.doc.components.is_empty());
    assert!(empty.model.state().bodies.is_empty());

    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Keep"}));
    run(&mut s, "ecad.schematic", json!({"parts": ["U1"]}));
    run(&mut s, "ecad.wire", json!({"a": "vcc", "b": "mid"}));
    run(&mut s, "ecad.wire", json!({"a": "mid", "b": "gnd"}));
    run(&mut s, "ecad.board", json!({"width": 30, "height": 20}));
    run(&mut s, "ecad.trace", json!({"x0": 0, "y0": 0, "x1": 5, "y1": 0, "width": 0.2}));
    let volume = body_volume(&mut s, "Keep");
    let before = s.doc.to_json();
    let features = s.doc.features.len();
    let mut nets = Vec::new();
    for n in 0..70 {
        let mut nodes = vec![format!("N{n}")];
        for k in 0..63 {
            nodes.push(format!("P{n}_{k}"));
        }
        nets.push(json!({"name": format!("NET{n}"), "nodes": nodes}));
    }
    let pins: Vec<String> = (0..65).map(|i| format!("P{i}")).collect();
    let attacks = [
        json!({"parts": [""]}),
        json!({"parts": [1]}),
        json!({"parts": ["U1", "U1"]}),
        json!({"parts": [{"refdes": "U9", "footprint": "header"}]}),
        json!({"parts": [{"refdes": "U9", "pins": pins}]}),
        json!({"parts": [{"refdes": "U9", "x": 1}]}),
        json!({"nets": nets}),
        json!({"autoroute": true, "parts": ["Z"]}),
    ];
    for params in attacks {
        assert!(s.execute("ecad.schematic", &params).is_err(), "{params}");
        assert_eq!(s.doc.to_json(), before);
    }
    assert!(s.execute("ecad.wire", &json!({"a": "vcc", "b": "vcc"})).is_err());
    assert!(s.execute("ecad.wire", &json!({"a": ""})).is_err());
    assert!(s.execute("ecad.board", &json!({"width": -5, "height": 10})).is_err());
    assert!(s.execute("ecad.board", &json!({"width": f64::NAN, "height": 10})).is_err());
    assert!(s.execute("ecad.board", &json!({"width": 10, "height": 10, "layer": "inner"})).is_err());
    assert!(s.execute("ecad.trace", &json!({"x0": 0, "y0": 0, "x1": 1, "y1": 0, "width": -1})).is_err());
    assert!(s.execute("ecad.trace", &json!({"x0": 20_000, "y0": 0, "x1": 1, "y1": 0, "width": 0.2})).is_err());
    assert!(s.execute("ecad.trace", &json!({"x0": 0, "y0": 0, "x1": 1, "y1": 0, "width": 0.2, "route": true})).is_err());
    assert!(s.execute("ecad.drc", &json!({"clearance": -1})).is_err());
    assert!(s.execute("ecad.drc", &json!({"min_width": "wide"})).is_err());
    assert!(s.execute("ecad.push_3d", &json!({"thickness": 0})).is_err());
    assert!(s.execute("ecad.push_3d", &json!({"autoroute": true})).is_err());
    assert!(s.execute("ecad.push_3d", &json!({"models": true})).is_err());
    assert!(s.execute("ecad.export", &json!({})).is_err());
    assert!(s.execute("ecad.export", &json!({"path": "board.gbr", "autoroute": true})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.doc.features.len(), features);
    assert!(s.model.state().body("PCB").is_none());
    assert!(rel(body_volume(&mut s, "Keep"), volume) < 1e-6);
    let erc = run(&mut s, "ecad.erc", json!({}));
    assert_eq!(erc["shorts"].as_u64(), Some(1), "{erc}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn push_3d_makes_an_assembly_component_and_exports_gerber() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Lid"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "body_name": "Lid"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "ecad.schematic", json!({"parts": ["U1"]}));
    run(&mut s, "ecad.board", json!({"width": 30, "height": 20}));
    run(&mut s, "ecad.trace", json!({"x0": 2, "y0": 2, "x1": 12, "y1": 2, "width": 0.25, "net": "vcc"}));
    let made = run(&mut s, "ecad.push_3d", json!({}));
    assert_eq!(made["body"], "PCB");
    assert_eq!(made["component"], "PCB");
    assert_eq!(s.active_component, 0);
    let (pcb_owner, lid_owner) = {
        let state = s.model.state();
        let pcb = state.body("PCB").expect("pcb body");
        let lid = state.body("Lid").expect("lid body");
        (s.doc.model_body_component(pcb), s.doc.model_body_component(lid))
    };
    assert_ne!(pcb_owner, 0);
    assert_ne!(pcb_owner, lid_owner);
    assert_eq!(s.doc.components.iter().find(|component| component.id == pcb_owner).map(|component| component.name.as_str()), Some("PCB"));
    assert!(s.doc.occurrences.iter().any(|occurrence| occurrence.component == pcb_owner && occurrence.parent == 0));
    assert_eq!(made["component_id"].as_u64(), Some(pcb_owner));
    assert!(rel(body_volume(&mut s, "PCB"), 30.0 * 20.0 * 1.6) < 1e-4);
    assert!(rel(body_volume(&mut s, "Lid"), 1000.0) < 1e-4);
    let png = std::env::temp_dir().join(format!("solidcraft-ecad-board-{}.png", std::process::id()));
    let shot = run(&mut s, "render.image", json!({"path": png.to_string_lossy()}));
    assert_eq!(shot["varied"], true, "{shot}");
    let bytes = std::fs::read(&png).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"), "not a png");
    let features = s.doc.features.len();
    let again = run(&mut s, "ecad.push_3d", json!({}));
    assert_eq!(again["unchanged"], true, "{again}");
    assert_eq!(s.doc.features.len(), features);
    let updated = run(&mut s, "ecad.push_3d", json!({"thickness": 2.0}));
    assert_eq!(updated["updated"], true, "{updated}");
    assert_eq!(s.active_component, 0);
    assert!(rel(body_volume(&mut s, "PCB"), 30.0 * 20.0 * 2.0) < 1e-4);
    assert!(rel(body_volume(&mut s, "Lid"), 1000.0) < 1e-4);
    let before = s.doc.to_json();
    let err = s.execute("ecad.push_3d", &json!({"autoroute": true})).unwrap_err();
    assert!(err.to_string().contains("not supported yet"), "{err}");
    assert!(err.to_string().contains("autorouting"), "{err}");
    assert_eq!(s.doc.to_json(), before);
    let gerber = std::env::temp_dir().join(format!("solidcraft-ecad-{}.gbr", std::process::id()));
    run(&mut s, "ecad.export", json!({"path": gerber.to_string_lossy()}));
    assert_eq!(s.doc.to_json(), before);
    let text = std::fs::read_to_string(&gerber).unwrap();
    assert!(text.contains("%FSLAX46Y46*%"), "{text}");
    assert!(text.contains("%MOMM*%"), "{text}");
    assert!(text.contains("M02*"), "{text}");
    assert!(text.contains("PCB autorouting is not supported yet"), "{text}");
    let layer = solidcraft_io::read_gerber(&text).unwrap();
    assert!((layer.width - 30.0).abs() < 1e-4, "{layer:?}");
    assert!((layer.height - 20.0).abs() < 1e-4, "{layer:?}");
    assert_eq!(layer.traces.len(), 1, "{layer:?}");
    let loaded = Document::from_json(&s.doc.to_json()).unwrap();
    let trace = loaded.board.as_ref().and_then(|board| board.traces.first()).expect("trace");
    assert_eq!(trace.net, "vcc");
    assert!((trace.width - 0.25).abs() < 1e-9);
    assert_eq!(loaded.schematic.as_ref().and_then(|sc| sc.components.first()).map(|part| part.refdes.as_str()), Some("U1"));
}

#[test]
fn multi_layer_copper_drc_erc_and_gerber_stay_read_only() {
    let mut s = Session::default();
    run(&mut s, "ecad.schematic", json!({"parts": [{"refdes": "U1", "pins": ["1", "2"]}], "nets": [{"name": "VCC", "nodes": ["VCC"]}]}));
    let before = s.doc.to_json();
    let erc = run(&mut s, "ecad.erc", json!({}));
    assert_eq!(erc["dangling"].as_u64(), Some(1), "{erc}");
    assert_eq!(erc["undriven"].as_u64(), Some(1), "{erc}");
    assert_eq!(erc["open_pins"].as_u64(), Some(2), "{erc}");
    assert_eq!(s.doc.to_json(), before);

    run(&mut s, "ecad.board", json!({"width": 40, "height": 30, "layers": 2}));
    run(&mut s, "ecad.trace", json!({"x0": 5, "y0": 5, "x1": 20, "y1": 5, "width": 0.2, "net": "a", "layer": "top"}));
    run(&mut s, "ecad.trace", json!({"x0": 5, "y0": 5.1, "x1": 20, "y1": 5.1, "width": 0.2, "net": "b", "layer": "bottom"}));
    run(&mut s, "ecad.trace", json!({"x0": 0.05, "y0": 15, "x1": 5, "y1": 15, "width": 0.2, "net": "edge", "layer": 1}));
    let before = s.doc.to_json();
    let same_plane = run(&mut s, "ecad.drc", json!({"clearance": 0.2, "min_width": 0.15}));
    assert_eq!(same_plane["clearance"].as_u64(), Some(0), "{same_plane}");
    let edged = run(&mut s, "ecad.drc", json!({"clearance": 0.2, "edge": 0.5}));
    assert!(edged["edge"].as_u64().unwrap_or(0) >= 1, "{edged}");
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.doc.board.as_ref().map(|b| b.layers), Some(2));

    assert!(s.execute("ecad.trace", &json!({"x0": 1, "y0": 1, "x1": 2, "y1": 1, "width": 0.2, "layer": "inner"})).is_err());
    assert!(s.execute("ecad.trace", &json!({"x0": 1, "y0": 1, "x1": 2, "y1": 1, "width": 0.2, "layer": 3})).is_err());
    assert!(s.execute("ecad.board", &json!({"width": 10, "height": 10, "layer": "inner"})).is_err());
    assert!(s.execute("ecad.export", &json!({"path": "board.gbr", "autoroute": true})).is_err());
    assert_eq!(s.doc.to_json(), before);

    let gerber = std::env::temp_dir().join(format!("solidcraft-ecad-ml-{}.gbr", std::process::id()));
    let exported = run(&mut s, "ecad.export", json!({"path": gerber.to_string_lossy()}));
    assert_eq!(s.doc.to_json(), before);
    let files = exported["files"].as_array().expect("files");
    assert_eq!(files.len(), 2, "{exported}");
    for file in files {
        let path = file.as_str().expect("path");
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("PCB autorouting is not supported yet"), "{text}");
        assert!(text.contains("%TF.FileFunction,Copper,"), "{text}");
        let layer = solidcraft_io::read_gerber(&text).unwrap();
        assert!((layer.width - 40.0).abs() < 1e-4);
        assert!(layer.layer_index == 1 || layer.layer_index == 2, "{layer:?}");
    }
    let top_only = std::env::temp_dir().join(format!("solidcraft-ecad-top-{}.gbr", std::process::id()));
    run(&mut s, "ecad.export", json!({"path": top_only.to_string_lossy(), "layer": "top"}));
    let top = solidcraft_io::read_gerber(&std::fs::read_to_string(&top_only).unwrap()).unwrap();
    assert_eq!(top.layer_index, 1);
    assert_eq!(top.traces.len(), 2, "{top:?}");
}

#[test]
fn arcs_regions_inner_layers_and_3d_stack_export() {
    let mut s = Session::default();
    run(&mut s, "ecad.board", json!({"width": 50, "height": 40, "layers": 4}));
    let arc = run(
        &mut s,
        "ecad.trace",
        json!({"x0": 10, "y0": 20, "x1": 30, "y1": 20, "width": 0.25, "net": "sig", "layer": "top", "i": 10, "j": 0, "clockwise": false}),
    );
    assert_eq!(arc["arc"], true, "{arc}");
    assert_eq!(arc["layer"].as_u64(), Some(1));
    let pad = run(&mut s, "ecad.trace", json!({"points": [[2.0, 2.0], [8.0, 2.0], [8.0, 8.0], [2.0, 8.0]], "net": "gnd", "layer": "top"}));
    assert_eq!(pad["regions"].as_u64(), Some(1), "{pad}");
    run(&mut s, "ecad.trace", json!({"x0": 5, "y0": 30, "x1": 25, "y1": 30, "width": 0.2, "net": "inner", "layer": "inner1"}));
    run(&mut s, "ecad.trace", json!({"x0": 5, "y0": 32, "x1": 25, "y1": 32, "width": 0.2, "net": "bot", "layer": "bottom"}));
    assert_eq!(s.doc.board.as_ref().map(|b| b.layers), Some(4));
    assert_eq!(s.doc.board.as_ref().map(|b| b.traces.len()), Some(3));
    assert_eq!(s.doc.board.as_ref().map(|b| b.regions.len()), Some(1));
    assert!(s.doc.board.as_ref().and_then(|b| b.traces.first()).is_some_and(|t| t.is_arc()));

    let before = s.doc.to_json();
    let drc = run(&mut s, "ecad.drc", json!({"clearance": 0.15, "edge": 1.0}));
    assert!(drc["layers"].as_u64().unwrap_or(0) >= 4, "{drc}");
    assert_eq!(s.doc.to_json(), before);

    let gerber = std::env::temp_dir().join(format!("solidcraft-ecad-arc-{}.gbr", std::process::id()));
    let exported = run(&mut s, "ecad.export", json!({"path": gerber.to_string_lossy()}));
    assert_eq!(exported["files"].as_array().map(|f| f.len()), Some(4), "{exported}");
    assert!(exported["regions"].as_u64().unwrap_or(0) >= 1, "{exported}");
    assert_eq!(s.doc.to_json(), before);
    let top_path = exported["files"].as_array().and_then(|f| f.first()).and_then(|v| v.as_str()).expect("top file");
    let top_text = std::fs::read_to_string(top_path).unwrap();
    assert!(top_text.contains("G03") || top_text.contains("G02"), "{top_text}");
    assert!(top_text.contains("G36*"), "{top_text}");
    let top = solidcraft_io::read_gerber(&top_text).unwrap();
    assert_eq!(top.layer_index, 1);
    assert!(top.traces.iter().any(|t| t.is_arc()), "{top:?}");
    assert_eq!(top.regions.len(), 1, "{top:?}");

    let pushed = run(&mut s, "ecad.push_3d", json!({}));
    assert_eq!(pushed["body"], "PCB");
    assert_eq!(pushed["layers"].as_u64(), Some(4), "{pushed}");
    assert!(rel(body_volume(&mut s, "PCB"), 50.0 * 40.0 * 1.6) < 1e-4);
    let again = run(&mut s, "ecad.push_3d", json!({"thickness": 1.6}));
    assert_eq!(again["unchanged"], true, "{again}");
    assert_eq!(again["layers"].as_u64(), Some(4), "{again}");
    assert!(s.execute("ecad.trace", &json!({"x0": 1, "y0": 1, "x1": 2, "y1": 1, "width": 0.2, "layer": "inner3"})).is_err());
    assert!(s.execute("ecad.trace", &json!({"points": [[0.0, 0.0], [1.0, 0.0]], "layer": 1})).is_err());
}
