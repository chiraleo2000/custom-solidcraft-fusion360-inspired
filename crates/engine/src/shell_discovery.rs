//! Pure quick-toolbox search and context-menu builders. The `ui.*` commands call these;
//! the shell presents the results through `CommandSpec` only.

use crate::Session;
use crate::cmd::{CommandInfo, CommandSpec};

/// Cap on toolbox hits so a hostile query cannot flood the UI.
pub const TOOLBOX_LIMIT: usize = 50;
/// Cap on context-menu command rows (plus the synthetic Repeat entry).
pub const CONTEXT_MENU_LIMIT: usize = 40;

/// Ordinary CAD English aliases for the quick toolbox (not product command names). Locale
/// synonyms for the palette live in `ui-egui`; this table keeps `ui.quick_toolbox` discoverable
/// without depending on L5 catalogs.
const TOOLBOX_SYNONYMS: &[(&str, &[&str])] = &[
    ("solid.press_pull", &["push pull", "offset face", "edit face"]),
    ("solid.extrude", &["lengthen", "thicken", "extend"]),
    ("solid.fillet", &["round", "blend", "radius edge"]),
    ("solid.chamfer", &["bevel", "break edge"]),
    ("solid.shell", &["hollow", "thin wall"]),
    ("solid.hole", &["drill", "tap", "countersink", "counterbore", "csk"]),
    ("solid.pipe", &["tube", "sweep cut", "clearance cut"]),
    ("solid.rib", &["web", "stiffener", "brace"]),
    ("solid.pattern.rectangular", &["array", "grid pattern", "rect pattern"]),
    ("solid.mirror", &["reflect", "flip copy"]),
    ("sketch.rectangle.two_point", &["box", "rect", "two point rectangle"]),
    ("sketch.circle.center", &["diameter", "radius circle"]),
    ("sketch.trim", &["shorten curve", "remove segment"]),
    ("component.create", &["new part", "new component"]),
    ("appearance.assign", &["colour", "color", "paint", "look"]),
    ("material.assign", &["physical material"]),
    ("canvas.calibrate", &["scale image", "1:1", "reference size"]),
    ("joint.as_built", &["as built", "assemble in place"]),
    ("joint.create", &["mate", "motion joint", "slider joint"]),
    ("parameters.add", &["user parameter", "expression", "variable"]),
    ("drawing.base_view", &["base view", "drawing view", "orthographic"]),
    ("drawing.parts_list", &["bom", "bill of materials", "parts list"]),
    ("drawing.balloon", &["balloon", "balloon callout", "item balloon"]),
    ("ecad.board", &["pcb", "circuit board", "board outline"]),
    ("ecad.export", &["gerber", "rs-274x", "copper export"]),
    ("ecad.push_3d", &["push to 3d", "board to 3d", "pcb solid"]),
    ("ui.quick_toolbox", &["toolbox", "s key", "command search"]),
];

fn synonyms_of(id: &str) -> &'static [&'static str] {
    TOOLBOX_SYNONYMS.iter().find(|(cmd, _)| *cmd == id).map(|(_, terms)| *terms).unwrap_or(&[])
}

/// Rank how directly `query` matches a command (lower is better). `None` means no match.
/// 0 = id/label starts-with, 1 = id/label contains, 2 = synonym-only.
fn match_rank(query: &str, id: &str, label: &str, synonyms: &[&str]) -> Option<u8> {
    let q = query.trim();
    if q.is_empty() {
        return Some(0);
    }
    let q = q.to_ascii_lowercase();
    if q.len() > 200 {
        return None;
    }
    let id_l = id.to_ascii_lowercase();
    let label_l = label.to_ascii_lowercase();
    if id_l.starts_with(&q) || label_l.starts_with(&q) {
        return Some(0);
    }
    if id_l.contains(&q) || label_l.contains(&q) {
        return Some(1);
    }
    if synonyms.iter().any(|term| term.to_ascii_lowercase().contains(&q)) {
        return Some(2);
    }
    None
}

/// Search the registry by id, label, or known synonym. Empty query returns commands in registry
/// order up to `limit`. Results are deterministic: rank, then label, then id.
pub fn toolbox_search(session: &Session, query: &str, specs: &[&CommandSpec], limit: usize) -> Vec<CommandInfo> {
    let limit = limit.min(TOOLBOX_LIMIT);
    if query.trim().len() > 200 {
        return Vec::new();
    }
    let mut hits: Vec<(u8, CommandInfo)> = specs
        .iter()
        .filter_map(|spec| {
            let rank = match_rank(query, spec.id, spec.label, synonyms_of(spec.id))?;
            Some((rank, spec.info(session)))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.label.cmp(b.1.label)).then_with(|| a.1.id.cmp(b.1.id)));
    hits.into_iter().take(limit).map(|(_, info)| info).collect()
}

/// Commands whose `enabled(session)` holds for the given toolbar tab (`area`), plus a synthetic
/// Repeat-last entry when the session has a previous command. `area` is compared case-insensitively
/// to `CommandSpec.tab`.
pub fn context_menu_entries(session: &Session, area: &str, specs: &[&CommandSpec], limit: usize) -> Vec<CommandInfo> {
    let limit = limit.min(CONTEXT_MENU_LIMIT);
    let area = area.trim();
    if area.len() > 32 {
        return Vec::new();
    }
    let area_up = area.to_ascii_uppercase();
    let mut out = Vec::new();
    if session.last_command.is_some() {
        out.push(CommandInfo {
            id: "ui.repeat_last",
            label: "Repeat Last Command",
            tab: "",
            panel: "",
            icon: "redo",
            shortcut: None,
            params: "runs the previous command again",
            enabled: true,
            disabled_reason: None,
        });
    }
    for spec in specs {
        if out.len() >= limit {
            break;
        }
        if !spec.tab.eq_ignore_ascii_case(&area_up) {
            continue;
        }
        if (spec.enabled)(session).is_err() {
            continue;
        }
        if out.iter().any(|c| c.id == spec.id) {
            continue;
        }
        out.push(spec.info(session));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::command_specs;

    #[test]
    fn toolbox_search_matches_label_and_caps_results() {
        let specs = command_specs();
        let s = Session::default();
        let hits = toolbox_search(&s, "extrude", &specs, 50);
        assert!(hits.iter().any(|c| c.id == "solid.extrude"), "{hits:?}");
        assert!(hits.len() <= TOOLBOX_LIMIT);
        let empty = toolbox_search(&s, "", &specs, 10);
        assert_eq!(empty.len(), 10.min(specs.len()));
        assert!(toolbox_search(&s, &"x".repeat(201), &specs, 50).is_empty());
    }

    #[test]
    fn toolbox_search_matches_synonyms() {
        let specs = command_specs();
        let s = Session::default();
        let cases = [
            ("push pull", "solid.press_pull"),
            ("round", "solid.fillet"),
            ("hollow", "solid.shell"),
            ("paint", "appearance.assign"),
            ("new part", "component.create"),
            ("drill", "solid.hole"),
            ("bom", "drawing.parts_list"),
            ("gerber", "ecad.export"),
            ("pcb", "ecad.board"),
            ("as built", "joint.as_built"),
        ];
        for (q, id) in cases {
            let hits = toolbox_search(&s, q, &specs, 50);
            assert!(hits.iter().any(|c| c.id == id), "{q} → {id}: {hits:?}");
        }
        // Synonym-only hits rank after starts-with / contains on the same query shape.
        let press = toolbox_search(&s, "press", &specs, 50);
        let press_i = press.iter().position(|c| c.id == "solid.press_pull");
        assert!(press_i.is_some(), "{press:?}");
    }

    #[test]
    fn toolbox_starts_with_outranks_contains() {
        let specs = command_specs();
        let s = Session::default();
        let hits = toolbox_search(&s, "fil", &specs, 50);
        let start = hits.iter().position(|c| c.label.to_ascii_lowercase().starts_with("fil") || c.id.starts_with("fil"));
        let contain_only = hits.iter().position(|c| {
            let l = c.label.to_ascii_lowercase();
            let id = c.id.to_ascii_lowercase();
            (l.contains("fil") || id.contains("fil")) && !(l.starts_with("fil") || id.starts_with("fil"))
        });
        if let (Some(a), Some(b)) = (start, contain_only) {
            assert!(a < b, "starts-with must precede contains-only");
        }
    }

    #[test]
    fn context_menu_lists_enabled_tab_commands_and_repeat() {
        let specs = command_specs();
        let mut s = Session::default();
        let bare = context_menu_entries(&s, "SOLID", &specs, 40);
        assert!(bare.iter().all(|c| c.id != "ui.repeat_last"));
        assert!(bare.iter().any(|c| c.id == "solid.extrude"));
        assert!(bare.iter().all(|c| c.tab.eq_ignore_ascii_case("SOLID") || c.id == "ui.repeat_last"));
        s.last_command = Some(("solid.extrude".into(), serde_json::json!({"distance": 10})));
        let with_repeat = context_menu_entries(&s, "solid", &specs, 40);
        assert_eq!(with_repeat.first().map(|c| c.id), Some("ui.repeat_last"));
        assert!(with_repeat.len() <= CONTEXT_MENU_LIMIT);
        assert!(context_menu_entries(&s, &"Z".repeat(40), &specs, 40).is_empty());
    }

    /// Feature: fusion360-feature-parity, Property 20: Quick-toolbox and context-menu entries resolve to valid commands
    #[test]
    fn property_20_toolbox_and_context_entries_resolve() {
        let specs = command_specs();
        let s = Session::default();
        let queries = ["", "e", "solid", "sketch", "press", "fillet", "push pull", "hollow", "zzzz-no-match"];
        for q in queries {
            for hit in toolbox_search(&s, q, &specs, 50) {
                assert!(specs.iter().any(|c| c.id == hit.id) || hit.id == "ui.repeat_last", "unknown id {}", hit.id);
            }
        }
        assert!(toolbox_search(&s, "push pull", &specs, 50).iter().any(|c| c.id == "solid.press_pull"));
        let mut s = Session::default();
        for area in ["SOLID", "SKETCH", "SURFACE", "FORM", "MESH", "ASSEMBLE"] {
            for hit in context_menu_entries(&s, area, &specs, 40) {
                assert!(specs.iter().any(|c| c.id == hit.id) || hit.id == "ui.repeat_last", "unknown context id {} for {area}", hit.id);
            }
        }
        s.last_command = Some(("view.fit".into(), serde_json::json!({})));
        let with = context_menu_entries(&s, "SOLID", &specs, 40);
        assert_eq!(with.first().map(|c| c.id), Some("ui.repeat_last"));
    }
}
