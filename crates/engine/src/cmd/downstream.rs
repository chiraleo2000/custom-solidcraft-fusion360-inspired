//! Later-workspace commands: machining setups, static stress, render, animation,
//! schematic and PCB checks, and a generative candidate. Each one is a SolidCraft command.
//! PCB autorouting stays deferred.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::motion::ExplodedView;
use solidcraft_doc::{
    Board, CamSetup, CamTool, EcadComponent, EcadNet, EcadTrace, GenerativeStudy, RenderScene, Schematic, StoryKey, Storyboard, Study,
    StudyConstraint, StudyLoad,
};
use solidcraft_geom::fea::{self, BeamEnd, BeamLoad, BeamSection, BeamSupport};
use solidcraft_geom::generative::{self, Objective};
use solidcraft_geom::{Mesh, Vec3};
use solidcraft_render::Quat;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.setup", "Machining Setup", cam_setup)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("name?; origin?: [x,y,z]; x_axis?, z_axis?: [x,y,z]; stock: [x,y,z] mm; bodies?: [names]"),
    CommandSpec::new("cam.tool", "Tool", cam_tool)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("setup?: name; name?; kind?: flat|ball|drill|thread|turn; diameter, flute?: mm; corner?: mm"),
    CommandSpec::new("cam.pocket", "2D Pocket", cam_pocket)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("sketch; tool_diameter; depth; stepdown?; stepover?. The tool centre stays a radius inside the profile and the cut is stepped"),
    CommandSpec::new("cam.face", "Face", cam_face)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("tool_diameter: mm. A pass over the latest setup's stock"),
    CommandSpec::new("cam.drill", "Drill", cam_drill)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("body; diameter; depth; stepdown?; at?: [x,y]. Peck steps down the hole"),
    CommandSpec::new("cam.adaptive", "3D Adaptive", cam_adaptive)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("body: name; tool_diameter: mm. A spiral of tool centres from outside the part in to the offset surface"),
    CommandSpec::new("cam.post", "Post Process", cam_post)
        .noundo()
        .params("units?: mm|inch; work?: G54..G59; safe_z?; name?. SolidCraft G-code text"),
    CommandSpec::new("cam.simulate", "Simulate Stock", cam_simulate).noundo().params("measured removed volume and the stock volume that remains"),
    CommandSpec::new("cam.slot", "Slot", cam_slot)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("sketch; tool_diameter; depth. The path stays a tool radius inside the slot"),
    CommandSpec::new("cam.trace", "Trace", cam_trace).at("UTILITIES", "MAKE").icon("extrude").params("sketch; tool_diameter; depth"),
    CommandSpec::new("cam.engrave", "Engrave", cam_engrave).at("UTILITIES", "MAKE").icon("extrude").params("sketch; tool_diameter; depth"),
    CommandSpec::new("cam.bore", "Bore", cam_bore)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("diameter; tool_diameter; depth. The path radius is the hole radius minus the tool radius"),
    CommandSpec::new("cam.thread_mill", "Thread Mill", cam_thread)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("diameter; tool_diameter; depth; pitch?"),
    CommandSpec::new("cam.chamfer_2d", "2D Chamfer", cam_chamfer).at("UTILITIES", "MAKE").icon("extrude").params("sketch; tool_diameter; depth"),
    CommandSpec::new("cam.recognize_holes", "Recognize Holes", cam_holes)
        .params("body: name. Closed cylindrical holes become drill jobs; the solid is not cut"),
    CommandSpec::new("cam.parallel", "Parallel", cam_parallel)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("body; tool_diameter; stepover?. Ball-nose passes on the upper surface"),
    CommandSpec::new("cam.scallop", "Scallop", cam_scallop)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("body; tool_diameter; stepover? or cusp?. Pass spacing comes from the cusp when stepover is omitted"),
    CommandSpec::new("cam.pencil", "Pencil", cam_pencil).at("UTILITIES", "MAKE").icon("extrude").params("not supported yet"),
    CommandSpec::new("cam.contour_3d", "3D Contour", cam_contour3d).at("UTILITIES", "MAKE").icon("extrude").params("not supported yet"),
    CommandSpec::new("cam.horizontal", "Horizontal", cam_horizontal).at("UTILITIES", "MAKE").icon("extrude").params("not supported yet"),
    CommandSpec::new("cam.flow", "Flow", cam_flow).at("UTILITIES", "MAKE").icon("extrude").params("not supported yet"),
    CommandSpec::new("cam.radial", "Radial", cam_radial).at("UTILITIES", "MAKE").icon("extrude").params("not supported yet"),
    CommandSpec::new("cam.spiral", "Spiral", cam_spiral).at("UTILITIES", "MAKE").icon("extrude").params("not supported yet"),
    CommandSpec::new("cam.turn.profile", "Turn Profile", cam_turn_profile)
        .at("UTILITIES", "MAKE")
        .icon("revolve")
        .params("tool_diameter. A lathe pass along the stock Z axis"),
    CommandSpec::new("cam.turn.groove", "Turn Groove", cam_turn_groove).at("UTILITIES", "MAKE").icon("revolve").params("tool_diameter; width; depth"),
    CommandSpec::new("cam.turn.thread", "Turn Thread", cam_turn_thread).at("UTILITIES", "MAKE").icon("revolve").params("tool_diameter; pitch; depth"),
    CommandSpec::new("cam.probe", "Probe", cam_probe).noundo().params("body: name. Returns points and does not write a toolpath"),
    CommandSpec::new("cam.additive", "Additive", cam_additive).at("UTILITIES", "MAKE").icon("extrude").params("body; layer?: mm"),
    CommandSpec::new("cam.nest", "Nest", cam_nest)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("sheet: [width, height]; profiles: [[w, h], …]. Overlap is an error"),
    CommandSpec::new("sim.study", "Simulation Study", sim_study).params(
        "name?; kind?: static; body: name; material?; modulus?: N/mm^2; force?: N; constraints?: [{kind: fixed|pin, target: min_x|max_x}]; loads?: [{kind: force, magnitude, direction?: [x,y,z], target?}]. Changing inputs marks results stale",
    ),
    CommandSpec::new("sim.static", "Static Stress", sim_static).params(
        "body: an axis-aligned box; force?: N along +Z at max X (default 100); modulus?: N/mm^2 (default 200000, or the material); material?; constraints?: default clamp at min X. Solves with solidcraft-beam-stiffness and reports min/max. The solid is unchanged",
    ),
    CommandSpec::new("sim.solve", "Solve Study", sim_solve).params(
        "name?: study; elements?: 1..32. A static axis-aligned box only. Other kinds, a free body, or a study over the size cap are refused. The solid is unchanged",
    ),
    CommandSpec::new("sim.modal", "Modal Study", sim_modal).params("body: name. Solving a modal study is not supported yet"),
    CommandSpec::new("sim.thermal", "Thermal Study", sim_thermal).params("body: name. Solving a thermal study is not supported yet"),
    CommandSpec::new("sim.thermal_stress", "Thermal Stress", sim_thermal_stress).params("body: name. Solving a thermal-stress study is not supported yet"),
    CommandSpec::new("sim.buckling", "Buckling Study", sim_buckling).params("body: name. Solving a buckling study is not supported yet"),
    CommandSpec::new("sim.nonlinear", "Nonlinear Study", sim_nonlinear).params("body: name. Solving a nonlinear study is not supported yet"),
    CommandSpec::new("sim.event", "Event Study", sim_event).params("body: name. Solving an event study is not supported yet"),
    CommandSpec::new("sim.shape", "Shape Study", sim_shape).params("body: name. Solving a shape study is not supported yet"),
    CommandSpec::new("sim.injection", "Injection Study", sim_injection).params("body: name. Solving an injection study is not supported yet"),
    CommandSpec::new("sim.cooling", "Cooling Study", sim_cooling).params("body: name. Solving a cooling study is not supported yet"),
    CommandSpec::new("render.environment", "Render Environment", render_environment)
        .noundo()
        .params("name?|environment?: studio|neutral|white|warm|cool; asset?: name; color?: [r,g,b] 0…1; intensity?: number. An unknown name falls back to white and is reported. No cloud renderer"),
    CommandSpec::new("render.camera", "Render Camera", render_camera).noundo().params("focal?: millimetres, positive"),
    CommandSpec::new("render.image", "Render Image", render_image).noundo().params(
        "path: png; environment?|asset?: studio name; appearance?: library name. Metalness and roughness shade the pixels. A missing name falls back and is reported. The design is not changed. A cloud renderer is not supported yet",
    ),
    CommandSpec::new("anim.storyboard", "Storyboard", anim_storyboard).params(
        "name?; view?: exploded view; study?: motion study; auto?: bool; scale?: number. Copies explode or motion keys when they exist. An empty assembly is an error. Placements stay unchanged",
    ),
    CommandSpec::new("anim.key", "Animation Key", anim_key)
        .params("story?: name; time: seconds; occurrence: id or name; translate?: [x,y,z]; rotate?: [x,y,z,w]. The assembly is not moved"),
    CommandSpec::new("anim.play", "Play Storyboard", anim_play)
        .noundo()
        .params("story?: name; time?: seconds. Interpolates preview offsets. Placements stay unchanged"),
    CommandSpec::new("anim.publish", "Publish Storyboard", anim_publish).noundo().params(
        "path: png; frames?: 1…24; story?: name. One image, or stem_000.png … when there are several frames. Video files are not supported yet. Placements stay unchanged",
    ),
    CommandSpec::new("ecad.schematic", "Schematic", ecad_schematic).params(
        "parts?: [refdes | {refdes, pins?, x?, y?}]; nets?: [{name, nodes}]. Stores components and nets. Part libraries are not supported yet. PCB autorouting is not supported yet",
    ),
    CommandSpec::new("ecad.wire", "Wire", ecad_wire).params("a, b: pin or net names. Joins them into a stored net. Does not route copper"),
    CommandSpec::new("ecad.erc", "Electrical Rules", ecad_erc).noundo().params(
        "Reports shorts between a power name and a ground name, unconnected parts, and open pins. Does not modify the schematic",
    ),
    CommandSpec::new("ecad.board", "Board", ecad_board).params("width, height: mm. One copper layer. PCB autorouting is not supported yet"),
    CommandSpec::new("ecad.trace", "Trace", ecad_trace).params("x0, y0, x1, y1, width: mm; net?: name. Places copper; it does not autoroute"),
    CommandSpec::new("ecad.drc", "Design Rules", ecad_drc).noundo().params(
        "clearance?: mm (default 0.2, copper edge gap); min_width?: mm (default 0.15). Reports clearance and width violations and does not move copper. Same-net copper may touch",
    ),
    CommandSpec::new("ecad.push_3d", "Push Board to 3D", ecad_push).params(
        "thickness?: mm (default 1.6). Creates a PCB component and a board body in the assembly. Component models and PCB autorouting are not supported yet",
    ),
    CommandSpec::new("ecad.export", "Export Gerber", ecad_export).noundo().params(
        "path: Gerber RS-274X subset (public Ucamco Gerber Layer Format), millimetres, outline plus one copper layer. Read back with the same subset. PCB autorouting is not supported yet",
    ),
    CommandSpec::new("drawing.export_dxf", "Export Drawing DXF", drawing_dxf).noundo().params("path: ASCII DXF of the sheet's projected model edges"),
    CommandSpec::new("generative.study", "Generative Study", gen_study).params(
        "name?; preserve: [body names]; obstacle?: [names]; loads?: [{kind, magnitude, direction, target}]; constraints?: [{kind, target}]; objective?: stiffness|mass. Changing inputs marks the study stale",
    ),
    CommandSpec::new("generative.solve", "Generate", gen_solve).params(
        "name?: study. Records a candidate box with method solidcraft-section-reduction. Mass keeps 60% of the width. The solid is unchanged",
    ),
    CommandSpec::new("generative.promote", "Promote Candidate", gen_promote)
        .params("name?: study. Inserts the candidate as an editable box body"),
    CommandSpec::new("drawing.parts_list", "Parts List", parts_list).noundo().params("rows are components, not loose bodies"),
    CommandSpec::new("drawing.detail", "Detail View", drawing_detail)
        .at("DRAWING", "CREATE")
        .icon("sketch")
        .params("sheet?; scale: number > 0; origin?: [x,y]"),
    CommandSpec::new("drawing.title_block", "Title Block", title_block).params("sheet?; title?: text; size?: A4|A3"),
];

fn finite(cmd: &str, n: f64, what: &str) -> Result<f64> {
    if n.is_finite() && n.abs() < 1e9 { Ok(n) } else { Err(bad(cmd, format!("`{what}` must be finite"))) }
}

fn axis_of(p: &Value, key: &str, cmd: &str, default: [f64; 3]) -> Result<[f64; 3]> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => {
            let q = crate::params::vec3(v).ok_or_else(|| bad(cmd, format!("`{key}` must be [x, y, z]")))?;
            Ok([q.x, q.y, q.z])
        }
    }
}

fn cam_setup(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.setup";
    if s.doc.cam_setups.len() >= 100 {
        return Err(bad(cmd, "too many setups"));
    }
    let stock = p.get("stock").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`stock` is [x,y,z]"))?;
    if stock.len() != 3 {
        return Err(bad(cmd, "`stock` is [x,y,z]"));
    }
    let mut size = [0.0; 3];
    for (i, slot) in size.iter_mut().enumerate() {
        let n = stock.get(i).and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`stock` is [x,y,z]"))?;
        *slot = finite(cmd, n, "stock")?;
        if *slot <= 0.0 {
            return Err(bad(cmd, "stock size must be positive"));
        }
    }
    let origin = match p.get("origin") {
        None => [0.0; 3],
        Some(v) => {
            let q = crate::params::vec3(v).ok_or_else(|| bad(cmd, "`origin` must be [x, y, z]"))?;
            [q.x, q.y, q.z]
        }
    };
    let x_axis = axis_of(p, "x_axis", cmd, [1.0, 0.0, 0.0])?;
    let z_axis = axis_of(p, "z_axis", cmd, [0.0, 0.0, 1.0])?;
    let frame = solidcraft_geom::toolpath::WorkCoords::from_xz(
        solidcraft_geom::Vec3::new(origin[0], origin[1], origin[2]),
        solidcraft_geom::Vec3::new(x_axis[0], x_axis[1], x_axis[2]),
        solidcraft_geom::Vec3::new(z_axis[0], z_axis[1], z_axis[2]),
    )
    .map_err(|e| bad(cmd, e))?;
    let name = str_(p, "name").unwrap_or("Setup");
    if name.is_empty() || name.len() > 200 {
        return Err(bad(cmd, "the setup name is not usable"));
    }
    let bodies = match p.get("bodies") {
        None => Vec::new(),
        Some(Value::String(one)) => {
            if one.is_empty() || one.len() > 200 {
                return Err(bad(cmd, "a body name is not usable"));
            }
            if s.model.state().body(one).is_none() {
                return Err(bad(cmd, format!("no body `{one}`")));
            }
            vec![one.clone()]
        }
        Some(Value::Array(items)) => {
            if items.len() > 100 {
                return Err(bad(cmd, "too many bodies"));
            }
            let mut names = Vec::with_capacity(items.len());
            for item in items {
                let Some(body) = item.as_str() else {
                    return Err(bad(cmd, "`bodies` must be a list of names"));
                };
                if body.is_empty() || body.len() > 200 {
                    return Err(bad(cmd, "a body name is not usable"));
                }
                if s.model.state().body(body).is_none() {
                    return Err(bad(cmd, format!("no body `{body}`")));
                }
                names.push(body.to_string());
            }
            names
        }
        Some(_) => return Err(bad(cmd, "`bodies` must be a list of names")),
    };
    let name = name.to_string();
    s.doc_mut().cam_setups.push(CamSetup {
        name: name.clone(),
        origin,
        x_axis: [frame.x.x, frame.x.y, frame.x.z],
        z_axis: [frame.z.x, frame.z.y, frame.z.z],
        stock: size,
        bodies,
        tools: Vec::new(),
    });
    Ok(json!({"setup": name}))
}

fn cam_tool(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.tool";
    let diameter = finite(cmd, p.get("diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`diameter` is required"))?, "diameter")?;
    if diameter <= 0.0 {
        return Err(bad(cmd, "diameter must be positive"));
    }
    let kind = str_(p, "kind").unwrap_or("flat").to_ascii_lowercase();
    if !matches!(kind.as_str(), "flat" | "ball" | "drill" | "thread" | "turn") {
        return Err(bad(cmd, "kind must be flat, ball, drill, thread or turn"));
    }
    let tool = CamTool {
        name: str_(p, "name").unwrap_or("Tool").to_string(),
        kind,
        diameter,
        flute: p.get("flute").and_then(Value::as_f64).unwrap_or(diameter * 3.0),
        corner: p.get("corner").and_then(Value::as_f64).unwrap_or(0.0),
    };
    if !(tool.flute.is_finite() && tool.corner.is_finite() && tool.flute >= 0.0 && tool.corner >= 0.0) {
        return Err(bad(cmd, "flute and corner must be finite"));
    }
    let name = str_(p, "setup");
    let setups = &mut s.doc_mut().cam_setups;
    let setup = setups.iter_mut().rev().find(|setup| name.is_none_or(|n| setup.name == n)).ok_or_else(|| bad(cmd, "create a setup first"))?;
    setup.tools.push(tool);
    Ok(json!({"tool": setup.tools.last().map(|t| t.name.clone())}))
}

fn cam_pocket(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::pocket(s, p)
}

fn push_cam(s: &mut Session, cmd: &str, name: &str, kind: &str, tool: f64, depth: f64, area: f64, gcode: String) -> Result<()> {
    super::cam::remember_job(s, cmd, name, "", kind, tool, depth, area, gcode)
}

fn cam_face(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.face";
    let tool = finite(cmd, p.get("tool_diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`tool_diameter` is required"))?, "tool_diameter")?;
    if tool <= 0.0 {
        return Err(bad(cmd, "tool diameter must be positive"));
    }
    let setup = s.doc.cam_setups.last().ok_or_else(|| bad(cmd, "create a setup first"))?;
    let (x, y) = (setup.stock[0], setup.stock[1]);
    let gcode = format!("G0 X0 Y0\nG1 X{x:.3} Y{y:.3} F300\n");
    push_cam(s, cmd, "Face", "face", tool, 0.0, 0.0, gcode)?;
    Ok(json!({"stock": [x, y]}))
}

fn cam_drill(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::drill(s, p)
}

fn cam_adaptive(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.adaptive";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?;
    let solid = s.model.state().body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?.body.clone();
    let tool = finite(cmd, p.get("tool_diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`tool_diameter` is required"))?, "tool_diameter")?;
    if tool <= 0.0 {
        return Err(bad(cmd, "tool diameter must be positive"));
    }
    let measured = solidcraft_kernel::measure(&solid).map_err(|e| bad(cmd, e.to_string()))?;
    let centre = (measured.bbox.min + measured.bbox.max) * 0.5;
    let half = (measured.bbox.max - measured.bbox.min) * 0.5;
    let radius = tool * 0.5;
    let inner = (half.x.min(half.y).min(half.z) - radius).max(radius);
    let outer = half.len() + radius;
    if !(outer.is_finite() && inner.is_finite() && outer + 1e-6 >= inner) {
        return Err(bad(cmd, "the body has no room for the tool"));
    }
    let steps = 36usize;
    let turns = 3.0;
    let mut samples = Vec::with_capacity(steps);
    let mut gcode = String::new();
    for i in 0..steps {
        let t = i as f64 / (steps - 1) as f64;
        let ang = t * turns * std::f64::consts::TAU;
        let rad = outer * (1.0 - t) + inner * t;
        let z = centre.z + (half.z - radius) * (1.0 - 2.0 * t);
        let mut point = centre + solidcraft_geom::Vec3::new(rad * ang.cos(), rad * ang.sin(), z);
        let away = point.dist(centre);
        if away + 1e-9 < inner {
            point = centre + (point - centre) * (inner / away.max(1e-9));
        }
        if point.dist(centre) + 1e-6 < inner {
            return Err(bad(cmd, "the toolpath entered the offset body"));
        }
        samples.push([point.x, point.y, point.z]);
        gcode.push_str(&format!("G1 X{:.3} Y{:.3} Z{:.3}\n", point.x, point.y, point.z));
    }
    let depth = (half.z * 2.0).max(tool);
    push_cam(s, cmd, "Adaptive", "adaptive", tool, depth, 0.0, gcode)?;
    Ok(json!({"samples": samples}))
}

fn cam_post(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::post(s, p)
}

fn tool_depth(cmd: &str, p: &Value) -> Result<(f64, f64)> {
    let tool = finite(cmd, p.get("tool_diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`tool_diameter` is required"))?, "tool_diameter")?;
    let depth = finite(cmd, p.get("depth").and_then(Value::as_f64).unwrap_or(1.0), "depth")?;
    if tool <= 0.0 || depth <= 0.0 {
        return Err(bad(cmd, "tool diameter and depth must be positive"));
    }
    Ok((tool, depth))
}

fn sketch_bounds(s: &Session, sketch: &str, cmd: &str) -> Result<(f64, f64, f64, f64)> {
    let state = s.model.state();
    let solved = state.sketches.iter().find(|item| item.name == sketch).ok_or_else(|| bad(cmd, format!("no sketch `{sketch}`")))?;
    let profile = solved.profiles.first().ok_or_else(|| bad(cmd, "the sketch needs one closed profile"))?;
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    for point in profile.region.outer.polyline(0.05) {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    Ok((min_x, min_y, max_x, max_y))
}

fn inset_path(cmd: &str, bounds: (f64, f64, f64, f64), tool: f64, depth: f64) -> Result<(f64, String)> {
    let (min_x, min_y, max_x, max_y) = bounds;
    let radius = tool * 0.5;
    if max_x - min_x <= tool || max_y - min_y <= tool {
        return Err(bad(cmd, "the tool does not fit inside the profile"));
    }
    let (x0, y0, x1, y1) = (min_x + radius, min_y + radius, max_x - radius, max_y - radius);
    let area = (x1 - x0) * (y1 - y0);
    let gcode = format!("G0 X{x0:.3} Y{y0:.3}\nG1 Z-{depth:.3}\nG1 X{x1:.3} Y{y1:.3}\n");
    Ok((area, gcode))
}

fn cam_slot(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.slot";
    let sketch = str_(p, "sketch").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`sketch` is required"))?.to_string();
    let (tool, depth) = tool_depth(cmd, p)?;
    let (area, gcode) = inset_path(cmd, sketch_bounds(s, &sketch, cmd)?, tool, depth)?;
    super::cam::remember_job(s, cmd, "Slot", &sketch, "slot", tool, depth, area, gcode)?;
    Ok(json!({"area": area}))
}

fn cam_trace(s: &mut Session, p: &Value) -> Result<Value> {
    traced(s, p, "cam.trace", "trace", "Trace")
}

fn cam_engrave(s: &mut Session, p: &Value) -> Result<Value> {
    traced(s, p, "cam.engrave", "engrave", "Engrave")
}

fn traced(s: &mut Session, p: &Value, cmd: &str, kind: &str, name: &str) -> Result<Value> {
    let sketch = str_(p, "sketch").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`sketch` is required"))?.to_string();
    let (tool, depth) = tool_depth(cmd, p)?;
    let (min_x, min_y, max_x, max_y) = sketch_bounds(s, &sketch, cmd)?;
    let gcode = format!("G1 X{min_x:.3} Y{min_y:.3}\nG1 X{max_x:.3} Y{max_y:.3} Z-{depth:.3}\n");
    let area = (max_x - min_x) * (max_y - min_y);
    super::cam::remember_job(s, cmd, name, &sketch, kind, tool, depth, area, gcode)?;
    Ok(json!({"area": area}))
}

fn cam_bore(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.bore";
    let (tool, depth) = tool_depth(cmd, p)?;
    let hole = finite(cmd, p.get("diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`diameter` is required"))?, "diameter")?;
    let path_r = hole * 0.5 - tool * 0.5;
    if path_r <= 0.0 {
        return Err(bad(cmd, "the tool does not fit in the hole"));
    }
    let gcode = format!("G1 X{path_r:.3} Y0 Z-{depth:.3}\nG2 I-{path_r:.3}\n");
    push_cam(s, cmd, "Bore", "bore", tool, depth, std::f64::consts::PI * path_r * path_r, gcode)?;
    Ok(json!({"radius": path_r}))
}

fn cam_thread(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.thread_mill";
    let (tool, depth) = tool_depth(cmd, p)?;
    let hole = finite(cmd, p.get("diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`diameter` is required"))?, "diameter")?;
    let pitch = p.get("pitch").and_then(Value::as_f64).unwrap_or(1.0);
    let pitch = finite(cmd, pitch, "pitch")?;
    if pitch <= 0.0 {
        return Err(bad(cmd, "pitch must be positive"));
    }
    let radius = hole * 0.5 - tool * 0.5;
    if radius <= 0.0 {
        return Err(bad(cmd, "the tool does not fit in the hole"));
    }
    let mut gcode = String::new();
    let turns = (depth / pitch).clamp(1.0, 100.0);
    for i in 0..=8 {
        let t = i as f64 / 8.0;
        let ang = t * turns * std::f64::consts::TAU;
        gcode.push_str(&format!("G1 X{:.3} Y{:.3} Z-{:.3}\n", radius * ang.cos(), radius * ang.sin(), depth * t));
    }
    push_cam(s, cmd, "Thread", "thread_mill", tool, depth, 0.0, gcode)?;
    Ok(json!({"radius": radius}))
}

fn cam_chamfer(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.chamfer_2d";
    let sketch = str_(p, "sketch").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`sketch` is required"))?.to_string();
    let (tool, depth) = tool_depth(cmd, p)?;
    let (area, gcode) = inset_path(cmd, sketch_bounds(s, &sketch, cmd)?, tool, depth)?;
    super::cam::remember_job(s, cmd, "Chamfer", &sketch, "chamfer_2d", tool, depth, area, gcode)?;
    Ok(json!({"area": area}))
}

fn cam_holes(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::holes(s, p)
}

fn cam_parallel(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::surface(s, p, "cam.parallel", "parallel", false)
}
fn cam_scallop(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::surface(s, p, "cam.scallop", "scallop", true)
}
fn cam_pencil(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::not_yet(s, p, "cam.pencil")
}
fn cam_contour3d(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::not_yet(s, p, "cam.contour_3d")
}
fn cam_horizontal(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::not_yet(s, p, "cam.horizontal")
}
fn cam_flow(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::not_yet(s, p, "cam.flow")
}
fn cam_radial(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::not_yet(s, p, "cam.radial")
}
fn cam_spiral(s: &mut Session, p: &Value) -> Result<Value> {
    super::cam::not_yet(s, p, "cam.spiral")
}

fn cam_turn_profile(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.turn.profile";
    let tool = finite(cmd, p.get("tool_diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`tool_diameter` is required"))?, "tool_diameter")?;
    let setup = s.doc.cam_setups.last().ok_or_else(|| bad(cmd, "create a setup first"))?;
    let length = setup.stock[2];
    let radius = setup.stock[0].min(setup.stock[1]) * 0.5;
    if tool <= 0.0 || length <= 0.0 {
        return Err(bad(cmd, "the lathe setup needs stock along Z"));
    }
    let gcode = format!("G1 X{:.3} Z0\nG1 Z-{:.3}\n", radius, length);
    push_cam(s, cmd, "Turn", "turn_profile", tool, length, 0.0, gcode)?;
    Ok(json!({"length": length}))
}

fn cam_turn_groove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.turn.groove";
    let tool = finite(cmd, p.get("tool_diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`tool_diameter` is required"))?, "tool_diameter")?;
    let width = finite(cmd, p.get("width").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`width` is required"))?, "width")?;
    let depth = finite(cmd, p.get("depth").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`depth` is required"))?, "depth")?;
    if tool <= 0.0 || width <= 0.0 || depth <= 0.0 || s.doc.cam_setups.is_empty() {
        return Err(bad(cmd, "a groove needs a lathe setup, width and depth"));
    }
    push_cam(s, cmd, "Groove", "turn_groove", tool, depth, width * depth, format!("G1 X-{depth:.3}\nG1 Z-{width:.3}\n"))?;
    Ok(json!({"width": width, "depth": depth}))
}

fn cam_turn_thread(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.turn.thread";
    let tool = finite(cmd, p.get("tool_diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`tool_diameter` is required"))?, "tool_diameter")?;
    let pitch = finite(cmd, p.get("pitch").and_then(Value::as_f64).unwrap_or(1.5), "pitch")?;
    let depth = finite(cmd, p.get("depth").and_then(Value::as_f64).unwrap_or(pitch), "depth")?;
    if tool <= 0.0 || pitch <= 0.0 || depth <= 0.0 || s.doc.cam_setups.is_empty() {
        return Err(bad(cmd, "a thread needs a lathe setup"));
    }
    push_cam(s, cmd, "Turn thread", "turn_thread", tool, depth, 0.0, format!("G33 Z-{depth:.3} K{pitch:.3}\n"))?;
    Ok(json!({"pitch": pitch}))
}

fn cam_probe(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.probe";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?;
    let solid = s.model.state().body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?.body.clone();
    let before = s.doc.cam.len();
    let measured = solidcraft_kernel::measure(&solid).map_err(|e| bad(cmd, e.to_string()))?;
    let points = [measured.bbox.min, measured.bbox.max];
    if s.doc.cam.len() != before {
        return Err(bad(cmd, "a probe wrote a toolpath"));
    }
    Ok(json!({"points": points, "distance": measured.bbox.min.dist(measured.bbox.max)}))
}

fn cam_additive(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.additive";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?;
    let solid = s.model.state().body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?.body.clone();
    let layer = p.get("layer").and_then(Value::as_f64).unwrap_or(0.2);
    let layer = finite(cmd, layer, "layer")?;
    if layer <= 0.0 || layer > 10.0 {
        return Err(bad(cmd, "layer height must be between 0 and 10 mm"));
    }
    let measured = solidcraft_kernel::measure(&solid).map_err(|e| bad(cmd, e.to_string()))?;
    let height = (measured.bbox.max.z - measured.bbox.min.z).max(layer);
    let layers = ((height / layer).ceil() as usize).clamp(1, 5000);
    let mut gcode = String::new();
    for i in 0..layers {
        let z = measured.bbox.min.z + (i as f64 + 1.0) * layer;
        gcode.push_str(&format!("G1 X{:.3} Y{:.3} Z{:.3}\n", measured.bbox.min.x, measured.bbox.min.y, z));
    }
    push_cam(s, cmd, "Additive", "additive", layer, height, 0.0, gcode)?;
    Ok(json!({"layers": layers}))
}

fn cam_nest(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.nest";
    let sheet = p.get("sheet").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`sheet` is [width, height]"))?;
    let width = finite(cmd, sheet.first().and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`sheet` is [width, height]"))?, "sheet")?;
    let height = finite(cmd, sheet.get(1).and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`sheet` is [width, height]"))?, "sheet")?;
    let profiles = p.get("profiles").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`profiles` must list [width, height]"))?;
    if profiles.is_empty() || profiles.len() > 500 || width <= 0.0 || height <= 0.0 {
        return Err(bad(cmd, "nesting needs a sheet and 1…500 profiles"));
    }
    let mut x = 0.0;
    let mut y = 0.0;
    let mut row = 0.0;
    let mut placed = Vec::new();
    for profile in profiles {
        let w = profile.get(0).and_then(Value::as_f64).ok_or_else(|| bad(cmd, "each profile is [width, height]"))?;
        let h = profile.get(1).and_then(Value::as_f64).ok_or_else(|| bad(cmd, "each profile is [width, height]"))?;
        let (w, h) = (finite(cmd, w, "profile")?, finite(cmd, h, "profile")?);
        if w <= 0.0 || h <= 0.0 {
            return Err(bad(cmd, "profile size must be positive"));
        }
        if x + w > width {
            x = 0.0;
            y += row;
            row = 0.0;
        }
        if y + h > height {
            return Err(bad(cmd, "the profiles overlap the sheet"));
        }
        placed.push([x, y, w, h]);
        x += w;
        row = row.max(h);
    }
    Ok(json!({"placed": placed}))
}

fn gcode_points(gcode: &str) -> Vec<[f64; 3]> {
    let mut cur = [0.0; 3];
    let mut out = Vec::new();
    for line in gcode.lines().take(20_000) {
        let mut saw = false;
        for tok in line.split_whitespace() {
            let mut chars = tok.chars();
            let Some(axis) = chars.next() else { continue };
            let Some(v) = chars.as_str().parse::<f64>().ok().filter(|n| n.is_finite()) else { continue };
            match axis {
                'X' | 'x' => cur[0] = v,
                'Y' | 'y' => cur[1] = v,
                'Z' | 'z' => cur[2] = v,
                _ => continue,
            }
            saw = true;
        }
        if saw {
            out.push(cur);
        }
    }
    out
}

fn path_prism(points: &[[f64; 3]], depth: f64) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y, mut max_z) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN, 0.0_f64);
    for p in points {
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
        max_z = max_z.max(p[2].abs());
    }
    let area = (max_x - min_x).max(0.0) * (max_y - min_y).max(0.0);
    area * depth.max(max_z)
}

fn tool_sweep(points: &[[f64; 3]], radius: f64) -> f64 {
    if points.is_empty() || !(radius > 0.0) {
        return 0.0;
    }
    let pitch = radius.max(0.5);
    let (mut min_x, mut min_y, mut min_z) = (f64::MAX, f64::MAX, f64::MAX);
    let (mut max_x, mut max_y, mut max_z) = (f64::MIN, f64::MIN, f64::MIN);
    for p in points {
        min_x = min_x.min(p[0] - radius);
        min_y = min_y.min(p[1] - radius);
        min_z = min_z.min(p[2] - radius);
        max_x = max_x.max(p[0] + radius);
        max_y = max_y.max(p[1] + radius);
        max_z = max_z.max(p[2] + radius);
    }
    let nx = ((max_x - min_x) / pitch).ceil() as usize + 1;
    let ny = ((max_y - min_y) / pitch).ceil() as usize + 1;
    let nz = ((max_z - min_z) / pitch).ceil() as usize + 1;
    if nx == 0 || ny == 0 || nz == 0 || nx > 48 || ny > 48 || nz > 48 || nx.saturating_mul(ny).saturating_mul(nz) > 48 * 48 * 48 {
        let mut length = 0.0;
        for w in points.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            length += ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        }
        return std::f64::consts::PI * radius * radius * length;
    }
    let mut mark = vec![false; nx * ny * nz];
    let r2 = radius * radius;
    for p in points {
        let i0 = (((p[0] - radius - min_x) / pitch).floor() as isize).clamp(0, nx as isize - 1) as usize;
        let i1 = (((p[0] + radius - min_x) / pitch).ceil() as isize).clamp(0, nx as isize - 1) as usize;
        let j0 = (((p[1] - radius - min_y) / pitch).floor() as isize).clamp(0, ny as isize - 1) as usize;
        let j1 = (((p[1] + radius - min_y) / pitch).ceil() as isize).clamp(0, ny as isize - 1) as usize;
        let k0 = (((p[2] - radius - min_z) / pitch).floor() as isize).clamp(0, nz as isize - 1) as usize;
        let k1 = (((p[2] + radius - min_z) / pitch).ceil() as isize).clamp(0, nz as isize - 1) as usize;
        for k in k0..=k1 {
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let x = min_x + i as f64 * pitch;
                    let y = min_y + j as f64 * pitch;
                    let z = min_z + k as f64 * pitch;
                    let d2 = (x - p[0]).powi(2) + (y - p[1]).powi(2) + (z - p[2]).powi(2);
                    if d2 <= r2 {
                        let idx = (k * ny + j) * nx + i;
                        if let Some(slot) = mark.get_mut(idx) {
                            *slot = true;
                        }
                    }
                }
            }
        }
    }
    mark.iter().filter(|on| **on).count() as f64 * pitch.powi(3)
}

fn job_removed(job: &solidcraft_doc::CamJob) -> f64 {
    let points = gcode_points(&job.gcode);
    let three_d =
        matches!(job.kind.as_str(), "adaptive" | "parallel" | "scallop" | "pencil" | "contour_3d" | "horizontal" | "flow" | "radial" | "spiral");
    if three_d {
        return tool_sweep(&points, job.tool_diameter * 0.5);
    }
    // A 2D job stores the cleared area (the tool-centre region for a pocket). Depth steps
    // do not multiply that volume: the floor is cut once.
    if job.area > 0.0 && job.depth.is_finite() && job.depth > 0.0 {
        return job.area * job.depth;
    }
    let prism = path_prism(&points, job.depth);
    if prism > 1e-9 { prism } else { (job.area * job.depth).max(0.0) }
}

fn cam_simulate(s: &mut Session, _p: &Value) -> Result<Value> {
    let cmd = "cam.simulate";
    let setup = s.doc.cam_setups.last().ok_or_else(|| bad(cmd, "create a setup first"))?;
    let stock = setup.stock[0] * setup.stock[1] * setup.stock[2];
    if !(stock.is_finite() && stock > 0.0) {
        return Err(bad(cmd, "the stock volume is not usable"));
    }
    let mut removed = 0.0;
    for job in &s.doc.cam {
        let volume = job_removed(job);
        if !(volume.is_finite() && volume >= 0.0) {
            return Err(bad(cmd, "a toolpath has no measurable volume"));
        }
        removed += volume;
    }
    if !removed.is_finite() {
        return Err(bad(cmd, "the removed volume is not measurable"));
    }
    Ok(json!({"removed": removed, "remaining": (stock - removed).max(0.0), "stock": stock, "method": "toolpath"}))
}

const MAX_STUDIES: usize = 100;

fn study_label(cmd: &str, p: &Value, fallback: &str) -> Result<String> {
    let name = str_(p, "name").unwrap_or(fallback);
    if name.is_empty() || name.len() > 200 || name.chars().any(char::is_control) {
        return Err(bad(cmd, "the study name is not usable"));
    }
    Ok(name.to_string())
}

fn explicit_kind(cmd: &str, p: &Value) -> Result<Option<String>> {
    match str_(p, "kind") {
        None => Ok(None),
        Some(kind) => {
            if kind.is_empty() || kind.len() > 40 || !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(bad(cmd, "the study kind is not usable"));
            }
            Ok(Some(kind.to_string()))
        }
    }
}

fn named_body(cmd: &str, p: &Value, key: &str) -> Result<Option<String>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::String(name)) => {
            if name.is_empty() || name.len() > 200 {
                return Err(bad(cmd, "a body name is not usable"));
            }
            Ok(Some(name.clone()))
        }
        Some(_) => Err(bad(cmd, format!("`{key}` must be a name"))),
    }
}

fn require_body(s: &Session, cmd: &str, name: &str) -> Result<()> {
    if s.model.state().body(name).is_none() { Err(bad(cmd, format!("no body `{name}`"))) } else { Ok(()) }
}

fn material_of(cmd: &str, p: &Value) -> Result<Option<String>> {
    match p.get("material") {
        None => Ok(None),
        Some(Value::String(name)) => {
            let name = name.trim();
            if name.is_empty() || name.len() > 200 {
                return Err(bad(cmd, "the material name is not usable"));
            }
            if fea::youngs_modulus(name).is_none() {
                return Err(bad(cmd, format!("unknown material `{name}`")));
            }
            Ok(Some(name.to_string()))
        }
        Some(_) => Err(bad(cmd, "`material` must be a name")),
    }
}

fn modulus_of(cmd: &str, p: &Value, material: &str) -> Result<f64> {
    if let Some(value) = p.get("modulus") {
        let n = value.as_f64().ok_or_else(|| bad(cmd, "`modulus` must be a number"))?;
        let n = finite(cmd, n, "modulus")?;
        if n <= 0.0 {
            return Err(bad(cmd, "modulus must be positive"));
        }
        return Ok(n);
    }
    if material.is_empty() {
        return Ok(200_000.0);
    }
    fea::youngs_modulus(material).ok_or_else(|| bad(cmd, format!("unknown material `{material}`")))
}

fn force_of(cmd: &str, p: &Value) -> Result<Option<f64>> {
    match p.get("force") {
        None => Ok(None),
        Some(value) => {
            let n = value.as_f64().ok_or_else(|| bad(cmd, "`force` must be a number"))?;
            Ok(Some(finite(cmd, n, "force")?))
        }
    }
}

fn element_count(cmd: &str, p: &Value) -> Result<usize> {
    let Some(value) = p.get("elements") else { return Ok(4) };
    let n = if let Some(n) = value.as_u64() {
        n
    } else if let Some(n) = value.as_f64() {
        if !n.is_finite() || n < 1.0 || n.fract() != 0.0 {
            return Err(bad(cmd, "`elements` must be a positive integer"));
        }
        n as u64
    } else {
        return Err(bad(cmd, "`elements` must be a positive integer"));
    };
    if n == 0 {
        return Err(bad(cmd, "`elements` must be a positive integer"));
    }
    if n > fea::MAX_ELEMENTS as u64 {
        return Err(bad(cmd, "the study exceeds the size cap"));
    }
    Ok(n as usize)
}

fn constraints_of(cmd: &str, p: &Value) -> Result<Option<Vec<StudyConstraint>>> {
    let Some(value) = p.get("constraints") else { return Ok(None) };
    let Some(items) = value.as_array() else { return Err(bad(cmd, "`constraints` must be a list")) };
    if items.len() > 32 {
        return Err(bad(cmd, "the study exceeds the size cap"));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        if !item.is_object() {
            return Err(bad(cmd, "`constraints` must be a list of objects"));
        }
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("fixed");
        let target = item.get("target").and_then(Value::as_str).unwrap_or("min_x");
        if !matches!(kind, "fixed" | "pin") {
            return Err(bad(cmd, format!("not supported yet: a {kind} constraint")));
        }
        if !matches!(target, "min_x" | "max_x") {
            return Err(bad(cmd, format!("not supported yet: a constraint on `{target}`")));
        }
        out.push(StudyConstraint { kind: kind.to_string(), target: target.to_string() });
    }
    Ok(Some(out))
}

fn loads_of_params(cmd: &str, p: &Value) -> Result<Option<Vec<StudyLoad>>> {
    let Some(value) = p.get("loads") else { return Ok(None) };
    let Some(items) = value.as_array() else { return Err(bad(cmd, "`loads` must be a list")) };
    if items.len() > 32 {
        return Err(bad(cmd, "the study exceeds the size cap"));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        if !item.is_object() {
            return Err(bad(cmd, "`loads` must be a list of objects"));
        }
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("force");
        if kind != "force" {
            return Err(bad(cmd, format!("not supported yet: a {kind} load")));
        }
        let magnitude = item.get("magnitude").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`magnitude` is required"))?;
        let magnitude = finite(cmd, magnitude, "magnitude")?;
        let direction = match item.get("direction") {
            None => [0.0, 0.0, 1.0],
            Some(value) => {
                let point = crate::params::vec3(value).ok_or_else(|| bad(cmd, "`direction` must be [x, y, z]"))?;
                [point.x, point.y, point.z]
            }
        };
        let len = (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2]).sqrt();
        if !(len.is_finite() && len > 1e-9) {
            return Err(bad(cmd, "the load direction is not usable"));
        }
        let target = item.get("target").and_then(Value::as_str).unwrap_or("max_x");
        if !matches!(target, "min_x" | "max_x") {
            return Err(bad(cmd, format!("not supported yet: a load on `{target}`")));
        }
        out.push(StudyLoad { kind: kind.to_string(), magnitude, direction, target: target.to_string() });
    }
    Ok(Some(out))
}

fn default_load(force: f64) -> StudyLoad {
    StudyLoad { kind: "force".into(), magnitude: force, direction: [0.0, 0.0, 1.0], target: "max_x".into() }
}

fn default_clamp() -> StudyConstraint {
    StudyConstraint { kind: "fixed".into(), target: "min_x".into() }
}

fn direction_len(direction: [f64; 3]) -> f64 {
    (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2]).sqrt()
}

fn signed_force(loads: &[StudyLoad], fallback: f64) -> f64 {
    if loads.is_empty() {
        return fallback;
    }
    let mut sum = 0.0;
    for load in loads {
        let len = direction_len(load.direction);
        if len > 1e-12 && load.direction[2].abs() / len > 0.999 {
            let sign = if load.direction[2] < 0.0 { -1.0 } else { 1.0 };
            sum += load.magnitude * sign;
        }
    }
    if sum.is_finite() { sum } else { fallback }
}

fn blank_study(
    name: String,
    kind: String,
    body: String,
    force: f64,
    modulus: f64,
    material: String,
    constraints: Vec<StudyConstraint>,
    loads: Vec<StudyLoad>,
) -> Study {
    Study {
        name,
        kind,
        body,
        force,
        modulus,
        tip: None,
        method: String::new(),
        stale: true,
        material,
        constraints,
        loads,
        displacement_min: None,
        displacement_max: None,
        stress_min: None,
        stress_max: None,
    }
}

fn feature_ids(s: &Session) -> Vec<u64> {
    s.doc.features.iter().map(|feature| feature.id).collect()
}

fn box_section(s: &Session, cmd: &str, body: &str) -> Result<(f64, f64, f64)> {
    let state = s.model.state();
    let solid = state.body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
    let measured = solidcraft_kernel::measure(&solid.body).map_err(|e| bad(cmd, e.to_string()))?;
    if measured.merged.face_types.get("plane").copied() != Some(6) {
        return Err(bad(cmd, "not supported yet: static stress of a body that is not a box"));
    }
    let length = measured.bbox.max.x - measured.bbox.min.x;
    let width = measured.bbox.max.y - measured.bbox.min.y;
    let height = measured.bbox.max.z - measured.bbox.min.z;
    if !(length.is_finite() && width.is_finite() && height.is_finite() && length > 1e-6 && width > 1e-6 && height > 1e-6) {
        return Err(bad(cmd, "the beam and the load must be finite"));
    }
    if length > fea::MAX_DIMENSION || width > fea::MAX_DIMENSION || height > fea::MAX_DIMENSION {
        return Err(bad(cmd, "the study exceeds the size cap"));
    }
    let box_volume = length * width * height;
    let gap = (measured.volume - box_volume).abs();
    if !(measured.volume.is_finite() && gap <= (1e-4 * box_volume).max(1e-3)) {
        return Err(bad(cmd, "not supported yet: static stress of a body that is not an axis-aligned box"));
    }
    Ok((length, width, height))
}

fn beam_supports(cmd: &str, constraints: &[StudyConstraint]) -> Result<Vec<BeamSupport>> {
    if constraints.is_empty() {
        return Err(bad(cmd, "under-constrained: the beam can move as a rigid body"));
    }
    let mut supports = Vec::with_capacity(constraints.len());
    for constraint in constraints {
        let end = match constraint.target.as_str() {
            "min_x" => BeamEnd::MinX,
            "max_x" => BeamEnd::MaxX,
            other => return Err(bad(cmd, format!("not supported yet: a constraint on `{other}`"))),
        };
        let clamp = match constraint.kind.as_str() {
            "fixed" => true,
            "pin" => false,
            other => return Err(bad(cmd, format!("not supported yet: a {other} constraint"))),
        };
        supports.push(BeamSupport { end, clamp });
    }
    Ok(supports)
}

fn beam_loads(cmd: &str, loads: &[StudyLoad], legacy_force: f64) -> Result<Vec<BeamLoad>> {
    let source = if loads.is_empty() && legacy_force != 0.0 { vec![default_load(legacy_force)] } else { loads.to_vec() };
    let mut out = Vec::with_capacity(source.len());
    for load in &source {
        if load.kind != "force" {
            return Err(bad(cmd, format!("not supported yet: a {} load", load.kind)));
        }
        let end = match load.target.as_str() {
            "min_x" => BeamEnd::MinX,
            "max_x" => BeamEnd::MaxX,
            other => return Err(bad(cmd, format!("not supported yet: a load on `{other}`"))),
        };
        let len = direction_len(load.direction);
        if !(len.is_finite() && len > 1e-9) {
            return Err(bad(cmd, "the load direction is not usable"));
        }
        if load.direction[0].abs() > 1e-3 * len || load.direction[1].abs() > 1e-3 * len {
            return Err(bad(cmd, "not supported yet: a load that is not along Z"));
        }
        let sign = if load.direction[2] < 0.0 { -1.0 } else { 1.0 };
        out.push(BeamLoad { end, force_z: load.magnitude * sign });
    }
    Ok(out)
}

fn run_beam(
    cmd: &str,
    size: (f64, f64, f64),
    modulus: f64,
    elements: usize,
    constraints: &[StudyConstraint],
    loads: &[StudyLoad],
    legacy_force: f64,
) -> Result<fea::BeamField> {
    let supports = beam_supports(cmd, constraints)?;
    let applied = beam_loads(cmd, loads, legacy_force)?;
    let section = BeamSection { length: size.0, width: size.1, height: size.2, modulus, elements };
    fea::solve_beam(section, &supports, &applied).map_err(|err| bad(cmd, err))
}

fn apply_field(study: &mut Study, field: &fea::BeamField) {
    study.tip = Some(field.tip);
    study.method = field.method.to_string();
    study.stale = false;
    study.displacement_min = Some(field.displacement_min);
    study.displacement_max = Some(field.displacement_max);
    study.stress_min = Some(field.stress_min);
    study.stress_max = Some(field.stress_max);
}

fn static_json(name: &str, field: &fea::BeamField) -> Value {
    let peak = field.stress_max.abs().max(field.stress_min.abs());
    let colours: Vec<[u8; 3]> = field
        .stress
        .iter()
        .map(|sample| {
            let colour = solidcraft_render::result_colour_value(*sample, 0.0, peak);
            [colour.0, colour.1, colour.2]
        })
        .collect();
    json!({
        "study": name,
        "tip_mm": field.tip,
        "value": field.tip,
        "unit": "mm",
        "displacement_min": field.displacement_min,
        "displacement_max": field.displacement_max,
        "stress_min": field.stress_min,
        "stress_max": field.stress_max,
        "stress_unit": "N/mm^2",
        "method": field.method,
        "colours": colours,
    })
}

fn store_study(s: &mut Session, p: &Value, explicit: Option<&str>, create_kind: &str) -> Result<Value> {
    let cmd = "sim.study";
    let name = study_label(cmd, p, explicit.unwrap_or(create_kind))?;
    let body = named_body(cmd, p, "body")?;
    if let Some(body) = &body {
        require_body(s, cmd, body)?;
    }
    let material = material_of(cmd, p)?;
    let modulus = if p.get("modulus").is_some() { Some(modulus_of(cmd, p, material.as_deref().unwrap_or(""))?) } else { None };
    let force = force_of(cmd, p)?;
    let constraints = constraints_of(cmd, p)?;
    let loads = loads_of_params(cmd, p)?;
    let index = s.doc.studies.iter().rposition(|study| study.name == name);
    let Some(index) = index else {
        let body = body.ok_or_else(|| bad(cmd, "`body` is required"))?;
        if s.doc.studies.len() >= MAX_STUDIES {
            return Err(bad(cmd, "the study exceeds the size cap"));
        }
        let material = material.unwrap_or_default();
        let modulus = match modulus {
            Some(value) => value,
            None => modulus_of(cmd, p, &material)?,
        };
        let force = force.unwrap_or(0.0);
        let constraints = constraints.unwrap_or_default();
        let mut loads = loads.unwrap_or_default();
        if loads.is_empty() && force != 0.0 {
            loads.push(default_load(force));
        }
        let kind = explicit.unwrap_or(create_kind).to_string();
        let stored_force = signed_force(&loads, force);
        s.doc_mut().studies.push(blank_study(name.clone(), kind.clone(), body, stored_force, modulus, material, constraints, loads));
        return Ok(json!({"study": name, "kind": kind, "stale": true}));
    };
    let Some(study) = s.doc_mut().studies.get_mut(index) else {
        return Err(bad(cmd, "no study"));
    };
    let mut changed = false;
    if let Some(kind) = explicit
        && study.kind != kind
    {
        study.kind = kind.to_string();
        changed = true;
    }
    if let Some(body) = body
        && study.body != body
    {
        study.body = body;
        changed = true;
    }
    if let Some(material) = material
        && study.material != material
    {
        let next = fea::youngs_modulus(&material);
        study.material = material;
        changed = true;
        if modulus.is_none()
            && let Some(next) = next
            && study.modulus != next
        {
            study.modulus = next;
        }
    }
    if let Some(modulus) = modulus
        && study.modulus != modulus
    {
        study.modulus = modulus;
        changed = true;
    }
    if let Some(force) = force {
        if study.force != force {
            study.force = force;
            changed = true;
        }
        if loads.is_none() {
            let target = study.loads.first().map(|load| load.target.clone()).unwrap_or_else(|| "max_x".into());
            let next = vec![StudyLoad { kind: "force".into(), magnitude: force, direction: [0.0, 0.0, 1.0], target }];
            if study.loads != next {
                study.loads = next;
                changed = true;
            }
        }
    }
    if let Some(constraints) = constraints
        && study.constraints != constraints
    {
        study.constraints = constraints;
        changed = true;
    }
    if let Some(loads) = loads
        && study.loads != loads
    {
        study.loads = loads;
        study.force = signed_force(&study.loads, study.force);
        changed = true;
    }
    if changed {
        study.stale = true;
    }
    let kind = study.kind.clone();
    let stale = study.stale;
    Ok(json!({"study": name, "kind": kind, "stale": stale}))
}

fn push_study(s: &mut Session, p: &Value, kind: &str) -> Result<Value> {
    store_study(s, p, Some(kind), kind)
}

fn sim_study(s: &mut Session, p: &Value) -> Result<Value> {
    let explicit = explicit_kind("sim.study", p)?;
    store_study(s, p, explicit.as_deref(), "static")
}

fn put_static(
    s: &mut Session,
    name: &str,
    body: String,
    material: String,
    modulus: f64,
    force: f64,
    constraints: Vec<StudyConstraint>,
    loads: Vec<StudyLoad>,
    field: &fea::BeamField,
) -> Result<()> {
    let cmd = "sim.static";
    let stored_force = signed_force(&loads, force);
    if let Some(index) = s.doc.studies.iter().rposition(|study| study.name == name) {
        let Some(study) = s.doc_mut().studies.get_mut(index) else {
            return Err(bad(cmd, "no study"));
        };
        study.kind = "static".into();
        study.body = body;
        study.material = material;
        study.modulus = modulus;
        study.force = stored_force;
        study.constraints = constraints;
        study.loads = loads;
        apply_field(study, field);
        return Ok(());
    }
    if s.doc.studies.len() >= MAX_STUDIES {
        return Err(bad(cmd, "the study exceeds the size cap"));
    }
    let mut study = blank_study(name.to_string(), "static".into(), body, stored_force, modulus, material, constraints, loads);
    apply_field(&mut study, field);
    s.doc_mut().studies.push(study);
    Ok(())
}

fn sim_static(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.static";
    let name = study_label(cmd, p, "static")?;
    let body = named_body(cmd, p, "body")?.ok_or_else(|| bad(cmd, "`body` is required"))?;
    require_body(s, cmd, &body)?;
    let material = material_of(cmd, p)?.unwrap_or_default();
    let modulus = modulus_of(cmd, p, &material)?;
    let force = force_of(cmd, p)?.unwrap_or(100.0);
    let constraints = constraints_of(cmd, p)?.unwrap_or_else(|| vec![default_clamp()]);
    let loads = loads_of_params(cmd, p)?.unwrap_or_else(|| vec![default_load(force)]);
    let elements = element_count(cmd, p)?;
    let features = feature_ids(s);
    let size = box_section(s, cmd, &body)?;
    let field = run_beam(cmd, size, modulus, elements, &constraints, &loads, force)?;
    if feature_ids(s) != features {
        return Err(bad(cmd, "the study changed the solid"));
    }
    put_static(s, &name, body, material, modulus, force, constraints, loads, &field)?;
    if feature_ids(s) != features {
        return Err(bad(cmd, "the study changed the solid"));
    }
    Ok(static_json(&name, &field))
}

fn sim_solve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.solve";
    let wanted = str_(p, "name");
    let (label, kind, body, force, modulus, constraints, loads) = {
        let study = s.doc.studies.iter().rev().find(|study| wanted.is_none_or(|n| study.name == n)).ok_or_else(|| bad(cmd, "no study"))?;
        (study.name.clone(), study.kind.clone(), study.body.clone(), study.force, study.modulus, study.constraints.clone(), study.loads.clone())
    };
    if kind != "static" {
        return Err(bad(cmd, format!("not supported yet: solving a {kind} study")));
    }
    if !(force.is_finite() && modulus.is_finite() && modulus > 0.0) {
        return Err(bad(cmd, "the beam and the load must be finite"));
    }
    let elements = element_count(cmd, p)?;
    let features = feature_ids(s);
    let size = box_section(s, cmd, &body)?;
    let field = run_beam(cmd, size, modulus, elements, &constraints, &loads, force)?;
    if feature_ids(s) != features {
        return Err(bad(cmd, "the study changed the solid"));
    }
    if let Some(study) = s.doc_mut().studies.iter_mut().rev().find(|study| study.name == label) {
        apply_field(study, &field);
    }
    if feature_ids(s) != features {
        return Err(bad(cmd, "the study changed the solid"));
    }
    Ok(static_json(&label, &field))
}

fn sim_modal(s: &mut Session, p: &Value) -> Result<Value> {
    push_study(s, p, "modal")
}
fn sim_thermal(s: &mut Session, p: &Value) -> Result<Value> {
    push_study(s, p, "thermal")
}
fn sim_thermal_stress(s: &mut Session, p: &Value) -> Result<Value> {
    push_study(s, p, "thermal_stress")
}
fn sim_buckling(s: &mut Session, p: &Value) -> Result<Value> {
    push_study(s, p, "buckling")
}
fn sim_nonlinear(s: &mut Session, p: &Value) -> Result<Value> {
    push_study(s, p, "nonlinear")
}
fn sim_event(s: &mut Session, p: &Value) -> Result<Value> {
    push_study(s, p, "event")
}
fn sim_shape(s: &mut Session, p: &Value) -> Result<Value> {
    push_study(s, p, "shape")
}
fn sim_injection(s: &mut Session, p: &Value) -> Result<Value> {
    push_study(s, p, "injection")
}
fn sim_cooling(s: &mut Session, p: &Value) -> Result<Value> {
    push_study(s, p, "cooling")
}

const IDENTITY_ROTATE: [f64; 4] = [0.0, 0.0, 0.0, 1.0];
const RENDER_W: usize = 160;
const RENDER_H: usize = 120;

fn scene(s: &mut Session) -> &mut RenderScene {
    s.doc_mut().render_scene.get_or_insert(RenderScene { color: [1.0, 1.0, 1.0], intensity: 1.0, focal: 50.0 })
}

fn rotate_is_identity(q: &[f64; 4]) -> bool {
    q.iter().zip(IDENTITY_ROTATE).all(|(a, b)| (a - b).abs() <= 1e-9)
}

fn require_assembly(s: &Session, cmd: &str) -> Result<()> {
    if s.doc.components.is_empty() || s.doc.occurrences.is_empty() {
        return Err(bad(cmd, "the assembly has no components"));
    }
    Ok(())
}

fn vec3_finite(cmd: &str, v: &Value, what: &str) -> Result<[f64; 3]> {
    let arr = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| bad(cmd, format!("`{what}` must be [x, y, z]")))?;
    let mut out = [0.0; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        let n = arr.get(i).and_then(Value::as_f64).ok_or_else(|| bad(cmd, format!("`{what}` must be [x, y, z]")))?;
        *slot = finite(cmd, n, what)?;
        if slot.abs() > 1e7 {
            return Err(bad(cmd, format!("`{what}` is too large")));
        }
    }
    Ok(out)
}

fn quat_param(cmd: &str, v: &Value) -> Result<[f64; 4]> {
    let arr = v.as_array().filter(|a| a.len() == 4).ok_or_else(|| bad(cmd, "`rotate` must be [x, y, z, w]"))?;
    let mut q = [0.0; 4];
    for (i, slot) in q.iter_mut().enumerate() {
        let n = arr.get(i).and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`rotate` must be [x, y, z, w]"))?;
        *slot = finite(cmd, n, "rotate")?;
    }
    let len = q.iter().map(|n| n * n).sum::<f64>().sqrt();
    if !(len.is_finite() && len >= 1e-8) {
        return Err(bad(cmd, "`rotate` must be a rotation"));
    }
    Ok([q[0] / len, q[1] / len, q[2] / len, q[3] / len])
}

fn occurrence_ref(s: &Session, v: Option<&Value>, cmd: &str) -> Result<u64> {
    let Some(v) = v else { return Err(bad(cmd, "`occurrence` is required")) };
    let key = match v {
        Value::Number(n) => n.to_string(),
        Value::String(name) if !name.is_empty() && name.len() <= 200 => name.clone(),
        _ => return Err(bad(cmd, "`occurrence` must be an id or a name")),
    };
    s.doc
        .occurrences
        .iter()
        .find(|o| o.id.to_string() == key || o.name == key)
        .map(|o| o.id)
        .ok_or_else(|| bad(cmd, format!("no occurrence `{key}`")))
}

fn key_sane(key: &StoryKey) -> bool {
    key.time.is_finite()
        && (0.0..=1e6).contains(&key.time)
        && key.translate.iter().all(|n| n.is_finite() && n.abs() <= 1e7)
        && key.rotate.iter().all(|n| n.is_finite())
        && key.rotate.iter().map(|n| n * n).sum::<f64>() > 1e-8
}

fn keys_resolve(s: &Session, keys: &[StoryKey], cmd: &str) -> Result<()> {
    if keys.len() > 4096 {
        return Err(bad(cmd, "too many keys"));
    }
    for key in keys {
        if !key_sane(key) {
            return Err(bad(cmd, "a step is not usable"));
        }
        if !s.doc.occurrences.iter().any(|o| o.id == key.occurrence) {
            return Err(bad(cmd, format!("no occurrence {}", key.occurrence)));
        }
    }
    Ok(())
}

fn pose_of(keys: &[StoryKey], occurrence: u64, time: f64, cmd: &str) -> Result<([f64; 3], [f64; 4])> {
    let mut list: Vec<(usize, &StoryKey)> = keys.iter().enumerate().filter(|(_, key)| key.occurrence == occurrence).collect();
    if list.len() > 4096 {
        return Err(bad(cmd, "too many keys"));
    }
    list.sort_by(|a, b| a.1.time.total_cmp(&b.1.time).then(a.0.cmp(&b.0)));
    let mut span: Vec<&StoryKey> = Vec::new();
    for (_, key) in list {
        if !key_sane(key) {
            return Err(bad(cmd, "a step is not usable"));
        }
        if span.last().is_some_and(|prev| (prev.time - key.time).abs() <= 1e-9) {
            span.pop();
        }
        span.push(key);
    }
    let (Some(first), Some(last)) = (span.first(), span.last()) else {
        return Ok(([0.0, 0.0, 0.0], IDENTITY_ROTATE));
    };
    if time <= first.time {
        return Ok((first.translate, first.rotate));
    }
    if time >= last.time {
        return Ok((last.translate, last.rotate));
    }
    for pair in span.windows(2) {
        let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else { continue };
        if time < a.time || time > b.time {
            continue;
        }
        let dt = b.time - a.time;
        let t = if dt.abs() <= 1e-12 { 0.0 } else { ((time - a.time) / dt).clamp(0.0, 1.0) };
        let translate = [
            a.translate[0] + (b.translate[0] - a.translate[0]) * t,
            a.translate[1] + (b.translate[1] - a.translate[1]) * t,
            a.translate[2] + (b.translate[2] - a.translate[2]) * t,
        ];
        let qa = Quat { x: a.rotate[0], y: a.rotate[1], z: a.rotate[2], w: a.rotate[3] }.normalized();
        let qb = Quat { x: b.rotate[0], y: b.rotate[1], z: b.rotate[2], w: b.rotate[3] }.normalized();
        let q = qa.slerp(qb, t);
        return Ok((translate, [q.x, q.y, q.z, q.w]));
    }
    Ok((last.translate, last.rotate))
}

fn pose_map(keys: &[StoryKey], time: f64, cmd: &str) -> Result<BTreeMap<u64, ([f64; 3], [f64; 4])>> {
    let mut ids = Vec::new();
    for key in keys.iter().take(4096) {
        if !ids.contains(&key.occurrence) {
            ids.push(key.occurrence);
        }
    }
    let mut map = BTreeMap::new();
    for id in ids {
        map.insert(id, pose_of(keys, id, time, cmd)?);
    }
    Ok(map)
}

fn read3(v: Option<&Value>) -> Option<[f64; 3]> {
    let arr = v?.as_array()?;
    if arr.len() != 3 {
        return None;
    }
    let mut out = [0.0; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = arr.get(i)?.as_f64()?;
        if !slot.is_finite() {
            return None;
        }
    }
    Some(out)
}

fn read_axis(v: Option<&Value>, fallback: Vec3) -> Vec3 {
    match read3(v).map(|a| Vec3::new(a[0], a[1], a[2])) {
        Some(axis) if axis.is_finite() => axis,
        _ => fallback,
    }
}

fn axes_close(a: [Vec3; 3], b: [Vec3; 3]) -> bool {
    a.iter().zip(b).all(|(p, q)| p.dist(q) < 1e-5)
}

fn delta_quat(rest: [Vec3; 3], posed: [Vec3; 3]) -> [f64; 4] {
    let [r0, r1, r2] = rest;
    let [p0, p1, p2] = posed;
    let col = |x: f64, y: f64, z: f64| p0 * x + p1 * y + p2 * z;
    let q = Quat::from_basis(col(r0.x, r1.x, r2.x), col(r0.y, r1.y, r2.y), col(r0.z, r1.z, r2.z));
    [q.x, q.y, q.z, q.w]
}

fn keys_from_offsets(moves: &[(u64, [f64; 3])]) -> Vec<StoryKey> {
    let mut keys = Vec::new();
    for (occurrence, translate) in moves.iter().take(4096) {
        keys.push(StoryKey { time: 0.0, occurrence: *occurrence, translate: [0.0, 0.0, 0.0], rotate: IDENTITY_ROTATE });
        keys.push(StoryKey { time: 1.0, occurrence: *occurrence, translate: *translate, rotate: IDENTITY_ROTATE });
    }
    keys
}

fn keys_from_explode(s: &Session, view: &ExplodedView) -> Result<Vec<StoryKey>> {
    let cmd = "anim.storyboard";
    if view.moves.is_empty() || view.moves.len() > 4096 {
        return Err(bad(cmd, "the exploded view has no usable steps"));
    }
    let mut moves = Vec::new();
    for (occurrence, translate) in &view.moves {
        if !translate.is_finite() {
            return Err(bad(cmd, "an explode step is not finite"));
        }
        if !s.doc.occurrences.iter().any(|o| o.id == *occurrence) {
            return Err(bad(cmd, format!("no occurrence {occurrence}")));
        }
        moves.push((*occurrence, [translate.x, translate.y, translate.z]));
    }
    Ok(keys_from_offsets(&moves))
}

fn keys_from_motion(s: &Session, study: &str) -> Result<Vec<StoryKey>> {
    let cmd = "anim.storyboard";
    let mut trial = s.scratch();
    let exported = trial.execute("motion.export", &json!({"study": study, "samples": 5})).map_err(|e| bad(cmd, e.to_string()))?;
    let samples = exported.get("samples").and_then(Value::as_array).ok_or_else(|| bad(cmd, "the motion study returned no samples"))?;
    if samples.is_empty() || samples.len() > 32 {
        return Err(bad(cmd, "the motion study returned no samples"));
    }
    struct Row {
        time: f64,
        occurrence: u64,
        origin: [f64; 3],
        axes: [Vec3; 3],
    }
    let mut rows = Vec::new();
    for (i, sample) in samples.iter().enumerate() {
        let time = sample.get("step").and_then(Value::as_f64).filter(|t| t.is_finite()).unwrap_or(i as f64);
        let positions = sample.get("positions").and_then(Value::as_array).ok_or_else(|| bad(cmd, "a motion sample has no positions"))?;
        if positions.len() > 10_000 {
            return Err(bad(cmd, "too many occurrences in a motion sample"));
        }
        for pos in positions {
            let Some(name) = pos.get("occurrence").and_then(Value::as_str) else { continue };
            let Some(occurrence) = s.doc.occurrences.iter().find(|o| o.name == name || o.id.to_string() == name).map(|o| o.id) else {
                continue;
            };
            let Some(pose) = pos.get("pose") else { continue };
            let Some(origin) = read3(pose.get("origin")) else {
                return Err(bad(cmd, "a motion pose has no origin"));
            };
            let axes = [read_axis(pose.get("x"), Vec3::X), read_axis(pose.get("y"), Vec3::Y), read_axis(pose.get("z"), Vec3::Z)];
            rows.push(Row { time, occurrence, origin, axes });
        }
    }
    if rows.is_empty() || rows.len() > 4096 {
        return Err(bad(cmd, "the motion study did not name an occurrence"));
    }
    rows.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.occurrence.cmp(&b.occurrence)));
    let mut ids = Vec::new();
    for row in &rows {
        if !ids.contains(&row.occurrence) {
            ids.push(row.occurrence);
        }
    }
    let mut keys = Vec::new();
    for id in ids {
        let mine: Vec<&Row> = rows.iter().filter(|row| row.occurrence == id).collect();
        let Some(rest) = mine.first() else { continue };
        let rest_origin = rest.origin;
        let rest_axes = rest.axes;
        for row in &mine {
            let translate = [row.origin[0] - rest_origin[0], row.origin[1] - rest_origin[1], row.origin[2] - rest_origin[2]];
            if translate.iter().any(|n| !n.is_finite() || n.abs() > 1e7) {
                return Err(bad(cmd, "a motion step is not usable"));
            }
            let rotate = if axes_close(rest_axes, row.axes) { IDENTITY_ROTATE } else { delta_quat(rest_axes, row.axes) };
            keys.push(StoryKey { time: row.time, occurrence: id, translate, rotate });
        }
    }
    if keys.len() > 4096 {
        return Err(bad(cmd, "too many keys"));
    }
    Ok(keys)
}

fn scale_of(p: &Value) -> Result<f64> {
    let cmd = "anim.storyboard";
    match p.get("scale") {
        None => Ok(1.0),
        Some(v) => {
            let n = v.as_f64().ok_or_else(|| bad(cmd, "`scale` must be a number"))?;
            let n = finite(cmd, n, "scale")?;
            if n.abs() > 100.0 {
                return Err(bad(cmd, "`scale` must be a number up to 100"));
            }
            Ok(n)
        }
    }
}

fn auto_moves(s: &Session, scale: f64) -> Result<Vec<(u64, [f64; 3])>> {
    let cmd = "anim.storyboard";
    let mut trial = s.scratch();
    let created = trial.execute("explode.create", &json!({"scale": scale, "name": "Story Explode"})).map_err(|e| bad(cmd, e.to_string()))?;
    let moves = created.get("moves").and_then(Value::as_array).ok_or_else(|| bad(cmd, "the explode step returned no moves"))?;
    if moves.is_empty() || moves.len() > 4096 {
        return Err(bad(cmd, "the explode step returned no moves"));
    }
    let mut out = Vec::new();
    for step in moves {
        let id = step.get("occurrence").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "an explode step has no occurrence"))?;
        if !s.doc.occurrences.iter().any(|o| o.id == id) {
            return Err(bad(cmd, format!("no occurrence {id}")));
        }
        let translate = vec3_finite(cmd, step.get("translate").unwrap_or(&Value::Null), "translate")?;
        out.push((id, translate));
    }
    Ok(out)
}

fn chosen_view(s: &Session, p: &Value) -> Result<Option<ExplodedView>> {
    let cmd = "anim.storyboard";
    let Some(v) = p.get("view") else { return Ok(None) };
    if v.is_null() {
        return Ok(None);
    }
    let key = match v {
        Value::Number(n) => n.to_string(),
        Value::String(name) if !name.is_empty() && name.len() <= 200 => name.clone(),
        _ => return Err(bad(cmd, "`view` must be an id or a name")),
    };
    s.doc
        .assembly
        .exploded
        .iter()
        .find(|view| view.id.to_string() == key || view.name == key)
        .cloned()
        .map(Some)
        .ok_or_else(|| bad(cmd, format!("no exploded view `{key}`")))
}

fn chosen_study(s: &Session, p: &Value) -> Result<Option<String>> {
    let cmd = "anim.storyboard";
    let Some(v) = p.get("study") else { return Ok(None) };
    if v.is_null() {
        return Ok(None);
    }
    let key = match v {
        Value::Number(n) => n.to_string(),
        Value::String(name) if !name.is_empty() && name.len() <= 200 => name.clone(),
        _ => return Err(bad(cmd, "`study` must be an id or a name")),
    };
    s.doc
        .assembly
        .studies
        .iter()
        .find(|study| study.id.to_string() == key || study.name == key)
        .map(|study| Some(study.name.clone()))
        .ok_or_else(|| bad(cmd, format!("no motion study `{key}`")))
}

fn story_name(p: &Value, cmd: &str) -> Result<String> {
    match str_(p, "name") {
        None => Ok("Story".to_string()),
        Some(name) if !name.is_empty() && name.len() <= 200 && !name.chars().any(char::is_control) => Ok(name.to_string()),
        Some(_) => Err(bad(cmd, "the storyboard name is not usable")),
    }
}

fn assembly_same(
    s: &Session,
    features: usize,
    occurrences: &[solidcraft_doc::Occurrence],
    pending: &BTreeMap<u64, solidcraft_doc::Mat>,
    assembly: &solidcraft_doc::joints::Assembly,
) -> bool {
    s.doc.features.len() == features && s.doc.occurrences == occurrences && &s.pending_moves == pending && &s.doc.assembly == assembly
}

struct BodyPaint {
    mesh: Arc<Mesh>,
    color: solidcraft_render::Rgb,
    metalness: f64,
    roughness: f64,
    face_colors: Vec<(u32, solidcraft_render::Rgb)>,
    face_shade: Vec<(u32, f64, f64)>,
    path: Vec<u64>,
}

fn body_paints(s: &Session, cmd: &str) -> Result<Vec<BodyPaint>> {
    let local = s.model.state();
    let frames = s.doc.body_frames(&local);
    let world = s.world_state();
    let mut out = Vec::new();
    for body in world.bodies.iter().take(10_000) {
        let mesh = body.mesh();
        if mesh.positions.is_empty() || mesh.triangles.is_empty() {
            continue;
        }
        if mesh.positions.len() > 2_000_000 || mesh.triangles.len() > 2_000_000 {
            return Err(bad(cmd, "the mesh is too large to shade"));
        }
        let look = s.doc.model_body_look(body);
        let color = if let Some(look) = &look {
            solidcraft_render::Rgb(look.color[0], look.color[1], look.color[2])
        } else {
            s.doc.body_color(body).map_or(solidcraft_render::Rgb(168, 176, 186), |rgb| {
                let to8 = |x: f32| {
                    let x = if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 };
                    (x * 255.0).round() as u8
                };
                solidcraft_render::Rgb(to8(rgb[0]), to8(rgb[1]), to8(rgb[2]))
            })
        };
        let metalness = look.as_ref().map(|l| l.metalness).unwrap_or(0.0);
        let roughness = look.as_ref().map(|l| l.roughness).unwrap_or(0.5);
        let faces = s.doc.face_colors(body);
        let mut face_colors = Vec::new();
        let mut face_shade = Vec::new();
        for (face, look) in faces {
            let Ok(index) = u32::try_from(face) else { continue };
            face_colors.push((index, solidcraft_render::Rgb(look.color[0], look.color[1], look.color[2])));
            face_shade.push((index, look.metalness, look.roughness));
        }
        let path = frames.iter().find(|(name, _, _, _)| name == &body.name).map(|(_, _, path, _)| path.clone()).unwrap_or_default();
        out.push(BodyPaint { mesh, color, metalness, roughness, face_colors, face_shade, path });
    }
    Ok(out)
}

fn apply_offset(mesh: &mut Mesh, origin: Vec3, translate: [f64; 3], rotate: [f64; 4]) {
    let spin = !rotate_is_identity(&rotate);
    let q = Quat { x: rotate[0], y: rotate[1], z: rotate[2], w: rotate[3] }.normalized();
    let shift = Vec3::new(translate[0], translate[1], translate[2]);
    let turn = |v: Vec3| {
        let (x, y, z) = q.basis();
        x * v.x + y * v.y + z * v.z
    };
    for p in &mut mesh.positions {
        if !p.is_finite() {
            continue;
        }
        if spin {
            *p = origin + turn(*p - origin) + shift;
        } else {
            *p += shift;
        }
    }
    if spin {
        for n in &mut mesh.normals {
            if n.is_finite() {
                *n = turn(*n);
            }
        }
    }
}

fn mesh_at(mesh: &Arc<Mesh>, path: &[u64], poses: &BTreeMap<u64, ([f64; 3], [f64; 4])>, origins: &BTreeMap<u64, Vec3>) -> Arc<Mesh> {
    let moves = path.iter().any(|id| poses.get(id).is_some_and(|(t, r)| *t != [0.0, 0.0, 0.0] || !rotate_is_identity(r)));
    if !moves {
        return Arc::clone(mesh);
    }
    let mut out = mesh.as_ref().clone();
    for id in path.iter().take(64) {
        let Some((translate, rotate)) = poses.get(id) else { continue };
        if *translate == [0.0, 0.0, 0.0] && rotate_is_identity(rotate) {
            continue;
        }
        let origin = origins.get(id).copied().unwrap_or(Vec3::ZERO);
        apply_offset(&mut out, origin, *translate, *rotate);
    }
    Arc::new(out)
}

fn origins_of(s: &Session) -> BTreeMap<u64, Vec3> {
    let mut out = BTreeMap::new();
    for occurrence in s.doc.occurrences.iter().take(10_000) {
        let m = solidcraft_doc::joints::occurrence_world(&s.doc, occurrence.id);
        out.insert(occurrence.id, Vec3::new(m[3][0], m[3][1], m[3][2]));
    }
    out
}

fn render_paints(
    paints: &[BodyPaint],
    poses: &BTreeMap<u64, ([f64; 3], [f64; 4])>,
    origins: &BTreeMap<u64, Vec3>,
    light: ([f64; 3], f64),
    focal: f64,
    cmd: &str,
) -> Result<solidcraft_render::Canvas> {
    let mut scene = solidcraft_render::Scene { light: Some(light), ..solidcraft_render::Scene::default() };
    let mut min = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut max = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    let mut any = false;
    for paint in paints.iter().take(10_000) {
        let mesh = mesh_at(&paint.mesh, &paint.path, poses, origins);
        let bounds = mesh.bounds();
        if bounds.is_empty() {
            continue;
        }
        min = Vec3::new(min.x.min(bounds.min.x), min.y.min(bounds.min.y), min.z.min(bounds.min.z));
        max = Vec3::new(max.x.max(bounds.max.x), max.y.max(bounds.max.y), max.z.max(bounds.max.z));
        scene.meshes.push(solidcraft_render::SceneMesh {
            mesh,
            color: paint.color,
            metalness: paint.metalness,
            roughness: paint.roughness,
            face_colors: paint.face_colors.clone(),
            face_shade: paint.face_shade.clone(),
        });
        any = true;
    }
    if !any || !min.is_finite() || !max.is_finite() {
        return Err(bad(cmd, "there is no solid to render"));
    }
    let center = (min + max) * 0.5;
    let radius = (max - min).len().max(1.0) * 0.5;
    let focal = if focal.is_finite() && focal > 0.0 { focal.clamp(5.0, 500.0) } else { 50.0 };
    let distance = (radius * 3.2).max(20.0) * (50.0 / focal);
    let cam = solidcraft_render::Camera { target: center, distance, ..Default::default() };
    scene.radius = radius;
    Ok(solidcraft_render::render(&scene, &cam, RENDER_W, RENDER_H))
}

fn image_varied(rgba: &[u8]) -> bool {
    let mut colors = 0u32;
    let mut seen = [0u8; 64];
    for px in rgba.chunks(4).step_by(24) {
        let key = ((px.first().copied().unwrap_or(0) / 16) << 4) | (px.get(1).copied().unwrap_or(0) / 16);
        let slot = (key as usize) % seen.len();
        if seen.get(slot).copied().unwrap_or(1) == 0 {
            if let Some(flag) = seen.get_mut(slot) {
                *flag = 1;
            }
            colors += 1;
        }
    }
    colors >= 2
}

fn write_png(cmd: &str, path: &str, canvas: solidcraft_render::Canvas) -> Result<()> {
    let bytes = solidcraft_render::png(canvas.w, canvas.h, canvas.rgba).ok_or_else(|| bad(cmd, "could not encode the image"))?;
    solidcraft_io::write_atomic(Path::new(path), &bytes, false).map_err(|e| bad(cmd, format!("{path}: {e}")))
}

fn usable_path<'a>(p: &'a Value, cmd: &str) -> Result<&'a str> {
    let path = str_(p, "path").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?;
    if path.len() > 4096 || path.contains('\0') {
        return Err(bad(cmd, "the path is not usable"));
    }
    Ok(path)
}

fn is_video(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "mp4" | "webm" | "mov" | "avi" | "mkv" | "gif")
}

fn sequence_path(path: &str, index: usize, count: usize) -> String {
    if count <= 1 {
        return path.to_string();
    }
    let path = Path::new(path);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("frame");
    let ext = path.extension().and_then(|s| s.to_str()).filter(|ext| !ext.is_empty()).unwrap_or("png");
    let name = format!("{stem}_{index:03}.{ext}");
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name).to_string_lossy().into_owned(),
        _ => name,
    }
}

fn key_span(keys: &[StoryKey]) -> (f64, f64) {
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    for key in keys.iter().take(4096) {
        if key.time.is_finite() {
            lo = lo.min(key.time);
            hi = hi.max(key.time);
        }
    }
    if lo <= hi { (lo, hi) } else { (0.0, 0.0) }
}

fn doc_unchanged(s: &Session, doc: *const solidcraft_doc::Document, pending: &BTreeMap<u64, solidcraft_doc::Mat>) -> bool {
    std::ptr::eq(doc, Arc::as_ptr(&s.doc)) && &s.pending_moves == pending
}

fn named_light(p: &Value, cmd: &str) -> Result<(Option<([f64; 3], f64)>, Vec<String>, bool)> {
    let mut missing = Vec::new();
    let mut light = None;
    let mut named = false;
    for key in ["name", "environment", "asset"] {
        let Some(name) = str_(p, key) else { continue };
        named = true;
        if name.len() > 4096 {
            return Err(bad(cmd, "the environment name is too long"));
        }
        match solidcraft_render::studio(name) {
            Some(studio) => light = Some((studio.color, studio.intensity)),
            None => {
                if !missing.iter().any(|have| have == name) {
                    missing.push(name.to_string());
                }
            }
        }
    }
    Ok((light, missing, named))
}

fn render_environment(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "render.environment";
    let features = s.doc.features.len();
    let occurrences = s.doc.occurrences.clone();
    let pending = s.pending_moves.clone();
    let assembly = s.doc.assembly.clone();
    let (named, mut missing, asked) = named_light(p, cmd)?;
    let mut color = named.map(|(color, _)| color).unwrap_or([1.0, 1.0, 1.0]);
    let mut intensity = named.map(|(_, intensity)| intensity).unwrap_or(1.0);
    if let Some(v) = p.get("color")
        && !v.is_null()
    {
        let values = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| bad(cmd, "`color` is [r, g, b]"))?;
        for (i, slot) in color.iter_mut().enumerate() {
            let n = values.get(i).and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`color` is [r, g, b]"))?;
            *slot = finite(cmd, n, "color")?.clamp(0.0, 4.0);
        }
    }
    if let Some(v) = p.get("intensity") {
        let n = v.as_f64().ok_or_else(|| bad(cmd, "`intensity` must be a number"))?;
        intensity = finite(cmd, n, "intensity")?.clamp(0.0, 8.0);
    }
    let fallback = !missing.is_empty() || (!asked && p.get("color").is_none());
    if !missing.is_empty() && named.is_none() && p.get("color").is_none() {
        color = [1.0, 1.0, 1.0];
        intensity = if p.get("intensity").is_some() { intensity } else { 1.0 };
    }
    let scene = scene(s);
    scene.color = color;
    scene.intensity = intensity;
    if !assembly_same(s, features, &occurrences, &pending, &assembly) {
        return Err(bad(cmd, "the environment changed the design"));
    }
    missing.sort();
    missing.dedup();
    Ok(json!({"color": color, "intensity": intensity, "fallback": fallback, "missing": missing}))
}

fn render_camera(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "render.camera";
    let focal = match p.get("focal") {
        None => 50.0,
        Some(v) => v.as_f64().ok_or_else(|| bad(cmd, "`focal` must be a number"))?,
    };
    let focal = finite(cmd, focal, "focal")?;
    if focal <= 0.0 || focal > 10_000.0 {
        return Err(bad(cmd, "`focal` must be positive"));
    }
    let features = s.doc.features.len();
    let occurrences = s.doc.occurrences.clone();
    scene(s).focal = focal;
    if s.doc.features.len() != features || s.doc.occurrences != occurrences {
        return Err(bad(cmd, "the camera changed the design"));
    }
    Ok(json!({"focal": focal}))
}

fn render_image(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "render.image";
    if p.get("cloud").and_then(Value::as_bool) == Some(true) || str_(p, "renderer").is_some_and(|name| name.eq_ignore_ascii_case("cloud")) {
        return Err(bad(cmd, "not supported yet: a cloud renderer"));
    }
    let path = usable_path(p, cmd)?;
    let doc = Arc::as_ptr(&s.doc);
    let pending = s.pending_moves.clone();
    let (named, mut missing, _) = named_light(p, cmd)?;
    if let Some(name) = str_(p, "appearance") {
        if name.len() > 200 {
            return Err(bad(cmd, "the appearance name is too long"));
        }
        if solidcraft_doc::appearance::library(name).is_none() {
            missing.push(name.to_string());
        }
    }
    let stored = s.doc.render_scene.clone().unwrap_or(RenderScene { color: [1.0, 1.0, 1.0], intensity: 1.0, focal: 50.0 });
    let asked_light = str_(p, "environment").is_some() || str_(p, "asset").is_some() || str_(p, "name").is_some();
    let (color, intensity) = match named {
        Some(light) => light,
        None if asked_light => ([1.0, 1.0, 1.0], 1.0),
        None => (stored.color, stored.intensity),
    };
    let paints = body_paints(s, cmd)?;
    let canvas = render_paints(&paints, &BTreeMap::new(), &origins_of(s), (color, intensity), stored.focal, cmd)?;
    if !image_varied(&canvas.rgba) {
        return Err(bad(cmd, "the render has no shaded solid"));
    }
    if !doc_unchanged(s, doc, &pending) {
        return Err(bad(cmd, "rendering changed the design"));
    }
    let (w, h) = (canvas.w, canvas.h);
    write_png(cmd, path, canvas)?;
    if !doc_unchanged(s, doc, &pending) {
        return Err(bad(cmd, "rendering changed the design"));
    }
    missing.sort();
    missing.dedup();
    Ok(json!({"path": path, "width": w, "height": h, "varied": true, "fallback": !missing.is_empty(), "missing": missing}))
}

fn anim_storyboard(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "anim.storyboard";
    require_assembly(s, cmd)?;
    let name = story_name(p, cmd)?;
    if s.doc.stories.len() >= 64 {
        return Err(bad(cmd, "too many storyboards"));
    }
    let auto = match p.get("auto") {
        None => false,
        Some(Value::Bool(v)) => *v,
        Some(_) => return Err(bad(cmd, "`auto` must be true or false")),
    };
    let (keys, source) = if auto {
        (keys_from_offsets(&auto_moves(s, scale_of(p)?)?), "auto")
    } else {
        let view = chosen_view(s, p)?;
        let study = chosen_study(s, p)?;
        if view.is_some() && study.is_some() {
            return Err(bad(cmd, "pass `view` or `study`, not both"));
        }
        if let Some(view) = view {
            (keys_from_explode(s, &view)?, "explode")
        } else if let Some(study) = study {
            (keys_from_motion(s, &study)?, "motion")
        } else if let Some(view) = s.doc.assembly.exploded.last().cloned() {
            (keys_from_explode(s, &view)?, "explode")
        } else if let Some(study) = s.doc.assembly.studies.last().map(|study| study.name.clone()) {
            (keys_from_motion(s, &study)?, "motion")
        } else {
            (Vec::new(), "empty")
        }
    };
    if keys.len() > 4096 {
        return Err(bad(cmd, "too many keys"));
    }
    keys_resolve(s, &keys, cmd)?;
    let features = s.doc.features.len();
    let occurrences = s.doc.occurrences.clone();
    let pending = s.pending_moves.clone();
    let assembly = s.doc.assembly.clone();
    s.doc_mut().stories.push(Storyboard { name: name.clone(), keys: keys.clone() });
    if !assembly_same(s, features, &occurrences, &pending, &assembly) {
        return Err(bad(cmd, "the storyboard moved the assembly"));
    }
    Ok(json!({"story": name, "keys": keys.len(), "source": source}))
}

fn anim_key(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "anim.key";
    require_assembly(s, cmd)?;
    let time = match p.get("time") {
        Some(v) => v.as_f64().ok_or_else(|| bad(cmd, "`time` must be a number"))?,
        None => return Err(bad(cmd, "`time` is required")),
    };
    let time = finite(cmd, time, "time")?;
    if !(0.0..=1e6).contains(&time) {
        return Err(bad(cmd, "`time` must be 0…1e6 seconds"));
    }
    let occurrence = occurrence_ref(s, p.get("occurrence"), cmd)?;
    let translate = match p.get("translate") {
        None => [0.0, 0.0, 0.0],
        Some(v) => vec3_finite(cmd, v, "translate")?,
    };
    let rotate = match p.get("rotate") {
        None => IDENTITY_ROTATE,
        Some(v) => quat_param(cmd, v)?,
    };
    let name = str_(p, "story");
    if let Some(n) = name
        && (n.is_empty() || n.len() > 200)
    {
        return Err(bad(cmd, "the storyboard name is not usable"));
    }
    let features = s.doc.features.len();
    let occurrences = s.doc.occurrences.clone();
    let pending = s.pending_moves.clone();
    let assembly = s.doc.assembly.clone();
    let count = {
        let stories = &mut s.doc_mut().stories;
        let story =
            stories.iter_mut().rev().find(|story| name.is_none_or(|n| story.name == n)).ok_or_else(|| bad(cmd, "create a storyboard first"))?;
        if story.keys.len() >= 4096 {
            return Err(bad(cmd, "too many keys"));
        }
        story.keys.push(StoryKey { time, occurrence, translate, rotate });
        story.keys.len()
    };
    if !assembly_same(s, features, &occurrences, &pending, &assembly) {
        return Err(bad(cmd, "the key moved the assembly"));
    }
    Ok(json!({"keys": count}))
}

fn anim_play(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "anim.play";
    require_assembly(s, cmd)?;
    let doc = Arc::as_ptr(&s.doc);
    let pending = s.pending_moves.clone();
    let name = str_(p, "story");
    let story = s.doc.stories.iter().rev().find(|story| name.is_none_or(|n| story.name == n)).ok_or_else(|| bad(cmd, "no storyboard"))?;
    let keys = story.keys.clone();
    let story_name = story.name.clone();
    keys_resolve(s, &keys, cmd)?;
    let time = match p.get("time") {
        None => 0.0,
        Some(v) => finite(cmd, v.as_f64().ok_or_else(|| bad(cmd, "`time` must be a number"))?, "time")?,
    };
    if time.abs() > 1e9 {
        return Err(bad(cmd, "`time` is too large"));
    }
    let mut poses = Vec::new();
    for occurrence in s.doc.occurrences.iter().take(10_000) {
        let (translate, rotate) = if keys.iter().any(|key| key.occurrence == occurrence.id) {
            pose_of(&keys, occurrence.id, time, cmd)?
        } else {
            ([0.0, 0.0, 0.0], IDENTITY_ROTATE)
        };
        poses.push(json!({"occurrence": occurrence.id, "name": occurrence.name, "translate": translate, "rotate": rotate}));
    }
    if !doc_unchanged(s, doc, &pending) {
        return Err(bad(cmd, "playback moved the assembly"));
    }
    let (start, end) = key_span(&keys);
    Ok(json!({"story": story_name, "time": time, "keys": keys.len(), "start": start, "end": end, "poses": poses}))
}

fn anim_publish(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "anim.publish";
    let path = usable_path(p, cmd)?;
    require_assembly(s, cmd)?;
    let name = str_(p, "story");
    let story = s.doc.stories.iter().rev().find(|story| name.is_none_or(|n| story.name == n)).ok_or_else(|| bad(cmd, "create a storyboard first"))?;
    let keys = story.keys.clone();
    keys_resolve(s, &keys, cmd)?;
    if is_video(path) {
        return Err(bad(cmd, "not supported yet: video. Publish a png image or an image sequence"));
    }
    let frames = match p.get("frames") {
        None => {
            let (start, end) = key_span(&keys);
            if end - start <= 1e-9 { 1 } else { 4 }
        }
        Some(v) => {
            let n = v.as_u64().filter(|n| (1..=24).contains(n)).ok_or_else(|| bad(cmd, "`frames` must be 1…24"))?;
            n as usize
        }
    };
    let doc = Arc::as_ptr(&s.doc);
    let pending = s.pending_moves.clone();
    let paints = body_paints(s, cmd)?;
    let origins = origins_of(s);
    let stored = s.doc.render_scene.clone().unwrap_or(RenderScene { color: [1.0, 1.0, 1.0], intensity: 1.0, focal: 50.0 });
    let (start, end) = key_span(&keys);
    let mut paths = Vec::new();
    for index in 0..frames {
        let t = if frames <= 1 { start } else { start + (end - start) * index as f64 / (frames - 1) as f64 };
        let poses = pose_map(&keys, t, cmd)?;
        let canvas = render_paints(&paints, &poses, &origins, (stored.color, stored.intensity), stored.focal, cmd)?;
        let file = sequence_path(path, index, frames);
        write_png(cmd, &file, canvas)?;
        paths.push(file);
    }
    if !doc_unchanged(s, doc, &pending) {
        return Err(bad(cmd, "publishing moved the assembly"));
    }
    Ok(json!({"path": path, "frames": frames, "paths": paths}))
}

// Electronics. Part names are the caller's; there is no vendor library.
// ERC and DRC only report. PCB autorouting stays deferred.
const MAX_ECAD_PARTS: usize = 1024;
const MAX_ECAD_PINS: usize = 64;
const MAX_ECAD_WIRES: usize = 4096;
const MAX_ECAD_TRACES: usize = 4096;
const MAX_ECAD_NETS: usize = 1024;
const MAX_ECAD_NODES: usize = 8192;
const MAX_ECAD_NAME: usize = 64;
const DEFAULT_CLEARANCE: f64 = 0.2;
const DEFAULT_MIN_WIDTH: f64 = 0.15;
const DEFAULT_THICKNESS: f64 = 1.6;

fn ecad_schematic(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.schematic";
    ecad_deferred(p, cmd)?;
    let components = read_components(cmd, p)?;
    let explicit = read_nets(cmd, p)?;
    let wires = expand_nets(cmd, &explicit)?;
    let (board_width, board_height, traces) = preserved_layout(s);
    let parts = components.iter().map(|part| part.refdes.clone()).collect();
    let mut schematic = Schematic { parts, wires, board_width, board_height, traces, components, nets: explicit };
    store_nets(cmd, &mut schematic)?;
    let parts = schematic.parts.len();
    let components = schematic.components.len();
    let nets = schematic.nets.len();
    let wires = schematic.wires.len();
    s.doc_mut().schematic = Some(schematic);
    Ok(json!({"parts": parts, "components": components, "nets": nets, "wires": wires}))
}

fn ecad_wire(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.wire";
    ecad_deferred(p, cmd)?;
    let a = ecad_name(cmd, str_(p, "a").ok_or_else(|| bad(cmd, "`a` is required"))?, "a", true)?;
    let b = ecad_name(cmd, str_(p, "b").ok_or_else(|| bad(cmd, "`b` is required"))?, "b", true)?;
    if a.eq_ignore_ascii_case(&b) {
        return Err(bad(cmd, "a wire needs two different names"));
    }
    if s.doc.schematic.as_ref().is_some_and(|schematic| schematic.wires.len() >= MAX_ECAD_WIRES) {
        return Err(bad(cmd, "too many wires"));
    }
    let schematic = s.doc_mut().schematic.get_or_insert_with(Schematic::default);
    schematic.wires.push((a, b));
    store_nets(cmd, schematic)?;
    Ok(json!({"wires": schematic.wires.len(), "nets": schematic.nets.len()}))
}

fn ecad_erc(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.erc";
    ecad_deferred(p, cmd)?;
    let before = s.doc.to_json();
    let report = match s.doc.schematic.clone() {
        None => json!({"shorts": 0, "unconnected": 0, "open_pins": 0, "nets": 0}),
        Some(schematic) => erc_report(cmd, &schematic)?,
    };
    if s.doc.to_json() != before {
        return Err(bad(cmd, "the check edited the schematic"));
    }
    Ok(report)
}

fn ecad_board(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.board";
    ecad_deferred(p, cmd)?;
    let width = positive_mm(cmd, p, "width")?;
    let height = positive_mm(cmd, p, "height")?;
    let traces = copper(s);
    if traces.iter().any(|trace| !trace_ok(trace)) {
        return Err(bad(cmd, "a trace is not usable"));
    }
    set_layout(s, width, height, traces);
    let count = s.doc.board.as_ref().map(|board| board.traces.len()).unwrap_or(0);
    Ok(json!({"width": width, "height": height, "traces": count}))
}

fn ecad_trace(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.trace";
    ecad_deferred(p, cmd)?;
    let mut coords = [0.0; 4];
    for (index, key) in ["x0", "y0", "x1", "y1"].into_iter().enumerate() {
        let value = p.get(key).and_then(Value::as_f64).ok_or_else(|| bad(cmd, format!("`{key}` is required")))?;
        if let Some(slot) = coords.get_mut(index) {
            *slot = board_mm(cmd, value, key)?;
        }
    }
    let width = positive_mm(cmd, p, "width")?;
    if width >= 100.0 {
        return Err(bad(cmd, "trace width must be under 100 mm"));
    }
    let net = match str_(p, "net") {
        None => String::new(),
        Some(net) => ecad_name(cmd, net, "net", true)?,
    };
    let mut traces = copper(s);
    if traces.len() >= MAX_ECAD_TRACES {
        return Err(bad(cmd, "too many traces"));
    }
    if traces.iter().any(|trace| !trace_ok(trace)) {
        return Err(bad(cmd, "a trace is not usable"));
    }
    let (board_width, board_height) = s
        .doc
        .board
        .as_ref()
        .map(|board| (board.width, board.height))
        .unwrap_or_else(|| s.doc.schematic.as_ref().map(|schematic| (schematic.board_width, schematic.board_height)).unwrap_or((0.0, 0.0)));
    traces.push(EcadTrace { x0: coords[0], y0: coords[1], x1: coords[2], y1: coords[3], width, net });
    set_layout(s, board_width, board_height, traces);
    let count = s.doc.board.as_ref().map(|board| board.traces.len()).unwrap_or(0);
    Ok(json!({"traces": count}))
}

fn ecad_drc(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.drc";
    ecad_deferred(p, cmd)?;
    let clearance = optional_mm(cmd, p, "clearance", DEFAULT_CLEARANCE)?;
    let min_width = optional_mm(cmd, p, "min_width", DEFAULT_MIN_WIDTH)?;
    let before = s.doc.to_json();
    let traces = copper(s);
    let (clearance_hits, width_hits) = count_drc(&traces, clearance, min_width)?;
    if s.doc.to_json() != before {
        return Err(bad(cmd, "the check moved the copper"));
    }
    Ok(json!({"violations": clearance_hits + width_hits, "clearance": clearance_hits, "width": width_hits}))
}

fn ecad_push(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.push_3d";
    ecad_deferred(p, cmd)?;
    if p.get("models").and_then(Value::as_bool) == Some(true) {
        return Err(bad(cmd, "not supported yet: component models on the board"));
    }
    let thickness = thickness_of(p, cmd)?;
    let (width, height) = board_size(s).ok_or_else(|| bad(cmd, "create a board first"))?;
    if pcb_exists(s) {
        return existing_board(width, height, thickness, pcb_volume(s));
    }
    let active = s.active_component;
    let active_path = s.active_occurrence_path.clone();
    let placed = place_board(s, width, height, thickness);
    s.active_component = active;
    s.active_occurrence_path = active_path;
    placed
}

fn ecad_export(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.export";
    // Public Gerber RS-274X subset. PCB autorouting stays deferred.
    ecad_deferred(p, cmd)?;
    let path = usable_path(p, cmd)?;
    let (width, height) = board_size(s).ok_or_else(|| bad(cmd, "create a board first"))?;
    let traces = copper(s);
    if traces.iter().any(|trace| !trace_ok(trace)) {
        return Err(bad(cmd, "a trace is not usable"));
    }
    let layer = solidcraft_io::GerberLayer {
        width,
        height,
        traces: traces
            .iter()
            .map(|trace| solidcraft_io::GerberTrace { x0: trace.x0, y0: trace.y0, x1: trace.x1, y1: trace.y1, width: trace.width })
            .collect(),
    };
    let text = solidcraft_io::write_gerber(&layer).map_err(|err| bad(cmd, err.to_string()))?;
    let back = solidcraft_io::read_gerber(&text).map_err(|err| bad(cmd, format!("export was not readable: {err}")))?;
    if (back.width - width).abs() > 1e-4 || (back.height - height).abs() > 1e-4 || back.traces.len() != layer.traces.len() {
        return Err(bad(cmd, "export did not round-trip"));
    }
    let before = s.doc.to_json();
    solidcraft_io::write_atomic(Path::new(path), text.as_bytes(), false).map_err(|err| bad(cmd, format!("{path}: {err}")))?;
    if s.doc.to_json() != before {
        return Err(bad(cmd, "export changed the design"));
    }
    Ok(json!({"path": path, "format": "gerber-rs274x", "traces": layer.traces.len()}))
}

fn ecad_deferred(p: &Value, cmd: &str) -> Result<()> {
    if matches!(p.get("autoroute"), Some(Value::Bool(true))) || matches!(p.get("route"), Some(Value::Bool(true))) {
        return Err(bad(cmd, "not supported yet: PCB autorouting"));
    }
    let Some(layer) = p.get("layer") else { return Ok(()) };
    let top = match layer {
        Value::String(name) => name.eq_ignore_ascii_case("top") || name == "1",
        Value::Number(number) => number.as_u64() == Some(1),
        _ => false,
    };
    if top { Ok(()) } else { Err(bad(cmd, "not supported yet: only one copper layer")) }
}

fn ecad_name(cmd: &str, raw: &str, what: &str, dotted: bool) -> Result<String> {
    let name = raw.trim();
    let rejected = |c: char| c.is_control() || c.is_whitespace() || matches!(c, '*' | '%' | ',' | '"') || (!dotted && c == '.');
    if name.is_empty() || name.len() > MAX_ECAD_NAME || name.chars().any(rejected) {
        return Err(bad(cmd, format!("`{what}` must be 1…{MAX_ECAD_NAME} characters")));
    }
    Ok(name.to_string())
}

fn board_mm(cmd: &str, value: f64, what: &str) -> Result<f64> {
    let value = finite(cmd, value, what)?;
    if value.abs() >= 10_000.0 {
        return Err(bad(cmd, format!("`{what}` must be within ±9999 mm")));
    }
    Ok(value)
}

fn positive_mm(cmd: &str, p: &Value, key: &str) -> Result<f64> {
    let value = p.get(key).and_then(Value::as_f64).ok_or_else(|| bad(cmd, format!("`{key}` is required")))?;
    let value = board_mm(cmd, value, key)?;
    if value <= 0.0 {
        return Err(bad(cmd, format!("`{key}` must be positive")));
    }
    Ok(value)
}

fn optional_mm(cmd: &str, p: &Value, key: &str, default: f64) -> Result<f64> {
    let Some(value) = p.get(key) else { return Ok(default) };
    let value = value.as_f64().ok_or_else(|| bad(cmd, format!("`{key}` must be a number")))?;
    let value = finite(cmd, value, key)?;
    if value < 0.0 || value >= 10_000.0 {
        return Err(bad(cmd, format!("`{key}` must be from 0 to 9999 mm")));
    }
    Ok(value)
}

fn thickness_of(p: &Value, cmd: &str) -> Result<f64> {
    let thickness = optional_mm(cmd, p, "thickness", DEFAULT_THICKNESS)?;
    if thickness <= 0.0 || thickness > 100.0 {
        return Err(bad(cmd, "`thickness` must be positive and at most 100 mm"));
    }
    Ok(thickness)
}

fn read_components(cmd: &str, p: &Value) -> Result<Vec<EcadComponent>> {
    let Some(value) = p.get("parts") else { return Ok(Vec::new()) };
    match value {
        Value::String(name) => Ok(vec![bare_part(cmd, name)?]),
        Value::Array(items) => read_part_list(cmd, items),
        _ => Err(bad(cmd, "`parts` must be a list of names")),
    }
}

fn bare_part(cmd: &str, name: &str) -> Result<EcadComponent> {
    Ok(EcadComponent { refdes: ecad_name(cmd, name, "refdes", false)?, ..EcadComponent::default() })
}

fn read_part_list(cmd: &str, items: &[Value]) -> Result<Vec<EcadComponent>> {
    if items.len() > MAX_ECAD_PARTS {
        return Err(bad(cmd, "too many parts"));
    }
    let mut components = Vec::with_capacity(items.len());
    for item in items {
        let component = match item {
            Value::String(name) => bare_part(cmd, name)?,
            Value::Object(obj) => read_part_object(cmd, obj)?,
            _ => return Err(bad(cmd, "`parts` must be a list of names")),
        };
        if components.iter().any(|have: &EcadComponent| have.refdes.eq_ignore_ascii_case(&component.refdes)) {
            return Err(bad(cmd, "a refdes is repeated"));
        }
        components.push(component);
    }
    Ok(components)
}

fn read_part_object(cmd: &str, obj: &serde_json::Map<String, Value>) -> Result<EcadComponent> {
    if ["footprint", "library", "package", "mpn"].iter().any(|key| obj.contains_key(*key)) {
        return Err(bad(cmd, "not supported yet: part libraries. Components are refdes and pin names only"));
    }
    let refdes = obj.get("refdes").or_else(|| obj.get("name")).and_then(Value::as_str).ok_or_else(|| bad(cmd, "a part needs a refdes"))?;
    let (x, y) = placement(cmd, obj)?;
    Ok(EcadComponent { refdes: ecad_name(cmd, refdes, "refdes", false)?, pins: read_pins(cmd, obj)?, x, y })
}

fn read_pins(cmd: &str, obj: &serde_json::Map<String, Value>) -> Result<Vec<String>> {
    let Some(value) = obj.get("pins") else { return Ok(Vec::new()) };
    let items = value.as_array().ok_or_else(|| bad(cmd, "`pins` must be a list of names"))?;
    if items.len() > MAX_ECAD_PINS {
        return Err(bad(cmd, "too many pins"));
    }
    let mut pins = Vec::with_capacity(items.len());
    for item in items {
        let pin = item.as_str().ok_or_else(|| bad(cmd, "`pins` must be a list of names"))?;
        let pin = ecad_name(cmd, pin, "pin", false)?;
        if pins.iter().any(|have: &String| have.eq_ignore_ascii_case(&pin)) {
            return Err(bad(cmd, "a pin name is repeated"));
        }
        pins.push(pin);
    }
    Ok(pins)
}

fn placement(cmd: &str, obj: &serde_json::Map<String, Value>) -> Result<(Option<f64>, Option<f64>)> {
    match (obj.get("x"), obj.get("y")) {
        (None, None) => Ok((None, None)),
        (Some(x), Some(y)) => {
            let x = x.as_f64().ok_or_else(|| bad(cmd, "`x` must be a number"))?;
            let y = y.as_f64().ok_or_else(|| bad(cmd, "`y` must be a number"))?;
            Ok((Some(board_mm(cmd, x, "x")?), Some(board_mm(cmd, y, "y")?)))
        }
        _ => Err(bad(cmd, "`x` and `y` are placed together")),
    }
}

fn read_nets(cmd: &str, p: &Value) -> Result<Vec<EcadNet>> {
    let Some(value) = p.get("nets") else { return Ok(Vec::new()) };
    let items = value.as_array().ok_or_else(|| bad(cmd, "`nets` must be a list"))?;
    if items.len() > MAX_ECAD_NETS {
        return Err(bad(cmd, "too many nets"));
    }
    let mut nets = Vec::with_capacity(items.len());
    for item in items {
        let obj = item.as_object().ok_or_else(|| bad(cmd, "`nets` must be a list of {name, nodes}"))?;
        let name = obj.get("name").and_then(Value::as_str).ok_or_else(|| bad(cmd, "a net needs a name"))?;
        let name = ecad_name(cmd, name, "name", true)?;
        if nets.iter().any(|net: &EcadNet| net.name.eq_ignore_ascii_case(&name)) {
            return Err(bad(cmd, "a net name is repeated"));
        }
        let nodes = read_nodes(cmd, obj)?;
        nets.push(EcadNet { name, nodes });
    }
    Ok(nets)
}

fn read_nodes(cmd: &str, obj: &serde_json::Map<String, Value>) -> Result<Vec<String>> {
    let items = obj.get("nodes").and_then(Value::as_array).ok_or_else(|| bad(cmd, "a net needs nodes"))?;
    if items.is_empty() || items.len() > MAX_ECAD_PINS {
        return Err(bad(cmd, "a net needs 1…64 nodes"));
    }
    let mut nodes = Vec::with_capacity(items.len());
    for item in items {
        let node = item.as_str().ok_or_else(|| bad(cmd, "a net node must be a name"))?;
        let node = ecad_name(cmd, node, "node", true)?;
        if nodes.iter().any(|have: &String| have.eq_ignore_ascii_case(&node)) {
            return Err(bad(cmd, "a net lists the same node twice"));
        }
        nodes.push(node);
    }
    Ok(nodes)
}

fn expand_nets(cmd: &str, nets: &[EcadNet]) -> Result<Vec<(String, String)>> {
    let mut wires = Vec::new();
    for net in nets {
        let Some(first) = net.nodes.first() else { continue };
        for other in net.nodes.iter().skip(1) {
            if wires.len() >= MAX_ECAD_WIRES {
                return Err(bad(cmd, "too many wires"));
            }
            wires.push((first.clone(), other.clone()));
        }
    }
    Ok(wires)
}

fn preserved_layout(s: &Session) -> (f64, f64, Vec<[f64; 5]>) {
    if let Some(board) = &s.doc.board {
        return (board.width, board.height, board.traces.iter().map(trace_array).collect());
    }
    match &s.doc.schematic {
        Some(schematic) => (schematic.board_width, schematic.board_height, schematic.traces.clone()),
        None => (0.0, 0.0, Vec::new()),
    }
}

fn store_nets(cmd: &str, schematic: &mut Schematic) -> Result<()> {
    schematic.nets = classify_nets(cmd, &schematic.wires, &schematic.nets)?;
    Ok(())
}

fn classify_nets(cmd: &str, wires: &[(String, String)], preferred: &[EcadNet]) -> Result<Vec<EcadNet>> {
    let mut names = Vec::new();
    let mut parent = Vec::new();
    for (a, b) in wires.iter().take(MAX_ECAD_WIRES) {
        let left = intern(cmd, &mut names, &mut parent, a)?;
        let right = intern(cmd, &mut names, &mut parent, b)?;
        unite(&mut parent, left, right);
    }
    let mut order = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for index in 0..names.len() {
        let root = find(&mut parent, index);
        if let Some(place) = order.iter().position(|have| *have == root) {
            if let Some(group) = groups.get_mut(place) {
                group.push(index);
            }
        } else {
            order.push(root);
            groups.push(vec![index]);
        }
    }
    let mut nets = Vec::new();
    for group in &groups {
        let nodes = group.iter().filter_map(|index| names.get(*index).cloned()).collect::<Vec<_>>();
        let name = choose_net_name(&nodes, preferred);
        nets.push(EcadNet { name, nodes });
    }
    for pref in preferred.iter().take(MAX_ECAD_NETS) {
        let Some(only) = pref.nodes.first() else { continue };
        if pref.nodes.len() == 1 && !pref.name.is_empty() && !nets.iter().any(|net| net.nodes.iter().any(|node| node.eq_ignore_ascii_case(only))) {
            nets.push(pref.clone());
        }
    }
    Ok(nets)
}

fn choose_net_name(nodes: &[String], preferred: &[EcadNet]) -> String {
    let mut best: Option<&EcadNet> = None;
    for pref in preferred {
        if pref.name.is_empty() || pref.nodes.is_empty() {
            continue;
        }
        let inside = pref.nodes.iter().all(|node| nodes.iter().any(|have| have.eq_ignore_ascii_case(node)));
        if !inside {
            continue;
        }
        let replace = best
            .is_none_or(|current| pref.nodes.len() > current.nodes.len() || (pref.nodes.len() == current.nodes.len() && pref.name < current.name));
        if replace {
            best = Some(pref);
        }
    }
    if let Some(pref) = best {
        return pref.name.clone();
    }
    if let Some(node) = nodes.iter().find(|node| net_class(node) == 1) {
        return node.clone();
    }
    if let Some(node) = nodes.iter().find(|node| net_class(node) == 2) {
        return node.clone();
    }
    nodes.first().cloned().unwrap_or_default()
}

fn net_class(name: &str) -> u8 {
    match name.to_ascii_lowercase().as_str() {
        "vcc" | "vdd" | "v+" | "pwr" | "power" | "+5v" | "+3v3" | "vbus" => 1,
        "gnd" | "vss" | "ground" | "0v" => 2,
        _ => 0,
    }
}

fn erc_report(cmd: &str, schematic: &Schematic) -> Result<Value> {
    if schematic.wires.len() > MAX_ECAD_WIRES
        || schematic.parts.len() > MAX_ECAD_PARTS
        || schematic.components.len() > MAX_ECAD_PARTS
        || schematic.nets.len() > MAX_ECAD_NETS
    {
        return Err(bad(cmd, "the schematic is too large"));
    }
    let (names, mut parent) = connect(cmd, schematic)?;
    let shorts = short_count(&names, &mut parent);
    let parts = erc_parts(schematic);
    let unconnected = parts.iter().filter(|part| part_unconnected(part, &names)).count();
    let open_pins = open_pin_count(&parts, &names);
    Ok(json!({"shorts": shorts, "unconnected": unconnected, "open_pins": open_pins, "nets": schematic.nets.len()}))
}

fn erc_parts(schematic: &Schematic) -> Vec<EcadComponent> {
    if !schematic.components.is_empty() {
        return schematic.components.clone();
    }
    schematic.parts.iter().take(MAX_ECAD_PARTS).map(|refdes| EcadComponent { refdes: refdes.clone(), ..EcadComponent::default() }).collect()
}

fn part_unconnected(part: &EcadComponent, names: &[String]) -> bool {
    let named = names.iter().any(|name| name.eq_ignore_ascii_case(&part.refdes));
    if part.pins.is_empty() {
        return !named;
    }
    let pin = part.pins.iter().any(|pin| names.iter().any(|name| name.eq_ignore_ascii_case(&format!("{}.{}", part.refdes, pin))));
    !named && !pin
}

fn open_pin_count(parts: &[EcadComponent], names: &[String]) -> usize {
    let mut count = 0;
    for part in parts.iter().take(MAX_ECAD_PARTS) {
        for pin in part.pins.iter().take(MAX_ECAD_PINS) {
            let node = format!("{}.{}", part.refdes, pin);
            if !names.iter().any(|name| name.eq_ignore_ascii_case(&node)) {
                count += 1;
            }
        }
    }
    count
}

fn connect(cmd: &str, schematic: &Schematic) -> Result<(Vec<String>, Vec<usize>)> {
    let mut names = Vec::new();
    let mut parent = Vec::new();
    for (a, b) in schematic.wires.iter().take(MAX_ECAD_WIRES) {
        let left = intern(cmd, &mut names, &mut parent, a)?;
        let right = intern(cmd, &mut names, &mut parent, b)?;
        unite(&mut parent, left, right);
    }
    for net in schematic.nets.iter().take(MAX_ECAD_NETS) {
        let Some(first) = net.nodes.first() else { continue };
        let head = intern(cmd, &mut names, &mut parent, first)?;
        for other in net.nodes.iter().skip(1).take(MAX_ECAD_PINS) {
            let node = intern(cmd, &mut names, &mut parent, other)?;
            unite(&mut parent, head, node);
        }
    }
    Ok((names, parent))
}

fn intern(cmd: &str, names: &mut Vec<String>, parent: &mut Vec<usize>, name: &str) -> Result<usize> {
    if let Some(index) = names.iter().position(|have| have.eq_ignore_ascii_case(name)) {
        return Ok(index);
    }
    if names.len() >= MAX_ECAD_NODES {
        return Err(bad(cmd, "the schematic is too large"));
    }
    names.push(name.to_string());
    parent.push(parent.len());
    Ok(parent.len() - 1)
}

fn find(parent: &mut [usize], start: usize) -> usize {
    let mut root = start;
    for _ in 0..MAX_ECAD_NODES {
        let Some(&next) = parent.get(root) else { return root };
        if next == root {
            break;
        }
        root = next;
    }
    let mut node = start;
    for _ in 0..MAX_ECAD_NODES {
        let Some(&next) = parent.get(node) else { break };
        if let Some(slot) = parent.get_mut(node) {
            *slot = root;
        }
        if next == node || next == root {
            break;
        }
        node = next;
    }
    root
}

fn unite(parent: &mut [usize], a: usize, b: usize) {
    let left = find(parent, a);
    let right = find(parent, b);
    if left != right
        && let Some(slot) = parent.get_mut(left)
    {
        *slot = right;
    }
}

fn short_count(names: &[String], parent: &mut [usize]) -> usize {
    let mut power = vec![false; names.len()];
    let mut ground = vec![false; names.len()];
    for (index, name) in names.iter().enumerate() {
        let root = find(parent, index);
        match net_class(name) {
            1 => {
                if let Some(flag) = power.get_mut(root) {
                    *flag = true;
                }
            }
            2 => {
                if let Some(flag) = ground.get_mut(root) {
                    *flag = true;
                }
            }
            _ => {}
        }
    }
    let mut shorts = 0;
    for index in 0..names.len() {
        if find(parent, index) == index && power.get(index).copied() == Some(true) && ground.get(index).copied() == Some(true) {
            shorts += 1;
        }
    }
    shorts
}

fn copper(s: &Session) -> Vec<EcadTrace> {
    if let Some(board) = &s.doc.board {
        return board.traces.clone();
    }
    let Some(schematic) = &s.doc.schematic else { return Vec::new() };
    schematic.traces.iter().copied().map(trace_from_array).collect()
}

fn trace_from_array(trace: [f64; 5]) -> EcadTrace {
    EcadTrace { x0: trace[0], y0: trace[1], x1: trace[2], y1: trace[3], width: trace[4], net: String::new() }
}

fn trace_array(trace: &EcadTrace) -> [f64; 5] {
    [trace.x0, trace.y0, trace.x1, trace.y1, trace.width]
}

fn trace_ok(trace: &EcadTrace) -> bool {
    [trace.x0, trace.y0, trace.x1, trace.y1, trace.width].iter().all(|value| value.is_finite() && value.abs() < 10_000.0)
        && trace.width > 0.0
        && trace.width < 100.0
}

fn set_layout(s: &mut Session, width: f64, height: f64, traces: Vec<EcadTrace>) {
    s.doc_mut().board = Some(Board { width, height, traces });
    mirror_layout(s);
}

fn mirror_layout(s: &mut Session) {
    let Some(board) = s.doc.board.clone() else { return };
    let schematic = s.doc_mut().schematic.get_or_insert_with(Schematic::default);
    schematic.board_width = board.width;
    schematic.board_height = board.height;
    schematic.traces = board.traces.iter().map(trace_array).collect();
}

fn board_size(s: &Session) -> Option<(f64, f64)> {
    if let Some(board) = &s.doc.board
        && board.width > 0.0
        && board.height > 0.0
    {
        return Some((board.width, board.height));
    }
    let schematic = s.doc.schematic.as_ref()?;
    (schematic.board_width > 0.0 && schematic.board_height > 0.0).then_some((schematic.board_width, schematic.board_height))
}

fn count_drc(traces: &[EcadTrace], clearance: f64, min_width: f64) -> Result<(usize, usize)> {
    if traces.len() > MAX_ECAD_TRACES {
        return Err(bad("ecad.drc", "too many traces"));
    }
    if traces.iter().any(|trace| !trace_ok(trace)) {
        return Err(bad("ecad.drc", "a trace is not usable"));
    }
    let mut clearance_hits = 0;
    let mut width_hits = 0;
    for (index, left) in traces.iter().enumerate() {
        if left.width + 1e-9 < min_width {
            width_hits += 1;
        }
        for right in traces.iter().skip(index + 1) {
            if same_net(left, right) {
                continue;
            }
            if edge_gap(left, right) + 1e-9 < clearance {
                clearance_hits += 1;
            }
        }
    }
    Ok((clearance_hits, width_hits))
}

fn same_net(left: &EcadTrace, right: &EcadTrace) -> bool {
    !left.net.is_empty() && left.net.eq_ignore_ascii_case(&right.net)
}

fn edge_gap(left: &EcadTrace, right: &EcadTrace) -> f64 {
    let distance = segment_distance([left.x0, left.y0], [left.x1, left.y1], [right.x0, right.y0], [right.x1, right.y1]);
    distance - (left.width + right.width) * 0.5
}

fn segment_distance(a0: [f64; 2], a1: [f64; 2], b0: [f64; 2], b1: [f64; 2]) -> f64 {
    if segments_touch(a0, a1, b0, b1) {
        return 0.0;
    }
    point_segment_distance(a0[0], a0[1], b0[0], b0[1], b1[0], b1[1])
        .min(point_segment_distance(a1[0], a1[1], b0[0], b0[1], b1[0], b1[1]))
        .min(point_segment_distance(b0[0], b0[1], a0[0], a0[1], a1[0], a1[1]))
        .min(point_segment_distance(b1[0], b1[1], a0[0], a0[1], a1[0], a1[1]))
}

fn point_segment_distance(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 < 1e-18 { 0.0 } else { ((px - ax) * dx + (py - ay) * dy) / len2 };
    let t = t.clamp(0.0, 1.0);
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

fn segments_touch(a0: [f64; 2], a1: [f64; 2], b0: [f64; 2], b1: [f64; 2]) -> bool {
    let o1 = orient(a0[0], a0[1], a1[0], a1[1], b0[0], b0[1]);
    let o2 = orient(a0[0], a0[1], a1[0], a1[1], b1[0], b1[1]);
    let o3 = orient(b0[0], b0[1], b1[0], b1[1], a0[0], a0[1]);
    let o4 = orient(b0[0], b0[1], b1[0], b1[1], a1[0], a1[1]);
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    let eps = 1e-9;
    (o1.abs() <= eps && near_segment(a0, a1, b0))
        || (o2.abs() <= eps && near_segment(a0, a1, b1))
        || (o3.abs() <= eps && near_segment(b0, b1, a0))
        || (o4.abs() <= eps && near_segment(b0, b1, a1))
}

fn orient(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> f64 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

fn near_segment(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> bool {
    let eps = 1e-9;
    p[0] >= a[0].min(b[0]) - eps && p[0] <= a[0].max(b[0]) + eps && p[1] >= a[1].min(b[1]) - eps && p[1] <= a[1].max(b[1]) + eps
}

fn pcb_exists(s: &Session) -> bool {
    s.model.state().body("PCB").is_some()
}

fn pcb_volume(s: &Session) -> Option<f64> {
    let state = s.model.state();
    let body = state.body("PCB")?;
    solidcraft_kernel::measure(&body.body).ok().map(|measured| measured.volume)
}

fn pcb_owner(s: &Session) -> Option<u64> {
    let state = s.model.state();
    let body = state.body("PCB")?;
    Some(s.doc.model_body_component(body))
}

fn existing_board(width: f64, height: f64, thickness: f64, volume: Option<f64>) -> Result<Value> {
    let cmd = "ecad.push_3d";
    let Some(volume) = volume else {
        return Err(bad(cmd, "not supported yet: updating a pushed board"));
    };
    let want = width * height * thickness;
    if (volume - want).abs() <= 1e-3 * want.max(1.0) {
        return Ok(json!({"body": "PCB", "component": "PCB", "unchanged": true, "thickness": thickness}));
    }
    Err(bad(cmd, "not supported yet: updating a pushed board"))
}

fn place_board(s: &mut Session, width: f64, height: f64, thickness: f64) -> Result<Value> {
    let cmd = "ecad.push_3d";
    let created = s.execute("component.create", &json!({"name": "PCB"}))?;
    let component = created.get("component").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "the PCB component was not created"))?;
    let occurrence = created.get("occurrence").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "the PCB component has no occurrence"))?;
    s.execute("solid.box", &json!({"length": width, "width": height, "height": thickness, "body_name": "PCB", "operation": "new"}))?;
    let owner = pcb_owner(s).ok_or_else(|| bad(cmd, "the PCB body was not created"))?;
    if owner != component {
        return Err(bad(cmd, "the PCB body is not in its component"));
    }
    Ok(json!({
        "body": "PCB",
        "component": "PCB",
        "component_id": component,
        "occurrence": occurrence,
        "width": width,
        "height": height,
        "thickness": thickness,
    }))
}
fn drawing_dxf(s: &mut Session, p: &Value) -> Result<Value> {
    super::drawing::export_dxf(s, p)
}

fn name_list(cmd: &str, p: &Value, key: &str) -> Result<Option<Vec<String>>> {
    let Some(value) = p.get(key) else { return Ok(None) };
    match value {
        Value::String(one) => {
            if one.is_empty() || one.len() > 200 {
                return Err(bad(cmd, "a body name is not usable"));
            }
            Ok(Some(vec![one.clone()]))
        }
        Value::Array(items) => {
            if items.len() > generative::MAX_REGIONS {
                return Err(bad(cmd, "the study exceeds the size cap"));
            }
            let mut names = Vec::with_capacity(items.len());
            for item in items {
                let Some(name) = item.as_str() else {
                    return Err(bad(cmd, format!("`{key}` must be a list of names")));
                };
                if name.is_empty() || name.len() > 200 {
                    return Err(bad(cmd, "a body name is not usable"));
                }
                names.push(name.to_string());
            }
            Ok(Some(names))
        }
        _ => Err(bad(cmd, format!("`{key}` must be a list of names"))),
    }
}

fn require_names(s: &Session, cmd: &str, names: &[String]) -> Result<()> {
    for name in names {
        require_body(s, cmd, name)?;
    }
    Ok(())
}

fn body_region(s: &Session, cmd: &str, body: &str) -> Result<generative::BoxRegion> {
    let solid = s.model.state().body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?.body.clone();
    let measured = solidcraft_kernel::measure(&solid).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(generative::BoxRegion {
        min: [measured.bbox.min.x, measured.bbox.min.y, measured.bbox.min.z],
        max: [measured.bbox.max.x, measured.bbox.max.y, measured.bbox.max.z],
    })
}

fn blank_generative(
    name: String,
    preserve: Vec<String>,
    obstacle: Vec<String>,
    objective: String,
    loads: Vec<StudyLoad>,
    constraints: Vec<StudyConstraint>,
) -> GenerativeStudy {
    GenerativeStudy {
        name,
        preserve,
        obstacle,
        objective,
        method: String::new(),
        candidate: None,
        loads,
        constraints,
        length: 0.0,
        width: 0.0,
        height: 0.0,
        origin: [0.0, 0.0, 0.0],
        stale: true,
    }
}

fn gen_study(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generative.study";
    let name = study_label(cmd, p, "Study")?;
    let preserve = name_list(cmd, p, "preserve")?;
    let obstacle = name_list(cmd, p, "obstacle")?;
    let objective = match str_(p, "objective") {
        None => None,
        Some(value) => {
            if !matches!(value, "stiffness" | "mass") {
                return Err(bad(cmd, "objective must be stiffness or mass"));
            }
            Some(value.to_string())
        }
    };
    let loads = loads_of_params(cmd, p)?;
    let constraints = constraints_of(cmd, p)?;
    if let Some(names) = &preserve {
        require_names(s, cmd, names)?;
    }
    if let Some(names) = &obstacle {
        require_names(s, cmd, names)?;
    }
    let index = s.doc.generative.iter().rposition(|study| study.name == name);
    let Some(index) = index else {
        let preserve = preserve.filter(|names| !names.is_empty()).ok_or_else(|| bad(cmd, "a generative study needs a preserve body"))?;
        if s.doc.generative.len() >= MAX_STUDIES {
            return Err(bad(cmd, "the study exceeds the size cap"));
        }
        let objective = objective.unwrap_or_else(|| "stiffness".into());
        s.doc_mut().generative.push(blank_generative(
            name.clone(),
            preserve,
            obstacle.unwrap_or_default(),
            objective,
            loads.unwrap_or_default(),
            constraints.unwrap_or_default(),
        ));
        return Ok(json!({"study": name, "stale": true}));
    };
    let Some(study) = s.doc_mut().generative.get_mut(index) else {
        return Err(bad(cmd, "no study"));
    };
    let mut changed = false;
    if let Some(preserve) = preserve {
        if preserve.is_empty() {
            return Err(bad(cmd, "a generative study needs a preserve body"));
        }
        if study.preserve != preserve {
            study.preserve = preserve;
            changed = true;
        }
    }
    if let Some(obstacle) = obstacle
        && study.obstacle != obstacle
    {
        study.obstacle = obstacle;
        changed = true;
    }
    if let Some(objective) = objective
        && study.objective != objective
    {
        study.objective = objective;
        changed = true;
    }
    if let Some(loads) = loads
        && study.loads != loads
    {
        study.loads = loads;
        changed = true;
    }
    if let Some(constraints) = constraints
        && study.constraints != constraints
    {
        study.constraints = constraints;
        changed = true;
    }
    if changed {
        study.stale = true;
    }
    let stale = study.stale;
    Ok(json!({"study": name, "stale": stale}))
}

fn gen_solve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generative.solve";
    let wanted = str_(p, "name").map(str::to_string);
    let (label, preserve, obstacle, objective) = {
        let study = s
            .doc
            .generative
            .iter()
            .rev()
            .find(|study| wanted.as_ref().is_none_or(|n| &study.name == n) && !study.preserve.is_empty())
            .ok_or_else(|| bad(cmd, "a generative study needs a preserve body"))?;
        (study.name.clone(), study.preserve.clone(), study.obstacle.clone(), study.objective.clone())
    };
    let goal = match objective.as_str() {
        "mass" => Objective::Mass,
        "stiffness" => Objective::Stiffness,
        _ => return Err(bad(cmd, "objective must be stiffness or mass")),
    };
    let features = feature_ids(s);
    let mut keep = Vec::with_capacity(preserve.len());
    for body in &preserve {
        keep.push(body_region(s, cmd, body)?);
    }
    let mut block = Vec::with_capacity(obstacle.len());
    for body in &obstacle {
        block.push(body_region(s, cmd, body)?);
    }
    let candidate = generative::section_reduction(&keep, &block, goal).map_err(|err| bad(cmd, err))?;
    if feature_ids(s) != features {
        return Err(bad(cmd, "the study changed the solid"));
    }
    if let Some(study) = s.doc_mut().generative.iter_mut().rev().find(|study| study.name == label) {
        study.method = candidate.method.to_string();
        study.length = candidate.size[0];
        study.width = candidate.size[1];
        study.height = candidate.size[2];
        study.origin = candidate.origin;
        study.candidate = None;
        study.stale = false;
    }
    if feature_ids(s) != features {
        return Err(bad(cmd, "the study changed the solid"));
    }
    Ok(json!({
        "method": candidate.method,
        "length": candidate.size[0],
        "width": candidate.size[1],
        "height": candidate.size[2],
        "origin": candidate.origin,
        "stale": false,
    }))
}

fn next_candidate_name(s: &Session, cmd: &str) -> Result<String> {
    if s.model.state().body("Candidate").is_none() {
        return Ok("Candidate".into());
    }
    for index in 2..100 {
        let name = format!("Candidate {index}");
        if s.model.state().body(&name).is_none() {
            return Ok(name);
        }
    }
    Err(bad(cmd, "the study exceeds the size cap"))
}

fn gen_promote(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generative.promote";
    let wanted = str_(p, "name");
    let (label, length, width, height, origin, method, stale, existing) = {
        let study = s
            .doc
            .generative
            .iter()
            .rev()
            .find(|study| wanted.is_none_or(|n| study.name == n))
            .ok_or_else(|| bad(cmd, "generate a candidate first"))?;
        (study.name.clone(), study.length, study.width, study.height, study.origin, study.method.clone(), study.stale, study.candidate.clone())
    };
    if method.is_empty() || !(length > 0.0 && width > 0.0 && height > 0.0) {
        return Err(bad(cmd, "generate a candidate first"));
    }
    if stale {
        return Err(bad(cmd, "the study is stale; generate again"));
    }
    if let Some(body) = &existing
        && s.model.state().body(body).is_some()
    {
        return Ok(json!({"body": body, "method": method}));
    }
    let body_name = next_candidate_name(s, cmd)?;
    let features = feature_ids(s);
    s.execute(
        "solid.box",
        &json!({
            "length": length,
            "width": width,
            "height": height,
            "corner": origin,
            "body_name": body_name,
            "operation": "new",
        }),
    )?;
    if s.model.state().body(&body_name).is_none() || feature_ids(s).len() <= features.len() {
        return Err(bad(cmd, "the candidate body was not created"));
    }
    if let Some(study) = s.doc_mut().generative.iter_mut().rev().find(|study| study.name == label) {
        study.candidate = Some(body_name.clone());
    }
    Ok(json!({"body": body_name, "method": method}))
}

#[cfg(test)]
#[path = "sim_tests.rs"]
mod sim_tests;

#[cfg(test)]
#[path = "ecad_tests.rs"]
mod ecad_tests;

#[cfg(test)]
#[path = "present_tests.rs"]
mod present_tests;

fn parts_list(s: &mut Session, p: &Value) -> Result<Value> {
    super::drawing::parts_list(s, p)
}

fn drawing_detail(s: &mut Session, p: &Value) -> Result<Value> {
    super::drawing::detail(s, p)
}

fn title_block(s: &mut Session, p: &Value) -> Result<Value> {
    super::drawing::title_block(s, p)
}
