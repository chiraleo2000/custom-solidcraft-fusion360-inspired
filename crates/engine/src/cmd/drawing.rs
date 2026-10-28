//! Drawing sheets: associative views, annotations, and SVG, DXF, and PDF export.

use serde_json::{Value, json};
use solidcraft_doc::{DrawingCurve, DrawingNote, DrawingPart, DrawingSheet, DrawingView};
use solidcraft_geom::drawing::{self, DetailWindow, ProjEdge, ProjRequest, ProjTri, SectionPlane, ViewFrame};
use solidcraft_geom::{Aabb3, Vec3};
use solidcraft_io::{DrawKind, DrawSeg, DrawText, write_drawing_dxf, write_drawing_pdf_titled, write_drawing_svg};

use super::CommandSpec;
use crate::params::{bad, bool_, str_, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.create", "New Drawing", create).at("DRAWING", "CREATE").icon("sketch").params("name?"),
    CommandSpec::new("drawing.base_view", "Base View", base_view).at("DRAWING", "CREATE").icon("sketch").params(
        "sheet?: name (default: the latest); direction?: [x,y,z] (default +Z); origin?: [x,y] on the sheet; scale?: number or a:b; hidden?: bool",
    ),
    CommandSpec::new("drawing.project", "Projected View", project).at("DRAWING", "CREATE").icon("sketch").params(
        "sheet?; direction?: [x,y,z]; from?: view index; side?: right|left|above|below (derived from the parent; first angle looks the other way); origin?: [x,y]",
    ),
    CommandSpec::new("drawing.section", "Section View", section).at("DRAWING", "CREATE").icon("sketch").params(
        "sheet?; direction?: [x,y,z]; from?: view index; through?: [x,y,z] (default: the model centre); origin?: [x,y]. The near side of the plane is removed",
    ),
    CommandSpec::new("drawing.dimension", "Drawing Dimension", dimension).at("DRAWING", "MODIFY").icon("dimension").params(
        "sheet?; text?: string; at?: [x,y]; body?: name; axis?: x|y|z. A body dimension is remeasured when the model changes",
    ),
    CommandSpec::new("drawing.export_svg", "Export Drawing", export_svg)
        .at("DRAWING", "CREATE")
        .icon("sketch")
        .noundo()
        .params("sheet?"),
    CommandSpec::new("drawing.break_view", "Break View", break_view).at("DRAWING", "CREATE").icon("sketch").params(
        "sheet?; stations: [a, b] along the view's horizontal axis; origin?: [x,y]. The outline stays the true model extent",
    ),
    CommandSpec::new("drawing.auxiliary", "Auxiliary View", auxiliary)
        .at("DRAWING", "CREATE")
        .icon("sketch")
        .params("sheet?; direction: [x,y,z] of the edge; origin?: [x,y]"),
    CommandSpec::new("drawing.center_mark", "Center Mark", center_mark)
        .at("DRAWING", "MODIFY")
        .icon("dimension")
        .params("sheet?; at?: [x,y]; body?: name; view?: index. A body center mark follows that body"),
    CommandSpec::new("drawing.centerline", "Centerline", centerline)
        .at("DRAWING", "MODIFY")
        .icon("dimension")
        .params("sheet?; at?: [x,y]; body?: name; text?"),
    CommandSpec::new("drawing.note", "Note", note_kind).at("DRAWING", "MODIFY").icon("dimension").params("sheet?; text; at?: [x,y]"),
    CommandSpec::new("drawing.leader", "Leader", leader).at("DRAWING", "MODIFY").icon("dimension").params("sheet?; text; at?: [x,y]"),
    CommandSpec::new("drawing.balloon", "Balloon", balloon)
        .at("DRAWING", "MODIFY")
        .icon("dimension")
        .params("sheet?; component: id or name; at?: [x,y]. The balloon's item is its parts-list row"),
    CommandSpec::new("drawing.standard", "Drawing Standard", standard)
        .at("DRAWING", "MODIFY")
        .icon("sketch")
        .params("sheet?; standard: first_angle|third_angle|iso|asme; arrows?: filled|open"),
    CommandSpec::new("drawing.export_pdf", "Export Drawing PDF", export_pdf)
        .at("DRAWING", "CREATE")
        .icon("sketch")
        .noundo()
        .params("sheet?; path: a PDF file (ISO 32000-1). The page is A4 or A3 landscape"),
];

struct BodyGeom {
    name: String,
    bounds: Aabb3,
    edges: Vec<ProjEdge>,
    tris: Vec<ProjTri>,
}

struct SheetDraw {
    segs: Vec<DrawSeg>,
    texts: Vec<DrawText>,
}

pub(super) fn sheet_index(s: &Session, p: &Value) -> Result<usize> {
    let name = str_(p, "sheet");
    let i = s.doc.drawings.iter().position(|d| name.is_none_or(|n| d.name == n)).ok_or_else(|| bad("drawing", "no drawing sheet"))?;
    if name.is_none() {
        return s.doc.drawings.len().checked_sub(1).ok_or_else(|| bad("drawing", "no drawing sheet"));
    }
    Ok(i)
}

fn unsupported(cmd: &str, p: &Value) -> Result<()> {
    if bool_(p, "perspective") == Some(true) {
        return Err(bad(cmd, "not supported yet: perspective drawing views"));
    }
    if let Some(mode) = str_(p, "mode")
        && !matches!(mode, "full" | "orthographic")
    {
        return Err(bad(cmd, format!("not supported yet: {mode} views")));
    }
    if let Some(shape) = str_(p, "shape")
        && shape != "circle"
    {
        return Err(bad(cmd, "not supported yet: detail views that are not circular"));
    }
    Ok(())
}

fn point_key(p: &Value, key: &str) -> Option<[f64; 2]> {
    let a = p.get(key)?.as_array()?;
    let x = a.first()?.as_f64()?;
    let y = a.get(1)?.as_f64()?;
    (x.is_finite() && y.is_finite() && x.abs() < 1.0e9 && y.abs() < 1.0e9).then_some([x, y])
}

fn origin_of(p: &Value) -> [f64; 2] {
    point_key(p, "origin").unwrap_or([0.0, 0.0])
}

fn sheet_point(p: &Value) -> [f64; 2] {
    point_key(p, "at").or_else(|| point_key(p, "origin")).unwrap_or([0.0, 0.0])
}

fn parse_scale_strict(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return Some(1.0);
    }
    if let Some((a, b)) = s.split_once(':') {
        let a = a.trim().parse::<f64>().ok()?;
        let b = b.trim().parse::<f64>().ok()?;
        return (a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0).then_some(a / b);
    }
    let n = s.parse::<f64>().ok()?;
    (n.is_finite() && n > 0.0).then_some(n)
}

fn parse_scale(s: &str) -> f64 {
    parse_scale_strict(s).unwrap_or(1.0)
}

fn scale_of(cmd: &str, p: &Value) -> Result<String> {
    let Some(v) = p.get("scale") else { return Ok(String::new()) };
    let text = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if text.is_empty() || text == "null" {
        return Ok(String::new());
    }
    if parse_scale_strict(&text).is_none() {
        return Err(bad(cmd, "scale must be positive"));
    }
    Ok(text)
}

fn format_mm(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let v = (v * 1000.0).round() / 1000.0;
    let mut s = format!("{v:.3}");
    while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
        s.pop();
    }
    if s.is_empty() || s == "-" { "0".into() } else { s }
}

fn scale_label(scale: &str) -> String {
    let s = scale.trim();
    if s.is_empty() {
        return "1:1".into();
    }
    if s.contains(':') {
        return s.to_string();
    }
    match parse_scale_strict(s) {
        Some(n) if n >= 1.0 => format!("{}:1", format_mm(n)),
        Some(n) if n > 0.0 => format!("1:{}", format_mm(1.0 / n)),
        _ => s.to_string(),
    }
}

fn normalize_side(side: &str) -> String {
    match side {
        "right" | "left" | "above" | "below" => side.to_string(),
        "top" => "above".into(),
        "bottom" => "below".into(),
        _ => String::new(),
    }
}

fn explicit_parent(cmd: &str, s: &Session, sheet: usize, p: &Value) -> Result<Option<usize>> {
    if p.get("from").is_none() {
        return Ok(None);
    }
    let views = s.doc.drawings.get(sheet).map(|d| d.views.len()).unwrap_or(0);
    let idx = match p.get("from") {
        Some(Value::Number(n)) => n.as_u64().and_then(|i| usize::try_from(i).ok()),
        Some(Value::String(name)) => {
            if let Ok(i) = name.parse::<usize>() {
                Some(i)
            } else {
                s.doc.drawings.get(sheet).and_then(|d| d.views.iter().position(|v| v.kind == *name))
            }
        }
        _ => None,
    };
    idx.filter(|i| *i < views).map(Some).ok_or_else(|| bad(cmd, "no parent view"))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_(p, "name").unwrap_or("Sheet").to_string();
    if name.len() > 200 {
        return Err(bad("drawing.create", "name is too long"));
    }
    if s.doc.drawings.len() >= 100 {
        return Err(bad("drawing.create", "too many drawing sheets"));
    }
    s.doc_mut().drawings.push(DrawingSheet::named(name.clone()));
    s.refresh();
    Ok(json!({"sheet": name}))
}

fn push_view(s: &mut Session, p: &Value, cmd: &str, kind: &str) -> Result<Value> {
    unsupported(cmd, p)?;
    let scale = scale_of(cmd, p)?;
    let given = p.get("direction").and_then(vec3);
    let mut direction = given.unwrap_or(Vec3::Z);
    direction = direction.normalized().ok_or_else(|| bad(cmd, "`direction` must be non-zero"))?;
    let i = sheet_index(s, p)?;
    let view_count = s.doc.drawings.get(i).map(|d| d.views.len()).unwrap_or(0);
    if view_count >= 64 {
        return Err(bad(cmd, "too many views on the sheet"));
    }
    let standard = s.doc.drawings.get(i).map(|d| d.standard.clone()).unwrap_or_default();
    let from = explicit_parent(cmd, s, i, p)?;
    let side = normalize_side(str_(p, "side").unwrap_or(""));
    let mut parent = None;
    let mut side_stored = String::new();
    if !side.is_empty() {
        let idx = from.or_else(|| view_count.checked_sub(1)).ok_or_else(|| bad(cmd, "a projected view needs a parent view"))?;
        let parent_dir = s.doc.drawings.get(i).and_then(|d| d.views.get(idx)).map(|v| v.direction).ok_or_else(|| bad(cmd, "no parent view"))?;
        direction = derived_direction(parent_dir, &side, &standard).ok_or_else(|| bad(cmd, "side must be right, left, above, or below"))?;
        parent = Some(idx);
        side_stored = side;
    } else if from.is_some() && given.is_none() {
        let idx = from.ok_or_else(|| bad(cmd, "no parent view"))?;
        direction = s.doc.drawings.get(i).and_then(|d| d.views.get(idx)).map(|v| v.direction).ok_or_else(|| bad(cmd, "no parent view"))?;
        parent = Some(idx);
        side_stored = "same".into();
    } else if let Some(idx) = from {
        parent = Some(idx);
    }
    let names = collect(&s.model.state()).into_iter().map(|g| g.name).collect();
    let mut view = DrawingView::plain(kind, direction, origin_of(p));
    view.scale = scale;
    view.hidden = bool_(p, "hidden").unwrap_or(false);
    view.bodies = names;
    view.parent = parent;
    view.side = side_stored;
    view.through = p.get("through").and_then(vec3).map(|v| [v.x, v.y, v.z]);
    if let Some(c) = p.get("center").and_then(crate::params::vec2) {
        view.center = Some([c.x, c.y]);
    } else if kind == "detail" {
        view.track_center = true;
    }
    if let Some(r) = p.get("radius").and_then(Value::as_f64) {
        if !(r.is_finite() && (0.0..1.0e9).contains(&r)) {
            return Err(bad(cmd, "radius must be a positive length"));
        }
        view.radius = r;
    }
    if kind == "break"
        && let Some(arr) = p.get("stations").and_then(Value::as_array)
    {
        view.stations = arr.iter().filter_map(Value::as_f64).filter(|n| n.is_finite()).take(2).collect();
    }
    let (name, n) = {
        let Some(sheet) = s.doc_mut().drawings.get_mut(i) else { return Err(bad(cmd, "no drawing sheet")) };
        sheet.views.push(view);
        sheet.svg.clear();
        (sheet.name.clone(), sheet.views.len())
    };
    s.refresh();
    Ok(json!({"sheet": name, "views": n}))
}

fn base_view(s: &mut Session, p: &Value) -> Result<Value> {
    push_view(s, p, "drawing.base_view", "base")
}

fn project(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.project";
    unsupported(cmd, p)?;
    let has_dir = p.get("direction").and_then(vec3).is_some();
    let side = str_(p, "side").unwrap_or("");
    if !side.is_empty() && normalize_side(side).is_empty() {
        return Err(bad(cmd, "side must be right, left, above, or below"));
    }
    if !has_dir && side.is_empty() {
        return Err(bad(cmd, "`direction` is required"));
    }
    push_view(s, p, cmd, "projected")
}

fn section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.section";
    unsupported(cmd, p)?;
    if p.get("direction").and_then(vec3).is_none() && p.get("from").is_none() {
        return Err(bad(cmd, "`direction` is required"));
    }
    push_view(s, p, cmd, "section")
}

pub(super) fn detail(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.detail";
    unsupported(cmd, p)?;
    let scale = p.get("scale").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`scale` is required"))?;
    if !(scale.is_finite() && scale > 0.0) {
        return Err(bad(cmd, "scale must be positive"));
    }
    push_view(s, p, cmd, "detail")
}

fn break_view(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.break_view";
    unsupported(cmd, p)?;
    if let Some(style) = str_(p, "style")
        && style != "straight"
    {
        return Err(bad(cmd, "not supported yet: break styles other than straight"));
    }
    let stations = p.get("stations").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`stations` must be two positions"))?;
    if stations.len() > 2 {
        return Err(bad(cmd, "not supported yet: more than one break gap"));
    }
    if stations.len() != 2 || stations.iter().any(|v| !v.as_f64().is_some_and(|n| n.is_finite() && n.abs() < 1.0e9)) {
        return Err(bad(cmd, "`stations` must be two finite positions"));
    }
    push_view(s, p, cmd, "break")
}

fn auxiliary(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.auxiliary";
    unsupported(cmd, p)?;
    if p.get("direction").and_then(vec3).is_none() {
        return Err(bad(cmd, "`direction` is required"));
    }
    push_view(s, p, cmd, "auxiliary")
}

fn note_view(s: &Session, sheet: usize, p: &Value) -> Option<usize> {
    let n = s.doc.drawings.get(sheet).map(|d| d.views.len()).unwrap_or(0);
    if let Some(v) = p.get("view").and_then(Value::as_u64) {
        let i = usize::try_from(v).unwrap_or(usize::MAX);
        return (i < n).then_some(i);
    }
    n.checked_sub(1)
}

fn body_exists(s: &Session, name: &str) -> bool {
    s.model.state().bodies.iter().any(|b| b.name == name)
}

fn dimension(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.dimension";
    unsupported(cmd, p)?;
    let text = str_(p, "text").unwrap_or("").to_string();
    let target = str_(p, "body").unwrap_or("").to_string();
    let axis = str_(p, "axis").unwrap_or("").to_string();
    if text.is_empty() && target.is_empty() {
        return Err(bad(cmd, "`text` is required"));
    }
    if text.len() > 400 || target.len() > 200 {
        return Err(bad(cmd, "dimension text is too long"));
    }
    if !axis.is_empty() && !matches!(axis.as_str(), "x" | "y" | "z") {
        return Err(bad(cmd, "axis must be x, y, or z"));
    }
    if !target.is_empty() && axis.is_empty() {
        return Err(bad(cmd, "`axis` is required"));
    }
    let i = sheet_index(s, p)?;
    if !target.is_empty() && !body_exists(s, &target) {
        return Err(bad(cmd, format!("no body `{target}`")));
    }
    let geoms = collect(&s.model.state());
    let measured = geoms.iter().find(|g| g.name == target).map(|g| axis_extent(g.bounds, &axis)).unwrap_or(0.0);
    let shown = if target.is_empty() { text } else { format_mm(measured) };
    let view = note_view(s, i, p);
    let at = sheet_point(p);
    let notes = {
        let Some(sheet) = s.doc_mut().drawings.get_mut(i) else { return Err(bad(cmd, "no drawing sheet")) };
        if sheet.notes.len() >= 400 {
            return Err(bad(cmd, "too many notes"));
        }
        let mut note = DrawingNote::plain(shown, at);
        note.kind = "dimension".into();
        note.target = target;
        note.axis = axis;
        note.view = view;
        note.measured = measured;
        sheet.notes.push(note);
        sheet.svg.clear();
        sheet.notes.len()
    };
    s.refresh();
    Ok(json!({"notes": notes, "measured": measured}))
}

fn annotate(s: &mut Session, p: &Value, kind: &str, text_required: bool) -> Result<Value> {
    let cmd = "drawing";
    unsupported(cmd, p)?;
    let text = str_(p, "text").unwrap_or("").chars().take(400).collect::<String>();
    if text_required && text.is_empty() {
        return Err(bad(cmd, "`text` is required"));
    }
    let target = str_(p, "body").unwrap_or("").to_string();
    if target.len() > 200 {
        return Err(bad(cmd, "body name is too long"));
    }
    let i = sheet_index(s, p)?;
    if !target.is_empty() && !body_exists(s, &target) {
        return Err(bad(cmd, format!("no body `{target}`")));
    }
    let view = note_view(s, i, p);
    let notes = {
        let Some(sheet) = s.doc_mut().drawings.get_mut(i) else { return Err(bad(cmd, "no drawing sheet")) };
        if sheet.notes.len() >= 400 {
            return Err(bad(cmd, "too many notes"));
        }
        let mut note = DrawingNote::plain(text, sheet_point(p));
        note.kind = kind.to_string();
        note.target = target;
        note.view = view;
        sheet.notes.push(note);
        sheet.svg.clear();
        sheet.notes.len()
    };
    s.refresh();
    Ok(json!({"kind": kind, "notes": notes}))
}

fn note_kind(s: &mut Session, p: &Value) -> Result<Value> {
    annotate(s, p, "note", true)
}

fn center_mark(s: &mut Session, p: &Value) -> Result<Value> {
    annotate(s, p, "center_mark", false)
}

fn centerline(s: &mut Session, p: &Value) -> Result<Value> {
    annotate(s, p, "centerline", false)
}

fn leader(s: &mut Session, p: &Value) -> Result<Value> {
    annotate(s, p, "leader", true)
}

fn balloon(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.balloon";
    unsupported(cmd, p)?;
    let token = str_(p, "component").filter(|c| !c.is_empty()).ok_or_else(|| bad(cmd, "`component` is required"))?.to_string();
    if token.len() > 200 {
        return Err(bad(cmd, "component name is too long"));
    }
    let i = sheet_index(s, p)?;
    let parts = build_parts(&s.doc);
    let found = parts.iter().find(|part| part.name == token || part.component.to_string() == token).cloned();
    let view = note_view(s, i, p);
    let (item, lost, notes) = {
        let Some(sheet) = s.doc_mut().drawings.get_mut(i) else { return Err(bad(cmd, "no drawing sheet")) };
        if sheet.notes.len() >= 400 {
            return Err(bad(cmd, "too many notes"));
        }
        sheet.parts = parts;
        let mut note = DrawingNote::plain(token.clone(), sheet_point(p));
        note.kind = "balloon".into();
        note.view = view;
        if let Some(part) = found {
            note.text = part.item.to_string();
            note.item = part.item;
            note.component = Some(part.component);
            note.target = part.name;
            note.lost = false;
        } else {
            note.target = token;
            note.lost = true;
        }
        let item = note.item;
        let lost = note.lost;
        sheet.notes.push(note);
        sheet.svg.clear();
        (item, lost, sheet.notes.len())
    };
    s.refresh();
    Ok(json!({"kind": "balloon", "notes": notes, "item": item, "lost": lost}))
}

fn standard(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.standard";
    unsupported(cmd, p)?;
    let raw = str_(p, "standard").unwrap_or("third_angle");
    let name = match raw {
        "third_angle" | "asme" => "third_angle",
        "first_angle" | "iso" => "first_angle",
        other => return Err(bad(cmd, format!("standard must be first_angle or third_angle (got {other})"))),
    };
    if let Some(arrows) = str_(p, "arrows")
        && !matches!(arrows, "filled" | "open")
    {
        return Err(bad(cmd, "arrows must be filled or open"));
    }
    let i = sheet_index(s, p)?;
    let arrows_out = {
        let Some(sheet) = s.doc_mut().drawings.get_mut(i) else { return Err(bad(cmd, "no drawing sheet")) };
        sheet.standard = name.to_string();
        if let Some(arrows) = str_(p, "arrows") {
            sheet.arrows = arrows.to_string();
        }
        sheet.svg.clear();
        sheet.arrows.clone()
    };
    s.refresh();
    Ok(json!({"standard": name, "arrows": arrows_out}))
}

fn field_of(cmd: &str, p: &Value, key: &str) -> Result<Option<String>> {
    let Some(raw) = str_(p, key) else { return Ok(None) };
    if raw.len() > 200 {
        return Err(bad(cmd, format!("{key} is too long")));
    }
    Ok(Some(raw.to_string()))
}

pub(super) fn title_block(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.title_block";
    unsupported(cmd, p)?;
    let size = str_(p, "size").unwrap_or("A4");
    if !matches!(size, "A4" | "A3") {
        return Err(bad(cmd, "size must be A4 or A3"));
    }
    let template = match str_(p, "template").unwrap_or("iso") {
        "iso" | "ISO" => "iso",
        "asme" | "ASME" => "asme",
        other => return Err(bad(cmd, format!("template must be iso or asme (got {other})"))),
    };
    let title = str_(p, "title").unwrap_or("SolidCraft");
    if title.len() > 200 {
        return Err(bad(cmd, "title is too long"));
    }
    let revision = field_of(cmd, p, "revision")?;
    let drawn = field_of(cmd, p, "drawn")?;
    let date = field_of(cmd, p, "date")?;
    let material = field_of(cmd, p, "material")?;
    let i = sheet_index(s, p)?;
    let Some(sheet) = s.doc_mut().drawings.get_mut(i) else { return Err(bad(cmd, "no drawing sheet")) };
    sheet.title = title.to_string();
    sheet.paper = size.to_string();
    sheet.template = template.to_string();
    if let Some(v) = revision {
        sheet.revision = v;
    }
    if let Some(v) = drawn {
        sheet.drawn = v;
    }
    if let Some(v) = date {
        sheet.date = v;
    }
    if let Some(v) = material {
        sheet.material = v;
    }
    sheet.svg.clear();
    if let Some(existing) = sheet.notes.iter_mut().find(|n| n.kind == "title") {
        existing.text = title.to_string();
    } else if sheet.notes.len() < 400 {
        let mut note = DrawingNote::plain(title, [0.0, 0.0]);
        note.kind = "title".into();
        sheet.notes.push(note);
    }
    Ok(json!({
        "title": title,
        "size": size,
        "template": template,
        "revision": sheet.revision,
        "drawn": sheet.drawn,
        "date": sheet.date,
        "material": sheet.material,
    }))
}

pub(super) fn parts_list(s: &mut Session, p: &Value) -> Result<Value> {
    let parts = build_parts(&s.doc);
    if !s.doc.drawings.is_empty() {
        let i = sheet_index(s, p)?;
        if let Some(sheet) = s.doc_mut().drawings.get_mut(i) {
            sheet.parts = parts.clone();
            sheet.svg.clear();
        }
    }
    let rows = parts.iter().map(part_json).collect::<Vec<_>>();
    Ok(json!({"rows": parts.len(), "parts": rows}))
}

fn part_json(part: &DrawingPart) -> Value {
    json!({"item": part.item, "component": part.component, "name": part.name, "qty": part.qty, "parent": part.parent})
}

fn build_parts(doc: &solidcraft_doc::Document) -> Vec<DrawingPart> {
    let mut ordered = Vec::new();
    fn walk(doc: &solidcraft_doc::Document, parent: u64, out: &mut Vec<u64>, depth: usize) {
        if depth > 64 || out.len() > 10_000 {
            return;
        }
        let mut kids: Vec<u64> = doc.components.iter().filter(|c| c.parent == parent).map(|c| c.id).collect();
        kids.sort_unstable();
        for id in kids {
            if out.contains(&id) {
                continue;
            }
            out.push(id);
            walk(doc, id, out, depth + 1);
        }
    }
    walk(doc, 0, &mut ordered, 0);
    for c in &doc.components {
        if !ordered.contains(&c.id) && ordered.len() < 10_000 {
            ordered.push(c.id);
        }
    }
    let mut parts = Vec::new();
    for (i, id) in ordered.iter().enumerate() {
        let Some(c) = doc.components.iter().find(|c| c.id == *id) else { continue };
        let qty = doc.occurrences.iter().filter(|o| o.component == c.id).count().max(1) as u32;
        parts.push(DrawingPart { item: (i + 1) as u32, component: c.id, name: c.name.clone(), qty, parent: c.parent });
    }
    parts
}

fn export_meta(sheet: &DrawingSheet, segs: usize, lost: Vec<String>) -> Value {
    json!({
        "sheet": sheet.name,
        "lines": segs,
        "lost": lost,
        "paper": if sheet.paper.is_empty() { "A4" } else { sheet.paper.as_str() },
        "standard": if sheet.standard.is_empty() { "third_angle" } else { sheet.standard.as_str() },
        "template": if sheet.template.is_empty() { "iso" } else { sheet.template.as_str() },
        "parts": sheet.parts.len(),
        "title": sheet.title,
    })
}

fn export_svg(s: &mut Session, p: &Value) -> Result<Value> {
    let i = sheet_index(s, p)?;
    let draw = compose_sheet(s, i)?;
    let svg = write_drawing_svg(&draw.segs, &draw.texts);
    let meta = {
        let Some(slot) = s.doc_mut().drawings.get_mut(i) else { return Err(bad("drawing.export_svg", "no drawing sheet")) };
        slot.svg = svg.clone();
        let lost = lost_report(slot);
        let mut meta = export_meta(slot, draw.segs.len(), lost);
        meta["svg"] = json!(svg);
        meta
    };
    s.refresh();
    Ok(meta)
}

fn export_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.export_pdf";
    let path = str_(p, "path").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?;
    if path.len() > 4_000 {
        return Err(bad(cmd, "path is too long"));
    }
    let i = sheet_index(s, p).map_err(|_| bad(cmd, "no drawing sheet"))?;
    let draw = compose_sheet(s, i)?;
    let sheet = s.doc.drawings.get(i).cloned().ok_or_else(|| bad(cmd, "no drawing sheet"))?;
    let page = paper_mm(&sheet.paper);
    let title = if sheet.title.is_empty() { "SolidCraft" } else { sheet.title.as_str() };
    let pdf = write_drawing_pdf_titled(page, title, &draw.segs, &draw.texts);
    std::fs::write(path, pdf).map_err(|e| bad(cmd, e.to_string()))?;
    let outline = sheet.views.iter().map(|v| v.outline).fold(0.0, f64::max);
    let mut meta = export_meta(&sheet, draw.segs.len(), lost_report(&sheet));
    meta["path"] = json!(path);
    meta["outline"] = json!(outline);
    meta["page_mm"] = json!(page);
    Ok(meta)
}

pub(super) fn export_dxf(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.export_dxf";
    let path = str_(p, "path").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?;
    if path.len() > 4_000 {
        return Err(bad(cmd, "path is too long"));
    }
    let i = sheet_index(s, p).map_err(|_| bad(cmd, "no drawing sheet"))?;
    let draw = compose_sheet(s, i)?;
    let lines = draw.segs.iter().filter(|seg| matches!(seg.kind, DrawKind::Visible | DrawKind::Hidden | DrawKind::Section)).count();
    if lines == 0 {
        return Err(bad(cmd, "the drawing has no model edges"));
    }
    let dxf = write_drawing_dxf(&draw.segs, &draw.texts);
    std::fs::write(path, dxf).map_err(|e| bad(cmd, e.to_string()))?;
    let sheet = s.doc.drawings.get(i).cloned().ok_or_else(|| bad(cmd, "no drawing sheet"))?;
    let mut meta = export_meta(&sheet, lines, lost_report(&sheet));
    meta["path"] = json!(path);
    Ok(meta)
}

fn compose_sheet(s: &mut Session, i: usize) -> Result<SheetDraw> {
    sync(s);
    let state = s.model.state();
    let geoms = collect(&state);
    let sheet = s.doc.drawings.get(i).cloned().ok_or_else(|| bad("drawing", "no drawing sheet"))?;
    Ok(compose(&sheet, &geoms))
}

fn paper_mm(paper: &str) -> [f64; 2] {
    match paper {
        "A3" => [420.0, 297.0],
        _ => [297.0, 210.0],
    }
}

fn lost_report(sheet: &DrawingSheet) -> Vec<String> {
    let mut out = Vec::new();
    for view in &sheet.views {
        for name in &view.lost {
            if !out.iter().any(|n| n == name) {
                out.push(name.clone());
            }
        }
    }
    for note in &sheet.notes {
        if !note.lost {
            continue;
        }
        let label = if note.target.is_empty() { note.kind.clone() } else { note.target.clone() };
        if !label.is_empty() && !out.iter().any(|n| n == &label) {
            out.push(label);
        }
    }
    out
}

/// Recompute each view's projection, outline, and the references it has lost.
pub(super) fn sync(s: &mut Session) {
    if s.doc.drawings.is_empty() {
        return;
    }
    let state = s.model.state();
    let geoms = collect(&state);
    let names: Vec<String> = geoms.iter().map(|g| g.name.clone()).collect();
    let parts = build_parts(&s.doc);
    let mut sheets = std::mem::take(&mut s.doc_mut().drawings);
    for sheet in &mut sheets {
        let standard = sheet.standard.clone();
        let passes = sheet.views.len().min(8);
        for _ in 0..passes {
            let dirs: Vec<Vec3> = sheet.views.iter().map(|v| v.direction).collect();
            let mut changed = false;
            for (vi, view) in sheet.views.iter_mut().enumerate() {
                let Some(pi) = view.parent else { continue };
                if pi == vi || view.side.is_empty() {
                    continue;
                }
                let Some(parent_dir) = dirs.get(pi).copied() else { continue };
                let Some(next) = derived_direction(parent_dir, &view.side, &standard) else { continue };
                if next != view.direction {
                    view.direction = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for view in &mut sheet.views {
            let have = selected(&geoms, &view.bodies);
            let bounds = union_bounds(&have);
            view.lost = view.bodies.iter().filter(|name| !names.iter().any(|have| have == *name)).cloned().collect();
            view.outline = outline_of(view.direction, bounds);
            if view.track_center
                && !bounds.is_empty()
                && let Some(frame) = ViewFrame::looking(view.direction)
            {
                view.center = Some(frame.project(bounds.center()));
            }
            let key = view_key(view, &have, &standard);
            if !view.cache_valid || view.cache_key != key {
                view.curves = project_bodies(view, &have, bounds);
                view.cache_key = key;
                view.cache_valid = true;
            }
        }
        let nviews = sheet.views.len();
        for (vi, view) in sheet.views.iter_mut().enumerate() {
            let Some(pi) = view.parent else { continue };
            if pi >= nviews || pi == vi {
                if !view.lost.iter().any(|n| n == "parent-view") {
                    view.lost.push("parent-view".into());
                }
            } else {
                view.lost.retain(|n| n != "parent-view");
            }
        }
        if !sheet.parts.is_empty() || sheet.notes.iter().any(|n| n.kind == "balloon") {
            sheet.parts = parts.clone();
        }
        let count = sheet.notes.len();
        for ni in 0..count {
            let kind = sheet.notes.get(ni).map(|n| n.kind.clone()).unwrap_or_default();
            match kind.as_str() {
                "dimension" => update_dimension(sheet, ni, &geoms),
                "center_mark" | "centerline" => update_center(sheet, ni, &geoms),
                "balloon" => update_balloon(sheet, ni, &parts),
                _ => {}
            }
        }
    }
    s.doc_mut().drawings = sheets;
}

fn derived_direction(parent: Vec3, side: &str, standard: &str) -> Option<Vec3> {
    if side == "same" {
        return parent.normalized();
    }
    ViewFrame::looking(parent)?.side(side, standard == "first_angle")
}

fn update_dimension(sheet: &mut DrawingSheet, ni: usize, geoms: &[BodyGeom]) {
    let Some(note) = sheet.notes.get(ni) else { return };
    if note.target.is_empty() || note.axis.is_empty() {
        return;
    }
    let target = note.target.clone();
    let axis = note.axis.clone();
    let view_i = note.view;
    let Some(body) = geoms.iter().find(|g| g.name == target) else {
        if let Some(note) = sheet.notes.get_mut(ni) {
            note.lost = true;
        }
        return;
    };
    let measured = axis_extent(body.bounds, &axis);
    let placed = view_i.and_then(|i| sheet.views.get(i)).or_else(|| sheet.views.last());
    let at = placed.and_then(|view| dim_anchor(view, body.bounds, &axis));
    if let Some(note) = sheet.notes.get_mut(ni) {
        note.measured = measured;
        note.text = format_mm(measured);
        note.lost = false;
        if let Some(at) = at {
            note.at = at;
        }
    }
}

/// Sheet position for a body linear dimension, just outside the projected extent.
fn dim_anchor(view: &DrawingView, bounds: Aabb3, axis: &str) -> Option<[f64; 2]> {
    if bounds.is_empty() {
        return None;
    }
    let frame = ViewFrame::looking(view.direction)?;
    let scale = parse_scale(&view.scale);
    let mut minu = f64::INFINITY;
    let mut maxu = f64::NEG_INFINITY;
    let mut minv = f64::INFINITY;
    let mut maxv = f64::NEG_INFINITY;
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            for z in [bounds.min.z, bounds.max.z] {
                let p = frame.project(Vec3::new(x, y, z));
                minu = minu.min(p[0]);
                maxu = maxu.max(p[0]);
                minv = minv.min(p[1]);
                maxv = maxv.max(p[1]);
            }
        }
    }
    if !minu.is_finite() {
        return None;
    }
    let (u, v) = match axis {
        "y" => ((minu + maxu) * 0.5, maxv + 4.0 / scale.max(1e-9)),
        "z" => ((minu + maxu) * 0.5, minv - 4.0 / scale.max(1e-9)),
        _ => (maxu + 4.0 / scale.max(1e-9), (minv + maxv) * 0.5),
    };
    Some([view.origin[0] + u * scale, view.origin[1] + v * scale])
}

fn update_center(sheet: &mut DrawingSheet, ni: usize, geoms: &[BodyGeom]) {
    let Some(note) = sheet.notes.get(ni) else { return };
    if note.target.is_empty() {
        return;
    }
    let target = note.target.clone();
    let view_i = note.view;
    let Some(body) = geoms.iter().find(|g| g.name == target).filter(|g| !g.bounds.is_empty()) else {
        if let Some(note) = sheet.notes.get_mut(ni) {
            note.lost = true;
        }
        return;
    };
    let bounds = body.bounds;
    let placed = view_i.and_then(|i| sheet.views.get(i)).or_else(|| sheet.views.last());
    let (origin, scale, frame) = match placed {
        Some(view) => (view.origin, parse_scale(&view.scale), ViewFrame::looking(view.direction)),
        None => ([0.0, 0.0], 1.0, ViewFrame::looking(Vec3::new(0.0, 0.0, -1.0))),
    };
    let Some(frame) = frame else {
        if let Some(note) = sheet.notes.get_mut(ni) {
            note.lost = true;
        }
        return;
    };
    let p = frame.project(bounds.center());
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            for z in [bounds.min.z, bounds.max.z] {
                let u = frame.project(Vec3::new(x, y, z))[0];
                lo = lo.min(u);
                hi = hi.max(u);
            }
        }
    }
    let span = if lo.is_finite() { (hi - lo) * scale } else { 0.0 };
    if let Some(note) = sheet.notes.get_mut(ni) {
        note.at = [origin[0] + p[0] * scale, origin[1] + p[1] * scale];
        note.measured = span.max(0.0);
        note.lost = false;
    }
}

fn update_balloon(sheet: &mut DrawingSheet, ni: usize, parts: &[DrawingPart]) {
    let Some(note) = sheet.notes.get(ni) else { return };
    let token = if note.target.is_empty() { note.text.clone() } else { note.target.clone() };
    let id = note.component;
    let found =
        parts.iter().find(|part| id.is_some_and(|id| part.component == id) || part.name == token || part.component.to_string() == token).cloned();
    if let Some(part) = found {
        if let Some(note) = sheet.notes.get_mut(ni) {
            note.item = part.item;
            note.component = Some(part.component);
            note.target = part.name;
            note.text = part.item.to_string();
            note.lost = false;
        }
    } else if let Some(note) = sheet.notes.get_mut(ni) {
        note.lost = true;
    }
}

fn axis_extent(bounds: Aabb3, axis: &str) -> f64 {
    if bounds.is_empty() {
        return 0.0;
    }
    let size = bounds.size();
    match axis {
        "y" => size.y,
        "z" => size.z,
        _ => size.x,
    }
}

fn legacy_up(direction: Vec3) -> Vec3 {
    let axis = if direction.cross(Vec3::X).len() > 1e-6 { Vec3::X } else { Vec3::Y };
    direction.cross(axis).normalized().unwrap_or(Vec3::Z)
}

fn outline_of(direction: Vec3, bounds: Aabb3) -> f64 {
    if bounds.is_empty() {
        return 0.0;
    }
    let up = legacy_up(direction);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            for z in [bounds.min.z, bounds.max.z] {
                let h = Vec3::new(x, y, z).dot(up);
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
    }
    if lo.is_finite() && hi.is_finite() { (hi - lo).max(0.0) } else { 0.0 }
}

fn collect(state: &solidcraft_doc::ModelState) -> Vec<BodyGeom> {
    let mut out = Vec::new();
    for body in &state.bodies {
        if out.len() >= 64 {
            break;
        }
        let mesh = body.mesh();
        let bounds = mesh.bounds();
        if body.body.is_mesh() {
            out.push(BodyGeom { name: body.name.clone(), bounds, edges: Vec::new(), tris: Vec::new() });
            continue;
        }
        let mut edges = Vec::new();
        for (i, pts) in mesh.edges.iter().enumerate() {
            if edges.len() >= 2_000 {
                break;
            }
            if !mesh.edge_pickable(i) || pts.len() < 2 {
                continue;
            }
            let faces = mesh.edge_faces.get(i).cloned().unwrap_or_default();
            let points = thin(pts);
            if points.len() >= 2 {
                edges.push(ProjEdge { points, faces });
            }
        }
        let mut tris = Vec::new();
        for (tri, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
            if tris.len() >= 8_000 {
                break;
            }
            let Some(p) = mesh.tri(tri) else { continue };
            if p.iter().all(|v| v.is_finite()) {
                tris.push(ProjTri { p, face: *face });
            }
        }
        out.push(BodyGeom { name: body.name.clone(), bounds, edges, tris });
    }
    out
}

fn thin(points: &[Vec3]) -> Vec<Vec3> {
    let finite: Vec<Vec3> = points.iter().copied().filter(|p| p.is_finite()).collect();
    if finite.len() <= 32 {
        return finite;
    }
    let mut out = Vec::new();
    let last = finite.len() - 1;
    for i in 0..32 {
        let idx = i * last / 31;
        if let Some(p) = finite.get(idx).copied()
            && out.last().is_none_or(|q: &Vec3| q.dist(p) > 1e-6)
        {
            out.push(p);
        }
    }
    out
}

fn selected<'a>(geoms: &'a [BodyGeom], names: &[String]) -> Vec<&'a BodyGeom> {
    if names.is_empty() { geoms.iter().collect() } else { geoms.iter().filter(|g| names.iter().any(|n| n == &g.name)).collect() }
}

fn union_bounds(bodies: &[&BodyGeom]) -> Aabb3 {
    let mut bounds = Aabb3::EMPTY;
    for body in bodies {
        if !body.bounds.is_empty() {
            bounds = bounds.union(&body.bounds);
        }
    }
    bounds
}

fn project_bodies(view: &DrawingView, bodies: &[&BodyGeom], bounds: Aabb3) -> Vec<DrawingCurve> {
    let Some(frame) = ViewFrame::looking(view.direction) else { return Vec::new() };
    let mut edges = Vec::new();
    let mut tris = Vec::new();
    let mut face_off = 0u32;
    for body in bodies {
        for edge in &body.edges {
            if edges.len() >= 2_000 {
                break;
            }
            let faces = edge.faces.iter().map(|f| f.saturating_add(face_off)).collect();
            edges.push(ProjEdge { points: edge.points.clone(), faces });
        }
        for tri in &body.tris {
            if tris.len() >= 8_000 {
                break;
            }
            tris.push(ProjTri { p: tri.p, face: tri.face.saturating_add(face_off) });
        }
        face_off = face_off.saturating_add(10_000);
    }
    let section = if view.kind == "section" && !bounds.is_empty() {
        let origin = view.through.map(Vec3::from).filter(|p| p.is_finite()).unwrap_or_else(|| bounds.center());
        view.direction.normalized().map(|normal| SectionPlane { origin, normal })
    } else {
        None
    };
    let detail = if view.kind == "detail" && view.radius > 0.0 {
        view.center.filter(|c| c.iter().all(|v| v.is_finite())).map(|center| DetailWindow { center, radius: view.radius })
    } else {
        None
    };
    let curves = drawing::project_view(&ProjRequest {
        frame,
        edges: &edges,
        tris: &tris,
        section,
        detail,
        stations: &view.stations,
        scale: parse_scale(&view.scale),
        origin: view.origin,
    });
    curves.into_iter().map(|c| DrawingCurve { a: c.a, b: c.b, hidden: c.hidden, section: c.section }).collect()
}

fn view_key(view: &DrawingView, bodies: &[&BodyGeom], standard: &str) -> u64 {
    let mut h = geom_key(bodies);
    h = mix(h, hash_str(&view.kind));
    h = mix(h, hash_str(&view.side));
    h = mix(h, hash_str(&view.scale));
    h = mix(h, hash_str(standard));
    for v in [view.direction.x, view.direction.y, view.direction.z, view.origin[0], view.origin[1], view.radius] {
        h = mix(h, v.to_bits());
    }
    h = mix(h, view.parent.unwrap_or(usize::MAX) as u64);
    if let Some(c) = view.center {
        h = mix(h, c[0].to_bits());
        h = mix(h, c[1].to_bits());
    }
    if let Some(t) = view.through {
        h = mix(h, t[0].to_bits());
        h = mix(h, t[1].to_bits());
        h = mix(h, t[2].to_bits());
    }
    for s in &view.stations {
        h = mix(h, s.to_bits());
    }
    for name in &view.bodies {
        h = mix(h, hash_str(name));
    }
    h
}

fn geom_key(bodies: &[&BodyGeom]) -> u64 {
    let mut h = 0u64;
    for body in bodies {
        h = mix(h, hash_str(&body.name));
        h = mix(h, body.edges.len() as u64);
        h = mix(h, body.tris.len() as u64);
        for v in [body.bounds.min.x, body.bounds.min.y, body.bounds.min.z, body.bounds.max.x, body.bounds.max.y, body.bounds.max.z] {
            h = mix(h, v.to_bits());
        }
        let mut n = 0usize;
        for edge in &body.edges {
            for p in &edge.points {
                h = mix(h, p.x.to_bits() ^ p.y.to_bits().rotate_left(11) ^ p.z.to_bits().rotate_left(23));
                n += 1;
                if n > 128 {
                    break;
                }
            }
            if n > 128 {
                break;
            }
        }
    }
    h
}

fn mix(h: u64, v: u64) -> u64 {
    h ^ v.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left((h & 31) as u32)
}

fn hash_str(s: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn default_top() -> DrawingView {
    DrawingView::plain("base", Vec3::new(0.0, 0.0, -1.0), [0.0, 0.0])
}

fn compose(sheet: &DrawingSheet, geoms: &[BodyGeom]) -> SheetDraw {
    let mut fallback = None;
    if sheet.views.is_empty() {
        fallback = Some(default_top());
    }
    let views = fallback.as_ref().map(std::slice::from_ref).unwrap_or(sheet.views.as_slice());
    let mut segs = Vec::new();
    let mut texts = Vec::new();
    for view in views {
        let curves = if view.cache_valid {
            view.curves.clone()
        } else {
            project_bodies(view, &selected(geoms, &view.bodies), union_bounds(&selected(geoms, &view.bodies)))
        };
        for curve in curves {
            if curve.hidden && !view.hidden {
                continue;
            }
            let kind = if curve.section {
                DrawKind::Section
            } else if curve.hidden {
                DrawKind::Hidden
            } else {
                DrawKind::Visible
            };
            push_seg(&mut segs, curve.a, curve.b, kind);
        }
    }
    for note in &sheet.notes {
        match note.kind.as_str() {
            "title" => {}
            "center_mark" => add_center_mark(&mut segs, note.at),
            "centerline" => add_centerline(&mut segs, note.at, note.measured),
            "balloon" => add_balloon(&mut segs, &mut texts, note.at, &note.text),
            "leader" => {
                push_seg(&mut segs, note.at, [note.at[0] + 10.0, note.at[1] + 10.0], DrawKind::Annotation);
                add_text(&mut texts, [note.at[0] + 11.0, note.at[1] + 10.0], &note.text, 3.0, DrawKind::Annotation);
            }
            _ => add_text(&mut texts, note.at, &note.text, 3.0, DrawKind::Annotation),
        }
    }
    let bounds = seg_bounds(&segs);
    add_title_block(&mut segs, &mut texts, sheet, bounds);
    add_parts(&mut segs, &mut texts, &sheet.parts, bounds);
    let lost = lost_report(sheet);
    if !lost.is_empty() {
        let y = if bounds.1.is_finite() { bounds.1 - 6.0 } else { -8.0 };
        add_text(&mut texts, [0.0, y], &format!("Lost: {}", lost.join(", ")), 3.0, DrawKind::Annotation);
    }
    SheetDraw { segs, texts }
}

fn seg_bounds(segs: &[DrawSeg]) -> (f64, f64, f64, f64) {
    let mut minx = f64::INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut maxy = f64::NEG_INFINITY;
    for seg in segs {
        for p in [seg.a, seg.b] {
            if p[0].is_finite() && p[1].is_finite() {
                minx = minx.min(p[0]);
                miny = miny.min(p[1]);
                maxx = maxx.max(p[0]);
                maxy = maxy.max(p[1]);
            }
        }
    }
    (minx, miny, maxx, maxy)
}

fn push_seg(segs: &mut Vec<DrawSeg>, a: [f64; 2], b: [f64; 2], kind: DrawKind) {
    if segs.len() >= 8_000 || !a.iter().chain(b.iter()).all(|v| v.is_finite()) {
        return;
    }
    if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6 {
        return;
    }
    segs.push(DrawSeg { a, b, kind });
}

fn add_text(texts: &mut Vec<DrawText>, at: [f64; 2], text: &str, height: f64, kind: DrawKind) {
    if texts.len() >= 2_000 || text.is_empty() || !at[0].is_finite() || !at[1].is_finite() {
        return;
    }
    texts.push(DrawText { at, text: text.chars().take(200).collect(), height, kind });
}

fn add_center_mark(segs: &mut Vec<DrawSeg>, at: [f64; 2]) {
    let (x, y) = (at[0], at[1]);
    push_seg(segs, [x - 4.0, y], [x - 1.2, y], DrawKind::Annotation);
    push_seg(segs, [x + 1.2, y], [x + 4.0, y], DrawKind::Annotation);
    push_seg(segs, [x, y - 4.0], [x, y - 1.2], DrawKind::Annotation);
    push_seg(segs, [x, y + 1.2], [x, y + 4.0], DrawKind::Annotation);
}

fn add_centerline(segs: &mut Vec<DrawSeg>, at: [f64; 2], measured: f64) {
    let half = (measured * 0.5).clamp(4.0, 500.0);
    push_seg(segs, [at[0] - half, at[1]], [at[0] + half, at[1]], DrawKind::Hidden);
}

fn add_balloon(segs: &mut Vec<DrawSeg>, texts: &mut Vec<DrawText>, at: [f64; 2], label: &str) {
    let r = 4.0;
    let tip = [at[0] - 12.0, at[1] - 8.0];
    push_seg(segs, tip, [at[0] - r * 0.7, at[1] - r * 0.7], DrawKind::Annotation);
    let mut prev = None;
    let mut first = None;
    for i in 0..8 {
        let a = i as f64 * std::f64::consts::TAU / 8.0;
        let p = [at[0] + r * a.cos(), at[1] + r * a.sin()];
        if first.is_none() {
            first = Some(p);
        }
        if let Some(q) = prev {
            push_seg(segs, q, p, DrawKind::Annotation);
        }
        prev = Some(p);
    }
    if let (Some(q), Some(f)) = (prev, first) {
        push_seg(segs, q, f, DrawKind::Annotation);
    }
    add_text(texts, [at[0] - 1.2, at[1] - 1.0], label, 3.0, DrawKind::Annotation);
}

fn add_title_block(segs: &mut Vec<DrawSeg>, texts: &mut Vec<DrawText>, sheet: &DrawingSheet, bounds: (f64, f64, f64, f64)) {
    if sheet.title.is_empty() && sheet.paper.is_empty() && sheet.template.is_empty() {
        return;
    }
    let x = if bounds.2.is_finite() { bounds.2 + 12.0 } else { 0.0 };
    let y = if bounds.1.is_finite() { bounds.1 } else { 0.0 };
    let asme = sheet.template == "asme";
    let (w, h) = if asme { (150.0, 42.0) } else { (140.0, 36.0) };
    let rows: &[f64] = if asme { &[0.0, 8.4, 16.8, 25.2, 33.6, 42.0] } else { &[0.0, 9.0, 18.0, 27.0, 36.0] };
    for row in rows {
        push_seg(segs, [x, y + row], [x + w, y + row], DrawKind::Title);
    }
    push_seg(segs, [x, y], [x, y + h], DrawKind::Title);
    push_seg(segs, [x + w, y], [x + w, y + h], DrawKind::Title);
    let mid = if asme { 80.0 } else { 70.0 };
    push_seg(segs, [x + mid, y], [x + mid, y + h], DrawKind::Title);
    if asme {
        push_seg(segs, [x + 40.0, y], [x + 40.0, y + 16.8], DrawKind::Title);
        push_seg(segs, [x + 115.0, y + 25.2], [x + 115.0, y + h], DrawKind::Title);
    }
    let paper = if sheet.paper.is_empty() { "A4" } else { sheet.paper.as_str() };
    let scale = sheet.views.first().map(|v| scale_label(&v.scale)).unwrap_or_else(|| "1:1".into());
    let standard = if sheet.standard == "first_angle" { "First angle" } else { "Third angle" };
    let arrows = if sheet.arrows.is_empty() { "filled" } else { sheet.arrows.as_str() };
    let title = if sheet.title.is_empty() { "SolidCraft" } else { sheet.title.as_str() };
    let rev = if sheet.revision.is_empty() { "-" } else { sheet.revision.as_str() };
    let drawn = if sheet.drawn.is_empty() { "-" } else { sheet.drawn.as_str() };
    let date = if sheet.date.is_empty() { "-" } else { sheet.date.as_str() };
    let material = if sheet.material.is_empty() { "-" } else { sheet.material.as_str() };
    if asme {
        add_text(texts, [x + 2.0, y + 35.0], "SolidCraft", 3.0, DrawKind::Title);
        add_text(texts, [x + 82.0, y + 35.0], paper, 3.0, DrawKind::Title);
        add_text(texts, [x + 117.0, y + 35.0], &format!("Rev {rev}"), 3.0, DrawKind::Title);
        add_text(texts, [x + 2.0, y + 26.5], title, 3.4, DrawKind::Title);
        add_text(texts, [x + 82.0, y + 26.5], &scale, 3.0, DrawKind::Title);
        add_text(texts, [x + 2.0, y + 18.0], standard, 2.8, DrawKind::Title);
        add_text(texts, [x + 82.0, y + 18.0], material, 2.8, DrawKind::Title);
        add_text(texts, [x + 2.0, y + 9.5], &format!("Drawn {drawn}"), 2.8, DrawKind::Title);
        add_text(texts, [x + 42.0, y + 9.5], date, 2.8, DrawKind::Title);
        add_text(texts, [x + 82.0, y + 9.5], &sheet.name, 2.8, DrawKind::Title);
        add_text(texts, [x + 2.0, y + 1.5], arrows, 2.8, DrawKind::Title);
        add_arrow(segs, [x + 120.0, y + 2.5], arrows);
    } else {
        add_text(texts, [x + 2.0, y + 28.5], "SolidCraft", 3.2, DrawKind::Title);
        add_text(texts, [x + 72.0, y + 28.5], paper, 3.2, DrawKind::Title);
        add_text(texts, [x + 2.0, y + 19.5], title, 3.5, DrawKind::Title);
        add_text(texts, [x + 72.0, y + 19.5], &format!("Rev {rev}"), 3.0, DrawKind::Title);
        add_text(texts, [x + 2.0, y + 10.5], standard, 3.0, DrawKind::Title);
        add_text(texts, [x + 72.0, y + 10.5], &scale, 3.0, DrawKind::Title);
        add_text(texts, [x + 2.0, y + 2.0], &format!("{drawn} {date}"), 2.8, DrawKind::Title);
        add_text(texts, [x + 72.0, y + 2.0], material, 2.8, DrawKind::Title);
        add_arrow(segs, [x + 118.0, y + 20.0], arrows);
    }
}

fn add_arrow(segs: &mut Vec<DrawSeg>, at: [f64; 2], style: &str) {
    let (x, y) = (at[0], at[1]);
    if style == "open" {
        push_seg(segs, [x, y], [x + 6.0, y + 1.5], DrawKind::Annotation);
        push_seg(segs, [x, y], [x + 6.0, y - 1.5], DrawKind::Annotation);
    } else {
        push_seg(segs, [x, y], [x + 6.0, y + 1.8], DrawKind::Annotation);
        push_seg(segs, [x + 6.0, y + 1.8], [x + 6.0, y - 1.8], DrawKind::Annotation);
        push_seg(segs, [x + 6.0, y - 1.8], [x, y], DrawKind::Annotation);
    }
}

fn add_parts(segs: &mut Vec<DrawSeg>, texts: &mut Vec<DrawText>, parts: &[DrawingPart], bounds: (f64, f64, f64, f64)) {
    if parts.is_empty() {
        return;
    }
    let x = if bounds.0.is_finite() { bounds.0 } else { 0.0 };
    let y = if bounds.3.is_finite() { bounds.3 + 8.0 } else { 48.0 };
    let shown = parts.len().min(40);
    let rh = 5.0;
    let w = 90.0;
    let h = (shown + 1) as f64 * rh;
    push_seg(segs, [x, y], [x + w, y], DrawKind::Title);
    push_seg(segs, [x + w, y], [x + w, y + h], DrawKind::Title);
    push_seg(segs, [x + w, y + h], [x, y + h], DrawKind::Title);
    push_seg(segs, [x, y + h], [x, y], DrawKind::Title);
    for col in [16.0, 70.0] {
        push_seg(segs, [x + col, y], [x + col, y + h], DrawKind::Title);
    }
    add_text(texts, [x + 2.0, y + h - 4.0], "Item", 2.6, DrawKind::Title);
    add_text(texts, [x + 18.0, y + h - 4.0], "Part", 2.6, DrawKind::Title);
    add_text(texts, [x + 72.0, y + h - 4.0], "Qty", 2.6, DrawKind::Title);
    for (i, part) in parts.iter().take(shown).enumerate() {
        let yy = y + h - (i as f64 + 2.0) * rh + 1.0;
        add_text(texts, [x + 2.0, yy], &part.item.to_string(), 2.6, DrawKind::Title);
        add_text(texts, [x + 18.0, yy], &part.name, 2.6, DrawKind::Title);
        add_text(texts, [x + 72.0, yy], &part.qty.to_string(), 2.6, DrawKind::Title);
    }
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod drawing_tests;
