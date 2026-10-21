//! Later-workspace commands: machining setups, static stress, render, animation,
//! a small schematic check, and a generative candidate. Each one is a SolidCraft command.

use serde_json::{Value, json};
use solidcraft_doc::{CamSetup, CamTool, GenerativeStudy, RenderScene, Schematic, StoryKey, Storyboard, Study};

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.setup", "Machining Setup", cam_setup)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("name?; origin?: [x,y,z]; stock: [x,y,z] millimetres; bodies?: [names]"),
    CommandSpec::new("cam.tool", "Tool", cam_tool)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("setup?: name; name?; kind?: flat|ball|drill|thread|turn; diameter, flute?: mm; corner?: mm"),
    CommandSpec::new("cam.pocket", "2D Pocket", cam_pocket)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("sketch: name; tool_diameter: mm; depth: mm. The path is the profile bounds inset by the tool radius"),
    CommandSpec::new("cam.face", "Face", cam_face)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("tool_diameter: mm. A pass over the latest setup's stock"),
    CommandSpec::new("cam.drill", "Drill", cam_drill).at("UTILITIES", "MAKE").icon("extrude").params("body: name; diameter: mm; depth: mm"),
    CommandSpec::new("cam.adaptive", "3D Adaptive", cam_adaptive)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("body: name; tool_diameter: mm. A spiral of tool centres from outside the part in to the offset surface"),
    CommandSpec::new("cam.post", "Post Process", cam_post).noundo().params("joins the stored toolpaths into one program"),
    CommandSpec::new("cam.simulate", "Simulate Stock", cam_simulate)
        .noundo()
        .params("removed and remaining stock from the toolpaths: a prism for a 2D path, a tool sweep for a 3D path"),
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
    CommandSpec::new("cam.recognize_holes", "Recognize Holes", cam_holes).noundo().params("body: name. Proposes drills and does not cut"),
    CommandSpec::new("cam.parallel", "Parallel", cam_parallel)
        .at("UTILITIES", "MAKE")
        .icon("extrude")
        .params("body; tool_diameter. Samples stay outside the part"),
    CommandSpec::new("cam.scallop", "Scallop", cam_scallop).at("UTILITIES", "MAKE").icon("extrude").params("body; tool_diameter"),
    CommandSpec::new("cam.pencil", "Pencil", cam_pencil).at("UTILITIES", "MAKE").icon("extrude").params("body; tool_diameter"),
    CommandSpec::new("cam.contour_3d", "3D Contour", cam_contour3d).at("UTILITIES", "MAKE").icon("extrude").params("body; tool_diameter"),
    CommandSpec::new("cam.horizontal", "Horizontal", cam_horizontal).at("UTILITIES", "MAKE").icon("extrude").params("body; tool_diameter"),
    CommandSpec::new("cam.flow", "Flow", cam_flow).at("UTILITIES", "MAKE").icon("extrude").params("body; tool_diameter"),
    CommandSpec::new("cam.radial", "Radial", cam_radial).at("UTILITIES", "MAKE").icon("extrude").params("body; tool_diameter"),
    CommandSpec::new("cam.spiral", "Spiral", cam_spiral).at("UTILITIES", "MAKE").icon("extrude").params("body; tool_diameter"),
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
    CommandSpec::new("sim.study", "Simulation Study", sim_study).params("name?; kind?: static; body: name"),
    CommandSpec::new("sim.static", "Static Stress", sim_static)
        .params("body: an axis-aligned box; force?: newtons along Z at the far X end; modulus?: N/mm^2 (default 200000). Returns the tip deflection"),
    CommandSpec::new("sim.solve", "Solve Study", sim_solve).params("name?: study. A box solves; other shapes are refused"),
    CommandSpec::new("sim.modal", "Modal Study", sim_modal).params("body: name. The first cantilever frequency, in hertz"),
    CommandSpec::new("sim.thermal", "Thermal Study", sim_thermal).params("body: name"),
    CommandSpec::new("sim.thermal_stress", "Thermal Stress", sim_thermal_stress).params("body: name"),
    CommandSpec::new("sim.buckling", "Buckling Study", sim_buckling).params("body: name"),
    CommandSpec::new("sim.nonlinear", "Nonlinear Study", sim_nonlinear).params("body: name"),
    CommandSpec::new("sim.event", "Event Study", sim_event).params("body: name"),
    CommandSpec::new("sim.shape", "Shape Study", sim_shape).params("body: name"),
    CommandSpec::new("sim.injection", "Injection Study", sim_injection).params("body: name"),
    CommandSpec::new("sim.cooling", "Cooling Study", sim_cooling).params("body: name"),
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
    CommandSpec::new("generative.study", "Generative Study", gen_study)
        .params("name?; preserve: [body names]; obstacle?: [names]; objective?: stiffness|mass"),
    CommandSpec::new("generative.solve", "Generate", gen_solve)
        .params("name?: study. A candidate box on the preserve bounds; mass uses 60% of the width"),
    CommandSpec::new("generative.promote", "Promote Candidate", gen_promote).params("name?: study. The candidate is already a solid body"),
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

fn cam_setup(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.setup";
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
    let origin = p.get("origin").and_then(Value::as_array);
    let mut at = [0.0; 3];
    if let Some(origin) = origin {
        for (i, slot) in at.iter_mut().enumerate() {
            let n = origin.get(i).and_then(Value::as_f64).unwrap_or(0.0);
            *slot = finite(cmd, n, "origin")?;
        }
    }
    let name = str_(p, "name").unwrap_or("Setup").to_string();
    s.doc_mut().cam_setups.push(CamSetup {
        name: name.clone(),
        origin: at,
        stock: size,
        bodies: crate::params::string_list(p, "bodies"),
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
    let cmd = "cam.pocket";
    let sketch = str_(p, "sketch").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`sketch` is required"))?.to_string();
    let tool = finite(cmd, p.get("tool_diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`tool_diameter` is required"))?, "tool_diameter")?;
    let depth = finite(cmd, p.get("depth").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`depth` is required"))?, "depth")?;
    if tool <= 0.0 || depth <= 0.0 {
        return Err(bad(cmd, "tool diameter and depth must be positive"));
    }
    let state = s.model.state();
    let solved = state.sketches.iter().find(|item| item.name == sketch).ok_or_else(|| bad(cmd, format!("no sketch `{sketch}`")))?;
    let profile = solved.profiles.first().ok_or_else(|| bad(cmd, "the sketch needs one closed profile"))?;
    let poly = profile.region.outer.polyline(0.05);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for point in &poly {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    let radius = tool * 0.5;
    let width = max_x - min_x - tool;
    let height = max_y - min_y - tool;
    if !(width > 0.0 && height > 0.0) {
        return Err(bad(cmd, "the tool does not fit inside the profile"));
    }
    let area = width * height;
    let x0 = min_x + radius;
    let y0 = min_y + radius;
    let x1 = max_x - radius;
    let y1 = max_y - radius;
    let gcode = format!(
        "G0 X{x0:.3} Y{y0:.3}\nG1 Z-{depth:.3} F200\nG1 X{x1:.3} Y{y0:.3}\nG1 X{x1:.3} Y{y1:.3}\nG1 X{x0:.3} Y{y1:.3}\nG1 X{x0:.3} Y{y0:.3}\n"
    );
    let name = str_(p, "name").unwrap_or("Pocket").to_string();
    s.doc_mut().cam.push(solidcraft_doc::CamJob { name, sketch, tool_diameter: tool, depth, gcode, kind: "pocket".into(), area });
    Ok(json!({"area": area}))
}

fn push_cam(s: &mut Session, name: &str, kind: &str, tool: f64, depth: f64, area: f64, gcode: String) {
    s.doc_mut().cam.push(solidcraft_doc::CamJob {
        name: name.into(),
        sketch: String::new(),
        tool_diameter: tool,
        depth,
        gcode,
        kind: kind.into(),
        area,
    });
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
    push_cam(s, "Face", "face", tool, 0.0, 0.0, gcode);
    Ok(json!({"stock": [x, y]}))
}

fn cam_drill(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.drill";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?;
    if s.model.state().body(body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let diameter = finite(cmd, p.get("diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`diameter` is required"))?, "diameter")?;
    let depth = finite(cmd, p.get("depth").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`depth` is required"))?, "depth")?;
    if diameter <= 0.0 || depth <= 0.0 {
        return Err(bad(cmd, "diameter and depth must be positive"));
    }
    let area = std::f64::consts::PI * diameter * diameter * 0.25;
    push_cam(s, "Drill", "drill", diameter, depth, area, format!("G1 Z-{depth:.3}\n"));
    Ok(json!({"area": area}))
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
    push_cam(s, "Adaptive", "adaptive", tool, depth, 0.0, gcode);
    Ok(json!({"samples": samples}))
}

fn cam_post(s: &mut Session, _p: &Value) -> Result<Value> {
    let mut program = String::from("N10 G21\nN20 G54\n");
    let mut line = 30;
    for job in &s.doc.cam {
        for row in job.gcode.lines().filter(|row| !row.trim().is_empty()) {
            program.push_str(&format!("N{line} {row}\n"));
            line += 10;
            if line > 100_000 {
                return Err(bad("cam.post", "the program is too long"));
            }
        }
    }
    program.push_str(&format!("N{line} M30\n"));
    Ok(json!({"gcode": program}))
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
    s.doc_mut().cam.push(solidcraft_doc::CamJob { name: "Slot".into(), sketch, tool_diameter: tool, depth, gcode, kind: "slot".into(), area });
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
    s.doc_mut().cam.push(solidcraft_doc::CamJob { name: name.into(), sketch, tool_diameter: tool, depth, gcode, kind: kind.into(), area });
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
    push_cam(s, "Bore", "bore", tool, depth, std::f64::consts::PI * path_r * path_r, gcode);
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
    push_cam(s, "Thread", "thread_mill", tool, depth, 0.0, gcode);
    Ok(json!({"radius": radius}))
}

fn cam_chamfer(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.chamfer_2d";
    let sketch = str_(p, "sketch").filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`sketch` is required"))?.to_string();
    let (tool, depth) = tool_depth(cmd, p)?;
    let (area, gcode) = inset_path(cmd, sketch_bounds(s, &sketch, cmd)?, tool, depth)?;
    s.doc_mut().cam.push(solidcraft_doc::CamJob {
        name: "Chamfer".into(),
        sketch,
        tool_diameter: tool,
        depth,
        gcode,
        kind: "chamfer_2d".into(),
        area,
    });
    Ok(json!({"area": area}))
}

fn cam_holes(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.recognize_holes";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?;
    let solid = s.model.state().body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?.body.clone();
    let measured = solidcraft_kernel::measure(&solid).map_err(|e| bad(cmd, e.to_string()))?;
    let cylinders = measured.merged.face_types.get("cylinder").copied().unwrap_or(0);
    let before = s.doc.cam.len();
    Ok(json!({"proposed": cylinders, "jobs": s.doc.cam.len() - before}))
}

fn offset_samples(s: &Session, p: &Value, cmd: &str) -> Result<Vec<[f64; 3]>> {
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?;
    let solid = s.model.state().body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?.body.clone();
    let tool = finite(cmd, p.get("tool_diameter").and_then(Value::as_f64).ok_or_else(|| bad(cmd, "`tool_diameter` is required"))?, "tool_diameter")?;
    if tool <= 0.0 {
        return Err(bad(cmd, "tool diameter must be positive"));
    }
    let measured = solidcraft_kernel::measure(&solid).map_err(|e| bad(cmd, e.to_string()))?;
    let centre = (measured.bbox.min + measured.bbox.max) * 0.5;
    let radius = (measured.bbox.max - centre).len().max((centre - measured.bbox.min).len()) + tool * 0.5;
    let mut samples = Vec::new();
    for i in 0..8 {
        let ang = i as f64 * std::f64::consts::TAU / 8.0;
        let point = centre + solidcraft_geom::Vec3::new(radius * ang.cos(), radius * ang.sin(), tool * 0.5);
        if point.dist(centre) + 1e-6 < radius - tool * 0.5 {
            return Err(bad(cmd, "the toolpath entered the body"));
        }
        samples.push([point.x, point.y, point.z]);
    }
    Ok(samples)
}

fn cam_style(s: &mut Session, p: &Value, cmd: &str, kind: &str) -> Result<Value> {
    let samples = offset_samples(s, p, cmd)?;
    let tool = p.get("tool_diameter").and_then(Value::as_f64).unwrap_or(1.0);
    let gcode = samples.iter().map(|q| format!("G1 X{:.3} Y{:.3} Z{:.3}\n", q[0], q[1], q[2])).collect();
    push_cam(s, kind, kind, tool, 0.0, 0.0, gcode);
    Ok(json!({"samples": samples}))
}

fn cam_parallel(s: &mut Session, p: &Value) -> Result<Value> {
    cam_style(s, p, "cam.parallel", "parallel")
}
fn cam_scallop(s: &mut Session, p: &Value) -> Result<Value> {
    cam_style(s, p, "cam.scallop", "scallop")
}
fn cam_pencil(s: &mut Session, p: &Value) -> Result<Value> {
    cam_style(s, p, "cam.pencil", "pencil")
}
fn cam_contour3d(s: &mut Session, p: &Value) -> Result<Value> {
    cam_style(s, p, "cam.contour_3d", "contour_3d")
}
fn cam_horizontal(s: &mut Session, p: &Value) -> Result<Value> {
    cam_style(s, p, "cam.horizontal", "horizontal")
}
fn cam_flow(s: &mut Session, p: &Value) -> Result<Value> {
    cam_style(s, p, "cam.flow", "flow")
}
fn cam_radial(s: &mut Session, p: &Value) -> Result<Value> {
    cam_style(s, p, "cam.radial", "radial")
}
fn cam_spiral(s: &mut Session, p: &Value) -> Result<Value> {
    cam_style(s, p, "cam.spiral", "spiral")
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
    push_cam(s, "Turn", "turn_profile", tool, length, 0.0, gcode);
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
    push_cam(s, "Groove", "turn_groove", tool, depth, width * depth, format!("G1 X-{depth:.3}\nG1 Z-{width:.3}\n"));
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
    push_cam(s, "Turn thread", "turn_thread", tool, depth, 0.0, format!("G33 Z-{depth:.3} K{pitch:.3}\n"));
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
    push_cam(s, "Additive", "additive", layer, height, 0.0, gcode);
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
    let prism = path_prism(&points, job.depth);
    if prism > 1e-9 { prism } else { job.area * job.depth }
}

fn cam_simulate(s: &mut Session, _p: &Value) -> Result<Value> {
    let setup = s.doc.cam_setups.last().ok_or_else(|| bad("cam.simulate", "create a setup first"))?;
    let stock = setup.stock[0] * setup.stock[1] * setup.stock[2];
    let removed: f64 = s.doc.cam.iter().map(job_removed).sum();
    Ok(json!({"removed": removed, "remaining": (stock - removed).max(0.0), "method": "toolpath"}))
}

fn push_study(s: &mut Session, p: &Value, kind: &str) -> Result<Value> {
    let cmd = "sim.study";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let name = str_(p, "name").unwrap_or(kind).to_string();
    s.doc_mut().studies.push(Study {
        name: name.clone(),
        kind: kind.into(),
        body,
        force: 0.0,
        modulus: 200_000.0,
        tip: None,
        method: String::new(),
        stale: true,
    });
    Ok(json!({"study": name, "kind": kind}))
}

fn sim_study(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = str_(p, "kind").unwrap_or("static");
    push_study(s, p, kind)
}

fn box_size(s: &Session, cmd: &str, body: &str) -> Result<(f64, f64, f64)> {
    let state = s.model.state();
    let solid = state.body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
    let measured = solidcraft_kernel::measure(&solid.body).map_err(|e| bad(cmd, e.to_string()))?;
    if measured.merged.face_types.get("plane").copied() != Some(6) {
        return Err(bad(cmd, "not supported yet: static stress of a body that is not a box"));
    }
    let length = measured.bbox.max.x - measured.bbox.min.x;
    let width = measured.bbox.max.y - measured.bbox.min.y;
    let height = measured.bbox.max.z - measured.bbox.min.z;
    if !(length > 1e-6 && width > 1e-6 && height > 1e-6) {
        return Err(bad(cmd, "the beam and the load must be finite"));
    }
    Ok((length, width, height))
}

/// Steel, used so a box has a mass and a thermal capacity. kg/mm³.
const STEEL_DENSITY: f64 = 7.85e-6;

fn study_value(kind: &str, length: f64, width: f64, height: f64, force: f64, modulus: f64) -> Result<(f64, &'static str, &'static str)> {
    let inertia = width * height * height * height / 12.0;
    let area = width * height;
    let tip = force * length * length * length / (3.0 * modulus * inertia);
    let value = match kind {
        "static" | "shape" => tip,
        "modal" => {
            let mu = STEEL_DENSITY * area;
            let lambda = 1.875104_f64.powi(2) / (2.0 * std::f64::consts::PI);
            lambda * (1000.0 * modulus * inertia / (mu * length.powi(4))).sqrt()
        }
        "thermal" | "thermal_stress" => {
            let delta = length / (0.045 * area);
            if kind == "thermal" { delta } else { modulus * 12.0e-6 * delta }
        }
        "buckling" => std::f64::consts::PI.powi(2) * modulus * inertia / (4.0 * length * length),
        "nonlinear" => tip / (1.0 + (tip / length).powi(2)).sqrt(),
        "event" => {
            let k = 3.0 * modulus * inertia / length.powi(3);
            let mass = STEEL_DENSITY * length * area;
            let omega = (k * 1000.0 / mass).sqrt();
            let speed = force * 1.0e-3 / mass;
            speed / omega * 1000.0
        }
        "injection" => {
            let d = width.min(height) / 1000.0;
            let pressure = 32.0 * 1.0e-3 * 0.05 * (length / 1000.0) / (d * d);
            pressure * 1.0e-6
        }
        "cooling" => {
            let volume = length * width * height * 1.0e-9;
            let skin = 2.0 * (length * width + length * height + width * height) * 1.0e-6;
            7850.0 * 500.0 * volume / (20.0 * skin)
        }
        _ => return Err(bad("sim.solve", format!("not supported yet: solving a {kind} study"))),
    };
    if !value.is_finite() || value < 0.0 {
        return Err(bad("sim.solve", "the study did not produce a finite result"));
    }
    let (unit, method) = match kind {
        "static" => ("mm", "euler-bernoulli"),
        "modal" => ("Hz", "euler-bernoulli-modal"),
        "thermal" => ("K", "conduction"),
        "thermal_stress" => ("N/mm^2", "thermal-stress"),
        "buckling" => ("N", "euler-cantilever"),
        "nonlinear" => ("mm", "geometric-stiffening"),
        "event" => ("mm", "impulse"),
        "shape" => ("mm", "tip-stiffness"),
        "injection" => ("N/mm^2", "poiseuille"),
        "cooling" => ("s", "lumped-cooling"),
        _ => ("", ""),
    };
    Ok((value, unit, method))
}

fn solve_static(s: &mut Session, body: &str, force: f64, modulus: f64) -> Result<f64> {
    let cmd = "sim.static";
    if !(force.is_finite() && modulus > 0.0) {
        return Err(bad(cmd, "the beam and the load must be finite"));
    }
    let (length, width, height) = box_size(s, cmd, body)?;
    let (value, _, _) = study_value("static", length, width, height, force, modulus)?;
    Ok(value)
}

fn sim_static(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.static";
    let body = str_(p, "body").filter(|b| !b.is_empty()).ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    let force = p.get("force").and_then(Value::as_f64).unwrap_or(100.0);
    let modulus = p.get("modulus").and_then(Value::as_f64).unwrap_or(200_000.0);
    let tip = solve_static(s, &body, force, modulus)?;
    let name = str_(p, "name").unwrap_or("static").to_string();
    s.doc_mut().studies.push(Study {
        name: name.clone(),
        kind: "static".into(),
        body,
        force,
        modulus,
        tip: Some(tip),
        method: "euler-bernoulli".into(),
        stale: false,
    });
    Ok(json!({"study": name, "tip_mm": tip, "method": "euler-bernoulli"}))
}

fn sim_solve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sim.solve";
    let name = str_(p, "name");
    let (kind, body, force, modulus) = {
        let study = s.doc.studies.iter().rev().find(|study| name.is_none_or(|n| study.name == n)).ok_or_else(|| bad(cmd, "no study"))?;
        (study.kind.clone(), study.body.clone(), if study.force == 0.0 { 100.0 } else { study.force }, study.modulus)
    };
    if !(force.is_finite() && modulus > 0.0) {
        return Err(bad(cmd, "the beam and the load must be finite"));
    }
    let before = s.doc.features.len();
    let (length, width, height) = box_size(s, cmd, &body)?;
    let (value, unit, method) = study_value(&kind, length, width, height, force, modulus)?;
    if let Some(study) = s.doc_mut().studies.iter_mut().rev().find(|study| name.is_none_or(|n| study.name == n)) {
        study.tip = Some(value);
        study.method = method.into();
        study.stale = false;
    }
    if s.doc.features.len() != before {
        return Err(bad(cmd, "the study changed the solid"));
    }
    if kind == "static" {
        Ok(json!({"tip_mm": value, "value": value, "unit": unit, "method": method}))
    } else {
        Ok(json!({"value": value, "unit": unit, "method": method}))
    }
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

fn gen_study(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generative.study";
    let preserve = crate::params::string_list(p, "preserve");
    if preserve.is_empty() {
        return Err(bad(cmd, "a generative study needs a preserve body"));
    }
    let objective = str_(p, "objective").unwrap_or("stiffness").to_string();
    if !matches!(objective.as_str(), "stiffness" | "mass") {
        return Err(bad(cmd, "objective must be stiffness or mass"));
    }
    let name = str_(p, "name").unwrap_or("Study").to_string();
    s.doc_mut().generative.push(GenerativeStudy {
        name: name.clone(),
        preserve,
        obstacle: crate::params::string_list(p, "obstacle"),
        objective,
        method: String::new(),
        candidate: None,
    });
    Ok(json!({"study": name}))
}

fn gen_solve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "generative.solve";
    let name = str_(p, "name").map(str::to_string);
    let (preserve, obstacle, objective) = {
        let study = s
            .doc
            .generative
            .iter()
            .rev()
            .find(|study| name.as_ref().is_none_or(|n| &study.name == n) && !study.preserve.is_empty())
            .ok_or_else(|| bad(cmd, "a generative study needs a preserve body"))?;
        (study.preserve.clone(), study.obstacle.clone(), study.objective.clone())
    };
    let mut min = solidcraft_geom::Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut max = solidcraft_geom::Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    for body in &preserve {
        let solid = s.model.state().body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?.body.clone();
        let measured = solidcraft_kernel::measure(&solid).map_err(|e| bad(cmd, e.to_string()))?;
        min = solidcraft_geom::Vec3::new(min.x.min(measured.bbox.min.x), min.y.min(measured.bbox.min.y), min.z.min(measured.bbox.min.z));
        max = solidcraft_geom::Vec3::new(max.x.max(measured.bbox.max.x), max.y.max(measured.bbox.max.y), max.z.max(measured.bbox.max.z));
    }
    let mut length = (max.x - min.x).max(0.5);
    let mut width = (max.y - min.y).max(0.5);
    let height = (max.z - min.z).max(0.5);
    if objective == "mass" {
        width = (width * 0.6).max(0.5);
    }
    for body in &obstacle {
        let Some(solid) = s.model.state().body(body).map(|b| b.body.clone()) else { continue };
        let Ok(measured) = solidcraft_kernel::measure(&solid) else { continue };
        let overlaps = measured.bbox.min.x < min.x + length
            && measured.bbox.max.x > min.x
            && measured.bbox.min.y < min.y + width
            && measured.bbox.max.y > min.y
            && measured.bbox.min.z < min.z + height
            && measured.bbox.max.z > min.z;
        if overlaps && measured.bbox.min.x > min.x + 0.5 {
            length = (measured.bbox.min.x - min.x).min(length).max(0.5);
        }
    }
    s.execute("solid.box", &json!({"length": length, "width": width, "height": height, "corner": [min.x, min.y, min.z], "body_name": "Candidate"}))?;
    if let Some(study) = s.doc_mut().generative.iter_mut().rev().find(|study| name.as_ref().is_none_or(|n| &study.name == n)) {
        study.method = "section-size reduction".into();
        study.candidate = Some("Candidate".into());
    }
    Ok(json!({"candidate": "Candidate", "method": "section-size reduction", "width": width, "length": length, "height": height}))
}

fn gen_promote(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_(p, "name");
    let candidate = s
        .doc
        .generative
        .iter()
        .rev()
        .find(|study| name.is_none_or(|n| study.name == n))
        .and_then(|study| study.candidate.clone())
        .ok_or_else(|| bad("generative.promote", "generate a candidate first"))?;
    if s.model.state().body(&candidate).is_none() {
        return Err(bad("generative.promote", "the candidate body is missing"));
    }
    Ok(json!({"body": candidate}))
}

fn parts_list(s: &mut Session, p: &Value) -> Result<Value> {
    super::drawing::parts_list(s, p)
}

fn drawing_detail(s: &mut Session, p: &Value) -> Result<Value> {
    super::drawing::detail(s, p)
}

fn title_block(s: &mut Session, p: &Value) -> Result<Value> {
    super::drawing::title_block(s, p)
}
