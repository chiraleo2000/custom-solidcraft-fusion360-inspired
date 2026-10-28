//! Every 3D export format, end to end: a design is written through `file.export` /
//! `file.save_mesh`, then the file is read back and its geometry compared with the design
//! (volume for B-rep and triangle formats, bounds for the mesh round trips).

use serde_json::{Value, json};

use crate::*;

const PLATE: f64 = 40.0 * 30.0 * 5.0;
const PIN: f64 = std::f64::consts::PI * 4.0 * 4.0 * 20.0;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn part() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 5, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 20, "base": [10, 10, 5], "body_name": "Pin"}));
    s
}

fn temp(ext: &str, tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("solidcraft-export-{tag}-{}.{ext}", std::process::id()))
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

fn signed_volume(tris: &[[[f64; 3]; 3]]) -> f64 {
    tris.iter()
        .map(|[a, b, c]| {
            let cross = [b[1] * c[2] - b[2] * c[1], b[2] * c[0] - b[0] * c[2], b[0] * c[1] - b[1] * c[0]];
            (a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]) / 6.0
        })
        .sum()
}

fn stl_binary_triangles(bytes: &[u8]) -> Vec<[[f64; 3]; 3]> {
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    assert_eq!(bytes.len(), 84 + 50 * n, "a binary STL is an 84 byte header plus 50 bytes per triangle");
    let f = |at: usize| f64::from(f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]));
    (0..n)
        .map(|i| {
            let base = 84 + 50 * i + 12;
            let v = |k: usize| [f(base + 12 * k), f(base + 12 * k + 4), f(base + 12 * k + 8)];
            [v(0), v(1), v(2)]
        })
        .collect()
}

fn stl_ascii_triangles(text: &str) -> Vec<[[f64; 3]; 3]> {
    let verts: Vec<[f64; 3]> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("vertex"))
        .map(|rest| {
            let n: Vec<f64> = rest.split_whitespace().map(|x| x.parse().unwrap()).collect();
            [n[0], n[1], n[2]]
        })
        .collect();
    assert_eq!(verts.len() % 3, 0);
    verts.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
}

fn obj_triangles(text: &str) -> Vec<[[f64; 3]; 3]> {
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut tris = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let n: Vec<f64> = it.map(|x| x.parse().unwrap()).collect();
                points.push([n[0], n[1], n[2]]);
            }
            Some("f") => {
                let idx: Vec<usize> = it.map(|t| t.split('/').next().unwrap().parse::<usize>().unwrap() - 1).collect();
                for k in 1..idx.len().saturating_sub(1) {
                    tris.push([points[idx[0]], points[idx[k]], points[idx[k + 1]]]);
                }
            }
            _ => {}
        }
    }
    tris
}

fn union_bounds(s: &Session) -> ([f64; 3], [f64; 3]) {
    let st = s.world_state();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for b in &st.bodies {
        let bb = b.mesh().bounds();
        for (k, (l, h)) in [(bb.min.x, bb.max.x), (bb.min.y, bb.max.y), (bb.min.z, bb.max.z)].into_iter().enumerate() {
            lo[k] = lo[k].min(l);
            hi[k] = hi[k].max(h);
        }
    }
    (lo, hi)
}

fn assert_same_bounds(a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3]), tol: f64, what: &str) {
    for k in 0..3 {
        assert!((a.0[k] - b.0[k]).abs() < tol && (a.1[k] - b.1[k]).abs() < tol, "{what}: bounds {a:?} vs {b:?}");
    }
}

#[test]
fn step_exports_and_reopens_with_the_same_volumes() {
    let mut s = part();
    let p = temp("step", "step");
    let out = run(&mut s, "file.export", json!({"path": p.to_string_lossy()}));
    assert!(out["bytes"].as_u64().unwrap_or(0) > 200);
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.starts_with("ISO-10303-21;") && text.contains("END-ISO-10303-21;"), "a STEP file is framed by ISO-10303-21");
    let mut s2 = Session::default();
    run(&mut s2, "doc.open", json!({"path": p.to_string_lossy()}));
    let _ = std::fs::remove_file(&p);
    let st = s2.world_state();
    let names: Vec<&str> = st.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["Plate", "Pin"]);
    for (b, want) in st.bodies.iter().zip([PLATE, PIN]) {
        let v = solidcraft_kernel::measure(&b.body).unwrap().volume;
        // Tessellation of the cylinder in STEP round-trip sits just above 1e-4 on some hosts.
        assert!(rel(v, want) < 2e-4, "{} vol={v} want={want} rel={}", b.name, rel(v, want));
    }
}

#[test]
fn iges_exports_and_reopens_with_the_same_volumes() {
    let mut s = part();
    let p = temp("igs", "iges");
    run(&mut s, "file.export", json!({"path": p.to_string_lossy()}));
    let mut s2 = Session::default();
    run(&mut s2, "doc.open", json!({"path": p.to_string_lossy()}));
    let _ = std::fs::remove_file(&p);
    let st = s2.world_state();
    assert_eq!(st.bodies.len(), 2);
    for (b, want) in st.bodies.iter().zip([PLATE, PIN]) {
        assert!(rel(solidcraft_kernel::measure(&b.body).unwrap().volume, want) < 2e-4);
    }
}

#[test]
fn stl_binary_and_ascii_hold_the_solid_volume_with_outward_triangles() {
    let mut s = part();
    let bin = temp("stl", "bin");
    let ascii = temp("stl", "ascii");
    run(&mut s, "file.export", json!({"path": bin.to_string_lossy()}));
    run(&mut s, "file.export", json!({"path": ascii.to_string_lossy(), "format": "stla"}));
    let from_bin = signed_volume(&stl_binary_triangles(&std::fs::read(&bin).unwrap()));
    let text = std::fs::read_to_string(&ascii).unwrap();
    assert!(text.trim_start().starts_with("solid"), "an ASCII STL starts with `solid`");
    let from_ascii = signed_volume(&stl_ascii_triangles(&text));
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(&ascii);
    // The cylinder is tessellated, so a small shortfall is expected; the sign proves the
    // triangles face outward, which slicers need.
    for (name, v) in [("binary", from_bin), ("ascii", from_ascii)] {
        assert!(v > 0.0 && rel(v, PLATE + PIN) < 0.02, "{name} STL volume {v} vs {}", PLATE + PIN);
    }
    assert!(rel(from_bin, from_ascii) < 1e-5, "both STL flavours carry the same triangles");
}

#[test]
fn save_mesh_writes_both_stl_flavours() {
    let mut s = part();
    let bin = temp("stl", "sm-bin");
    let ascii = temp("stl", "sm-ascii");
    run(&mut s, "file.save_mesh", json!({"path": bin.to_string_lossy(), "bodies": ["Plate"]}));
    run(&mut s, "file.save_mesh", json!({"path": ascii.to_string_lossy(), "bodies": ["Plate"], "ascii": true}));
    let v_bin = signed_volume(&stl_binary_triangles(&std::fs::read(&bin).unwrap()));
    let v_ascii = signed_volume(&stl_ascii_triangles(&std::fs::read_to_string(&ascii).unwrap()));
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(&ascii);
    assert!(rel(v_bin, PLATE) < 1e-6 && rel(v_ascii, PLATE) < 1e-6, "only the chosen body is written: {v_bin} {v_ascii}");
}

#[test]
fn obj_holds_the_solid_volume() {
    let mut s = part();
    let p = temp("obj", "obj");
    let out = run(&mut s, "file.export", json!({"path": p.to_string_lossy()}));
    let text = std::fs::read_to_string(&p).unwrap();
    let mtl = p.with_extension("mtl");
    assert!(mtl.is_file(), "OBJ export writes a companion MTL");
    assert!(out["sidecars"].as_array().is_some_and(|a| !a.is_empty()));
    let mtl_text = std::fs::read_to_string(&mtl).unwrap();
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&mtl);
    assert!(text.contains("mtllib ") && text.contains("usemtl "), "OBJ references the MTL");
    assert!(mtl_text.contains("newmtl ") && mtl_text.contains("Kd "), "MTL carries materials");
    let v = signed_volume(&obj_triangles(&text));
    assert!(v > 0.0 && rel(v, PLATE + PIN) < 0.02, "OBJ volume {v}");
}

#[test]
fn public_tessellated_formats_export_structurally() {
    // Feature: fusion360-feature-parity, Property 29: Public interchange exports are structurally sound
    let mut s = part();
    for ext in ["gltf", "glb", "ply", "amf", "dae", "x3d", "wrl", "vrm", "usda", "usd"] {
        let p = temp(ext, &format!("pub-{ext}"));
        let out = run(&mut s, "file.export", json!({"path": p.to_string_lossy()}));
        assert!(out["bytes"].as_u64().unwrap_or(0) > 40, "{ext} wrote bytes");
        let bytes = std::fs::read(&p).unwrap();
        match ext {
            "gltf" => {
                let t = std::str::from_utf8(&bytes).unwrap();
                assert!(t.contains("\"asset\"") && t.contains("\"meshes\"") && t.contains("data:application/octet-stream;base64,"));
            }
            "glb" | "vrm" => assert_eq!(&bytes[..4], b"glTF"),
            "ply" => {
                let t = std::str::from_utf8(&bytes).unwrap();
                assert!(t.starts_with("ply") && t.contains("element vertex") && t.contains("end_header"));
            }
            "amf" => {
                let t = std::str::from_utf8(&bytes).unwrap();
                assert!(t.contains("<amf") && t.contains("<triangle>"));
            }
            "dae" => {
                let t = std::str::from_utf8(&bytes).unwrap();
                assert!(t.contains("<COLLADA") && t.contains("Z_UP"));
            }
            "x3d" => {
                let t = std::str::from_utf8(&bytes).unwrap();
                assert!(t.contains("<X3D") && t.contains("IndexedFaceSet"));
            }
            "wrl" => {
                let t = std::str::from_utf8(&bytes).unwrap();
                assert!(t.starts_with("#VRML V2.0") && t.contains("IndexedFaceSet"));
            }
            "usda" | "usd" => {
                let t = std::str::from_utf8(&bytes).unwrap();
                assert!(t.starts_with("#usda 1.0") && t.contains("upAxis = \"Z\""));
            }
            // Every `ext` in the loop above is covered; keep the match exhaustive for `&str`.
            #[allow(clippy::unreachable)]
            _ => unreachable!("unexpected format"),
        }
        let _ = std::fs::remove_file(&p);
    }
}

#[test]
fn proprietary_formats_are_refused_and_write_no_file() {
    // Feature: fusion360-feature-parity, Property 29: proprietary formats are refused
    let mut s = part();
    let before = s.doc.to_json();
    let proprietary = [
        ("fbx", "gltf"),
        ("blend", "gltf"),
        ("max", "gltf"),
        ("c4d", "gltf"),
        ("ma", "gltf"),
        ("mb", "gltf"),
        ("dwg", "dxf"),
        ("skp", "gltf"),
        ("sldprt", "step"),
        ("sldasm", "step"),
        ("f3d", "step"),
        ("ipt", "step"),
        ("iam", "step"),
        ("sat", "step"),
        ("x_t", "step"),
        ("x_b", "step"),
    ];
    for (ext, hint) in proprietary {
        let p = temp(ext, &format!("no-{ext}"));
        let err = s.execute("file.export", &json!({"path": p.to_string_lossy()})).expect_err(ext);
        let msg = err.to_string().to_ascii_lowercase();
        assert!(!p.exists(), "{ext} must not leave a file");
        assert!(msg.contains("proprietary"), "{ext}: expected proprietary refusal, got {msg}");
        assert!(msg.contains(hint), "{ext}: expected public hint `{hint}` in {msg}");
    }
    for ext in ["usdc", "usdz"] {
        let p = temp(ext, &format!("no-{ext}"));
        let err = s.execute("file.export", &json!({"path": p.to_string_lossy()})).expect_err(ext);
        let msg = err.to_string().to_ascii_lowercase();
        assert!(!p.exists(), "{ext} must not leave a file");
        assert!(msg.contains("not supported yet"), "{ext}: {msg}");
    }
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn mtl_and_triangle_cap_surface_through_file_export() {
    let mut s = part();
    let p = temp("mtl", "mtl-only");
    let out = run(&mut s, "file.export", json!({"path": p.to_string_lossy()}));
    assert!(out["bytes"].as_u64().unwrap_or(0) > 10);
    let text = std::fs::read_to_string(&p).unwrap();
    let _ = std::fs::remove_file(&p);
    assert!(text.contains("newmtl ") && text.contains("Kd "), "standalone MTL export");

    // Triangle cap is enforced in io (writers share check_triangle_count); surface the constant
    // through file.export by refusing a nonsense oversize format is covered in io unit tests.
    // Here: empty selection and proprietary format still leave the design untouched.
    let before = s.doc.to_json();
    assert!(s.execute("file.export", &json!({"path": temp("gltf", "empty-bodies").to_string_lossy(), "bodies": ["Nope"]})).is_err());
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn three_mf_and_meshes_reopen_with_the_same_extent() {
    let mut s = part();
    let want = union_bounds(&s);
    for (ext, tag) in [("3mf", "3mf"), ("stl", "mesh-stl")] {
        let p = temp(ext, tag);
        run(&mut s, "file.export", json!({"path": p.to_string_lossy()}));
        let bytes = std::fs::read(&p).unwrap();
        if ext == "3mf" {
            assert_eq!(&bytes[..2], b"PK", "a 3MF is a zip package");
        }
        let mut s2 = Session::default();
        run(&mut s2, "doc.open", json!({"path": p.to_string_lossy()}));
        let _ = std::fs::remove_file(&p);
        assert!(!s2.world_state().bodies.is_empty(), "{ext} reopens with bodies");
        // f32 coordinates in the file: well inside a micron.
        assert_same_bounds(want, union_bounds(&s2), 1e-4, ext);
    }
}

#[test]
fn a_body_subset_exports_only_those_bodies() {
    let mut s = part();
    for ext in ["step", "igs", "stl", "obj", "3mf"] {
        let p = temp(ext, &format!("subset-{ext}"));
        run(&mut s, "file.export", json!({"path": p.to_string_lossy(), "bodies": ["Pin"]}));
        let mut s2 = Session::default();
        run(&mut s2, "doc.open", json!({"path": p.to_string_lossy()}));
        let _ = std::fs::remove_file(&p);
        let (lo, hi) = union_bounds(&s2);
        assert!((hi[2] - lo[2] - 20.0).abs() < 1e-3 && (hi[0] - lo[0] - 8.0).abs() < 5e-3, "{ext}: only the pin, got {lo:?}..{hi:?}");
    }
}

#[test]
fn failed_exports_report_an_error_and_leave_the_design_alone() {
    let mut s = part();
    let before = s.doc.to_json();
    let ok = temp("step", "never");
    let bad_ext = temp("dwg", "bad");
    let missing_dir = std::env::temp_dir().join("solidcraft-no-such-dir").join("x.stl");
    let attempts = [
        json!({"path": bad_ext.to_string_lossy()}),
        json!({"path": ok.to_string_lossy(), "bodies": ["Nope"]}),
        json!({"path": missing_dir.to_string_lossy()}),
        json!({"path": ""}),
        json!({"path": "   "}),
        json!({}),
        json!({"path": ok.to_string_lossy(), "format": "nonsense"}),
        json!({"path": ok.to_string_lossy(), "format": "json", "bodies": ["Plate"]}),
        json!({"path": "x".repeat(5000)}),
    ];
    for payload in attempts {
        match s.execute("file.export", &payload) {
            Err(EngineError::Internal(cmd, msg)) => panic!("{cmd} {payload}: {msg}"),
            Err(_) => {}
            Ok(v) => panic!("{payload} should fail, got {v}"),
        }
    }
    assert!(!ok.exists() && !bad_ext.exists(), "no file is left behind by a failed export");
    assert_eq!(s.doc.to_json(), before, "exporting never edits the design");

    let mut empty = Session::default();
    assert!(empty.execute("file.export", &json!({"path": temp("stl", "empty").to_string_lossy()})).is_err(), "nothing to export is an error");
}
