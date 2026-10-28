//! `cargo xtask workflows`: Stage-1 PDF-day recipe gate.
//!
//! Structural check (always): every required Year-1 day has a `tests/workflows/pdf_day*.json`
//! with a non-empty `commands` array, and every step id appears in `docs/features.md`
//! (registry dump). Optional `--run` executes the engine recipe test
//! (`pdf_day_workflow_recipes_run_through_commands`). `--ci` sets `SOLIDCRAFT_WORKFLOWS=ci`
//! so only the fast subset runs (Days 1–12 plus smoke recipes).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Year-1 Wave E required days (plus Day 23 smoke). Days 20–22/24 and 28–30 are Year 2–3.
const REQUIRED_DAYS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 18, 19, 23, 25, 26, 27];

/// PR / `cargo xtask ci` subset: early design days plus smoke-flagged recipes.
const CI_DAYS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 19, 23];

/// Command ids from `docs/features.md` (`cargo xtask features`). Falls back to scanning
/// `CommandSpec::new("…")` under `crates/engine/src/cmd` when the doc is missing.
fn registered_command_ids(root: &Path) -> Result<BTreeSet<String>, String> {
    let features = root.join("docs/features.md");
    let mut ids = BTreeSet::new();
    if features.is_file() {
        let text = fs::read_to_string(&features).map_err(|e| format!("{}: {e}", features.display()))?;
        for line in text.lines() {
            // Table rows: `| Label | \`area.thing\` | … |`
            for chunk in line.split('`').skip(1).step_by(2) {
                let id = chunk.trim();
                if id.contains('.') && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_') {
                    ids.insert(id.to_string());
                }
            }
        }
    }
    if ids.len() < 50 {
        // Doc missing or stale — scrape the registry source so the gate still works.
        let cmd_dir = root.join("crates/engine/src/cmd");
        scrape_command_specs(&cmd_dir, &mut ids)?;
    }
    if ids.len() < 50 {
        return Err(format!(
            "could not load a command registry (found {} ids); run `cargo xtask features` or check crates/engine/src/cmd",
            ids.len()
        ));
    }
    Ok(ids)
}

fn scrape_command_specs(dir: &Path, ids: &mut BTreeSet<String>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            scrape_command_specs(&path, ids)?;
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut rest = text.as_str();
        while let Some(at) = rest.find("CommandSpec::new(\"") {
            rest = &rest[at + "CommandSpec::new(\"".len()..];
            let Some(end) = rest.find('"') else { break };
            let id = &rest[..end];
            if id.contains('.') {
                ids.insert(id.to_string());
            }
            rest = &rest[end + 1..];
        }
    }
    Ok(())
}

pub fn run(root: &Path, ci: bool, execute: bool) -> Result<(), String> {
    let registry = registered_command_ids(root)?;
    let dir = root.join("tests/workflows");
    let mut found: BTreeSet<u32> = BTreeSet::new();
    let mut files = 0usize;
    let mut smoke = 0usize;
    let mut unknown: Vec<String> = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(name.starts_with("pdf_day") && name.ends_with(".json")) {
            continue;
        }
        files += 1;
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let recipe: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let day = recipe["day"].as_u64().ok_or_else(|| format!("{}: missing numeric `day`", path.display()))? as u32;
        let commands = recipe["commands"].as_array().ok_or_else(|| format!("{}: missing `commands`", path.display()))?;
        if commands.is_empty() {
            return Err(format!("{}: empty `commands`", path.display()));
        }
        for (i, step) in commands.iter().enumerate() {
            let cmd = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str);
            let Some(cmd) = cmd.filter(|c| !c.is_empty()) else {
                return Err(format!("{} step {}: missing `command`", path.display(), i + 1));
            };
            if !registry.contains(cmd) {
                unknown.push(format!("{name} step {}: `{cmd}`", i + 1));
            }
        }
        if recipe["smoke"].as_bool() == Some(true) {
            smoke += 1;
        }
        found.insert(day);
    }
    if !unknown.is_empty() {
        return Err(format!("workflow recipes reference unregistered commands (not in docs/features.md):\n  {}", unknown.join("\n  ")));
    }
    let missing: Vec<u32> = REQUIRED_DAYS.iter().copied().filter(|d| !found.contains(d)).collect();
    if !missing.is_empty() {
        return Err(format!("missing PDF-day recipes for days {missing:?} under {}", dir.display()));
    }
    let want = if ci { CI_DAYS } else { REQUIRED_DAYS };
    let covered: Vec<u32> = want.iter().copied().filter(|d| found.contains(d)).collect();
    if ci {
        let absent: Vec<u32> = CI_DAYS.iter().copied().filter(|d| !found.contains(d)).collect();
        if !absent.is_empty() {
            return Err(format!("ci gate missing PDF-day recipes for days {absent:?}"));
        }
    }
    println!(
        "workflows: {files} recipe files, {smoke} smoke; {} registered cmds checked; required days ok; {} gate covers days {covered:?}",
        registry.len(),
        if ci { "ci" } else { "full" }
    );
    if !execute {
        return Ok(());
    }
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(root);
    cmd.args(["test", "-p", "solidcraft-engine", "--lib", "pdf_day_workflow_recipes_run_through_commands", "--", "--nocapture"]);
    if ci {
        cmd.env("SOLIDCRAFT_WORKFLOWS", "ci");
    } else {
        cmd.env_remove("SOLIDCRAFT_WORKFLOWS");
    }
    eprintln!("$ cargo test -p solidcraft-engine --lib pdf_day_workflow_recipes_run_through_commands");
    let status = cmd.status().map_err(|e| format!("cargo test workflows: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("workflow recipe tests failed ({status})")) }
}
