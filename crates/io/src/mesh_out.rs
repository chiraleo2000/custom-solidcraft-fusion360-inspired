//! Tessellated 3D interchange writers from public specifications: Wavefront MTL, Khronos
//! glTF/GLB, Stanford PLY, ASTM AMF, COLLADA, X3D, VRML 97, VRM (glTF-based), and OpenUSD
//! ASCII (`.usda` / `.usd`). Proprietary vendor formats are refused in [`crate::Format::from_name`].

use solidcraft_geom::Mesh;

use crate::{IoError, Result};

/// Most triangles one tessellated export may write (hostile-input cap).
pub const MAX_EXPORT_TRIANGLES: usize = 5_000_000;

/// One body ready for a mesh/scene writer.
#[derive(Clone, Debug)]
pub struct MeshExport {
    pub name: String,
    pub mesh: Mesh,
    /// sRGB in 0..1; default grey when the body has no colour.
    pub color: [f32; 3],
    /// 1 = opaque.
    pub opacity: f32,
}

impl MeshExport {
    pub fn from_body(name: String, mesh: Mesh, color: Option<[f32; 3]>, opacity: f32) -> Result<Self> {
        for p in &mesh.positions {
            if !p.is_finite() {
                return Err(IoError::Invalid("export mesh has non-finite positions".into()));
            }
        }
        Ok(Self {
            name,
            mesh,
            color: color.unwrap_or([0.75, 0.75, 0.75]),
            opacity: opacity.clamp(0.0, 1.0),
        })
    }
}

/// Total triangle count across bodies; errors past [`MAX_EXPORT_TRIANGLES`].
pub fn check_triangle_cap(bodies: &[MeshExport]) -> Result<()> {
    let n: usize = bodies.iter().map(|b| b.mesh.triangles.len()).sum();
    if n > MAX_EXPORT_TRIANGLES {
        return Err(IoError::Invalid(format!("export with {n} triangles (limit {MAX_EXPORT_TRIANGLES})")));
    }
    Ok(())
}

fn safe_ident(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    if s.is_empty() { "Body".into() } else { s }
}

fn mtl_name(body: &str) -> String {
    format!("mat_{}", safe_ident(body))
}

/// Wavefront MTL beside an OBJ (public Wavefront companion format).
pub fn write_mtl(bodies: &[MeshExport]) -> String {
    let mut s = String::from("# SolidCraft MTL export\n");
    for b in bodies {
        s += &format!("newmtl {}\n", mtl_name(&b.name));
        let [r, g, bl] = b.color;
        s += &format!("Kd {r} {g} {bl}\n");
        s += &format!("d {}\nillum 1\n\n", b.opacity);
    }
    s
}

/// Wavefront OBJ that references `mtl_file` via `mtllib` / `usemtl`.
pub fn write_obj_with_mtl(bodies: &[MeshExport], mtl_file: &str) -> String {
    let mut s = String::from("# SolidCraft OBJ export (mm)\n");
    if !mtl_file.is_empty() {
        s += &format!("mtllib {mtl_file}\n");
    }
    let mut base = 1usize;
    for b in bodies {
        let oname = safe_ident(&b.name);
        s += &format!("o {oname}\nusemtl {}\n", mtl_name(&b.name));
        for p in &b.mesh.positions {
            s += &format!("v {} {} {}\n", p.x, p.y, p.z);
        }
        for n in &b.mesh.normals {
            s += &format!("vn {} {} {}\n", n.x, n.y, n.z);
        }
        let has_n = !b.mesh.normals.is_empty();
        for t in &b.mesh.triangles {
            let [a, c, d] = t.map(|i| i as usize + base);
            if has_n {
                s += &format!("f {a}//{a} {c}//{c} {d}//{d}\n");
            } else {
                s += &format!("f {a} {c} {d}\n");
            }
        }
        base += b.mesh.positions.len();
    }
    s
}

/// Stanford PLY (ASCII).
pub fn write_ply(bodies: &[MeshExport]) -> String {
    let n_v: usize = bodies.iter().map(|b| b.mesh.positions.len()).sum();
    let n_f: usize = bodies.iter().map(|b| b.mesh.triangles.len()).sum();
    let mut s = format!(
        "ply\nformat ascii 1.0\ncomment SolidCraft PLY export (mm, Z-up)\nelement vertex {n_v}\nproperty float x\nproperty float y\nproperty float z\nelement face {n_f}\nproperty list uchar int vertex_indices\nend_header\n"
    );
    for b in bodies {
        for p in &b.mesh.positions {
            s += &format!("{} {} {}\n", p.x, p.y, p.z);
        }
    }
    let mut base = 0usize;
    for b in bodies {
        for t in &b.mesh.triangles {
            let [a, c, d] = t.map(|i| i as usize + base);
            s += &format!("3 {a} {c} {d}\n");
        }
        base += b.mesh.positions.len();
    }
    s
}

/// ASTM AMF (XML triangle mesh + colour).
pub fn write_amf(bodies: &[MeshExport], name: &str) -> String {
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<amf unit=\"millimeter\" version=\"1.1\">\n  <metadata type=\"name\">{}</metadata>\n  <metadata type=\"producer\">SolidCraft</metadata>\n",
        xml_escape(name)
    );
    for (i, b) in bodies.iter().enumerate() {
        let [r, g, bl] = b.color;
        s += &format!(
            "  <object id=\"{i}\">\n    <metadata type=\"name\">{}</metadata>\n    <color><r>{r}</r><g>{g}</g><b>{bl}</b><a>{}</a></color>\n    <mesh>\n      <vertices>\n",
            xml_escape(&b.name),
            b.opacity
        );
        for p in &b.mesh.positions {
            s += &format!("        <vertex><coordinates><x>{}</x><y>{}</y><z>{}</z></coordinates></vertex>\n", p.x, p.y, p.z);
        }
        s += "      </vertices>\n      <volume>\n";
        for t in &b.mesh.triangles {
            s += &format!("        <triangle><v1>{}</v1><v2>{}</v2><v3>{}</v3></triangle>\n", t[0], t[1], t[2]);
        }
        s += "      </volume>\n    </mesh>\n  </object>\n";
    }
    s += "</amf>\n";
    s
}

/// COLLADA 1.4.1 (triangle meshes).
pub fn write_dae(bodies: &[MeshExport], name: &str) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <asset>
    <contributor><authoring_tool>SolidCraft</authoring_tool></contributor>
    <unit name="millimeter" meter="0.001"/>
    <up_axis>Z_UP</up_axis>
  </asset>
  <library_geometries>
"#,
    );
    for (i, b) in bodies.iter().enumerate() {
        let id = format!("geom{i}");
        let pos: String = b.mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).map(|x| format!("{x} ")).collect();
        let idx: String = b.mesh.triangles.iter().flat_map(|t| t.iter().copied()).map(|i| format!("{i} ")).collect();
        let n_v = b.mesh.positions.len();
        let n_t = b.mesh.triangles.len();
        let name = xml_escape(&b.name);
        // Build with push_str so COLLADA `#id-…` refs are not confused with Rust format syntax.
        s += "    <geometry id=\"";
        s += &id;
        s += "\" name=\"";
        s += &name;
        s += "\">\n      <mesh>\n        <source id=\"";
        s += &id;
        s += "-pos\"><float_array id=\"";
        s += &id;
        s += "-pos-array\" count=\"";
        s += &format!("{}", n_v * 3);
        s += "\">";
        s += &pos;
        s += "</float_array>\n          <technique_common><accessor source=\"#";
        s += &id;
        s += "-pos-array\" count=\"";
        s += &format!("{n_v}");
        s += "\" stride=\"3\">\n            <param name=\"X\" type=\"float\"/><param name=\"Y\" type=\"float\"/><param name=\"Z\" type=\"float\"/>\n          </accessor></technique_common>\n        </source>\n        <vertices id=\"";
        s += &id;
        s += "-vtx\"><input semantic=\"POSITION\" source=\"#";
        s += &id;
        s += "-pos\"/></vertices>\n        <triangles count=\"";
        s += &format!("{n_t}");
        s += "\"><input semantic=\"VERTEX\" source=\"#";
        s += &id;
        s += "-vtx\" offset=\"0\"/><p>";
        s += &idx;
        s += "</p></triangles>\n      </mesh>\n    </geometry>\n";
    }
    s += "  </library_geometries>\n  <library_visual_scenes>\n    <visual_scene id=\"Scene\" name=\"";
    s += &xml_escape(name);
    s += "\">\n";
    for (i, b) in bodies.iter().enumerate() {
        s += &format!(
            "      <node id=\"node{i}\" name=\"{}\"><instance_geometry url=\"#geom{i}\"/></node>\n",
            xml_escape(&b.name)
        );
    }
    s += "    </visual_scene>\n  </library_visual_scenes>\n  <scene><instance_visual_scene url=\"#Scene\"/></scene>\n</COLLADA>\n";
    s
}

/// X3D XML encoding.
pub fn write_x3d(bodies: &[MeshExport], name: &str) -> String {
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<X3D profile=\"Interchange\" version=\"3.3\" xmlns:xsd=\"http://www.w3.org/2001/XMLSchema-instance\">\n  <head><meta name=\"title\" content=\"{}\"/><meta name=\"generator\" content=\"SolidCraft\"/></head>\n  <Scene>\n    <WorldInfo title=\"{}\"/>\n",
        xml_escape(name),
        xml_escape(name)
    );
    for b in bodies {
        let [r, g, bl] = b.color;
        let coord: String = b.mesh.positions.iter().map(|p| format!("{} {} {},", p.x, p.y, p.z)).collect();
        let idx: String = b.mesh.triangles.iter().map(|t| format!("{} {} {} -1,", t[0], t[1], t[2])).collect();
        s += &format!(
            "    <Shape>\n      <Appearance><Material diffuseColor=\"{r} {g} {bl}\" transparency=\"{}\"/></Appearance>\n      <IndexedFaceSet solid=\"true\" coordIndex=\"{}\">\n        <Coordinate point=\"{}\"/>\n      </IndexedFaceSet>\n    </Shape>\n",
            1.0 - b.opacity,
            idx.trim_end_matches(','),
            coord.trim_end_matches(',')
        );
    }
    s += "  </Scene>\n</X3D>\n";
    s
}

/// VRML 97 (VRML 2.0) text.
pub fn write_vrml(bodies: &[MeshExport], name: &str) -> String {
    let mut s = format!("#VRML V2.0 utf8\n# SolidCraft export (mm, Z-up)\nWorldInfo {{ title \"{}\" }}\n", vrml_escape(name));
    for b in bodies {
        let [r, g, bl] = b.color;
        let coord: String = b.mesh.positions.iter().map(|p| format!("{} {} {},", p.x, p.y, p.z)).collect();
        let idx: String = b.mesh.triangles.iter().map(|t| format!("{} {} {} -1,", t[0], t[1], t[2])).collect();
        s += &format!(
            "Shape {{\n  appearance Appearance {{\n    material Material {{ diffuseColor {r} {g} {bl} transparency {} }}\n  }}\n  geometry IndexedFaceSet {{\n    coord Coordinate {{ point [ {} ] }}\n    coordIndex [ {} ]\n    solid TRUE\n  }}\n}}\n",
            1.0 - b.opacity,
            coord.trim_end_matches(','),
            idx.trim_end_matches(',')
        );
    }
    s
}

/// OpenUSD ASCII (`.usda`). `.usd` may carry the same ASCII payload.
pub fn write_usda(bodies: &[MeshExport], name: &str) -> String {
    let root = safe_ident(name);
    let mut s = format!(
        r#"#usda 1.0
(
    defaultPrim = "{root}"
    upAxis = "Z"
    metersPerUnit = 0.001
    doc = "SolidCraft USD export"
)

def Xform "{root}" {{
"#
    );
    for b in bodies {
        let n = safe_ident(&b.name);
        let points: String = b.mesh.positions.iter().map(|p| format!("({}, {}, {}),", p.x, p.y, p.z)).collect();
        let counts: String = b.mesh.triangles.iter().map(|_| "3,").collect();
        let indices: String = b.mesh.triangles.iter().flat_map(|t| t.iter().copied()).map(|i| format!("{i},")).collect();
        let [r, g, bl] = b.color;
        s += &format!(
            r#"    def Mesh "{n}" {{
        int[] faceVertexCounts = [{counts}]
        int[] faceVertexIndices = [{indices}]
        point3f[] points = [{points}]
        color3f[] primvars:displayColor = [({r}, {g}, {bl})]
        float primvars:displayOpacity = {opacity}
        uniform token subdivisionScheme = "none"
    }}
"#,
            counts = counts.trim_end_matches(','),
            indices = indices.trim_end_matches(','),
            points = points.trim_end_matches(','),
            opacity = b.opacity
        );
    }
    s += "}\n";
    s
}

/// glTF 2.0 JSON document (embedded buffer as a data URI) plus optional VRM extension.
fn gltf_document(bodies: &[MeshExport], name: &str, vrm: bool) -> Result<(serde_json::Value, Vec<u8>)> {
    // Pack all meshes into one buffer: interleaved is unnecessary; concatenate positions then indices.
    let mut bin = Vec::new();
    let mut meshes_json = Vec::new();
    let mut accessors = Vec::new();
    let mut buffer_views = Vec::new();
    let mut nodes = Vec::new();
    let mut materials = Vec::new();

    for (i, b) in bodies.iter().enumerate() {
        let pos_pad = align4(bin.len());
        bin.resize(pos_pad, 0);
        let pos_offset = bin.len();
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &b.mesh.positions {
            let v = [p.x as f32, p.y as f32, p.z as f32];
            for (k, x) in v.iter().enumerate() {
                if !x.is_finite() {
                    return Err(IoError::Invalid("export mesh has non-finite positions".into()));
                }
                min[k] = min[k].min(*x);
                max[k] = max[k].max(*x);
            }
            bin.extend_from_slice(&v[0].to_le_bytes());
            bin.extend_from_slice(&v[1].to_le_bytes());
            bin.extend_from_slice(&v[2].to_le_bytes());
        }
        let pos_len = bin.len() - pos_offset;
        let pos_view = buffer_views.len();
        buffer_views.push(serde_json::json!({"buffer": 0, "byteOffset": pos_offset, "byteLength": pos_len}));
        let pos_acc = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": pos_view, "componentType": 5126, "count": b.mesh.positions.len(),
            "type": "VEC3", "min": min, "max": max
        }));

        let idx_pad = align4(bin.len());
        bin.resize(idx_pad, 0);
        let idx_offset = bin.len();
        for t in &b.mesh.triangles {
            for &vi in t {
                bin.extend_from_slice(&vi.to_le_bytes());
            }
        }
        let idx_len = bin.len() - idx_offset;
        let idx_view = buffer_views.len();
        buffer_views.push(serde_json::json!({"buffer": 0, "byteOffset": idx_offset, "byteLength": idx_len}));
        let idx_acc = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": idx_view, "componentType": 5125, "count": b.mesh.triangles.len() * 3, "type": "SCALAR"
        }));

        let mat = materials.len();
        let [r, g, bl] = b.color;
        materials.push(serde_json::json!({
            "name": mtl_name(&b.name),
            "pbrMetallicRoughness": {
                "baseColorFactor": [r, g, bl, b.opacity],
                "metallicFactor": 0.0,
                "roughnessFactor": 0.5
            },
            "alphaMode": if b.opacity < 0.999 { "BLEND" } else { "OPAQUE" }
        }));

        meshes_json.push(serde_json::json!({
            "name": b.name,
            "primitives": [{
                "attributes": {"POSITION": pos_acc},
                "indices": idx_acc,
                "material": mat,
                "mode": 4
            }]
        }));
        nodes.push(serde_json::json!({"name": b.name, "mesh": i}));
    }

    let scene_nodes: Vec<usize> = (0..nodes.len()).collect();
    let mut root = serde_json::json!({
        "asset": {"version": "2.0", "generator": "SolidCraft", "extras": {"upAxis": "Z", "units": "mm"}},
        "scene": 0,
        "scenes": [{"name": name, "nodes": scene_nodes}],
        "nodes": nodes,
        "meshes": meshes_json,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{"byteLength": bin.len()}],
    });

    if vrm {
        root["extensionsUsed"] = serde_json::json!(["VRMC_vrm"]);
        root["extensions"] = serde_json::json!({
            "VRMC_vrm": {
                "specVersion": "1.0",
                "meta": {
                    "name": name,
                    "version": "1.0",
                    "authors": ["SolidCraft"],
                    "licenseUrl": "https://vrm.dev/licenses/1.0/",
                    "avatarPermission": "onlyAuthor",
                    "allowExcessivelyViolentUsage": false,
                    "allowExcessivelySexualUsage": false,
                    "allowPoliticalOrReligiousUsage": false,
                    "allowAntisocialOrHateUsage": false,
                    "creditNotation": "required",
                    "allowRedistribution": false,
                    "allowModification": "prohibited"
                }
            }
        });
    }

    Ok((root, bin))
}

/// glTF 2.0 with an embedded base64 buffer (single `.gltf` file).
pub fn write_gltf(bodies: &[MeshExport], name: &str) -> Result<Vec<u8>> {
    let (mut root, bin) = gltf_document(bodies, name, false)?;
    let b64 = base64_encode(&bin);
    root["buffers"] = serde_json::json!([{
        "byteLength": bin.len(),
        "uri": format!("data:application/octet-stream;base64,{b64}")
    }]);
    serde_json::to_vec_pretty(&root).map_err(|e| IoError::Invalid(format!("gltf encode: {e}")))
}

/// Binary glTF (`.glb`), optionally tagged as a minimal VRM 1.0 container.
pub fn write_glb(bodies: &[MeshExport], name: &str, vrm: bool) -> Result<Vec<u8>> {
    let (mut root, bin) = gltf_document(bodies, name, vrm)?;
    // GLB carries the buffer in chunk 1; drop any uri.
    root["buffers"] = serde_json::json!([{"byteLength": bin.len()}]);
    let json = serde_json::to_vec(&root).map_err(|e| IoError::Invalid(format!("glb encode: {e}")))?;
    let json_pad = align4(json.len());
    let bin_pad = align4(bin.len());
    let mut json_chunk = json;
    json_chunk.resize(json_pad, b' ');
    let mut bin_chunk = bin;
    bin_chunk.resize(bin_pad, 0);

    let total = 12 + 8 + json_chunk.len() + 8 + bin_chunk.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&0x4654_6C67u32.to_le_bytes()); // glTF
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // JSON
    out.extend_from_slice(&json_chunk);
    out.extend_from_slice(&(bin_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // BIN\0
    out.extend_from_slice(&bin_chunk);
    Ok(out)
}

fn align4(n: usize) -> usize {
    n.div_ceil(4) * 4
}

fn xml_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&apos;".into(),
            c => c.to_string(),
        })
        .collect()
}

fn vrml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Minimal base64 (no dependency) for glTF data URIs.
fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (a << 16) | (b << 8) | c;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Collect tessellated export meshes from selected model bodies.
pub fn collect_meshes(
    sel: &[&solidcraft_doc::ModelBody],
    tessellate: impl Fn(&solidcraft_kernel::Body) -> Result<Mesh>,
) -> Result<Vec<MeshExport>> {
    let mut out = Vec::with_capacity(sel.len());
    for b in sel {
        let mesh = tessellate(&b.body)?;
        let opacity = b.body.paint().map(|p| p.opacity).unwrap_or(1.0);
        out.push(MeshExport::from_body(b.name.clone(), mesh, b.body.color(), opacity)?);
    }
    check_triangle_cap(&out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;

    fn tri() -> MeshExport {
        let mesh = Mesh {
            positions: vec![Vec3::new(0., 0., 0.), Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)],
            triangles: vec![[0, 1, 2]],
            tri_face: vec![0],
            ..Default::default()
        };
        MeshExport::from_body("A".into(), mesh, Some([1., 0., 0.]), 1.0).unwrap()
    }

    #[test]
    fn ply_and_obj_mtl_mention_geometry() {
        let b = [tri()];
        let ply = write_ply(&b);
        assert!(ply.contains("element vertex 3") && ply.contains("element face 1"));
        let mtl = write_mtl(&b);
        assert!(mtl.contains("newmtl mat_A") && mtl.contains("Kd 1"));
        let obj = write_obj_with_mtl(&b, "part.mtl");
        assert!(obj.contains("mtllib part.mtl") && obj.contains("usemtl mat_A") && obj.contains("f "));
    }

    #[test]
    fn glb_has_gltf_magic() {
        let b = [tri()];
        let glb = write_glb(&b, "t", false).unwrap();
        assert_eq!(&glb[..4], b"glTF");
        let vrm = write_glb(&b, "t", true).unwrap();
        assert_eq!(&vrm[..4], b"glTF");
    }

    #[test]
    fn usda_is_ascii_usd() {
        let u = write_usda(&[tri()], "Part");
        assert!(u.starts_with("#usda 1.0") && u.contains("upAxis = \"Z\"") && u.contains("def Mesh"));
    }
}
