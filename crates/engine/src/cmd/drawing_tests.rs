//! Associative drawing views, annotations, parts lists, and vector export.

use serde_json::{Value, json};
use solidcraft_doc::{DrawingCurve, DrawingSheet};
use solidcraft_geom::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.25
}

fn span(curves: &[DrawingCurve]) -> (f64, f64) {
    let mut x0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y0 = f64::INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for c in curves {
        for p in [c.a, c.b] {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
    }
    (x1 - x0, y1 - y0)
}

fn box_sheet(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "height", "expression": "10 mm", "unit": "mm"}));
    run(s, "solid.box", json!({"length": 20, "width": 10, "height": "height", "name": "Block", "body_name": "Block"}));
    run(s, "drawing.create", json!({"name": "Sheet"}));
}

/// Feature: fusion360-feature-parity, Property 22: Drawing views and annotations are associative
#[test]
fn property_22_dimensions_follow_the_model_and_lost_refs_are_reported() {
    let mut s = Session::default();
    box_sheet(&mut s);
    run(&mut s, "drawing.base_view", json!({"direction": [0, -1, 0], "hidden": true}));
    run(&mut s, "drawing.dimension", json!({"body": "Block", "axis": "z", "at": [0, -12]}));
    let mut cases = 0usize;
    for i in 1..=100 {
        let height = 10.0 + i as f64;
        run(&mut s, "parameters.change", json!({"name": "height", "expression": format!("{height} mm")}));
        let view = &s.doc.drawings[0].views[0];
        let note = &s.doc.drawings[0].notes[0];
        assert!(near(view.outline, height), "outline {} vs {height}", view.outline);
        assert!(near(note.measured, height), "measured {} vs {height}", note.measured);
        assert_eq!(note.text, format!("{}", height as i64));
        assert!(!note.lost);
        assert!(view.lost.is_empty());
        assert!(view.curves.iter().any(|c| c.hidden), "hidden lines should survive the edit");
        assert!(view.curves.iter().any(|c| !c.hidden));
        cases += 1;
    }
    assert_eq!(cases, 100);
    let feature = s.doc.features.iter().find(|f| f.name == "Block").expect("feature").name.clone();
    run(&mut s, "timeline.suppress", json!({"feature": feature, "suppressed": true}));
    assert!(s.doc.drawings[0].views[0].lost.iter().any(|n| n == "Block"));
    assert!(s.doc.drawings[0].notes[0].lost);
    assert!(s.model.state().body("Block").is_none());
}

#[test]
fn projected_section_and_detail_views_follow_the_parent() {
    let mut s = Session::default();
    box_sheet(&mut s);
    run(&mut s, "drawing.base_view", json!({"direction": [0, 0, -1], "hidden": true, "origin": [0, 0]}));
    run(&mut s, "drawing.project", json!({"from": 0, "side": "right", "origin": [40, 0]}));
    assert!(s.doc.drawings[0].views[1].direction.dist(Vec3::X) < 1e-6, "{:?}", s.doc.drawings[0].views[1].direction);
    run(&mut s, "drawing.standard", json!({"standard": "first_angle"}));
    assert!(s.doc.drawings[0].views[1].direction.dist(-Vec3::X) < 1e-6, "{:?}", s.doc.drawings[0].views[1].direction);
    run(&mut s, "drawing.standard", json!({"standard": "third_angle", "arrows": "open"}));
    assert!(s.doc.drawings[0].views[1].direction.dist(Vec3::X) < 1e-6);
    assert_eq!(s.doc.drawings[0].arrows, "open");

    let top = &s.doc.drawings[0].views[0];
    let (hidden, visible) = (top.curves.iter().filter(|c| c.hidden).count(), top.curves.iter().filter(|c| !c.hidden && !c.section).count());
    assert!(hidden >= 1 && visible >= 4, "hidden {hidden} visible {visible}");
    let (w, h) = span(&top.curves);
    assert!(near(w, 20.0) && near(h, 10.0), "{w} {h}");

    run(&mut s, "drawing.section", json!({"direction": [1, 0, 0], "origin": [0, 40]}));
    let section: Vec<_> = s.doc.drawings[0].views[2].curves.iter().filter(|c| c.section).copied().collect();
    assert!(section.len() >= 4, "{}", section.len());
    let (sw, sh) = span(&section);
    assert!(near(sw, 10.0) && near(sh, 10.0), "section {sw} {sh}");

    run(&mut s, "drawing.detail", json!({"from": 0, "scale": 2, "center": [0, 0], "radius": 12, "origin": [80, 0]}));
    let detail = &s.doc.drawings[0].views[3];
    assert!(detail.direction.dist(Vec3::new(0.0, 0.0, -1.0)) < 1e-6);
    assert!(!detail.curves.is_empty());
    for curve in &detail.curves {
        for p in [curve.a, curve.b] {
            let d = (p[0] - 80.0).hypot(p[1]);
            assert!(d <= 24.2, "detail point {p:?} is {d} from the window");
        }
    }

    run(&mut s, "drawing.break_view", json!({"direction": [0, 0, -1], "stations": [5, 15]}));
    let broken = &s.doc.drawings[0].views[4];
    let (bw, _) = span(&broken.curves);
    assert!(near(broken.outline, 10.0), "outline stays the true width {}", broken.outline);
    assert!(near(bw, 18.0), "drawn width {bw}");
}

#[test]
fn balloons_follow_the_parts_list_and_exports_keep_the_sheet() {
    let mut s = Session::default();
    box_sheet(&mut s);
    run(&mut s, "drawing.base_view", json!({"direction": [0, 0, -1], "hidden": true}));
    run(&mut s, "drawing.center_mark", json!({"body": "Block"}));
    let mark = s.doc.drawings[0].notes[0].at;
    assert!(near(mark[0], 10.0) && near(mark[1], 5.0), "{mark:?}");
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "component.create", json!({"name": "Lid"}));
    let list = run(&mut s, "drawing.parts_list", json!({}));
    assert_eq!(list["rows"], 2);
    assert_eq!(list["parts"][0]["name"], "Base");
    assert_eq!(list["parts"][0]["item"], 1);
    assert_eq!(list["parts"][1]["name"], "Lid");
    assert_eq!(list["parts"][1]["qty"], 1);
    let balloon = run(&mut s, "drawing.balloon", json!({"component": "Lid", "at": [30, 30]}));
    assert_eq!(balloon["item"], 2);
    assert_eq!(balloon["lost"], false);
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.delete", json!({"component": "Lid"}));
    let note = s.doc.drawings[0].notes.iter().find(|n| n.kind == "balloon").expect("balloon");
    assert!(note.lost, "{note:?}");
    assert_eq!(s.doc.drawings[0].parts.len(), 1);

    run(&mut s, "drawing.title_block", json!({"title": "Bracket", "size": "A4"}));
    run(&mut s, "drawing.standard", json!({"standard": "iso", "arrows": "filled"}));
    assert_eq!(s.doc.drawings[0].standard, "first_angle");
    let svg = run(&mut s, "drawing.export_svg", json!({}));
    let svg = svg["svg"].as_str().unwrap();
    assert!(svg.contains("<svg") && svg.contains("Bracket") && svg.contains("SolidCraft") && svg.contains("stroke-dasharray"), "{svg}");
    let dir = std::env::temp_dir();
    let dxf_path = dir.join("solidcraft-drawing-stage2.dxf");
    let pdf_path = dir.join("solidcraft-drawing-stage2.pdf");
    run(&mut s, "drawing.export_dxf", json!({"path": dxf_path.to_string_lossy()}));
    let dxf = std::fs::read_to_string(&dxf_path).unwrap();
    assert!(dxf.contains("ENTITIES") && dxf.contains("SC_DASH") && dxf.contains("$INSUNITS"));
    let back = solidcraft_io::read_dxf(&dxf).unwrap();
    assert!(back.iter().filter(|g| matches!(g, solidcraft_io::Geom2::Line(_, _))).count() > 4, "{back:?}");
    run(&mut s, "drawing.export_pdf", json!({"path": pdf_path.to_string_lossy()}));
    let pdf = std::fs::read_to_string(&pdf_path).unwrap();
    assert!(pdf.contains("%PDF-1.4") && pdf.contains("xref") && pdf.contains("Helvetica") && pdf.contains(" l ") && pdf.contains("Bracket"), "{pdf}");
    let _ = std::fs::remove_file(dxf_path);
    let _ = std::fs::remove_file(pdf_path);
}

#[test]
fn unsupported_drawing_configs_leave_the_design_unchanged() {
    let mut s = Session::default();
    box_sheet(&mut s);
    let before = s.doc.to_json();
    for (id, params) in [
        ("drawing.section", json!({"direction": [1, 0, 0], "mode": "aligned"})),
        ("drawing.base_view", json!({"perspective": true})),
        ("drawing.detail", json!({"scale": 2, "shape": "rect"})),
        ("drawing.break_view", json!({"stations": [1, 2, 3]})),
        ("drawing.break_view", json!({"stations": [1, 2], "style": "jagged"})),
    ] {
        let err = s.execute(id, &params).expect_err(id);
        assert!(err.to_string().contains("not supported yet"), "{id}: {err}");
        assert_eq!(s.doc.to_json(), before, "{id}");
    }
    let err = s.execute("drawing.base_view", &json!({"scale": -2})).expect_err("scale");
    assert!(err.to_string().contains("scale"), "{err}");
    assert_eq!(s.doc.to_json(), before);
    let err = s.execute("drawing.project", &json!({"direction": [f64::NAN, 0.0, 0.0]})).expect_err("nan");
    assert_eq!(s.doc.to_json(), before, "{err}");
}

#[test]
fn an_old_drawing_sheet_still_loads() {
    let raw = r#"{"name":"S","views":[{"kind":"base","direction":[0,0,-1],"origin":[1,2]}],"notes":[{"text":"hi","at":[3,4]}]}"#;
    let sheet: DrawingSheet = serde_json::from_str(raw).unwrap();
    assert_eq!(sheet.views[0].origin, [1.0, 2.0]);
    assert!(sheet.parts.is_empty() && sheet.title.is_empty());
    assert!(!sheet.views[0].cache_valid && sheet.views[0].curves.is_empty());
    assert_eq!(sheet.notes[0].text, "hi");
    let again: DrawingSheet = serde_json::from_str(&serde_json::to_string(&sheet).unwrap()).unwrap();
    assert_eq!(sheet, again);
}
