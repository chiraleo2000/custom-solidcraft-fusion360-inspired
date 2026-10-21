//! Mold box, cavity, split, pins, spout, and print clearance / nesting.
//! These record their own features and evaluate through the same kernel booleans as solids.

use serde_json::Value;
use solidcraft_doc::{FeatureKind, PlaneRef, expr::Kind};

use super::CommandSpec;
use super::features::{add_feature, plane_param};
use crate::params::{bad, expr, str_};
use crate::{Result, Session};

const MAX_NEST: usize = 256;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("mold.box", "Mold Box", box_cmd)
        .at("SOLID", "CREATE")
        .icon("box")
        .params("body: prototype name; margin?: expr (default 5 mm); name?"),
    CommandSpec::new("mold.cavity", "Mold Cavity", cavity)
        .at("SOLID", "MODIFY")
        .icon("combine")
        .params("mold: box body; tool: prototype body (kept)"),
    CommandSpec::new("mold.split", "Split Mold", split)
        .at("SOLID", "MODIFY")
        .icon("split")
        .params("body: mold name; plane: XY|XZ|YZ|plane|{origin, normal}"),
    CommandSpec::new("mold.alignment_pins", "Alignment Pins", pins)
        .at("SOLID", "CREATE")
        .icon("hole")
        .params("upper, lower: half names; diameter, depth: expr"),
    CommandSpec::new("mold.pour_spout", "Pour Spout", spout).at("SOLID", "MODIFY").icon("extrude").params("body: mold name; width, depth: expr"),
    CommandSpec::new("dfam.clearance", "Clearance Gap", clearance)
        .at("SOLID", "MODIFY")
        .icon("offset")
        .params("body: name; gap: expr (faces move inward)"),
    CommandSpec::new("dfam.nest", "Nest Bodies", nest)
        .at("SOLID", "MODIFY")
        .icon("rect")
        .params("bodies: [names]; width, depth: bed size; gap?: expr"),
];

fn require_body(s: &Session, name: &str, cmd: &str) -> Result<()> {
    if s.model.state().body(name).is_none() { Err(bad(cmd, format!("no body `{name}`"))) } else { Ok(()) }
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
    let plane = if p.get("plane").is_none() { PlaneRef::Origin { name: "XY".into() } } else { plane_param(s, p.get("plane"), cmd)? };
    add_feature(s, p, FeatureKind::MoldSplit { body, plane })
}

fn pins(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mold.alignment_pins";
    let upper = str_(p, "upper").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`upper` is required"))?.to_string();
    let lower = str_(p, "lower").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`lower` is required"))?.to_string();
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
    for name in &bodies {
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
}
