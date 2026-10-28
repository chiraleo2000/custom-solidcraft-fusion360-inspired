//! Surface commands: Patch, Stitch, Thicken and Trim (surface bodies and turning them into
//! solids).

use serde_json::Value;
use solidcraft_doc::{FaceAt, FeatureKind, PlaneRef, ProfileSel, expr::Kind};

use super::CommandSpec;
use super::features::{add_feature, check_expr, feature_sketch, operation, plane_param, profiles};
use crate::params::{bad, bool_, expr, str_, string_list, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("surface.patch", "Patch", patch)
        .at("SURFACE", "CREATE")
        .icon("patch")
        .params("sketch?: id|name and profiles? (planar patches of sketch regions) | edges: [[x,y,z]] (points on a closed loop of a body's edges, or picks on the sketch loop) and body?; guide?: [[x,y,z]] (a curve whose ends meet the boundary; coplanar keeps the polygon area); name?, body_name?"),
    CommandSpec::new("surface.stitch", "Stitch", stitch)
        .at("SURFACE", "MODIFY")
        .icon("stitch")
        .params("bodies: [names] (surfaces sewn into the first; a solid when they close, otherwise a joined surface); tolerance?: expr (edges within it are welded). The result lists gap edges that stayed open"),
    CommandSpec::new("surface.thicken", "Thicken", thicken)
        .at("SURFACE", "CREATE")
        .icon("thicken")
        .params("bodies: [surface names] (several bodies, each a plane or a circular cylinder); thickness: expr (negative: against the normal); symmetric?: bool; operation?: new|join|cut|intersect; targets?. A sharp edge or any other surface is not supported yet"),
    CommandSpec::new("surface.extend", "Extend", extend)
        .at("SURFACE", "MODIFY")
        .icon("extend")
        .params("body: planar surface name; edges: [[x,y,z]] (points on straight edges to push out); distance: expr"),
    CommandSpec::new("surface.trim", "Trim", trim)
        .at("SURFACE", "MODIFY")
        .icon("trim")
        .params("body: surface name; plane: XY|XZ|YZ|plane name|{origin, normal} or tool: {body, point} (a face, its surface extended) or sketch + curve (a line; other curves are not supported yet); keep: [x,y,z] (a point on the side to keep)"),
    CommandSpec::new("surface.extrude", "Extrude Surface", extrude_surface)
        .at("SURFACE", "CREATE")
        .icon("extrude")
        .params("same as solid.extrude, as a zero-thickness open body with no caps (open profiles allowed). taper, through_all, to/from and join/cut/intersect are not supported yet"),
    CommandSpec::new("surface.revolve", "Revolve Surface", revolve_surface)
        .at("SURFACE", "CREATE")
        .icon("revolve")
        .params("same as solid.revolve, as a zero-thickness open body with no caps. to, angle2 and join/cut/intersect are not supported yet"),
    CommandSpec::new("surface.sweep", "Sweep Surface", sweep_surface)
        .at("SURFACE", "CREATE")
        .icon("sweep")
        .params("same as solid.sweep, as a zero-thickness open body with no caps. join/cut/intersect and a self-crossing path are not supported yet"),
    CommandSpec::new("surface.loft", "Loft Surface", loft_surface)
        .at("SURFACE", "CREATE")
        .icon("loft")
        .params("same as solid.loft, as a zero-thickness open body with no caps. guide rails and join/cut/intersect are not supported yet"),
    CommandSpec::new("surface.untrim", "Untrim", untrim)
        .at("SURFACE", "MODIFY")
        .icon("trim")
        .params("body: surface name. Removes the latest trim of that body"),
    CommandSpec::new("surface.offset", "Offset Surface", offset)
        .at("SURFACE", "CREATE")
        .icon("offset")
        .params("body: planar surface or face body; distance: expr"),
    CommandSpec::new("surface.rule", "Ruled Surface", rule)
        .at("SURFACE", "CREATE")
        .icon("loft")
        .params("sketch: id|name; curve_a, curve_b: curve ids"),
    CommandSpec::new("surface.flatten", "Flatten", flatten)
        .at("SURFACE", "MODIFY")
        .icon("unfold")
        .params("body: a cylindrical body. A non-cylinder is refused"),
    CommandSpec::new("surface.reverse", "Reverse Normal", reverse)
        .at("SURFACE", "MODIFY")
        .icon("mirror")
        .params("body: name. Reflects it through the YZ plane"),
];

fn patch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.patch";
    let edges: Vec<solidcraft_geom::Vec3> = p.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if edges.len() > 10_000 {
        return Err(bad(cmd, "too many edges"));
    }
    if !edges.is_empty() && edges.len() < 3 {
        return Err(bad(cmd, "a patch boundary does not close"));
    }
    let guide: Vec<solidcraft_geom::Vec3> = p.get("guide").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if !guide.is_empty() && (guide.len() < 2 || guide.len() > 10_000) {
        return Err(bad(cmd, "a guide curve needs 2…10000 points"));
    }
    let want_sketch = p.get("sketch").is_some() || edges.is_empty();
    let sketch = if want_sketch { Some(feature_sketch(s, p, cmd)?) } else { None };
    let profiles = if sketch.is_some() { profiles(p, cmd)? } else { ProfileSel::All };
    let body = str_(p, "body").map(str::to_string);
    if sketch.is_none() && edges.is_empty() {
        return Err(bad(cmd, "a patch needs a sketch region or a loop of edges"));
    }
    add_feature(s, p, FeatureKind::Patch { sketch, profiles, body, edges, guide })
}

fn bodies_param(s: &Session, p: &Value, cmd: &str) -> Result<Vec<String>> {
    let bodies = string_list(p, "bodies");
    if bodies.is_empty() {
        return Err(bad(cmd, "`bodies` must list bodies"));
    }
    let st = s.model.state();
    if let Some(b) = bodies.iter().find(|b| st.body(b).is_none()) {
        return Err(bad(cmd, format!("no body `{b}`")));
    }
    Ok(bodies)
}

fn stitch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.stitch";
    let bodies = bodies_param(s, p, cmd)?;
    let tolerance = expr(p, "tolerance").unwrap_or_else(|| "0".into());
    check_expr(s, &tolerance, Kind::Length, cmd, "tolerance")?;
    let made = add_feature(s, p, FeatureKind::Stitch { bodies: bodies.clone(), tolerance })?;
    let st = s.model.state();
    let slot = st.body(bodies.first().map(String::as_str).unwrap_or(""));
    let closed = slot.is_some_and(|b| !b.body.is_surface());
    let gaps = slot.and_then(|b| solidcraft_kernel::gap_edges(&b.body).ok()).unwrap_or_default();
    let mut made = made;
    if let Some(obj) = made.as_object_mut() {
        let point = |p: solidcraft_geom::Vec3| [p.x, p.y, p.z];
        obj.insert("gaps".into(), serde_json::json!(gaps.iter().map(|g| point(g.mid)).collect::<Vec<_>>()));
        obj.insert(
            "gap_edges".into(),
            serde_json::json!(
                gaps.iter().map(|g| serde_json::json!({"start": point(g.start), "end": point(g.end), "mid": point(g.mid)})).collect::<Vec<_>>()
            ),
        );
        obj.insert("closed".into(), serde_json::json!(closed));
    }
    Ok(made)
}

fn thicken(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.thicken";
    let bodies = bodies_param(s, p, cmd)?;
    let thickness = expr(p, "thickness").ok_or_else(|| bad(cmd, "`thickness` is required"))?;
    check_expr(s, &thickness, Kind::Length, cmd, "thickness")?;
    let kind = FeatureKind::Thicken {
        bodies,
        thickness,
        symmetric: bool_(p, "symmetric").unwrap_or(false),
        operation: operation(p, cmd)?,
        targets: string_list(p, "targets"),
    };
    add_feature(s, p, kind)
}

fn extend(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.extend";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let edges: Vec<solidcraft_geom::Vec3> = p.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if edges.is_empty() || edges.len() > 10_000 {
        return Err(bad(cmd, "`edges` must list points on the edges to extend"));
    }
    let distance = expr(p, "distance").ok_or_else(|| bad(cmd, "`distance` is required"))?;
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    add_feature(s, p, FeatureKind::SurfaceExtend { body, edges, distance })
}

fn trim(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.trim";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let keep = p.get("keep").and_then(vec3).ok_or_else(|| bad(cmd, "`keep` must be [x, y, z] on the side to keep"))?;
    let (curve_sketch, curve) = match str_(p, "curve") {
        Some(id) if !id.is_empty() => {
            let sk = super::features::sketch_id(s, p.get("sketch").or_else(|| p.get("curve_sketch")), cmd, "sketch")?;
            let sketch = s.doc.sketch(sk).map_err(|e| bad(cmd, e.to_string()))?;
            let found = sketch.curves.iter().find(|c| c.id == id).ok_or_else(|| bad(cmd, format!("no curve `{id}`")))?;
            if !matches!(found.kind, solidcraft_sketch::CurveKind::Line { .. }) {
                return Err(bad(cmd, "not supported yet: trimming with a curve that is not a line"));
            }
            (Some(sk), Some(id.to_string()))
        }
        _ => (None, None),
    };
    let tool_given = p.get("tool").is_some_and(|t| !t.is_null());
    if curve.is_some() && tool_given {
        return Err(bad(cmd, "not supported yet: a trim that uses both a curve and a face"));
    }
    let (plane, tool) = if curve.is_some() {
        (PlaneRef::Origin { name: "XY".into() }, None)
    } else {
        match p.get("tool").filter(|t| !t.is_null()) {
            Some(t) => {
                let tb = t.get("body").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`tool.body` must name a body"))?.to_string();
                let point = t.get("point").and_then(vec3).ok_or_else(|| bad(cmd, "`tool.point` must be [x, y, z] on the tool face"))?;
                (PlaneRef::Origin { name: "XY".into() }, Some(FaceAt { body: tb, point }))
            }
            None => (plane_param(s, p.get("plane"), cmd)?, None),
        }
    };
    add_feature(s, p, FeatureKind::SurfaceTrim { body, plane, tool, keep, curve_sketch, curve })
}

fn as_surface(p: &Value) -> Value {
    let mut q = p.clone();
    if let Some(obj) = q.as_object_mut() {
        obj.insert("surface".into(), Value::Bool(true));
    }
    q
}

fn refuse_open_boolean(cmd: &str, p: &Value) -> Result<()> {
    match str_(p, "operation").map(str::to_ascii_lowercase).as_deref() {
        Some("join" | "cut" | "intersect") => Err(bad(cmd, "not supported yet: an open surface that joins, cuts or intersects")),
        _ => Ok(()),
    }
}

fn extrude_surface(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.extrude";
    refuse_open_boolean(cmd, p)?;
    let through = bool_(p, "through_all") == Some(true) || str_(p, "extent").is_some_and(|e| e.eq_ignore_ascii_case("through_all"));
    if p.get("taper").is_some() || through || p.get("to").is_some() || p.get("from").is_some() {
        return Err(bad(cmd, "not supported yet: an open surface with taper, through all, or to/from an object"));
    }
    super::features::extrude(s, &as_surface(p))
}

fn revolve_surface(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.revolve";
    refuse_open_boolean(cmd, p)?;
    if p.get("to").is_some() || p.get("angle2").is_some() {
        return Err(bad(cmd, "not supported yet: an open revolve to an object or with two angles"));
    }
    super::features::revolve(s, &as_surface(p))
}

fn sweep_surface(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.sweep";
    refuse_open_boolean(cmd, p)?;
    super::features::sweep(s, &as_surface(p))
}

fn loft_surface(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.loft";
    refuse_open_boolean(cmd, p)?;
    if p.get("rails").is_some() {
        return Err(bad(cmd, "not supported yet: a guide rail on an open loft"));
    }
    super::features::loft(s, &as_surface(p))
}

fn untrim(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.untrim";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?;
    let id = s
        .doc
        .features
        .iter()
        .rev()
        .find(|f| matches!(&f.kind, FeatureKind::SurfaceTrim { body: b, .. } if b == body))
        .map(|f| f.id)
        .ok_or_else(|| bad(cmd, format!("`{body}` has no trim to remove")))?;
    s.execute("timeline.delete", &serde_json::json!({"features": [id.to_string()]}))
}

fn offset(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.offset";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    let distance = crate::params::req_expr(cmd, p, "distance")?;
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    add_feature(s, p, FeatureKind::SurfaceOffset { body, distance })
}

fn rule(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.rule";
    let sketch = super::features::sketch_id(s, p.get("sketch"), cmd, "sketch")?;
    let curve_a = str_(p, "curve_a").filter(|c| !c.is_empty()).ok_or_else(|| bad(cmd, "`curve_a` is required"))?.to_string();
    let curve_b = str_(p, "curve_b").filter(|c| !c.is_empty()).ok_or_else(|| bad(cmd, "`curve_b` is required"))?.to_string();
    add_feature(s, p, FeatureKind::SurfaceRule { sketch, curve_a, curve_b })
}

fn flatten(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.flatten";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    add_feature(s, p, FeatureKind::SurfaceFlatten { body })
}

fn reverse(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.reverse";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    add_feature(s, p, FeatureKind::SurfaceReverse { body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"))
    }
    fn volume(s: &Session, name: &str) -> f64 {
        let st = s.world_state();
        solidcraft_kernel::measure(&st.body(name).unwrap().body).unwrap().volume
    }

    /// A rectangle patched, trimmed by a plane and thickened; a box's open top patched; a rim
    /// that is not planar patched.
    #[test]
    fn patch_trim_thicken_and_stitch() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "S", "body_name": "Sheet"}));
        let st = s.world_state();
        assert!(st.body("Sheet").unwrap().body.is_surface());
        run(&mut s, "surface.trim", json!({"body": "Sheet", "plane": {"origin": [3, 0, 0], "normal": [1, 0, 0]}, "keep": [8, 5, 0]}));
        run(&mut s, "surface.extend", json!({"body": "Sheet", "edges": [[10, 5, 0]], "distance": 5}));
        run(&mut s, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": "2 mm", "body_name": "Slab"}));
        let v = volume(&s, "Slab");
        assert!((v - 240.0).abs() < 1e-9, "{v}");
        // Stitch the patch of a box's open top back on.
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Cup"}));
        run(&mut s, "surface.patch", json!({"edges": [[25, 0, 10], [30, 5, 10], [25, 10, 10], [20, 5, 10]], "body": "Cup", "body_name": "Lid"}));
        assert!(s.world_state().body("Lid").unwrap().body.is_surface());
        // A rim that is not planar: a box with its top cut away by a sideways cylinder.
        run(&mut s, "solid.box", json!({"length": 28, "width": 28, "height": 20, "corner": [-14, -14, 0], "body_name": "Saddle"}));
        run(
            &mut s,
            "solid.cylinder",
            json!({"radius": 40, "height": 120, "base": [0, -60, 50], "axis": "Y", "operation": "cut", "targets": ["Saddle"]}),
        );
        let z = |x: f64| 50.0 - (1600.0 - x * x).sqrt();
        let rim = json!([[14, 0, z(14.0)], [0, 14, z(0.0)], [-14, 0, z(-14.0)], [0, -14, z(0.0)]]);
        run(&mut s, "surface.patch", json!({"edges": rim, "body": "Saddle", "body_name": "Cover"}));
        let st = s.world_state();
        let cover = &st.body("Cover").unwrap().body;
        assert!(cover.is_surface() && cover.face_count() == 1);
        // The cover is a smooth surface on the rim, close to the cylinder that cut it (its area
        // within 1% of the cylinder's inside the rim).
        let area = solidcraft_kernel::measure(cover).unwrap().area;
        let want = 28.0 * 40.0 * 2.0 * (14.0f64 / 40.0).asin();
        assert!((area - want).abs() < want * 1e-2, "{area} vs {want}");
        assert!(s.execute("surface.thicken", &json!({"bodies": ["Nope"], "thickness": 1})).is_err());
        assert!(s.execute("surface.trim", &json!({"body": "Sheet"})).is_err());
    }

    #[test]
    fn an_open_line_extrudes_as_one_surface_feature() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
        run(&mut s, "sketch.finish", json!({}));
        let before = s.doc.features.len();
        run(&mut s, "surface.extrude", json!({"sketch": "S", "distance": 10, "body_name": "Wall"}));
        assert_eq!(s.doc.features.len(), before + 1);
        assert!(matches!(s.doc.features.last().map(|f| &f.kind), Some(FeatureKind::Extrude { open: true, .. })));
        let st = s.world_state();
        let wall = st.body("Wall").unwrap();
        assert!(wall.body.is_surface());
        let m = solidcraft_kernel::measure(&wall.body).unwrap();
        assert!(m.volume.abs() < 1e-4, "{}", m.volume);
        assert!((m.area - 100.0).abs() < 1e-2, "{}", m.area);
        let saved = s.doc.to_json();
        let mut cold = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        cold.refresh();
        let cold_st = cold.world_state();
        assert!(cold_st.body("Wall").unwrap().body.is_surface());
        assert_eq!(cold.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Extrude { open: true, .. })).count(), 1);
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "body_name": "Box"}));
        s.doc_mut().add_feature(FeatureKind::SheetWalls { body: "Box".into(), axis: solidcraft_geom::Vec3::Z }, Some("Legacy")).unwrap();
        s.refresh();
        let again = s.world_state();
        let legacy = again.bodies.iter().find(|b| b.name != "Wall" && b.name != "Box" && b.body.is_surface());
        assert!(legacy.is_some(), "an old SheetWalls feature still evaluates");
    }

    #[test]
    fn open_revolve_sweep_and_loft_are_one_surface_each() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Turn"}));
        run(&mut s, "sketch.line", json!({"points": [[20, 0], [20, 10]]}));
        run(&mut s, "sketch.finish", json!({}));
        let n = s.doc.features.len();
        run(&mut s, "surface.revolve", json!({"sketch": "Turn", "axis": "y", "angle": "90 deg", "body_name": "Band"}));
        assert_eq!(s.doc.features.len(), n + 1);
        let band = s.world_state().body("Band").unwrap().body.clone();
        assert!(band.is_surface());
        let area = solidcraft_kernel::measure(&band).unwrap().area;
        let want = std::f64::consts::FRAC_PI_2 * 20.0 * 10.0;
        assert!((area - want).abs() < 0.02 * want, "{area} vs {want}");

        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Prof"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [8, 0]]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
        let path = run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 12]]}))["curves"][0].as_str().unwrap().to_string();
        run(&mut s, "sketch.finish", json!({}));
        let n = s.doc.features.len();
        run(&mut s, "surface.sweep", json!({"sketch": "Prof", "path_sketch": "Path", "path": [path], "body_name": "Ribbon"}));
        assert_eq!(s.doc.features.len(), n + 1);
        let ribbon = s.world_state().body("Ribbon").unwrap().body.clone();
        assert!(ribbon.is_surface());
        let area = solidcraft_kernel::measure(&ribbon).unwrap().area;
        assert!((area - 96.0).abs() < 1.0, "{area}");

        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "A"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "B"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 6], [10, 6]]}));
        run(&mut s, "sketch.finish", json!({}));
        let n = s.doc.features.len();
        run(&mut s, "surface.loft", json!({"sections": [{"sketch": "A"}, {"sketch": "B"}], "body_name": "Ruled"}));
        assert_eq!(s.doc.features.len(), n + 1);
        let ruled = s.world_state().body("Ruled").unwrap().body.clone();
        assert!(ruled.is_surface());
        let m = solidcraft_kernel::measure(&ruled).unwrap();
        assert!(m.volume.abs() < 1e-3, "{}", m.volume);
        assert!((m.area - 60.0).abs() < 1.0, "{}", m.area);
    }

    #[test]
    fn untrim_drops_the_latest_trim() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "S", "body_name": "Sheet"}));
        run(&mut s, "surface.trim", json!({"body": "Sheet", "plane": {"origin": [3, 0, 0], "normal": [1, 0, 0]}, "keep": [8, 5, 0]}));
        assert!(s.doc.features.iter().any(|f| matches!(f.kind, FeatureKind::SurfaceTrim { .. })));
        run(&mut s, "surface.untrim", json!({"body": "Sheet"}));
        assert!(!s.doc.features.iter().any(|f| matches!(f.kind, FeatureKind::SurfaceTrim { .. })));
        assert!(s.world_state().body("Sheet").unwrap().body.is_surface());
    }

    #[test]
    fn patch_trim_stitch_thicken_and_the_new_surface_commands() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pad"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "Pad", "body_name": "Sheet"}));
        let sheet = s.world_state().body("Sheet").unwrap().body.clone();
        let area = solidcraft_kernel::measure(&sheet).unwrap().area;
        assert!((area - 100.0).abs() < 1e-4, "planar patch area {area}");
        let before = s.doc.features.len();
        assert!(s.execute("surface.patch", &json!({"sketch": "Pad", "guide": [[5, 5, 4], [6, 5, 4]]})).is_err());
        assert_eq!(s.doc.features.len(), before, "a guide off the plane adds no feature");
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "body_name": "Block"}));
        assert!(s.execute("surface.patch", &json!({"body": "Block", "edges": [[45, 0, 10], [50, 5, 10]]})).is_err());
        assert_eq!(s.doc.features.len(), before + 1);
        // Mixed sketch outer + edge hole: one face whose area is the polygon minus the hole.
        run(
            &mut s,
            "surface.patch",
            json!({
                "sketch": "Pad",
                "edges": [[2, 2, 0], [4, 2, 0], [4, 4, 0], [2, 4, 0]],
                "body_name": "WithHole"
            }),
        );
        assert!(matches!(
            s.doc.features.last().map(|f| &f.kind),
            Some(FeatureKind::Patch { sketch: Some(_), edges, .. }) if edges.len() == 4
        ));
        let with_hole = s.world_state().body("WithHole").unwrap().body.clone();
        assert!(with_hole.is_surface());
        assert!((solidcraft_kernel::measure(&with_hole).unwrap().area - 96.0).abs() < 1e-3);
        // Edges that do not form a hole in the sketch are refused; the design is unchanged.
        let n = s.doc.features.len();
        let err = s
            .execute("surface.patch", &json!({"sketch": "Pad", "body": "Block", "edges": [[45, 0, 10], [50, 5, 10], [45, 10, 10], [40, 5, 10]]}))
            .unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
        assert_eq!(s.doc.features.len(), n);
        // A lid still patches from a body's edge loop alone.
        run(&mut s, "surface.patch", json!({"body": "Block", "edges": [[45, 0, 10], [50, 5, 10], [45, 10, 10], [40, 5, 10]], "body_name": "Lid"}));
        assert!(s.world_state().body("Lid").unwrap().body.is_surface());

        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cut"}));
        let curve = run(&mut s, "sketch.line", json!({"points": [[4, 0], [4, 10]]}))["curves"][0].as_str().unwrap().to_string();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.trim", json!({"body": "Sheet", "sketch": "Cut", "curve": curve, "keep": [8, 5, 0]}));
        let trimmed = s.world_state().body("Sheet").unwrap().body.clone();
        let trimmed_area = solidcraft_kernel::measure(&trimmed).unwrap().area;
        assert!((trimmed_area - 60.0).abs() < 1.0, "curve trim area {trimmed_area}");
        run(&mut s, "surface.untrim", json!({"body": "Sheet"}));
        let back = s.world_state().body("Sheet").unwrap().body.clone();
        assert!((solidcraft_kernel::measure(&back).unwrap().area - 100.0).abs() < 1e-3);

        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Other"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, 0], "p1": [40, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "Other", "body_name": "Sheet2"}));
        run(&mut s, "surface.thicken", json!({"bodies": ["Sheet2"], "thickness": "2 mm", "body_name": "Slab"}));
        let stitched = run(&mut s, "surface.stitch", json!({"bodies": ["Sheet", "Sheet2"]}));
        assert_eq!(stitched["closed"], json!(false));
        assert!(stitched["gaps"].as_array().is_some_and(|g| !g.is_empty()), "{stitched}");
        assert!(s.world_state().body("Sheet").unwrap().body.is_surface());

        let slab = s.world_state().body("Slab").unwrap().body.clone();
        let volume = solidcraft_kernel::measure(&slab).unwrap().volume.abs();
        assert!((volume - 200.0).abs() < 1e-3, "thicken volume {volume}");

        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rails"}));
        let a = run(&mut s, "sketch.line", json!({"points": [[0, 0], [12, 0]]}))["curves"][0].as_str().unwrap().to_string();
        let b = run(&mut s, "sketch.line", json!({"points": [[0, 4], [12, 4]]}))["curves"][0].as_str().unwrap().to_string();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.rule", json!({"sketch": "Rails", "curve_a": a, "curve_b": b, "body_name": "Ruled"}));
        let ruled = s.world_state().body("Ruled").unwrap().body.clone();
        assert!(ruled.is_surface());
        let ruled_area = solidcraft_kernel::measure(&ruled).unwrap().area;
        assert!((ruled_area - 48.0).abs() < 2.0, "{ruled_area}");

        run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "body_name": "Can"}));
        run(&mut s, "surface.flatten", json!({"body": "Can", "body_name": "Flat"}));
        let flat = s.world_state().body("Flat").unwrap().body.clone();
        let flat_area = solidcraft_kernel::measure(&flat).unwrap().area;
        let want = std::f64::consts::PI * 10.0 * 10.0;
        assert!((flat_area - want).abs() < 1.0, "{flat_area} vs {want}");
        let n = s.doc.features.len();
        assert!(s.execute("surface.flatten", &json!({"body": "Slab"})).is_err());
        assert_eq!(s.doc.features.len(), n);

        let z0 = s.world_state().body("Sheet").unwrap().body.tessellate(0.2).unwrap().bounds().max.z;
        run(&mut s, "surface.reverse", json!({"body": "Sheet"}));
        run(&mut s, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": "2 mm", "operation": "new", "body_name": "Flipped"}));
        let flipped = s.world_state().body("Flipped").unwrap().body.clone();
        assert!(flipped.tessellate(0.2).unwrap().bounds().min.z < z0 - 0.5, "reversing the normal thickens the other way");
    }

    /// Feature: fusion360-feature-parity, Property 3: Surface-create yields a zero-thickness surface and one timeline feature
    #[test]
    fn a_closed_profile_extrudes_as_an_open_sheet() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        let n = s.doc.features.len();
        run(&mut s, "surface.extrude", json!({"sketch": "S", "distance": 10, "body_name": "Tube"}));
        assert_eq!(s.doc.features.len(), n + 1);
        assert!(matches!(s.doc.features.last().map(|f| &f.kind), Some(FeatureKind::Extrude { open: true, .. })));
        let tube = s.world_state().body("Tube").unwrap().body.clone();
        assert!(tube.is_surface());
        assert_eq!(tube.face_count(), 4, "four walls and no caps");
        let m = solidcraft_kernel::measure(&tube).unwrap();
        assert!((m.area - 400.0).abs() < 1.0, "{}", m.area);
        let before = s.doc.to_json();
        let err = s.execute("surface.extrude", &json!({"sketch": "S", "distance": 10, "taper": "5 deg"})).unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
        assert_eq!(s.doc.to_json(), before);
        let err = s
            .execute("surface.loft", &json!({"sections": [{"sketch": "S"}, {"sketch": "S"}], "rails": [{"sketch": "S", "curves": ["nope"]}]}))
            .unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
        assert_eq!(s.doc.to_json(), before);
    }

    /// Feature: fusion360-feature-parity, Property 4: Planar patch area equals the enclosed area
    #[test]
    fn a_guide_on_the_boundary_keeps_the_polygon_area() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pad"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "surface.patch",
            json!({"sketch": "Pad", "guide": [[0, 5, 0], [10, 5, 0]], "edges": [[0, 5, 0], [10, 5, 0], [5, 0, 0]], "body_name": "Sheet"}),
        );
        let sheet = s.world_state().body("Sheet").unwrap().body.clone();
        assert!(sheet.is_surface());
        assert!((solidcraft_kernel::measure(&sheet).unwrap().area - 100.0).abs() < 1e-3);
        assert_eq!(s.world_state().bodies.iter().filter(|b| b.body.is_surface()).count(), 1);
        let n = s.doc.features.len();
        let err = s.execute("surface.patch", &json!({"sketch": "Pad", "guide": [[0, 5, 0], [10, 5, 0]], "edges": [[0, 5, 5]]})).unwrap_err();
        assert!(err.to_string().contains("boundary") || err.to_string().contains("not supported yet"), "{err}");
        assert_eq!(s.doc.features.len(), n);
        run(&mut s, "surface.patch", json!({"sketch": "Pad", "guide": [[0, 5, 0], [5, 5, 2], [10, 5, 0]], "body_name": "Tent"}));
        let tent = s.world_state().body("Tent").unwrap().body.clone();
        assert!(tent.is_surface());
        assert!(tent.tessellate(0.2).unwrap().bounds().max.z > 1.0);
    }

    /// Feature: fusion360-feature-parity, Property 5: Trim then untrim restores the surface
    #[test]
    fn a_face_tool_trims_and_untrim_restores() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "S", "body_name": "Sheet"}));
        run(&mut s, "solid.box", json!({"length": 2, "width": 12, "height": 10, "corner": [4, -1, -5], "body_name": "Tool"}));
        run(&mut s, "surface.trim", json!({"body": "Sheet", "tool": {"body": "Tool", "point": [4, 5, 0]}, "keep": [1, 5, 0]}));
        let kept = solidcraft_kernel::measure(&s.world_state().body("Sheet").unwrap().body).unwrap().area;
        assert!((kept - 40.0).abs() < 1.0 || (kept - 60.0).abs() < 1.0, "{kept}");
        run(&mut s, "surface.untrim", json!({"body": "Sheet"}));
        let back = solidcraft_kernel::measure(&s.world_state().body("Sheet").unwrap().body).unwrap().area;
        assert!((back - 100.0).abs() < 1e-3, "{back}");
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Round"}));
        let circle = run(&mut s, "sketch.circle.center", json!({"center": [5, 5], "radius": 2}))["curves"][0].as_str().unwrap().to_string();
        run(&mut s, "sketch.finish", json!({}));
        let before = s.doc.to_json();
        let err = s.execute("surface.trim", &json!({"body": "Sheet", "sketch": "Round", "curve": circle, "keep": [1, 1, 0]})).unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
        assert_eq!(s.doc.to_json(), before);
        let err = s
            .execute(
                "surface.trim",
                &json!({"body": "Sheet", "sketch": "Round", "curve": circle, "tool": {"body": "Tool", "point": [4, 5, 0]}, "keep": [1, 5, 0]}),
            )
            .unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
        assert_eq!(s.doc.to_json(), before);
    }

    /// Feature: fusion360-feature-parity, Property 6: Stitch closes to a solid exactly when faces bound a volume
    /// Feature: fusion360-feature-parity, Property 7: Thicken volume equals area times thickness
    #[test]
    fn stitch_reports_gap_edges_and_thicken_scales_area() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "A"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "B"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, 0], "p1": [40, 5]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "A", "body_name": "Sheet"}));
        run(&mut s, "surface.patch", json!({"sketch": "B", "body_name": "Sheet2"}));
        let made = run(&mut s, "surface.thicken", json!({"bodies": ["Sheet", "Sheet2"], "thickness": "2 mm", "body_name": "Slab"}));
        let id = made["feature"].as_u64().unwrap();
        let st = s.world_state();
        let solids: Vec<_> = st.bodies.iter().filter(|b| b.feature == id).collect();
        assert_eq!(solids.len(), 2);
        let volume: f64 = solids.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume.abs()).sum();
        assert!((volume - 300.0).abs() < 1e-2, "{volume}");
        let stitched = run(&mut s, "surface.stitch", json!({"bodies": ["Sheet", "Sheet2"], "tolerance": "0.01 mm"}));
        assert_eq!(stitched["closed"], json!(false));
        let edges = stitched["gap_edges"].as_array().unwrap();
        assert!(!edges.is_empty());
        assert!(edges.iter().all(|e| e.get("start").is_some() && e.get("end").is_some() && e.get("mid").is_some()));
        assert_eq!(stitched["gaps"].as_array().map(|g| g.len()), Some(edges.len()));
        let before = s.doc.to_json();
        let err = s.execute("surface.thicken", &json!({"bodies": ["Tool"], "thickness": 1})).unwrap_err();
        assert!(err.to_string().contains("no body") || err.to_string().contains("not supported yet"), "{err}");
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [80, 0, 0], "body_name": "Sharp"}));
        let before = s.doc.to_json();
        let err = s.execute("surface.thicken", &json!({"bodies": ["Sharp"], "thickness": "1 mm"})).unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
        assert_eq!(s.doc.to_json(), before);
    }

    /// Feature: fusion360-feature-parity, Property 1: Hostile input never crashes and leaves the design unchanged
    #[test]
    fn hostile_surface_params_do_not_panic_or_change_the_design() {
        let mut base = Session::default();
        run(&mut base, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
        run(&mut base, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut base, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [5, 5]}));
        run(&mut base, "sketch.finish", json!({}));
        run(&mut base, "surface.patch", json!({"sketch": "S", "body_name": "Sheet"}));
        let saved = base.doc.to_json();
        let ids = [
            "surface.patch",
            "surface.stitch",
            "surface.thicken",
            "surface.extend",
            "surface.trim",
            "surface.untrim",
            "surface.extrude",
            "surface.revolve",
            "surface.sweep",
            "surface.loft",
            "surface.offset",
            "surface.rule",
            "surface.flatten",
            "surface.reverse",
        ];
        let payloads = [
            json!(null),
            json!({}),
            json!({"thickness": "nan", "distance": "inf", "bodies": [], "keep": [1.0e20, 0.0, 0.0]}),
            json!({"body": "", "bodies": ["Nope"], "sketch": "Nope", "edges": []}),
            json!({"guide": [[1.0e20, 0.0, 0.0]], "curve": "", "operation": "cut", "taper": "10 deg"}),
            json!({"tolerance": "-1", "symmetric": true, "thickness": 0, "angle2": "10 deg"}),
            json!({"rails": [], "through_all": true, "to": [0, 0, 0], "from": [0, 0, 0]}),
            json!({"tool": {"body": "Nope", "point": [0.0, 0.0, 1.0e308]}, "plane": {"normal": [0.0, 0.0, 0.0]}}),
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
