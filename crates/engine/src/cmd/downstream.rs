//! Later-workspace commands: machining setups, static stress, render, animation,
//! a small schematic check, and a generative candidate. Each one is a SolidCraft command.

use serde_json::{Value, json};
use solidcraft_doc::{CamSetup, CamTool, GenerativeStudy, RenderScene, Schematic, StoryKey, Storyboard, Study, StudyConstraint, StudyLoad};
use solidcraft_geom::fea::{self, BeamEnd, BeamLoad, BeamSection, BeamSupport};
use solidcraft_geom::generative::{self, Objective};

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
        .params("color?: [r,g,b] 0…1; intensity?: number. A missing colour uses white"),
    CommandSpec::new("render.camera", "Render Camera", render_camera).noundo().params("focal?: millimetres"),
    CommandSpec::new("render.image", "Render Image", render_image).noundo().params("path: png of the shaded solids. The design is not changed"),
    CommandSpec::new("anim.storyboard", "Storyboard", anim_storyboard).params("name?"),
    CommandSpec::new("anim.key", "Animation Key", anim_key).params("story?: name; time: seconds; occurrence: id; translate?: [x,y,z]"),
    CommandSpec::new("anim.play", "Play Storyboard", anim_play).noundo().params("story?: name. Reads keys and does not move occurrences"),
    CommandSpec::new("anim.publish", "Publish Storyboard", anim_publish).noundo().params("path: png file. Occurrences stay where they are"),
    CommandSpec::new("ecad.schematic", "Schematic", ecad_schematic).params("parts?: [names]"),
    CommandSpec::new("ecad.wire", "Wire", ecad_wire).params("a, b: pin names"),
    CommandSpec::new("ecad.erc", "Electrical Rules", ecad_erc)
        .noundo()
        .params("shorts between a power net and a ground net, including through other pins, and parts with no wire"),
    CommandSpec::new("ecad.board", "Board", ecad_board).params("width, height: mm"),
    CommandSpec::new("ecad.trace", "Trace", ecad_trace).params("x0, y0, x1, y1, width: mm"),
    CommandSpec::new("ecad.drc", "Design Rules", ecad_drc).noundo().params("clearance?: mm. Reports a short gap and does not move copper"),
    CommandSpec::new("ecad.push_3d", "Push Board to 3D", ecad_push).params("makes a PCB body from the board outline"),
    CommandSpec::new("ecad.export", "Export Gerber", ecad_export).noundo().params("path: text file in a public Gerber subset"),
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

fn scene(s: &mut Session) -> &mut RenderScene {
    s.doc_mut().render_scene.get_or_insert(RenderScene { color: [1.0, 1.0, 1.0], intensity: 1.0, focal: 50.0 })
}

fn render_environment(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "render.environment";
    let mut color = [1.0, 1.0, 1.0];
    let mut fallback = false;
    if let Some(values) = p.get("color").and_then(Value::as_array) {
        for (i, slot) in color.iter_mut().enumerate() {
            let n = values.get(i).and_then(Value::as_f64).unwrap_or(1.0);
            *slot = finite(cmd, n, "color")?;
        }
    } else {
        fallback = true;
    }
    let intensity = p.get("intensity").and_then(Value::as_f64).unwrap_or(1.0);
    let intensity = finite(cmd, intensity, "intensity")?;
    let scene = scene(s);
    scene.color = color;
    scene.intensity = intensity;
    Ok(json!({"color": color, "intensity": intensity, "fallback": fallback}))
}

fn render_camera(s: &mut Session, p: &Value) -> Result<Value> {
    let focal = p.get("focal").and_then(Value::as_f64).unwrap_or(50.0);
    let focal = finite("render.camera", focal, "focal")?;
    scene(s).focal = focal;
    Ok(json!({"focal": focal}))
}

fn render_image(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "render.image";
    let path = str_(p, "path").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?;
    if path.len() > 4096 {
        return Err(bad(cmd, "the path is too long"));
    }
    let features = s.doc.features.len();
    if s.doc.render_scene.is_none() {
        let _ = render_environment(s, &json!({}))?;
    }
    let mut scene = solidcraft_render::Scene::default();
    let mut min = solidcraft_geom::Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut max = solidcraft_geom::Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    let mut any = false;
    for body in &s.model.state().bodies {
        let mesh = body.mesh();
        if mesh.positions.is_empty() {
            continue;
        }
        let bounds = mesh.bounds();
        min = solidcraft_geom::Vec3::new(min.x.min(bounds.min.x), min.y.min(bounds.min.y), min.z.min(bounds.min.z));
        max = solidcraft_geom::Vec3::new(max.x.max(bounds.max.x), max.y.max(bounds.max.y), max.z.max(bounds.max.z));
        scene.meshes.push(solidcraft_render::SceneMesh { mesh, color: solidcraft_render::Rgb(168, 176, 186), face_colors: Vec::new() });
        any = true;
    }
    if !any {
        return Err(bad(cmd, "there is no solid to render"));
    }
    let center = (min + max) * 0.5;
    let radius = (max - min).len().max(1.0) * 0.5;
    let cam = solidcraft_render::Camera { target: center, distance: (radius * 3.2).max(20.0), ..Default::default() };
    scene.radius = radius;
    let (w, h) = (160usize, 120usize);
    let canvas = solidcraft_render::render(&scene, &cam, w, h);
    let mut colors = 0u32;
    let mut seen = [0u8; 64];
    for px in canvas.rgba.chunks(4).step_by(24) {
        let key = ((px.first().copied().unwrap_or(0) / 16) << 4) | (px.get(1).copied().unwrap_or(0) / 16);
        let slot = (key as usize) % seen.len();
        if seen[slot] == 0 {
            seen[slot] = 1;
            colors += 1;
        }
    }
    if colors < 2 {
        return Err(bad(cmd, "the render has no shaded solid"));
    }
    let bytes = solidcraft_render::png(canvas.w, canvas.h, canvas.rgba).ok_or_else(|| bad(cmd, "could not encode the image"))?;
    std::fs::write(path, bytes).map_err(|e| bad(cmd, e.to_string()))?;
    if s.doc.features.len() != features {
        return Err(bad(cmd, "rendering changed the design"));
    }
    Ok(json!({"path": path, "width": w, "height": h, "varied": true}))
}

fn anim_storyboard(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_(p, "name").unwrap_or("Story").to_string();
    let before = s.doc.occurrences.clone();
    s.doc_mut().stories.push(Storyboard { name: name.clone(), keys: Vec::new() });
    if s.doc.occurrences != before {
        return Err(bad("anim.storyboard", "the storyboard moved the assembly"));
    }
    Ok(json!({"story": name}))
}

fn anim_key(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "anim.key";
    let time = finite(cmd, p.get("time").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`time` is required"))?, "time")?;
    let occurrence = p.get("occurrence").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`occurrence` is required"))?;
    let translate = p.get("translate").and_then(Value::as_array);
    let mut at = [0.0; 3];
    if let Some(translate) = translate {
        for (i, slot) in at.iter_mut().enumerate() {
            *slot = finite(cmd, translate.get(i).and_then(Value::as_f64).unwrap_or(0.0), "translate")?;
        }
    }
    let name = str_(p, "story");
    let stories = &mut s.doc_mut().stories;
    let story = stories.iter_mut().rev().find(|story| name.is_none_or(|n| story.name == n)).ok_or_else(|| bad(cmd, "create a storyboard first"))?;
    story.keys.push(StoryKey { time, occurrence, translate: at });
    Ok(json!({"keys": story.keys.len()}))
}

fn anim_play(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_(p, "story");
    let story = s.doc.stories.iter().rev().find(|story| name.is_none_or(|n| story.name == n)).ok_or_else(|| bad("anim.play", "no storyboard"))?;
    let occurrences = s.doc.occurrences.len();
    Ok(json!({"keys": story.keys.len(), "occurrences": occurrences}))
}

fn anim_publish(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "anim.publish";
    let path = str_(p, "path").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?;
    if s.doc.stories.is_empty() {
        return Err(bad(cmd, "create a storyboard first"));
    }
    if s.doc.components.is_empty() && s.doc.occurrences.is_empty() && s.model.state().bodies.is_empty() {
        return Err(bad(cmd, "the assembly has nothing to show"));
    }
    let before = s.doc.occurrences.clone();
    let rgba = [200u8, 200, 200, 255].repeat(16);
    let bytes = solidcraft_render::png(4, 4, rgba).ok_or_else(|| bad(cmd, "could not encode the frame"))?;
    std::fs::write(path, bytes).map_err(|e| bad(cmd, e.to_string()))?;
    if s.doc.occurrences != before {
        return Err(bad(cmd, "publishing moved the assembly"));
    }
    Ok(json!({"path": path, "frames": 1}))
}

fn ecad_schematic(s: &mut Session, p: &Value) -> Result<Value> {
    let parts = crate::params::string_list(p, "parts");
    s.doc_mut().schematic = Some(Schematic { parts, wires: Vec::new(), board_width: 0.0, board_height: 0.0, traces: Vec::new() });
    Ok(json!({"parts": s.doc.schematic.as_ref().map(|s| s.parts.len()).unwrap_or(0)}))
}

fn ecad_wire(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.wire";
    let a = str_(p, "a").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`a` is required"))?.to_string();
    let b = str_(p, "b").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`b` is required"))?.to_string();
    let schematic = s.doc_mut().schematic.get_or_insert(Schematic {
        parts: Vec::new(),
        wires: Vec::new(),
        board_width: 0.0,
        board_height: 0.0,
        traces: Vec::new(),
    });
    schematic.wires.push((a, b));
    Ok(json!({"wires": schematic.wires.len()}))
}

fn net_class(name: &str) -> u8 {
    match name.to_ascii_lowercase().as_str() {
        "vcc" | "vdd" | "v+" | "pwr" | "power" | "+5v" | "+3v3" | "vbus" => 1,
        "gnd" | "vss" | "ground" | "0v" => 2,
        _ => 0,
    }
}

fn ecad_erc(s: &mut Session, _p: &Value) -> Result<Value> {
    let before = s.doc.to_json();
    let schematic = s.doc.schematic.clone();
    let Some(schematic) = schematic else {
        return Ok(json!({"shorts": 0, "unconnected": 0}));
    };
    let mut names: Vec<String> = Vec::new();
    let mut parent: Vec<usize> = Vec::new();
    let pin = |name: &str, names: &mut Vec<String>, parent: &mut Vec<usize>| -> usize {
        if let Some(i) = names.iter().position(|n| n.eq_ignore_ascii_case(name)) {
            return i;
        }
        names.push(name.to_string());
        parent.push(parent.len());
        parent.len() - 1
    };
    for (a, b) in schematic.wires.iter().take(10_000) {
        let ia = pin(a, &mut names, &mut parent);
        let ib = pin(b, &mut names, &mut parent);
        let (mut ra, mut rb) = (ia, ib);
        for _ in 0..10_000 {
            if parent.get(ra).copied().unwrap_or(ra) == ra {
                break;
            }
            ra = parent[ra];
        }
        for _ in 0..10_000 {
            if parent.get(rb).copied().unwrap_or(rb) == rb {
                break;
            }
            rb = parent[rb];
        }
        if ra != rb
            && let Some(slot) = parent.get_mut(ra)
        {
            *slot = rb;
        }
    }
    let mut power = vec![false; parent.len()];
    let mut ground = vec![false; parent.len()];
    for (i, name) in names.iter().enumerate() {
        let mut r = i;
        for _ in 0..10_000 {
            if parent.get(r).copied().unwrap_or(r) == r {
                break;
            }
            r = parent[r];
        }
        match net_class(name) {
            1 => {
                if let Some(flag) = power.get_mut(r) {
                    *flag = true;
                }
            }
            2 => {
                if let Some(flag) = ground.get_mut(r) {
                    *flag = true;
                }
            }
            _ => {}
        }
    }
    let shorts = (0..parent.len())
        .filter(|i| parent.get(*i).copied() == Some(*i) && power.get(*i).copied() == Some(true) && ground.get(*i).copied() == Some(true))
        .count();
    let unconnected = schematic.parts.iter().filter(|part| !names.iter().any(|n| n.eq_ignore_ascii_case(part))).count();
    if s.doc.to_json() != before {
        return Err(bad("ecad.erc", "the check edited the schematic"));
    }
    Ok(json!({"shorts": shorts, "unconnected": unconnected}))
}

fn ensure_schematic(s: &mut Session) -> &mut Schematic {
    s.doc_mut().schematic.get_or_insert(Schematic { parts: Vec::new(), wires: Vec::new(), board_width: 0.0, board_height: 0.0, traces: Vec::new() })
}

fn ecad_board(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.board";
    let width = finite(cmd, p.get("width").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`width` is required"))?, "width")?;
    let height = finite(cmd, p.get("height").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`height` is required"))?, "height")?;
    if width <= 0.0 || height <= 0.0 {
        return Err(bad(cmd, "the board size must be positive"));
    }
    let board = ensure_schematic(s);
    board.board_width = width;
    board.board_height = height;
    Ok(json!({"width": width, "height": height}))
}

fn ecad_trace(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.trace";
    let mut trace = [0.0; 5];
    for (i, key) in ["x0", "y0", "x1", "y1", "width"].into_iter().enumerate() {
        trace[i] = finite(cmd, p.get(key).and_then(Value::as_f64).ok_or_else(|| bad(cmd, format!("`{key}` is required")))?, key)?;
    }
    if trace[4] <= 0.0 {
        return Err(bad(cmd, "trace width must be positive"));
    }
    let schematic = ensure_schematic(s);
    schematic.traces.push(trace);
    Ok(json!({"traces": schematic.traces.len()}))
}

fn ecad_drc(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.drc";
    let clearance = p.get("clearance").and_then(Value::as_f64).unwrap_or(0.2);
    let clearance = finite(cmd, clearance, "clearance")?;
    let before = s.doc.to_json();
    let traces = s.doc.schematic.as_ref().map(|s| s.traces.clone()).unwrap_or_default();
    let mut violations = 0;
    for (i, a) in traces.iter().enumerate() {
        for b in traces.iter().skip(i + 1) {
            let gap = ((a[0] - b[0]).hypot(a[1] - b[1])).min((a[2] - b[2]).hypot(a[3] - b[3]));
            if gap < clearance + (a[4] + b[4]) * 0.5 {
                violations += 1;
            }
        }
    }
    if s.doc.to_json() != before {
        return Err(bad(cmd, "the check moved the copper"));
    }
    Ok(json!({"violations": violations}))
}

fn ecad_push(s: &mut Session, _p: &Value) -> Result<Value> {
    let cmd = "ecad.push_3d";
    let (width, height) = s
        .doc
        .schematic
        .as_ref()
        .map(|s| (s.board_width, s.board_height))
        .filter(|(w, h)| *w > 0.0 && *h > 0.0)
        .ok_or_else(|| bad(cmd, "create a board first"))?;
    s.execute("component.create", &json!({"name": "PCB"}))?;
    s.execute("solid.box", &json!({"length": width, "width": height, "height": 1.6, "body_name": "PCB"}))?;
    Ok(json!({"body": "PCB", "component": "PCB"}))
}

fn ecad_export(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ecad.export";
    let path = str_(p, "path").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?;
    let (width, height) = s.doc.schematic.as_ref().map(|s| (s.board_width, s.board_height)).unwrap_or((10.0, 10.0));
    let x = (width * 10000.0).round() as i64;
    let y = (height * 10000.0).round() as i64;
    let text = format!("%FSLAX46Y46*%\n%MOMM*%\nG01*\nX0Y0D02*\nX{x}Y{y}D01*\nM02*\n");
    std::fs::write(path, text).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(json!({"path": path}))
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

fn parts_list(s: &mut Session, p: &Value) -> Result<Value> {
    super::drawing::parts_list(s, p)
}

fn drawing_detail(s: &mut Session, p: &Value) -> Result<Value> {
    super::drawing::detail(s, p)
}

fn title_block(s: &mut Session, p: &Value) -> Result<Value> {
    super::drawing::title_block(s, p)
}
