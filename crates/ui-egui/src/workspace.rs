//! Explicit toolbar layouts: Sketch CREATE, the SOLID tab's ASSEMBLE panel, SHEET METAL and
//! PLASTIC. A command is registered on one tab and panel;
//! these panels list the commands they show in order (the first few get big buttons), so Extrude
//! or Joint appear on several tabs. Commands registered on such a panel but not listed follow at
//! the end.

use solidcraft_engine::CommandSpec;
use solidcraft_engine::doc::FeatureKind;

const ASSEMBLE: &[&str] = &[
    "component.with_sketch",
    "component.create",
    "joint.create",
    "joint.as_built",
    "joint.origin",
    "joint.rigid_group",
    "joint.drive",
    "joint.motion_link",
    "motion.study",
    "explode.create",
    "contact.enable_sets",
    "contact.enable_all",
    "contact.disable_all",
    "contact.create",
    "inspect.interference",
];

const SHEET_CREATE: &[&str] = &[
    "sketch.create",
    "sheet.flange",
    "sheet.flat_pattern",
    "sheet.hem",
    "sheet.fold",
    "sheet.convert",
    "component.create",
    "solid.extrude",
    "solid.hole",
    "solid.thread",
    "solid.pattern.rectangular",
    "solid.pattern.circular",
    "solid.pattern.path",
    "solid.mirror",
    "joint.origin",
];

const SHEET_MODIFY: &[&str] = &[
    "sheet.manage_rules",
    "sheet.unfold",
    "sheet.refold",
    "solid.fillet",
    "solid.chamfer",
    "solid.move",
    "solid.align",
    "timeline.delete",
    "solid.remove",
    "material.assign",
    "parameters.change",
];

const PLASTIC_SETUP: &[&str] = &["plastic.manage_rules", "plastic.assign_rule"];

const PLASTIC_CREATE: &[&str] = &[
    "sketch.create",
    "plastic.boss",
    "plastic.snap_fit",
    "plastic.lip",
    "plastic.rest",
    "component.create",
    "solid.extrude",
    "solid.revolve",
    "solid.sweep",
    "solid.loft",
    "solid.rib",
    "solid.web",
    "solid.emboss",
    "solid.hole",
    "solid.thread",
    "solid.box",
    "solid.cylinder",
    "solid.sphere",
    "solid.torus",
    "solid.coil",
    "solid.pipe",
    "solid.pattern.rectangular",
    "solid.pattern.circular",
    "solid.pattern.path",
    "solid.mirror",
    "joint.origin",
];

/// Design-workspace ribbon tabs. Manufacture / Simulation / Render / Animation stay out until
/// those workspaces deepen; Drawing stays reachable for seeded drawing commands.
pub const DESIGN_TABS: &[&str] = &["SOLID", "SURFACE", "FORM", "MESH", "SHEET METAL", "PLASTIC", "DRAWING", "ELECTRONICS", "UTILITIES"];

/// Ribbon tabs for a workspace name (`DESIGN` today). Unknown names fall back to Design.
pub fn tabs_for_workspace(workspace: &str) -> &'static [&'static str] {
    match workspace.trim().to_ascii_uppercase().as_str() {
        "DESIGN" | "" => DESIGN_TABS,
        // Later workspaces deepen here; until then they share the Design ribbon so the shell
        // never shows an empty tab strip.
        _ => DESIGN_TABS,
    }
}

/// Panels of a tab, when this module lays the tab out.
pub fn panels(tab: &str) -> Option<&'static [&'static str]> {
    match tab {
        "SOLID" => Some(&["CREATE", "MODIFY", "ASSEMBLE", "POSITION", "CONFIGURE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"]),
        "PLASTIC" => Some(&["SETUP", "CREATE", "MODIFY", "ASSEMBLE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"]),
        "ELECTRONICS" => Some(&["CREATE", "INSPECT"]),
        "UTILITIES" => Some(&["CREATE", "MODIFY", "MAKE", "SELECT"]),
        _ => None,
    }
}

/// Solid MODIFY panel order: Press/Pull first so the unified edit gesture is a toolbar button.
/// `solid.scale` stays in the promoted four so panel-drop scenarios keep a visible Scale handle.
const SOLID_MODIFY: &[&str] = &[
    "solid.press_pull",
    "solid.fillet",
    "solid.chamfer",
    "solid.scale",
    "solid.shell",
    "solid.move",
    "solid.combine",
    "solid.offset_face",
    "solid.draft",
    "solid.split_body",
    "solid.split_face",
    "face.delete",
    "solid.remove",
];

/// Drawing CREATE: base view first, then project/section/detail and exports.
const DRAWING_CREATE: &[&str] = &[
    "drawing.create",
    "drawing.base_view",
    "drawing.project",
    "drawing.section",
    "drawing.detail",
    "drawing.export_svg",
    "drawing.export_dxf",
    "drawing.export_pdf",
];

/// Drawing MODIFY: dimensions and BOM/balloons before notes.
const DRAWING_MODIFY: &[&str] = &[
    "drawing.dimension",
    "drawing.center_mark",
    "drawing.balloon",
    "drawing.parts_list",
    "drawing.title_block",
    "drawing.standard",
    "drawing.note",
    "drawing.leader",
];

/// Electronics CREATE: board → copper → push/export (autoroute stays refused in-command).
const ELECTRONICS_CREATE: &[&str] = &["ecad.schematic", "ecad.board", "ecad.trace", "ecad.wire", "ecad.push_3d", "ecad.export"];

const ELECTRONICS_INSPECT: &[&str] = &["ecad.erc", "ecad.drc"];

/// UTILITIES CREATE: mold stock and alignment before cavity edits.
const UTILITIES_CREATE: &[&str] = &["mold.box", "mold.alignment_pins"];

/// UTILITIES MODIFY: cavity / split / spout, then print clearance and nesting.
const UTILITIES_MODIFY: &[&str] = &["mold.cavity", "mold.split", "mold.pour_spout", "dfam.clearance", "dfam.nest"];

/// UTILITIES MAKE: setup → tool → common 2.5D paths (rest stay in the panel overflow).
const UTILITIES_MAKE: &[&str] = &["cam.setup", "cam.tool", "cam.pocket", "cam.face", "cam.drill", "cam.adaptive"];

/// The commands a panel shows, in order, and how many get a big button; `None` leaves the panel
/// to the registry.
pub fn layout<'a>(tab: &str, panel: &str, specs: &[&'a CommandSpec]) -> Option<(Vec<&'a CommandSpec>, usize)> {
    let (ids, promoted, own_tab): (&[&str], usize, &str) = match (tab, panel) {
        ("SKETCH", "CREATE") => (
            &[
                "sketch.line",
                "sketch.rectangle.two_point",
                "sketch.rectangle.three_point",
                "sketch.rectangle.center",
                "sketch.circle.center",
                "sketch.circle.two_point",
                "sketch.point",
                "sketch.trim",
                "sketch.extend",
                "sketch.break",
            ],
            8,
            tab,
        ),
        ("SOLID", "CONFIGURE") => (&["parameters.change", "config.table"], 2, tab),
        ("SOLID", "MODIFY") => (SOLID_MODIFY, 4, tab),
        ("SOLID" | "SHEET METAL" | "PLASTIC", "ASSEMBLE") => (ASSEMBLE, 4, tab),
        ("SHEET METAL", "CREATE") => (SHEET_CREATE, 3, tab),
        ("SHEET METAL", "MODIFY") => (SHEET_MODIFY, 2, tab),
        ("PLASTIC", "SETUP") => (PLASTIC_SETUP, 2, tab),
        ("PLASTIC", "CREATE") => (PLASTIC_CREATE, 5, tab),
        ("DRAWING", "CREATE") => (DRAWING_CREATE, 4, tab),
        ("DRAWING", "MODIFY") => (DRAWING_MODIFY, 4, tab),
        ("ELECTRONICS", "CREATE") => (ELECTRONICS_CREATE, 4, tab),
        ("ELECTRONICS", "INSPECT") => (ELECTRONICS_INSPECT, 2, tab),
        ("UTILITIES", "CREATE") => (UTILITIES_CREATE, 2, tab),
        ("UTILITIES", "MODIFY") => (UTILITIES_MODIFY, 3, tab),
        ("UTILITIES", "MAKE") => (UTILITIES_MAKE, 4, tab),
        // The solid tab's panels.
        ("PLASTIC", "MODIFY") => (&[], 4, "SOLID"),
        ("SURFACE" | "SHEET METAL" | "PLASTIC", "CONSTRUCT" | "INSPECT" | "INSERT") => (&[], 2, "SOLID"),
        ("SURFACE", "ASSEMBLE") => (ASSEMBLE, 4, "SOLID"),
        _ => return None,
    };
    let mut out: Vec<&CommandSpec> = ids.iter().filter_map(|id| specs.iter().find(|c| c.id == *id).copied()).collect();
    for c in specs.iter().filter(|c| (c.tab == tab || c.tab == own_tab) && c.panel == panel) {
        if !out.iter().any(|o| o.id == c.id) {
            out.push(*c);
        }
    }
    Some((out, promoted))
}

/// Timeline and browser icon of the sheet metal and plastic features (`None`: not one of them).
pub fn feature_icon(k: &FeatureKind) -> Option<&'static str> {
    Some(match k {
        FeatureKind::SheetBase { .. } | FeatureKind::SheetContour { .. } | FeatureKind::SheetFlange { .. } => "flange",
        FeatureKind::SheetHem { .. } => "hem",
        FeatureKind::SheetFold { .. } => "fold",
        FeatureKind::SheetUnfold { refold: false, .. } => "unfold",
        FeatureKind::SheetUnfold { .. } => "refold",
        FeatureKind::SheetConvert { .. } => "convert_sheet",
        FeatureKind::SheetRip { .. } => "unfold",
        FeatureKind::SheetLoft { .. } => "loft",
        FeatureKind::Boss { .. } => "boss",
        FeatureKind::Lip { .. } => "lip",
        FeatureKind::SnapFit { .. } => "snap",
        FeatureKind::Rest { .. } => "rest",
        FeatureKind::Rib { .. } => "rib",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(tab: &str, panel: &str) -> Vec<&'static str> {
        let specs = solidcraft_engine::command_specs();
        layout(tab, panel, &specs).map(|(v, _)| v.iter().map(|c| c.id).collect()).unwrap_or_default()
    }

    #[test]
    fn configure_header_clicks_open_parameters_and_configurations_separately() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let commands = ids("SOLID", "CONFIGURE");
        assert_eq!(&commands[..2], &["parameters.change", "config.table"]);
        assert!(commands.contains(&"config.configure"), "Configure remains available in the dropdown");
        h.step(&json!({"widget":"Parameters"})).unwrap();
        assert!(h.app.params.is_some(), "the ordinary Parameters button opens the editor");
        assert!(h.app.dialog.is_none());
        assert!(h.widget("User Parameters").is_some());
        assert!(h.widget("Model Parameters").is_some());
        // Exercise the editor's Add button, not a script-only parameter command.
        let parameters = h.app.params.as_mut().unwrap();
        parameters.new_name = "width".into();
        parameters.new_expr = "25 mm".into();
        h.frames(3);
        h.step(&json!({"widget":"Add"})).unwrap();
        let values = h.app.session.execute("parameters.list", &json!({})).unwrap();
        assert!(values["parameters"].as_array().unwrap().iter().any(|p| p["name"] == "width" && p["value"] == 25.0));
        h.step(&json!({"click":{"handle":"params:close"}})).unwrap();
        assert!(h.app.params.is_none());
        h.step(&json!({"widget":"Configurations"})).unwrap();
        assert!(h.app.params.is_none());
        assert!(matches!(
            h.app.dialog.as_ref().map(|d| &d.kind),
            Some(crate::dialogs::Kind::Motion(crate::dialogs_motion::Mo::Configurations { .. }))
        ));
        assert!(h.widget("Add Column").is_some());
        assert!(h.widget("Add Row").is_some());
    }

    /// Fusion's layout: joints on every tab's ASSEMBLE panel, sheet metal and plastic tabs
    /// reach the shared solid commands, the plastic rules sit on SETUP.
    #[test]
    fn tabs_share_commands_in_fusion_order() {
        for tab in ["SOLID", "SHEET METAL", "PLASTIC"] {
            let a = ids(tab, "ASSEMBLE");
            assert_eq!(a.first().copied(), Some("component.with_sketch"), "{tab}: {a:?}");
            assert_eq!(a.get(2).copied(), Some("joint.create"), "{tab}: {a:?}");
            assert!(a.contains(&"joint.drive") && a.contains(&"component.create"));
        }
        let c = ids("SHEET METAL", "CREATE");
        assert_eq!(c.get(1).copied(), Some("sheet.flange"));
        assert!(c.contains(&"solid.extrude") && c.contains(&"sheet.flat_pattern"));
        assert!(ids("SHEET METAL", "MODIFY").contains(&"sheet.unfold"));
        assert!(ids("PLASTIC", "SETUP").contains(&"plastic.manage_rules"));
        let p = ids("PLASTIC", "CREATE");
        assert!(p.contains(&"plastic.boss") && p.contains(&"solid.rib") && p.contains(&"plastic.lip"));
        assert!(ids("PLASTIC", "MODIFY").contains(&"solid.shell"));
        let sketch = layout("SKETCH", "CREATE", &solidcraft_engine::command_specs()).expect("sketch create");
        let trim = sketch.0.iter().position(|c| c.id == "sketch.trim").expect("trim");
        assert!(trim < sketch.1, "trim is a large button");
        assert!(panels("PLASTIC").is_some_and(|p| p.first() == Some(&"SETUP")));
        let modify = ids("SOLID", "MODIFY");
        assert_eq!(modify.first().copied(), Some("solid.press_pull"), "{modify:?}");
        assert!(tabs_for_workspace("DESIGN").contains(&"SOLID"));
        assert!(tabs_for_workspace("DESIGN").contains(&"SURFACE"));
        assert!(tabs_for_workspace("DESIGN").contains(&"ELECTRONICS"));
        assert!(tabs_for_workspace("DESIGN").contains(&"DRAWING"));
        assert!(panels("ELECTRONICS").is_some_and(|p| p.contains(&"CREATE") && p.contains(&"INSPECT")));
        let draw = ids("DRAWING", "CREATE");
        assert_eq!(draw.first().copied(), Some("drawing.create"), "{draw:?}");
        assert!(draw.contains(&"drawing.base_view") && draw.contains(&"drawing.export_svg"), "{draw:?}");
        let bom = ids("DRAWING", "MODIFY");
        assert!(bom.contains(&"drawing.parts_list") && bom.contains(&"drawing.balloon"), "{bom:?}");
        let ecreate = ids("ELECTRONICS", "CREATE");
        assert_eq!(ecreate.first().copied(), Some("ecad.schematic"), "{ecreate:?}");
        assert!(ecreate.contains(&"ecad.push_3d") && ecreate.contains(&"ecad.export"), "{ecreate:?}");
        assert!(ids("ELECTRONICS", "INSPECT").contains(&"ecad.drc"));
        assert_eq!(tabs_for_workspace("DESIGN"), tabs_for_workspace("unknown"));
        assert!(tabs_for_workspace("DESIGN").contains(&"UTILITIES"));
        assert!(panels("UTILITIES").is_some_and(|p| p == ["CREATE", "MODIFY", "MAKE", "SELECT"]));
        let mold = ids("UTILITIES", "CREATE");
        assert_eq!(mold.first().copied(), Some("mold.box"), "{mold:?}");
        assert!(mold.contains(&"mold.alignment_pins"), "{mold:?}");
        let mold_mod = ids("UTILITIES", "MODIFY");
        assert!(mold_mod.contains(&"mold.cavity") && mold_mod.contains(&"dfam.nest"), "{mold_mod:?}");
        let make = ids("UTILITIES", "MAKE");
        assert_eq!(make.first().copied(), Some("cam.setup"), "{make:?}");
        assert!(make.contains(&"cam.pocket") && make.contains(&"cam.adaptive"), "{make:?}");
        assert_eq!(crate::toolbar::panels("UTILITIES"), ["CREATE", "MODIFY", "MAKE", "SELECT"]);
    }
}
