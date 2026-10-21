//! Program commands that are not modelling operations. Coaching progress is application
//! settings: these commands never change the design or its undo history.

use serde_json::Value;

use super::CommandSpec;
use crate::coaching::{self, coaching_state};
use crate::params::{bad, bool_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("ui.coaching.set", "Set Coaching", set)
        .noundo()
        .doc_only()
        .params("enabled: bool. Stores whether the coaching layer is on. Returns {enabled, index, dismissed, completed, steps}"),
    CommandSpec::new("ui.coaching.advance", "Advance Coaching", advance).noundo().doc_only().params(
        "steps: number of coaching steps (from the UI table). Moves to the next step; the last step marks the sequence complete. Returns {enabled, index, dismissed, completed, steps}",
    ),
    CommandSpec::new("ui.coaching.dismiss", "Dismiss Coaching Step", dismiss)
        .noundo()
        .doc_only()
        .params("steps?: number of coaching steps. Hides the current step without disabling the layer. Returns {enabled, index, dismissed, completed, steps}"),
    CommandSpec::new("ui.coaching.state", "Coaching State", state)
        .noundo()
        .doc_only()
        .params("(steps?: number). Returns {enabled, index, dismissed, completed, steps} without changing anything"),
    CommandSpec::new("ui.select_filter", "Selection Filter", select_filter)
        .noundo()
        .doc_only()
        .params("kinds: [face|edge|vertex|sketch_curve|body|component]. Empty allows every kind"),
    CommandSpec::new("ui.quick_toolbox", "Quick Toolbox", quick_toolbox)
        .noundo()
        .doc_only()
        .params("query?: text. Returns matching command ids. The shell opens the command palette"),
    CommandSpec::new("ui.repeat_last", "Repeat Last Command", repeat_last).noundo().params("runs the previous command again"),
    CommandSpec::new("ui.context_menu", "Context Menu", context_menu)
        .noundo()
        .doc_only()
        .params("area?: SOLID|SKETCH|SURFACE|ASSEMBLE. Returns enabled commands for that toolbar tab, plus repeat when there is a previous command"),
    CommandSpec::new("ui.select_depth", "Depth Chooser", select_depth).noundo().doc_only().params(
        "candidates: [{id, kind, depth, min:[x,y], max:[x,y], anchor:[x,y]}]. Returns them nearest-first. Nothing is hidden",
    ),
];

fn steps_of(cmd: &str, p: &Value, required: bool) -> Result<usize> {
    match p.get("steps") {
        Some(Value::Number(n)) => {
            let n = n.as_u64().filter(|n| *n <= 10_000).ok_or_else(|| bad(cmd, "`steps` must be a count"))?;
            Ok(usize::try_from(n).unwrap_or(10_000))
        }
        Some(_) => Err(bad(cmd, "`steps` must be a count")),
        None if required => Err(bad(cmd, "`steps` is required")),
        None => Ok(0),
    }
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ui.coaching.set";
    let enabled = bool_(p, "enabled").ok_or_else(|| bad(cmd, "`enabled` must be a bool"))?;
    let steps = steps_of(cmd, p, false)?;
    coaching::coaching_set(&mut s.coaching, enabled);
    Ok(coaching_state(&s.coaching, steps))
}

fn advance(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ui.coaching.advance";
    let steps = steps_of(cmd, p, true)?;
    coaching::coaching_advance(&mut s.coaching, steps);
    Ok(coaching_state(&s.coaching, steps))
}

fn dismiss(s: &mut Session, p: &Value) -> Result<Value> {
    let steps = steps_of("ui.coaching.dismiss", p, false)?;
    coaching::coaching_dismiss(&mut s.coaching);
    Ok(coaching_state(&s.coaching, steps))
}

fn state(s: &mut Session, p: &Value) -> Result<Value> {
    let steps = steps_of("ui.coaching.state", p, false)?;
    Ok(coaching_state(&s.coaching, steps))
}

fn select_filter(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ui.select_filter";
    let Some(kinds) = p.get("kinds").and_then(Value::as_array) else {
        return Err(bad(cmd, "`kinds` must be a list"));
    };
    if kinds.len() > 32 {
        return Err(bad(cmd, "too many kinds"));
    }
    let mut mask = 0u32;
    for kind in kinds {
        let Some(name) = kind.as_str() else { return Err(bad(cmd, "a kind is text")) };
        let bit = match name {
            "face" | "faces" => 1,
            "edge" | "edges" => 2,
            "vertex" | "vertices" => 4,
            "sketch_curve" | "sketch_curves" => 8,
            "body" | "bodies" => 16,
            "component" | "components" => 32,
            other => return Err(bad(cmd, format!("unknown kind `{other}`"))),
        };
        mask |= bit;
    }
    s.select_filter = mask;
    Ok(serde_json::json!({"kinds": kinds}))
}

fn quick_toolbox(s: &mut Session, p: &Value) -> Result<Value> {
    let query = crate::params::str_(p, "query").unwrap_or("").to_ascii_lowercase();
    if query.len() > 200 {
        return Err(bad("ui.quick_toolbox", "the query is too long"));
    }
    let mut ids = Vec::new();
    for spec in super::command_specs() {
        if query.is_empty() || spec.id.contains(&query) || spec.label.to_ascii_lowercase().contains(&query) {
            ids.push(spec.id);
        }
        if ids.len() >= 50 {
            break;
        }
    }
    let _ = s;
    Ok(serde_json::json!({"commands": ids}))
}

fn repeat_last(s: &mut Session, _p: &Value) -> Result<Value> {
    let Some((id, params)) = s.last_command.clone() else {
        return Err(bad("ui.repeat_last", "there is no command to repeat"));
    };
    s.execute(&id, &params)
}

fn context_menu(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ui.context_menu";
    let area = crate::params::str_(p, "area").unwrap_or("SOLID").to_ascii_uppercase();
    if area.len() > 32 {
        return Err(bad(cmd, "the area name is too long"));
    }
    let mut ids = Vec::new();
    if s.last_command.is_some() {
        ids.push("ui.repeat_last");
    }
    for spec in super::command_specs() {
        if spec.tab == area && (spec.enabled)(s).is_ok() {
            ids.push(spec.id);
        }
        if ids.len() >= 40 {
            break;
        }
    }
    Ok(serde_json::json!({"commands": ids}))
}

fn select_depth(s: &mut Session, p: &Value) -> Result<Value> {
    let _ = s;
    let cmd = "ui.select_depth";
    let Some(list) = p.get("candidates").and_then(Value::as_array) else {
        return Err(bad(cmd, "`candidates` must be a list"));
    };
    if list.len() > 4_096 {
        return Err(bad(cmd, "too many candidates"));
    }
    let mut picks = Vec::new();
    for item in list {
        let id = item.get("id").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "a candidate needs an id"))?;
        let kind = solidcraft_render::PickKind::parse(item.get("kind").and_then(Value::as_str).unwrap_or("face"))
            .ok_or_else(|| bad(cmd, "unknown candidate kind"))?;
        let depth = item.get("depth").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "a candidate needs a depth"))?;
        let min = pair(item.get("min")).ok_or_else(|| bad(cmd, "`min` must be [x, y]"))?;
        let max = pair(item.get("max")).ok_or_else(|| bad(cmd, "`max` must be [x, y]"))?;
        let anchor = pair(item.get("anchor")).unwrap_or(min);
        picks.push(solidcraft_render::Pickable { id, kind, depth, min, max, anchor });
    }
    let ordered = solidcraft_render::depth_stack(&picks, solidcraft_render::SelectFilter::ALL);
    let ids: Vec<u64> = ordered.iter().map(|p| p.id).collect();
    Ok(serde_json::json!({"ids": ids}))
}

fn pair(v: Option<&Value>) -> Option<solidcraft_geom::Vec2> {
    let arr = v?.as_array()?;
    if arr.len() != 2 {
        return None;
    }
    let x = arr.first()?.as_f64()?;
    let y = arr.get(1)?.as_f64()?;
    Some(solidcraft_geom::Vec2::new(x, y))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: serde_json::Value) -> serde_json::Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    #[test]
    fn coaching_commands_change_only_the_preference() {
        let mut s = Session::default();
        let doc = s.doc.to_json();
        let undo = s.undo.len();
        let v = run(&mut s, "ui.coaching.set", json!({"enabled": false, "steps": 6}));
        assert_eq!(v["enabled"], json!(false));
        assert!(!s.coaching.enabled);
        let v = run(&mut s, "ui.coaching.set", json!({"enabled": true, "steps": 6}));
        assert_eq!(v["enabled"], json!(true));
        let v = run(&mut s, "ui.coaching.advance", json!({"steps": 6}));
        assert_eq!(v["index"], json!(1));
        assert_eq!(v["steps"], json!(6));
        let v = run(&mut s, "ui.coaching.dismiss", json!({"steps": 6}));
        assert_eq!(v["dismissed"], json!(true));
        assert_eq!(v["enabled"], json!(true));
        assert_eq!(s.doc.to_json(), doc);
        assert_eq!(s.undo.len(), undo);
        let v = run(&mut s, "ui.coaching.state", json!({"steps": 6}));
        assert_eq!(v["index"], json!(1));
        assert_eq!(v["dismissed"], json!(true));
    }

    #[test]
    fn coaching_commands_reject_malformed_params() {
        let mut s = Session::default();
        let doc = s.doc.to_json();
        let undo = s.undo.len();
        assert!(s.execute("ui.coaching.set", &json!({"enabled": "yes"})).is_err());
        assert!(s.execute("ui.coaching.set", &json!({})).is_err());
        assert!(s.execute("ui.coaching.advance", &json!({})).is_err());
        assert!(s.execute("ui.coaching.advance", &json!({"steps": -1})).is_err());
        assert!(s.execute("ui.coaching.advance", &json!({"steps": "6"})).is_err());
        assert_eq!(s.doc.to_json(), doc);
        assert_eq!(s.undo.len(), undo);
        assert!(s.coaching.enabled);
    }

    #[test]
    fn advancing_past_the_last_step_completes() {
        let mut s = Session::default();
        for _ in 0..5 {
            run(&mut s, "ui.coaching.advance", json!({"steps": 6}));
        }
        let v = run(&mut s, "ui.coaching.advance", json!({"steps": 6}));
        assert_eq!(v["completed"], json!(true));
        assert_eq!(v["index"], json!(5));
        assert_eq!(s.undo.len(), 0);
    }
}
