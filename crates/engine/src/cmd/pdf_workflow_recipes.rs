//! PDF-day control-channel/CLI workflow recipes under `tests/workflows/pdf_day*.json`.
//! Every step is a registered `CommandSpec`. An unsupported step must return
//! `"not supported yet"` and leave the design unchanged; other errors fail the recipe.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{Session, find_command};

fn workflows_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/workflows")
}

/// Year-1 Wave E days plus Year-2 sheet/mold days (20–22, 24). Days 28–30 stay optional.
const REQUIRED_DAYS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27];

/// PR / `SOLIDCRAFT_WORKFLOWS=ci` subset: early design days plus smoke-flagged recipes.
const CI_DAYS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 19, 23];

fn ci_subset() -> bool {
    std::env::var("SOLIDCRAFT_WORKFLOWS").map(|v| v.eq_ignore_ascii_case("ci")).unwrap_or(false)
}

fn recipe_day(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    let recipe: Value = serde_json::from_str(&text).ok()?;
    Some(recipe["day"].as_u64()? as u32)
}

fn recipe_is_smoke(path: &Path) -> bool {
    fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["smoke"].as_bool()).unwrap_or(false)
}

fn recipe_paths() -> Vec<PathBuf> {
    let dir = workflows_dir();
    let ci = ci_subset();
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("pdf_day") && n.ends_with(".json")))
        .filter(|p| {
            if !ci {
                return true;
            }
            match recipe_day(p) {
                Some(day) if CI_DAYS.contains(&day) => true,
                Some(_) => recipe_is_smoke(p),
                None => false,
            }
        })
        .collect();
    paths.sort();
    paths
}

fn lookup(root: &Value, path: &str) -> Option<Value> {
    let mut cur = root.clone();
    for part in path.split('.') {
        cur = if part == "planar" {
            // `faces.planar.0.centroid` — only faces with a plane_normal (shell openings).
            let arr = cur.as_array()?;
            Value::Array(arr.iter().filter(|face| face.get("plane_normal").and_then(Value::as_array).is_some()).cloned().collect())
        } else if let Ok(i) = part.parse::<usize>() {
            cur.as_array()?.get(i)?.clone()
        } else {
            cur.get(part)?.clone()
        };
    }
    Some(cur)
}

fn expand_str(raw: &str, tmp: &Path, binds: &BTreeMap<String, String>, prev: &Value) -> String {
    let mut out = raw.replace("${tmp}", &tmp.to_string_lossy());
    for (k, v) in binds {
        out = out.replace(&format!("${{{k}}}"), v);
    }
    // ${prev.bodies.0} style paths into the previous command result.
    while let Some(start) = out.find("${prev.") {
        let after = start + "${prev.".len();
        let Some(end) = out[after..].find('}') else { break };
        let path = &out[after..after + end];
        let repl = lookup(prev, path)
            .map(|v| match v {
                Value::String(s) => s,
                Value::Number(n) => n.to_string(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        out.replace_range(start..after + end + 1, &repl);
    }
    out
}

/// When a string is exactly one `${…}` placeholder, keep structured JSON (arrays/objects/numbers)
/// so recipes can pass `faces: ["${prev.faces.0.centroid}"]` without stringifying the point.
fn expand_whole_placeholder(raw: &str, tmp: &Path, binds: &BTreeMap<String, String>, prev: &Value) -> Option<Value> {
    let s = raw.trim();
    if !(s.starts_with("${") && s.ends_with('}') && s.matches("${").count() == 1) {
        return None;
    }
    let inner = &s[2..s.len() - 1];
    if inner == "tmp" {
        return Some(Value::String(tmp.to_string_lossy().into_owned()));
    }
    if let Some(path) = inner.strip_prefix("prev.") {
        return lookup(prev, path);
    }
    binds.get(inner).map(|v| Value::String(v.clone()))
}

fn expand_value(v: &Value, tmp: &Path, binds: &BTreeMap<String, String>, prev: &Value) -> Value {
    match v {
        Value::String(s) => expand_whole_placeholder(s, tmp, binds, prev).unwrap_or_else(|| Value::String(expand_str(s, tmp, binds, prev))),
        Value::Array(a) => Value::Array(a.iter().map(|x| expand_value(x, tmp, binds, prev)).collect()),
        Value::Object(o) => {
            let mut m = serde_json::Map::new();
            for (k, x) in o {
                m.insert(k.clone(), expand_value(x, tmp, binds, prev));
            }
            Value::Object(m)
        }
        other => other.clone(),
    }
}

fn apply_binds(binds: &mut BTreeMap<String, String>, spec: &Value, result: &Value) {
    let Some(obj) = spec.as_object() else { return };
    for (name, path_v) in obj {
        let Some(path) = path_v.as_str() else { continue };
        if let Some(v) = lookup(result, path) {
            let text = match v {
                Value::String(s) => s,
                Value::Number(n) => n.to_string(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            if !text.is_empty() && text != "null" {
                binds.insert(name.clone(), text);
            }
        }
    }
    // Mesh insert often returns bodies: [] until named; fall back to the feature name.
    if binds.get("mesh").map(String::as_str).unwrap_or("").is_empty() {
        if let Some(feat) = binds.get("mesh_feat").cloned().filter(|s| !s.is_empty()) {
            binds.insert("mesh".into(), feat);
        } else if let Some(Value::String(name)) = result.get("name")
            && !name.is_empty()
        {
            binds.insert("mesh".into(), name.clone());
        }
    }
}

fn run_recipe(path: &Path, tmp: &Path) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let recipe: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let id = recipe["id"].as_str().or_else(|| path.file_stem().and_then(|s| s.to_str())).unwrap_or("recipe");
    let commands = recipe["commands"].as_array().ok_or_else(|| format!("{id}: missing `commands` array"))?;
    if commands.is_empty() {
        return Err(format!("{id}: empty commands"));
    }

    let mut session = Session::default();
    let mut binds = BTreeMap::new();
    let mut prev = Value::Null;

    for (i, step) in commands.iter().enumerate() {
        let cmd = step
            .get("command")
            .or_else(|| step.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id} step {}: missing `command`", i + 1))?;
        if find_command(cmd).is_none() {
            return Err(format!("{id} step {}: `{cmd}` is not a registered command", i + 1));
        }
        let params = expand_value(step.get("params").unwrap_or(&Value::Null), tmp, &binds, &prev);
        let before = session.doc.to_json();
        match session.execute(cmd, &params) {
            Ok(result) => {
                if let Some(bind) = step.get("bind") {
                    apply_binds(&mut binds, bind, &result);
                }
                prev = result;
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("not supported yet") {
                    if session.doc.to_json() != before {
                        return Err(format!("{id} step {} ({cmd}): unsupported step changed the design", i + 1));
                    }
                    prev = json!({"unsupported": msg});
                    continue;
                }
                return Err(format!("{id} step {} ({cmd}) {params}: {msg}", i + 1));
            }
        }
    }

    let errors: Vec<String> = session.model.results.iter().filter_map(|r| r.error.clone().map(|e| format!("{}: {e}", r.name))).collect();
    if !errors.is_empty() {
        return Err(format!("{id}: timeline errors {errors:?}"));
    }
    Ok(())
}

#[test]
fn pdf_day_workflow_recipes_run_through_commands() {
    let dir = workflows_dir();
    let all: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("pdf_day") && n.ends_with(".json")))
        .collect();
    let present: BTreeMap<u32, PathBuf> = all.iter().filter_map(|p| recipe_day(p).map(|d| (d, p.clone()))).collect();
    let missing: Vec<u32> = REQUIRED_DAYS.iter().copied().filter(|d| !present.contains_key(d)).collect();
    assert!(missing.is_empty(), "missing PDF-day recipes for days {missing:?} under {}", dir.display());

    let paths = recipe_paths();
    let want_min = if ci_subset() { CI_DAYS.len() } else { REQUIRED_DAYS.len() };
    assert!(
        paths.len() >= want_min,
        "expected at least {want_min} PDF day recipes under {} (ci={}): found {}",
        dir.display(),
        ci_subset(),
        paths.len()
    );
    let tmp = std::env::temp_dir().join(format!("solidcraft-pdf-wf-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap_or_else(|e| panic!("{}: {e}", tmp.display()));
    let mut failures = Vec::new();
    for path in &paths {
        if let Err(e) = run_recipe(path, &tmp) {
            failures.push(e);
        }
    }
    let _ = fs::remove_dir_all(&tmp);
    assert!(failures.is_empty(), "workflow recipe failures:\n{}", failures.join("\n"));
}
