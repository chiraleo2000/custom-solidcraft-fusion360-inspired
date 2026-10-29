//! Mold and DfAM dialogs: box, keep-tool cavity, parting split, pins, spout, clearance, nest.
//! Toolbar clicks on UTILITIES open these instead of running with empty parameters.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::{Feature, FeatureKind, PlaneRef};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, field, row_label};
use crate::selection::{BODIES, PLANES, SelInput};

/// A mold or print-layout dialog's values.
#[derive(Clone, Debug, PartialEq)]
pub enum Mold {
    Box {
        margin: String,
        body_name: String,
    },
    Cavity,
    /// `plane` is the stored parting (edit). A new pick replaces it; empty uses mid-height.
    Split {
        lower: String,
        upper: String,
        plane: Option<Value>,
    },
    Pins {
        diameter: String,
        depth: String,
    },
    Spout {
        width: String,
        depth: String,
    },
    Clearance {
        gap: String,
    },
    Nest {
        width: String,
        depth: String,
        gap: String,
    },
}

impl Mold {
    pub fn title(&self) -> &'static str {
        match self {
            Mold::Box { .. } => "MOLD BOX",
            Mold::Cavity => "MOLD CAVITY",
            Mold::Split { .. } => "SPLIT MOLD",
            Mold::Pins { .. } => "ALIGNMENT PINS",
            Mold::Spout { .. } => "POUR SPOUT",
            Mold::Clearance { .. } => "CLEARANCE",
            Mold::Nest { .. } => "NEST",
        }
    }

    pub fn primary(&mut self) -> Option<(&'static str, ValueKind, &mut String)> {
        Some(match self {
            Mold::Box { margin, .. } => ("Margin", ValueKind::Length, margin),
            Mold::Pins { diameter, .. } => ("Diameter", ValueKind::Length, diameter),
            Mold::Spout { width, .. } => ("Width", ValueKind::Length, width),
            Mold::Clearance { gap } => ("Gap", ValueKind::Length, gap),
            Mold::Nest { width, .. } => ("Bed width", ValueKind::Length, width),
            Mold::Cavity | Mold::Split { .. } => return None,
        })
    }
}

fn inputs(k: &Mold) -> Vec<SelInput> {
    match k {
        Mold::Box { .. } | Mold::Spout { .. } | Mold::Clearance { .. } => {
            vec![SelInput::new("Body", BODIES, false)]
        }
        Mold::Cavity => vec![SelInput::new("Mold", BODIES, false), SelInput::new("Prototype (kept)", BODIES, false)],
        Mold::Split { .. } => vec![SelInput::new("Mold", BODIES, false), SelInput::new("Parting plane (mid-height if empty)", PLANES, false)],
        Mold::Pins { .. } => vec![SelInput::new("Upper (bosses)", BODIES, false), SelInput::new("Lower (holes)", BODIES, false)],
        Mold::Nest { .. } => vec![SelInput::new("Bodies", BODIES, true)],
    }
}

fn note(ui: &mut egui::Ui, text: &str) {
    row_label(ui, "");
    ui.label(egui::RichText::new(text).weak().small());
    ui.end_row();
}

/// The dialog for a mold or DfAM command.
pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let k = match id {
        "mold.box" => Mold::Box { margin: "5 mm".into(), body_name: String::new() },
        "mold.cavity" => Mold::Cavity,
        "mold.split" => Mold::Split { lower: "Lower".into(), upper: "Upper".into(), plane: None },
        "mold.alignment_pins" => Mold::Pins { diameter: "3 mm".into(), depth: "4 mm".into() },
        "mold.pour_spout" => Mold::Spout { width: "5 mm".into(), depth: "6 mm".into() },
        "dfam.clearance" => Mold::Clearance { gap: "0.4 mm".into() },
        "dfam.nest" => Mold::Nest { width: "200 mm".into(), depth: "200 mm".into(), gap: "2 mm".into() },
        _ => return None,
    };
    let i = inputs(&k);
    Some((Kind::Mold(k), i))
}

/// The rows after the inputs; true when Enter was pressed in a field.
pub fn rows(ui: &mut egui::Ui, k: &mut Mold) -> bool {
    let mut enter = false;
    match k {
        Mold::Box { margin, body_name } => {
            row_label(ui, "Margin");
            enter |= field(ui, margin);
            ui.end_row();
            row_label(ui, "Mold name");
            let r = ui.add(egui::TextEdit::singleline(body_name).hint_text("Mold"));
            enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.end_row();
            note(ui, "Grows a stock box around the prototype. The prototype stays, for a later cavity cut.");
        }
        Mold::Cavity => {
            note(ui, "Cuts the prototype out of the mold and keeps the tool body (same as Combine Cut with Keep Tools).");
        }
        Mold::Split { lower, upper, .. } => {
            row_label(ui, "Lower name");
            enter |= field(ui, lower);
            ui.end_row();
            row_label(ui, "Upper name");
            enter |= field(ui, upper);
            ui.end_row();
            note(
                ui,
                "No plane: the parting is the mold's mid-height (Z up). Lower is below, upper is above. Pins: bosses on upper, holes in lower. The pour spout usually cuts the lower half.",
            );
        }
        Mold::Pins { diameter, depth } => {
            row_label(ui, "Diameter");
            enter |= field(ui, diameter);
            ui.end_row();
            row_label(ui, "Depth");
            enter |= field(ui, depth);
            ui.end_row();
            note(ui, "Four bosses on the upper half and matching holes in the lower, at the parting corners.");
        }
        Mold::Spout { width, depth } => {
            row_label(ui, "Width");
            enter |= field(ui, width);
            ui.end_row();
            row_label(ui, "Depth");
            enter |= field(ui, depth);
            ui.end_row();
            note(ui, "Cuts a channel from the outer −Y face into the cavity.");
        }
        Mold::Clearance { gap } => {
            row_label(ui, "Gap");
            enter |= field(ui, gap);
            ui.end_row();
            note(ui, "Moves faces inward by this length. A parameter name rebuilds when you edit it.");
        }
        Mold::Nest { width, depth, gap } => {
            row_label(ui, "Bed width");
            enter |= field(ui, width);
            ui.end_row();
            row_label(ui, "Bed depth");
            enter |= field(ui, depth);
            ui.end_row();
            row_label(ui, "Gap");
            enter |= field(ui, gap);
            ui.end_row();
            note(ui, "Tallest-first shelf pack. Parts stay on the bed and keep the gap. A parameter name rebuilds with the bed.");
        }
    }
    enter
}

fn body_at(inputs: &[SelInput], i: usize) -> Option<String> {
    inputs.get(i)?.items.iter().find_map(|s| if let Sel::Body { name } = s { Some(name.clone()) } else { None })
}

fn bodies(inputs: &[SelInput], i: usize) -> Vec<String> {
    inputs
        .get(i)
        .map(|inp| inp.items.iter().filter_map(|s| if let Sel::Body { name } = s { Some(name.clone()) } else { None }).collect())
        .unwrap_or_default()
}

fn plane_from_pick(app: &SolveApp, sel: Option<&Sel>) -> Option<Value> {
    match sel {
        Some(Sel::Plane { name }) => Some(json!(name)),
        Some(Sel::Face { body, index, point }) => {
            let (_, n) = crate::dialogs::planar_face(&app.session, body, *index)?;
            Some(json!({"origin": [point.x, point.y, point.z], "normal": [n.x, n.y, n.z]}))
        }
        _ => None,
    }
}

fn plane_json(plane: &PlaneRef) -> Option<Value> {
    match plane {
        PlaneRef::Origin { name } | PlaneRef::Construction { name } => Some(json!(name)),
        PlaneRef::Custom { plane } => {
            let o = plane.origin;
            let n = plane.normal();
            Some(json!({"origin": [o.x, o.y, o.z], "normal": [n.x, n.y, n.z]}))
        }
        _ => None,
    }
}

fn named(slot: &str, fallback: &str) -> String {
    let t = slot.trim();
    if t.is_empty() { fallback.to_string() } else { t.to_string() }
}

/// The command a mold or DfAM dialog makes.
pub fn commands(app: &SolveApp, k: &Mold, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    let need = |i: usize, what: &str| {
        if body_at(inputs, i).is_none() && bodies(inputs, i).is_empty() { Err(format!("select {what} first")) } else { Ok(()) }
    };
    let (cmd, p) = match k {
        Mold::Box { margin, body_name } => {
            need(0, "the prototype body")?;
            let body = body_at(inputs, 0).ok_or("select the prototype body")?;
            let mut p = json!({"body": body, "margin": margin});
            if !body_name.trim().is_empty() {
                p["body_name"] = json!(body_name.trim());
            }
            ("mold.box", p)
        }
        Mold::Cavity => {
            need(0, "the mold body")?;
            need(1, "the prototype to keep")?;
            let mold = body_at(inputs, 0).ok_or("select the mold")?;
            let tool = body_at(inputs, 1).ok_or("select the prototype")?;
            if mold == tool {
                return Err("the mold and the prototype must be different bodies".into());
            }
            ("mold.cavity", json!({"mold": mold, "tool": tool}))
        }
        Mold::Split { lower, upper, plane } => {
            need(0, "the mold body")?;
            let body = body_at(inputs, 0).ok_or("select the mold")?;
            let mut p = json!({"body": body, "body_names": [named(lower, "Lower"), named(upper, "Upper")]});
            if let Some(picked) = plane_from_pick(app, inputs.get(1).and_then(|i| i.items.first())).or_else(|| plane.clone()) {
                p["plane"] = picked;
            }
            ("mold.split", p)
        }
        Mold::Pins { diameter, depth } => {
            need(0, "the upper half")?;
            need(1, "the lower half")?;
            let upper = body_at(inputs, 0).ok_or("select the upper half")?;
            let lower = body_at(inputs, 1).ok_or("select the lower half")?;
            if upper == lower {
                return Err("the pin halves must be different bodies".into());
            }
            ("mold.alignment_pins", json!({"upper": upper, "lower": lower, "diameter": diameter, "depth": depth}))
        }
        Mold::Spout { width, depth } => {
            need(0, "the mold half")?;
            let body = body_at(inputs, 0).ok_or("select the mold half")?;
            ("mold.pour_spout", json!({"body": body, "width": width, "depth": depth}))
        }
        Mold::Clearance { gap } => {
            need(0, "the body")?;
            let body = body_at(inputs, 0).ok_or("select the body")?;
            ("dfam.clearance", json!({"body": body, "gap": gap}))
        }
        Mold::Nest { width, depth, gap } => {
            let names = bodies(inputs, 0);
            if names.is_empty() {
                return Err("select the bodies to nest".into());
            }
            ("dfam.nest", json!({"bodies": names, "width": width, "depth": depth, "gap": gap}))
        }
    };
    Ok(vec![(cmd.to_string(), p)])
}

fn set_bodies(d: &mut Dialog, index: usize, names: &[String]) {
    if let Some(inp) = d.inputs.get_mut(index) {
        inp.items = names.iter().map(|n| Sel::Body { name: n.clone() }).collect();
    }
}

/// Edit Feature for a mold or DfAM timeline entry.
pub fn from_feature(app: &SolveApp, f: &Feature) -> Option<Dialog> {
    let d = match &f.kind {
        FeatureKind::MoldBox { body, margin } => {
            let mut d = Dialog::for_command(app, "mold.box")?;
            if let Kind::Mold(Mold::Box { margin: slot, body_name }) = &mut d.kind {
                *slot = margin.clone();
                if let Some(n) = f.body_names.first() {
                    *body_name = n.clone();
                }
            }
            set_bodies(&mut d, 0, std::slice::from_ref(body));
            d
        }
        FeatureKind::MoldCavity { mold, tool } => {
            let mut d = Dialog::for_command(app, "mold.cavity")?;
            set_bodies(&mut d, 0, std::slice::from_ref(mold));
            set_bodies(&mut d, 1, std::slice::from_ref(tool));
            d
        }
        FeatureKind::MoldSplit { body, plane } => {
            let mut d = Dialog::for_command(app, "mold.split")?;
            if let Kind::Mold(Mold::Split { lower, upper, plane: stored }) = &mut d.kind {
                if let Some(n) = f.body_names.first() {
                    *lower = n.clone();
                }
                if let Some(n) = f.body_names.get(1) {
                    *upper = n.clone();
                }
                *stored = plane_json(plane);
            }
            set_bodies(&mut d, 0, std::slice::from_ref(body));
            d
        }
        FeatureKind::AlignmentPins { upper, lower, diameter, depth } => {
            let mut d = Dialog::for_command(app, "mold.alignment_pins")?;
            if let Kind::Mold(Mold::Pins { diameter: dia, depth: dep }) = &mut d.kind {
                *dia = diameter.clone();
                *dep = depth.clone();
            }
            set_bodies(&mut d, 0, std::slice::from_ref(upper));
            set_bodies(&mut d, 1, std::slice::from_ref(lower));
            d
        }
        FeatureKind::PourSpout { body, width, depth } => {
            let mut d = Dialog::for_command(app, "mold.pour_spout")?;
            if let Kind::Mold(Mold::Spout { width: w, depth: dep }) = &mut d.kind {
                *w = width.clone();
                *dep = depth.clone();
            }
            set_bodies(&mut d, 0, std::slice::from_ref(body));
            d
        }
        FeatureKind::ClearanceGap { body, gap } => {
            let mut d = Dialog::for_command(app, "dfam.clearance")?;
            if let Kind::Mold(Mold::Clearance { gap: slot }) = &mut d.kind {
                *slot = gap.clone();
            }
            set_bodies(&mut d, 0, std::slice::from_ref(body));
            d
        }
        FeatureKind::Nesting { bodies, width, depth, gap } => {
            let mut d = Dialog::for_command(app, "dfam.nest")?;
            if let Kind::Mold(Mold::Nest { width: w, depth: dep, gap: g }) = &mut d.kind {
                *w = width.clone();
                *dep = depth.clone();
                *g = gap.clone();
            }
            set_bodies(&mut d, 0, bodies);
            d
        }
        _ => return None,
    };
    Some(d)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_engine::Session;

    use super::*;
    use crate::Services;
    use crate::dialogs::apply_commands;

    fn app_with_part() -> SolveApp {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 16, "height": 10, "body_name": "Proto"})).unwrap();
        SolveApp::new(s, Services::default())
    }

    #[test]
    fn mold_box_dialog_keeps_the_prototype_margin_and_optional_name() {
        let mut app = app_with_part();
        app.start("mold.box");
        let d = app.dialog.as_ref().expect("mold box dialog");
        assert!(matches!(d.kind, Kind::Mold(Mold::Box { .. })));
        let mut d = d.clone();
        d.inputs[0].items = vec![Sel::Body { name: "Proto".into() }];
        let cmds = apply_commands(&app, &d).unwrap();
        assert_eq!(cmds[0].0, "mold.box");
        assert_eq!(cmds[0].1["body"], "Proto");
        assert_eq!(cmds[0].1["margin"], "5 mm");
        assert!(cmds[0].1.get("body_name").is_none());
    }

    #[test]
    fn split_dialog_defaults_to_mid_height_and_names_the_halves() {
        let mut app = app_with_part();
        app.start("mold.split");
        let mut d = app.dialog.clone().expect("split dialog");
        d.inputs[0].items = vec![Sel::Body { name: "Proto".into() }];
        let cmds = apply_commands(&app, &d).unwrap();
        assert_eq!(cmds[0].0, "mold.split");
        assert!(cmds[0].1.get("plane").is_none(), "empty parting pick keeps the mid-height default");
        assert_eq!(cmds[0].1["body_names"], json!(["Lower", "Upper"]));
    }

    #[test]
    fn clearance_dialog_passes_a_parameter_gap() {
        let mut app = app_with_part();
        app.start("dfam.clearance");
        let mut d = app.dialog.clone().expect("clearance dialog");
        if let Kind::Mold(Mold::Clearance { gap }) = &mut d.kind {
            *gap = "clearance".into();
        }
        d.inputs[0].items = vec![Sel::Body { name: "Proto".into() }];
        let cmds = apply_commands(&app, &d).unwrap();
        assert_eq!(cmds[0].0, "dfam.clearance");
        assert_eq!(cmds[0].1["gap"], "clearance");
    }
}
