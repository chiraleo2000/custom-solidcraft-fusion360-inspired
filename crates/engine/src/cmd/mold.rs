//! Mold box, cavity, split, pins, spout, and print clearance / nesting.
//! These record their own features and evaluate through the same kernel booleans as solids.

use serde_json::Value;
use solidcraft_doc::{FeatureKind, PlaneRef, expr::Kind};
use solidcraft_geom::{Plane, Vec3};

use super::CommandSpec;
use super::features::{add_feature, plane_param};
use crate::params::{bad, expr, str_};
use crate::{Result, Session};

const MAX_NEST: usize = 256;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("mold.box", "Mold Box", box_cmd)
        .at("SOLID", "CREATE")
        .icon("box")
        .params("body: prototype name; margin?: expr (default 5 mm); name?, body_name?"),
    CommandSpec::new("mold.cavity", "Mold Cavity", cavity)
        .at("SOLID", "MODIFY")
        .icon("combine")
        .params("mold: box body; tool: prototype body (kept — same as combine cut with keep_tools)"),
    CommandSpec::new("mold.split", "Split Mold", split).at("SOLID", "MODIFY").icon("split").params(
        "body: mold name; plane?: XY|XZ|YZ|plane|{origin, normal} (default: mid-height parting through the mold AABB, Z-up); \
             body_names?: [lower, upper] preferred half names",
    ),
    CommandSpec::new("mold.alignment_pins", "Alignment Pins", pins)
        .at("SOLID", "CREATE")
        .icon("hole")
        .params("upper, lower: half names; diameter, depth: expr (bosses on upper, matching holes in lower at the four parting corners)"),
    CommandSpec::new("mold.pour_spout", "Pour Spout", spout)
        .at("SOLID", "MODIFY")
        .icon("extrude")
        .params("body: mold name; width, depth: expr (channel cut from the outer −Y face into the cavity)"),
    CommandSpec::new("dfam.clearance", "Clearance Gap", clearance)
        .at("SOLID", "MODIFY")
        .icon("offset")
        .params("body: name; gap: expr (faces move inward; a parameter name re-evaluates on change)"),
    CommandSpec::new("dfam.nest", "Nest Bodies", nest)
        .at("SOLID", "MODIFY")
        .icon("rect")
        .params("bodies: [unique names]; width, depth: bed size; gap?: expr (tallest-first shelf pack)"),
];

fn require_body(s: &Session, name: &str, cmd: &str) -> Result<()> {
    if s.model.state().body(name).is_none() { Err(bad(cmd, format!("no body `{name}`"))) } else { Ok(()) }
}

/// Mid-height XY parting through the mold's AABB — the usual two-part guidance when no plane is given.
fn mid_height_parting(s: &Session, body: &str, cmd: &str) -> Result<PlaneRef> {
    let state = s.model.state();
    let mb = state.body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
    let bb = mb.mesh().bounds();
    if bb.is_empty() {
        return Err(bad(cmd, "the mold has no size to part"));
    }
    let mid = (bb.min.z + bb.max.z) * 0.5;
    if !(mid.is_finite()) {
        return Err(bad(cmd, "the mold bounds are not finite"));
    }
    let plane =
        Plane::from_normal(Vec3::new(bb.center().x, bb.center().y, mid), Vec3::Z).ok_or_else(|| bad(cmd, "could not build a parting plane"))?;
    Ok(PlaneRef::Custom { plane })
}

fn box_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mold.box";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    require_body(s, &body, cmd)?;
    let margin = expr(p, "margin").unwrap_or_else(|| "5 mm".into());
    super::features::check_expr(s, &margin, Kind::Length, cmd, "margin")?;
    add_feature(s, p, FeatureKind::MoldBox { body, margin })
}

fn cavity(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mold.cavity";
    let mold = str_(p, "mold").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`mold` is required"))?.to_string();
    let tool = str_(p, "tool").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`tool` is required"))?.to_string();
    if mold == tool {
        return Err(bad(cmd, "the mold and the prototype must be different bodies"));
    }
    require_body(s, &mold, cmd)?;
    require_body(s, &tool, cmd)?;
    add_feature(s, p, FeatureKind::MoldCavity { mold, tool })
}

fn split(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mold.split";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    require_body(s, &body, cmd)?;
    let defaulted = p.get("plane").is_none();
    let plane = if defaulted { mid_height_parting(s, &body, cmd)? } else { plane_param(s, p.get("plane"), cmd)? };
    let named = crate::params::string_list(p, "body_names");
    let mut out = add_feature(s, p, FeatureKind::MoldSplit { body: body.clone(), plane })?;
    // Parting guidance: lower = below the plane, upper = above (also the `body_names` order).
    let st = s.world_state();
    let (lower, upper) = if named.len() >= 2 && st.body(&named[0]).is_some() && st.body(&named[1]).is_some() {
        (named[0].clone(), named[1].clone())
    } else {
        let fid = out.get("feature").and_then(Value::as_u64);
        let mut halves: Vec<(String, f64)> = st
            .bodies
            .iter()
            .filter(|b| fid == Some(b.feature) || b.name == body || b.name == "MoldLower" || b.name == "MoldUpper")
            .map(|b| (b.name.clone(), b.mesh().bounds().center().z))
            .collect();
        halves.sort_by(|a, b| a.1.total_cmp(&b.1));
        halves.dedup_by(|a, b| a.0 == b.0);
        match halves.as_slice() {
            [lo, .., hi] if lo.0 != hi.0 => (lo.0.clone(), hi.0.clone()),
            _ => (String::new(), String::new()),
        }
    };
    if !lower.is_empty() && !upper.is_empty() {
        out["parting"] = serde_json::json!({
            "lower": lower,
            "upper": upper,
            "default_mid_height": defaulted,
            "guidance": "alignment pins: upper gets bosses, lower gets holes; pour spout usually on the lower half",
        });
    }
    Ok(out)
}

fn pins(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mold.alignment_pins";
    let upper = str_(p, "upper").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`upper` is required"))?.to_string();
    let lower = str_(p, "lower").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`lower` is required"))?.to_string();
    if upper == lower {
        return Err(bad(cmd, "the pin halves must be different bodies"));
    }
    require_body(s, &upper, cmd)?;
    require_body(s, &lower, cmd)?;
    let diameter = crate::params::req_expr(cmd, p, "diameter")?;
    let depth = crate::params::req_expr(cmd, p, "depth")?;
    super::features::check_expr(s, &diameter, Kind::Length, cmd, "diameter")?;
    super::features::check_expr(s, &depth, Kind::Length, cmd, "depth")?;
    add_feature(s, p, FeatureKind::AlignmentPins { upper, lower, diameter, depth })
}

fn spout(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mold.pour_spout";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    require_body(s, &body, cmd)?;
    let width = crate::params::req_expr(cmd, p, "width")?;
    let depth = crate::params::req_expr(cmd, p, "depth")?;
    super::features::check_expr(s, &width, Kind::Length, cmd, "width")?;
    super::features::check_expr(s, &depth, Kind::Length, cmd, "depth")?;
    add_feature(s, p, FeatureKind::PourSpout { body, width, depth })
}

fn clearance(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "dfam.clearance";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    require_body(s, &body, cmd)?;
    let gap = crate::params::req_expr(cmd, p, "gap")?;
    super::features::check_expr(s, &gap, Kind::Length, cmd, "gap")?;
    add_feature(s, p, FeatureKind::ClearanceGap { body, gap })
}

fn nest(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "dfam.nest";
    let bodies = crate::params::string_list(p, "bodies");
    if bodies.is_empty() || bodies.len() > MAX_NEST {
        return Err(bad(cmd, "`bodies` must list 1…256 bodies"));
    }
    if bodies.iter().any(|n| n.is_empty()) {
        return Err(bad(cmd, "`bodies` must list non-empty names"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for name in &bodies {
        if !seen.insert(name.as_str()) {
            return Err(bad(cmd, format!("`bodies` lists `{name}` more than once")));
        }
        require_body(s, name, cmd)?;
    }
    let width = crate::params::req_expr(cmd, p, "width")?;
    let depth = crate::params::req_expr(cmd, p, "depth")?;
    let gap = expr(p, "gap").unwrap_or_else(|| "0".into());
    super::features::check_expr(s, &width, Kind::Length, cmd, "width")?;
    super::features::check_expr(s, &depth, Kind::Length, cmd, "depth")?;
    super::features::check_expr(s, &gap, Kind::Length, cmd, "gap")?;
    add_feature(s, p, FeatureKind::Nesting { bodies, width, depth, gap })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"))
    }

    #[test]
    fn mold_box_encloses_and_cavity_keeps_the_tool() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Part"}));
        run(&mut s, "mold.box", json!({"body": "Part", "margin": "5", "body_name": "Mold"}));
        let st = s.world_state();
        let mold = &st.body("Mold").unwrap().body;
        let part = &st.body("Part").unwrap().body;
        let mv = solidcraft_kernel::measure(mold).unwrap().volume;
        let pv = solidcraft_kernel::measure(part).unwrap().volume;
        assert!(mv > pv * 4.0, "mold {mv} part {pv}");
        run(&mut s, "mold.cavity", json!({"mold": "Mold", "tool": "Part"}));
        assert!(s.world_state().body("Part").is_some());
        let after = solidcraft_kernel::measure(&s.world_state().body("Mold").unwrap().body).unwrap().volume;
        assert!((mv - after - pv).abs() < 1e-4, "removed {after} from {mv}, part {pv}");
    }

    #[test]
    fn nest_separates_two_boxes() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 4, "body_name": "A"}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 4, "corner": [0, 0, 0], "body_name": "B"}));
        run(&mut s, "dfam.nest", json!({"bodies": ["A", "B"], "width": "40", "depth": "40", "gap": "2"}));
        let st = s.world_state();
        let a = st.body("A").unwrap().mesh().bounds();
        let b = st.body("B").unwrap().mesh().bounds();
        let overlap_x = a.min.x < b.max.x && b.min.x < a.max.x;
        let overlap_y = a.min.y < b.max.y && b.min.y < a.max.y;
        assert!(!(overlap_x && overlap_y), "a {a:?} b {b:?}");
    }

    /// Default parting is mid-height; pins add four bosses/holes; spout removes volume.
    #[test]
    fn default_parting_pins_and_spout_deepen_a_two_part_mold() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 16, "height": 12, "body_name": "Part"}));
        run(&mut s, "mold.box", json!({"body": "Part", "margin": "6", "body_name": "Mold"}));
        run(&mut s, "mold.cavity", json!({"mold": "Mold", "tool": "Part"}));
        assert!(s.world_state().body("Part").is_some(), "cavity keeps the tool");
        let before_split = solidcraft_kernel::measure(&s.world_state().body("Mold").unwrap().body).unwrap().volume;
        let split = run(&mut s, "mold.split", json!({"body": "Mold", "body_names": ["Lower", "Upper"]}));
        assert_eq!(split["parting"]["lower"], "Lower", "{split}");
        assert_eq!(split["parting"]["upper"], "Upper", "{split}");
        assert_eq!(split["parting"]["default_mid_height"], true, "{split}");
        let st = s.world_state();
        let lower = st.body("Lower").expect("lower half");
        let upper = st.body("Upper").expect("upper half");
        let halves = solidcraft_kernel::measure(&lower.body).unwrap().volume + solidcraft_kernel::measure(&upper.body).unwrap().volume;
        assert!((halves - before_split).abs() < 1e-2, "split union {halves} vs {before_split}");

        let upper_name = upper.name.clone();
        let lower_name = lower.name.clone();
        let before_pins = solidcraft_kernel::measure(&s.world_state().body(&upper_name).unwrap().body).unwrap().volume;
        run(&mut s, "mold.alignment_pins", json!({"upper": upper_name, "lower": lower_name, "diameter": "3", "depth": "4"}));
        let after_pins = solidcraft_kernel::measure(&s.world_state().body(&upper_name).unwrap().body).unwrap().volume;
        assert!(after_pins > before_pins + 1.0, "pins add bosses: {before_pins} -> {after_pins}");

        let before_spout = solidcraft_kernel::measure(&s.world_state().body(&lower_name).unwrap().body).unwrap().volume;
        run(&mut s, "mold.pour_spout", json!({"body": lower_name, "width": "4", "depth": "8"}));
        let after_spout = solidcraft_kernel::measure(&s.world_state().body(&lower_name).unwrap().body).unwrap().volume;
        assert!(after_spout < before_spout - 1.0, "spout removes material: {before_spout} -> {after_spout}");
        assert!(s.world_state().body("Part").is_some());
    }

    #[test]
    fn nest_rejects_duplicate_bodies_without_changing_the_design() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 8, "width": 8, "height": 3, "body_name": "A"}));
        let before = s.doc.to_json();
        assert!(s.execute("dfam.nest", &json!({"bodies": ["A", "A"], "width": "40", "depth": "40", "gap": "1"})).is_err());
        assert_eq!(s.doc.to_json(), before);
    }

    #[test]
    fn hostile_mold_params_do_not_change_the_design() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Part"}));
        let before = s.doc.to_json();
        assert!(s.execute("mold.box", &json!({"body": "Part", "margin": "nan"})).is_err());
        assert!(s.execute("mold.cavity", &json!({"mold": "Missing", "tool": "Part"})).is_err());
        assert!(s.execute("dfam.nest", &json!({"bodies": [], "width": "10", "depth": "10"})).is_err());
        assert_eq!(s.doc.to_json(), before);
    }

    fn volume_of_all_but(s: &Session, skip: &str) -> f64 {
        let st = s.world_state();
        st.bodies.iter().filter(|b| b.name != skip).map(|b| solidcraft_kernel::measure(&b.body).map(|m| m.volume).unwrap_or(0.0)).sum()
    }

    /// Feature: fusion360-feature-parity, Property 28 (later-phase): a split union equals the
    /// pre-split mold, and the prototype survives the cavity cut (Keep Tools).
    #[test]
    fn property_28_split_halves_add_up_to_the_cut_mold() {
        let mut split_cases = 0;
        for (len, wid, hei, margin) in [(10.0, 10.0, 10.0, 5.0), (12.0, 8.0, 6.0, 3.0), (20.0, 15.0, 10.0, 4.0), (7.0, 7.0, 7.0, 2.0)] {
            let mut s = Session::default();
            run(&mut s, "solid.box", json!({"length": len, "width": wid, "height": hei, "body_name": "Part"}));
            run(&mut s, "mold.box", json!({"body": "Part", "margin": margin.to_string(), "body_name": "Mold"}));
            run(&mut s, "mold.cavity", json!({"mold": "Mold", "tool": "Part"}));
            assert!(s.world_state().body("Part").is_some(), "Keep Tools keeps the prototype");
            let cut_volume = volume_of_all_but(&s, "Part");
            let part_volume = len * wid * hei;
            let outer = (len + 2.0 * margin) * (wid + 2.0 * margin) * (hei + 2.0 * margin);
            assert!((cut_volume - (outer - part_volume)).abs() < 1e-3, "cavity volume {cut_volume} vs {}", outer - part_volume);
            let split = s.execute("mold.split", &json!({"body": "Mold", "plane": {"origin": [0.0, 0.0, hei * 0.5], "normal": [0.0, 0.0, 1.0]}}));
            if let Err(e) = split {
                // A plane the kernel cannot split through the cavity is reported, never panicked on.
                assert!(!matches!(e, crate::EngineError::Internal(..)), "{e}");
                continue;
            }
            let halves = volume_of_all_but(&s, "Part");
            assert!((halves - cut_volume).abs() < 1e-3, "split union {halves} vs mold {cut_volume}");
            assert!(s.world_state().bodies.len() >= 3, "the split adds a second half");
            split_cases += 1;
        }
        assert!(split_cases >= 1, "at least one case must really split");
    }

    /// Feature: fusion360-feature-parity, Property 28 (later-phase): nested bodies are
    /// non-overlapping and stay inside the bed, for generated sizes and gaps.
    #[test]
    fn property_28_nested_bodies_do_not_overlap_and_fit_the_bed() {
        let mut checked = 0;
        for count in 2..=5usize {
            for (size, gap) in [(4.0, 1.0), (6.0, 2.0), (8.0, 0.5), (5.0, 3.0), (3.0, 0.0)] {
                let mut s = Session::default();
                let names: Vec<String> = (0..count).map(|i| format!("N{i}")).collect();
                for n in &names {
                    run(&mut s, "solid.box", json!({"length": size, "width": size, "height": 2, "body_name": n}));
                }
                let bed = (size + gap) * count as f64 + size;
                run(&mut s, "dfam.nest", json!({"bodies": names, "width": bed.to_string(), "depth": bed.to_string(), "gap": gap.to_string()}));
                let st = s.world_state();
                let boxes: Vec<_> = names.iter().map(|n| st.body(n).unwrap().mesh().bounds()).collect();
                for b in &boxes {
                    assert!(b.max.x - b.min.x <= bed + 1e-6 && b.max.y - b.min.y <= bed + 1e-6);
                }
                for (i, a) in boxes.iter().enumerate() {
                    for b in boxes.iter().skip(i + 1) {
                        let x = a.min.x < b.max.x - 1e-9 && b.min.x < a.max.x - 1e-9;
                        let y = a.min.y < b.max.y - 1e-9 && b.min.y < a.max.y - 1e-9;
                        assert!(!(x && y), "overlap {a:?} {b:?}");
                    }
                }
                checked += 1;
            }
        }
        assert!(checked >= 20);
    }

    /// Feature: fusion360-feature-parity, Property 1: Hostile input never crashes and leaves the
    /// design unchanged, over all `mold.*` / `dfam.*` commands.
    #[test]
    fn property_1_hostile_mold_and_dfam_inputs_leave_the_design_unchanged() {
        let mut base = Session::default();
        run(&mut base, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Part"}));
        run(&mut base, "mold.box", json!({"body": "Part", "margin": "5", "body_name": "Mold"}));
        let saved = base.doc.to_json();
        let ids = ["mold.box", "mold.cavity", "mold.split", "mold.alignment_pins", "mold.pour_spout", "dfam.clearance", "dfam.nest"];
        let payloads = [
            json!(null),
            json!({}),
            json!({"body": "Mold", "margin": "nan", "gap": "inf", "width": "nan", "depth": "-inf"}),
            json!({"body": "", "mold": "", "tool": "", "upper": "", "lower": "", "bodies": []}),
            json!({"body": "Nope", "mold": "Nope", "tool": "Nope", "upper": "Nope", "lower": "Nope", "bodies": ["Nope"]}),
            json!({"mold": "Mold", "tool": "Mold", "upper": "Mold", "lower": "Mold", "diameter": "2", "depth": "2"}),
            json!({"body": "Mold", "width": "0", "depth": "0", "diameter": "0", "gap": "0", "margin": "0"}),
            json!({"body": "Mold", "width": "-5", "depth": "-5", "diameter": "-1", "gap": "-1", "margin": "-1"}),
            json!({"body": "Mold", "width": "1e300", "depth": "1e300", "diameter": "1e300", "gap": "1e300", "margin": "1e300"}),
            json!({"body": "Mold", "plane": {"origin": [0.0, 0.0, 1.0e308], "normal": [0.0, 0.0, 0.0]}}),
            json!({"body": "Mold", "plane": {"origin": [0.0, 0.0, 500.0], "normal": [0.0, 0.0, 1.0]}}),
            json!({"bodies": ["Mold", "Mold", "Part"], "width": "1", "depth": "1", "gap": "1"}),
            json!({"bodies": (0..300).map(|i| format!("B{i}")).collect::<Vec<_>>(), "width": "10", "depth": "10"}),
            json!({"body": 42, "mold": [1], "tool": {"x": 1}, "bodies": "Mold", "width": [1], "gap": {"a": 1}}),
            json!({"body": "Mold", "gap": "1 mm", "width": "5 mm", "depth": "1000000 mm", "diameter": "1000000 mm"}),
        ];
        assert!(ids.len() * payloads.len() >= 100);
        for id in ids {
            for payload in &payloads {
                let mut s = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
                let before = s.doc.to_json();
                match s.execute(id, payload) {
                    Err(crate::EngineError::Internal(cmd, msg)) => panic!("{cmd} {payload}: {msg}"),
                    Err(_) => assert_eq!(s.doc.to_json(), before, "{id} {payload} changed the design"),
                    Ok(_) => {}
                }
            }
        }
    }
}
