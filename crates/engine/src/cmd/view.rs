//! Discrete view changes. The camera is view state the UI owns: these commands take the current
//! camera and return the next one. They do not change the design or the undo history.

use serde_json::{Value, json};
use solidcraft_geom::Aabb3;
use solidcraft_render::{Camera, StandardView};

use super::CommandSpec;
use crate::params::bad;
use crate::{Result, Sel, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("view.standard.top", "Top View", top).noundo().doc_only().params(
        "camera: {target, yaw, pitch, roll, distance, fov?, up?}. Returns {camera} looking down, with the same target and distance",
    ),
    CommandSpec::new("view.standard.front", "Front View", front).noundo().doc_only().params(
        "camera: {target, yaw, pitch, roll, distance, fov?, up?}. Returns {camera} looking along the front, with the same target and distance",
    ),
    CommandSpec::new("view.standard.right", "Right View", right).noundo().doc_only().params(
        "camera: {target, yaw, pitch, roll, distance, fov?, up?}. Returns {camera} looking from the right, with the same target and distance",
    ),
    CommandSpec::new("view.standard.home", "Home View", home).noundo().doc_only().params(
        "camera: {target, yaw, pitch, roll, distance, fov?, up?}. Returns {camera} in the isometric orientation, with the same target and distance",
    ),
    CommandSpec::new("view.standard.isometric", "Isometric View", isometric).noundo().doc_only().params(
        "camera: {target, yaw, pitch, roll, distance, fov?, up?}. Returns {camera} in the isometric orientation, with the same target and distance",
    ),
    CommandSpec::new("view.component_colors", "Component Colours", component_colors)
        .noundo()
        .doc_only()
        .params("enabled?: bool (omit to toggle). Paints each component its own colour"),
    CommandSpec::new("view.select_mode", "Selection Mode", select_mode)
        .noundo()
        .doc_only()
        .params("mode: box|lasso|paint"),
    CommandSpec::new("view.visual_style", "Visual Style", visual_style).noundo().doc_only().params(
        "style: shaded|shaded_edges|hidden_edges|wireframe. A view setting: it does not change the design",
    ),
    CommandSpec::new("view.fit", "Fit View", fit).noundo().doc_only().params(
        "camera: {target, yaw, pitch, roll, distance, fov?, up?}; aspect: number > 0 (width ÷ height); selection?: bool (frame selected bodies instead of the model). Returns {camera, changed}. An empty scene returns the camera unchanged and changed:false",
    ),
    CommandSpec::new("view.orient", "Orient View", orient_cmd).noundo().doc_only().params(
        "direction: [x, y, z] each −1, 0 or 1, not all zero; camera: {target, yaw, pitch, roll, distance, fov?, up?}. Looks from that diagonal with Z kept up",
    ),
];

fn top(s: &mut Session, p: &Value) -> Result<Value> {
    standard("view.standard.top", "top", s, p)
}

fn front(s: &mut Session, p: &Value) -> Result<Value> {
    standard("view.standard.front", "front", s, p)
}

fn right(s: &mut Session, p: &Value) -> Result<Value> {
    standard("view.standard.right", "right", s, p)
}

fn home(s: &mut Session, p: &Value) -> Result<Value> {
    standard("view.standard.home", "home", s, p)
}

fn isometric(s: &mut Session, p: &Value) -> Result<Value> {
    standard("view.standard.isometric", "isometric", s, p)
}

fn orient_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let _ = s;
    let cmd = "view.orient";
    let coords = p.get("direction").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`direction` must be [x, y, z]"))?;
    if coords.len() != 3 {
        return Err(bad(cmd, "`direction` must be [x, y, z]"));
    }
    let mut dir = [0i8; 3];
    for (i, c) in coords.iter().enumerate() {
        let n = c.as_i64().or_else(|| c.as_f64().filter(|x| x.fract() == 0.0).map(|x| x as i64));
        let Some(n) = n.filter(|n| (-1..=1).contains(n)) else {
            return Err(bad(cmd, "a direction component must be -1, 0 or 1"));
        };
        if let Some(slot) = dir.get_mut(i) {
            *slot = n as i8;
        }
    }
    let eye = solidcraft_render::eye_direction(dir).ok_or_else(|| bad(cmd, "the direction must not be zero"))?;
    let cam = camera_of(cmd, p)?;
    let oriented = cam.looking_from(eye);
    if !oriented.is_valid() {
        return Err(bad(cmd, "the view produced an invalid camera"));
    }
    Ok(json!({"camera": oriented}))
}

fn standard(cmd: &str, name: &str, s: &mut Session, p: &Value) -> Result<Value> {
    let _ = s;
    let cam = camera_of(cmd, p)?;
    let oriented = orient(cam, name);
    if !oriented.is_valid() {
        return Err(bad(cmd, "the view produced an invalid camera"));
    }
    Ok(json!({"camera": oriented}))
}

/// Apply a named orientation. An unknown name leaves the camera where it is.
pub fn orient(mut cam: Camera, name: &str) -> Camera {
    let Some(view) = StandardView::parse(name) else { return cam };
    let target = cam.target;
    let distance = cam.distance;
    cam.set_view(view);
    cam.target = target;
    cam.distance = distance;
    cam
}

fn component_colors(s: &mut Session, p: &Value) -> Result<Value> {
    s.component_colors = p.get("enabled").and_then(Value::as_bool).unwrap_or(!s.component_colors);
    Ok(json!({"enabled": s.component_colors}))
}

fn visual_style(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.visual_style";
    let style = p.get("style").and_then(Value::as_str).unwrap_or("shaded_edges");
    s.visual_style = match style {
        "shaded_edges" => 0,
        "shaded" => 1,
        "wireframe" => 2,
        "hidden_edges" => 3,
        other => return Err(bad(cmd, format!("unknown visual style `{other}`"))),
    };
    Ok(json!({"style": style}))
}

fn select_mode(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.select_mode";
    let mode = p.get("mode").and_then(Value::as_str).unwrap_or("box");
    s.select_mode = match mode {
        "box" | "window" => 0,
        "lasso" | "freeform" => 1,
        "paint" => 2,
        other => return Err(bad(cmd, format!("unknown selection mode `{other}` (box, lasso or paint)"))),
    };
    Ok(json!({"mode": mode}))
}

fn fit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.fit";
    let mut cam = camera_of(cmd, p)?;
    let aspect = p
        .get("aspect")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n > 0.0)
        .ok_or_else(|| bad(cmd, "`aspect` must be a positive number"))?;
    let selection = p.get("selection").and_then(Value::as_bool).unwrap_or(false);
    let bounds = if selection { selection_bounds(s) } else { crate::view::bounds(s) };
    if bounds.is_empty() {
        return Ok(json!({"camera": cam, "changed": false}));
    }
    cam.fit(&bounds);
    if cam.fov <= 0.0 && aspect > 1.0 {
        cam.distance *= aspect;
    }
    if !cam.is_valid() {
        return Err(bad(cmd, "the fit produced an invalid camera"));
    }
    Ok(json!({"camera": cam, "changed": true}))
}

fn camera_of(cmd: &str, p: &Value) -> Result<Camera> {
    let raw = p.get("camera").ok_or_else(|| bad(cmd, "`camera` is required"))?;
    let cam: Camera = serde_json::from_value(raw.clone()).map_err(|_| bad(cmd, "`camera` is not a camera"))?;
    if !cam.is_valid() {
        return Err(bad(cmd, "`camera` must be finite with a positive distance"));
    }
    Ok(cam)
}

fn selection_bounds(s: &Session) -> Aabb3 {
    let state = s.world_state();
    let mut bounds = Aabb3::EMPTY;
    for item in &s.selection {
        if let Sel::Body { name } = item
            && let Some(body) = state.body(name)
        {
            bounds = bounds.union(&body.mesh().bounds());
        }
    }
    bounds
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_render::{Camera, StandardView};

    use super::orient;
    use crate::Session;

    fn camera_json(cam: &Camera) -> serde_json::Value {
        serde_json::to_value(cam).expect("camera json")
    }

    /// Feature: ux-ui-shell-improvements, Property 18: A standard view applies its orientation and rejects unknown names
    #[test]
    fn property_18_standard_views_apply_their_orientation_and_unknown_names_do_not() {
        let names = ["top", "front", "right", "home", "isometric"];
        let ids = ["view.standard.top", "view.standard.front", "view.standard.right", "view.standard.home", "view.standard.isometric"];
        let mut cases = 0usize;
        for n in 0..120 {
            let mut cam = Camera::default();
            cam.target.x = n as f64;
            cam.distance = 80.0 + (n % 17) as f64;
            cam.yaw = 0.2;
            cam.pitch = -0.2;
            let name = names[n % names.len()];
            let kept = orient(cam, name);
            let parsed = StandardView::parse(name).expect("known view");
            let (yaw, pitch) = parsed.angles();
            assert!((kept.yaw - yaw).abs() < 1e-6 && (kept.pitch - pitch).abs() < 1e-4, "{name}");
            assert_eq!(kept.target, cam.target);
            assert_eq!(kept.distance, cam.distance);
            assert!(kept.is_valid());
            let unknown = orient(cam, &format!("diagonal-{n}"));
            assert_eq!(unknown.yaw, cam.yaw);
            assert_eq!(unknown.pitch, cam.pitch);
            let mut s = Session::default();
            let out = s.execute(ids[n % ids.len()], &json!({"camera": camera_json(&cam)})).expect("view");
            let back: Camera = serde_json::from_value(out["camera"].clone()).expect("returned camera");
            assert!(back.is_valid());
            assert_eq!(back.target, cam.target);
            cases += 1;
        }
        let mut s = Session::default();
        let before = s.doc.to_json();
        assert!(s.execute("view.standard.top", &json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 0}})).is_err());
        assert!(s.execute("view.fit", &json!({"camera": camera_json(&Camera::default()), "aspect": 0})).is_err());
        assert_eq!(s.doc.to_json(), before);
        assert!(cases >= 100, "property ran {cases} times");
    }

    /// Feature: ux-ui-shell-improvements, Property 19: Fit frames a non-empty model and leaves an empty one unchanged
    #[test]
    fn property_19_fit_frames_a_model_and_leaves_an_empty_one() {
        let mut cases = 0usize;
        for n in 1..=100 {
            let mut empty = Session::default();
            let cam = Camera { distance: 40.0 + f64::from(n), ..Camera::default() };
            let aspect = if n % 2 == 0 { 1.6 } else { 0.8 };
            let out = empty.execute("view.fit", &json!({"camera": camera_json(&cam), "aspect": aspect})).expect("fit empty");
            assert_eq!(out["changed"], json!(false));
            let back: Camera = serde_json::from_value(out["camera"].clone()).expect("camera");
            assert_eq!(back.distance, cam.distance);
            assert_eq!(back.target, cam.target);

            let mut solid = Session::default();
            solid.execute("solid.box", &json!({"length": n, "width": 10, "height": 8, "body_name": "Block"})).expect("box");
            let fitted = solid.execute("view.fit", &json!({"camera": camera_json(&cam), "aspect": aspect, "selection": false})).expect("fit");
            assert_eq!(fitted["changed"], json!(true));
            let next: Camera = serde_json::from_value(fitted["camera"].clone()).expect("camera");
            assert!(next.is_valid());
            assert!(next.distance > 0.0);
            cases += 1;
        }
        assert!(cases >= 100, "property ran {cases} times");
    }

    /// Feature: fusion360-feature-parity, Property 18: View orientation preserves Z-up
    #[test]
    fn property_18_diagonal_views_keep_z_up_and_do_not_edit_the_design() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        let cam = Camera::default();
        let mut cases = 0usize;
        for dir in solidcraft_render::view_directions() {
            let out = s.execute("view.orient", &json!({"direction": dir, "camera": camera_json(&cam)})).unwrap_or_else(|e| panic!("{dir:?}: {e}"));
            let back: Camera = serde_json::from_value(out["camera"].clone()).expect("camera");
            assert!(back.is_valid());
            assert_eq!(back.roll, 0.0);
            assert_eq!(back.up, solidcraft_render::UpAxis::Z);
            assert_eq!(back.target, cam.target);
            let eye = back.back();
            let want = solidcraft_render::eye_direction(dir).expect("dir");
            assert!(eye.dist(want) < 1e-6, "{dir:?} eye {eye:?} want {want:?}");
            cases += 1;
        }
        assert!(s.execute("view.orient", &json!({"direction": [0, 0, 0], "camera": camera_json(&cam)})).is_err());
        assert!(s.execute("view.orient", &json!({"direction": [2, 0, 0], "camera": camera_json(&cam)})).is_err());
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(cases, 26);
    }
}
