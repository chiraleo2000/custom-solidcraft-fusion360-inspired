//! Drawing-sheet export from the public DXF and PDF specifications.
//!
//! DXF is ASCII R12 (Autodesk DXF reference): HEADER `$INSUNITS` = 4 (millimetres), a LTYPE table,
//! a LAYER table, and an ENTITIES section of LINE and TEXT. The dashed linetype `SC_DASH` is
//! SolidCraft's own pattern, not a vendor linetype file.
//!
//! PDF is a minimal ISO 32000-1 file: catalog, page tree, one page, a content stream, and the
//! standard Helvetica type-1 font. Coordinates are scaled to fit the page.

/// How a sheet segment is drawn. Layers and dash patterns follow from this, not from a template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawKind {
    Visible,
    Hidden,
    Section,
    Title,
    Annotation,
}

/// One straight segment in sheet millimetres, y up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawSeg {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub kind: DrawKind,
}

/// One single-line label in sheet millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawText {
    pub at: [f64; 2],
    pub text: String,
    pub height: f64,
    pub kind: DrawKind,
}

impl DrawKind {
    fn layer(self) -> &'static str {
        match self {
            Self::Visible => "VISIBLE",
            Self::Hidden => "HIDDEN",
            Self::Section => "SECTION",
            Self::Title => "TITLE",
            Self::Annotation => "ANNOTATION",
        }
    }

    fn dxf_color(self) -> &'static str {
        match self {
            Self::Visible | Self::Title | Self::Annotation => "7",
            Self::Hidden => "8",
            Self::Section => "1",
        }
    }

    fn linetype(self) -> &'static str {
        if self == Self::Hidden { "SC_DASH" } else { "CONTINUOUS" }
    }
}

fn finite(x: f64) -> bool {
    x.is_finite() && x.abs() < 1.0e9
}

fn num(x: f64) -> String {
    if finite(x) { format!("{x:.6}") } else { "0".into() }
}

fn plain(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii() && !c.is_control() && *c != '<' && *c != '>' && *c != '&').take(200).collect()
}

fn pdf_escape(text: &str) -> String {
    let mut out = String::new();
    for c in plain(text).chars() {
        if c == '(' || c == ')' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// ASCII DXF of the sheet. Millimetres (`$INSUNITS` 4). Empty input is still a valid file.
pub fn write_drawing_dxf(segs: &[DrawSeg], texts: &[DrawText]) -> String {
    let mut s = String::new();
    let mut w = |c: i32, v: &str| {
        s.push_str(&format!("{c}\n{v}\n"));
    };
    w(0, "SECTION");
    w(2, "HEADER");
    w(9, "$ACADVER");
    w(1, "AC1009");
    w(9, "$INSUNITS");
    w(70, "4");
    w(0, "ENDSEC");
    w(0, "SECTION");
    w(2, "TABLES");
    w(0, "TABLE");
    w(2, "LTYPE");
    w(70, "2");
    w(0, "LTYPE");
    w(2, "CONTINUOUS");
    w(70, "0");
    w(3, "Solid line");
    w(72, "65");
    w(73, "0");
    w(40, "0.0");
    w(0, "LTYPE");
    w(2, "SC_DASH");
    w(70, "0");
    w(3, "SolidCraft dashed");
    w(72, "65");
    w(73, "2");
    w(40, "3.75");
    w(49, "2.5");
    w(74, "0");
    w(49, "-1.25");
    w(74, "0");
    w(0, "ENDTAB");
    w(0, "TABLE");
    w(2, "LAYER");
    w(70, "5");
    for kind in [DrawKind::Visible, DrawKind::Hidden, DrawKind::Section, DrawKind::Title, DrawKind::Annotation] {
        w(0, "LAYER");
        w(2, kind.layer());
        w(70, "0");
        w(62, kind.dxf_color());
        w(6, kind.linetype());
    }
    w(0, "ENDTAB");
    w(0, "ENDSEC");
    w(0, "SECTION");
    w(2, "ENTITIES");
    for seg in segs.iter().take(8_000) {
        if !finite(seg.a[0]) || !finite(seg.a[1]) || !finite(seg.b[0]) || !finite(seg.b[1]) {
            continue;
        }
        w(0, "LINE");
        w(8, seg.kind.layer());
        w(6, seg.kind.linetype());
        w(62, seg.kind.dxf_color());
        w(10, &num(seg.a[0]));
        w(20, &num(seg.a[1]));
        w(11, &num(seg.b[0]));
        w(21, &num(seg.b[1]));
    }
    for text in texts.iter().take(2_000) {
        let body = plain(&text.text);
        if body.is_empty() || !finite(text.at[0]) || !finite(text.at[1]) {
            continue;
        }
        let height = if finite(text.height) && text.height > 0.0 { text.height } else { 2.5 };
        w(0, "TEXT");
        w(8, text.kind.layer());
        w(10, &num(text.at[0]));
        w(20, &num(text.at[1]));
        w(40, &num(height));
        w(1, &body);
    }
    w(0, "ENDSEC");
    w(0, "EOF");
    s
}

/// SVG 1.1 of the same segments. Y is flipped so the sheet reads upright in a viewer.
/// Segments and labels are grouped by SolidCraft layer (`VISIBLE`, `HIDDEN`, …).
pub fn write_drawing_svg(segs: &[DrawSeg], texts: &[DrawText]) -> String {
    let mut pts = Vec::new();
    for seg in segs.iter().take(8_000) {
        pts.push(svg_xy(seg.a));
        pts.push(svg_xy(seg.b));
    }
    for text in texts.iter().take(2_000) {
        pts.push(svg_xy(text.at));
    }
    let (minx, miny, maxx, maxy) = if pts.is_empty() {
        (-200.0, -200.0, 200.0, 200.0)
    } else {
        let mut minx = f64::INFINITY;
        let mut miny = f64::INFINITY;
        let mut maxx = f64::NEG_INFINITY;
        let mut maxy = f64::NEG_INFINITY;
        for p in pts {
            if !finite(p[0]) || !finite(p[1]) {
                continue;
            }
            minx = minx.min(p[0]);
            miny = miny.min(p[1]);
            maxx = maxx.max(p[0]);
            maxy = maxy.max(p[1]);
        }
        if minx.is_finite() { (minx - 8.0, miny - 8.0, maxx + 8.0, maxy + 8.0) } else { (-200.0, -200.0, 200.0, 200.0) }
    };
    let mut body = String::new();
    for kind in [DrawKind::Visible, DrawKind::Hidden, DrawKind::Section, DrawKind::Title, DrawKind::Annotation] {
        let mut group = String::new();
        for seg in segs.iter().take(8_000).filter(|s| s.kind == kind) {
            let a = svg_xy(seg.a);
            let b = svg_xy(seg.b);
            if !finite(a[0]) || !finite(a[1]) || !finite(b[0]) || !finite(b[1]) {
                continue;
            }
            let (stroke, width, dash) = svg_style(seg.kind);
            group.push_str(&format!(
                "<line x1=\"{:.3}\" y1=\"{:.3}\" x2=\"{:.3}\" y2=\"{:.3}\" stroke=\"{stroke}\" stroke-width=\"{width:.2}\" {dash}/>",
                a[0], a[1], b[0], b[1]
            ));
        }
        for text in texts.iter().take(2_000).filter(|t| t.kind == kind) {
            let shown = plain(&text.text);
            if shown.is_empty() {
                continue;
            }
            let at = svg_xy(text.at);
            if !finite(at[0]) || !finite(at[1]) {
                continue;
            }
            let height = if finite(text.height) && text.height > 0.0 { text.height } else { 3.0 };
            group.push_str(&format!("<text x=\"{:.3}\" y=\"{:.3}\" font-size=\"{height:.2}\" fill=\"#111\">{shown}</text>", at[0], at[1]));
        }
        if !group.is_empty() {
            body.push_str(&format!("<g id=\"{}\" data-layer=\"{}\">{group}</g>", kind.layer(), kind.layer()));
        }
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{minx:.3} {miny:.3} {:.3} {:.3}\">{body}</svg>",
        (maxx - minx).max(1.0),
        (maxy - miny).max(1.0)
    )
}

fn svg_xy(p: [f64; 2]) -> [f64; 2] {
    [p[0], -p[1]]
}

fn svg_style(kind: DrawKind) -> (&'static str, f64, &'static str) {
    match kind {
        DrawKind::Visible => ("#222", 0.25, ""),
        DrawKind::Hidden => ("#222", 0.18, "stroke-dasharray=\"1.2 0.7\""),
        DrawKind::Section => ("#9a3412", 0.4, ""),
        DrawKind::Title => ("#444", 0.25, ""),
        DrawKind::Annotation => ("#1d4e89", 0.2, ""),
    }
}

/// One-page PDF. `page_mm` is the sheet width and height. The drawing is fitted inside a margin.
pub fn write_drawing_pdf(page_mm: [f64; 2], segs: &[DrawSeg], texts: &[DrawText]) -> String {
    write_drawing_pdf_titled(page_mm, "SolidCraft", segs, texts)
}

/// One-page PDF with a document `/Title` in the Info dictionary (ISO 32000-1).
pub fn write_drawing_pdf_titled(page_mm: [f64; 2], title: &str, segs: &[DrawSeg], texts: &[DrawText]) -> String {
    let page_w = if finite(page_mm[0]) && page_mm[0] > 10.0 { page_mm[0] } else { 297.0 };
    let page_h = if finite(page_mm[1]) && page_mm[1] > 10.0 { page_mm[1] } else { 210.0 };
    let pt = 72.0 / 25.4;
    let pw = page_w * pt;
    let ph = page_h * pt;
    let mut stream = String::new();
    stream.push_str(&format!("0.6 w 28.3 28.3 m {:.2} 28.3 l {:.2} {:.2} l 28.3 {:.2} l h S\n", pw - 28.3, pw - 28.3, ph - 28.3, ph - 28.3));
    let map = fit_map(page_w, page_h, segs, texts);
    for seg in segs.iter().take(8_000) {
        let (Some(a), Some(b)) = (map.point(seg.a), map.point(seg.b)) else { continue };
        stream.push_str(pdf_stroke(seg.kind));
        stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", a[0], a[1], b[0], b[1]));
    }
    for text in texts.iter().take(2_000) {
        let shown = pdf_escape(&text.text);
        let Some(at) = map.point(text.at) else { continue };
        if shown.is_empty() {
            continue;
        }
        let height = map.scale * if finite(text.height) && text.height > 0.0 { text.height } else { 3.0 };
        let height = height.clamp(6.0, 24.0);
        stream.push_str(&format!("BT /F1 {height:.2} Tf 1 0 0 1 {:.2} {:.2} Tm ({shown}) Tj ET\n", at[0], at[1]));
    }
    let contents = format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len());
    let page = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>");
    let info_title = pdf_escape(title);
    let info = format!("<< /Title ({info_title}) /Producer (SolidCraft) >>");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        page,
        contents,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        info,
    ];
    assemble_with_info(&objects, 6)
}

fn assemble_with_info(objects: &[String], info_obj: usize) -> String {
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let startxref = out.len();
    out.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
    out.push_str("0000000000 65535 f \n");
    for off in &offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R /Info {} 0 R >>\nstartxref\n{startxref}\n%%EOF\n", objects.len() + 1, info_obj));
    out
}

struct Fit {
    minx: f64,
    miny: f64,
    scale: f64,
    ox: f64,
    oy: f64,
    pt: f64,
}

impl Fit {
    fn point(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        if !finite(p[0]) || !finite(p[1]) || !self.scale.is_finite() {
            return None;
        }
        Some([(self.ox + (p[0] - self.minx) * self.scale) * self.pt, (self.oy + (p[1] - self.miny) * self.scale) * self.pt])
    }
}

fn fit_map(page_w: f64, page_h: f64, segs: &[DrawSeg], texts: &[DrawText]) -> Fit {
    let mut minx = f64::INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut maxy = f64::NEG_INFINITY;
    let mut consider = |p: [f64; 2]| {
        if finite(p[0]) && finite(p[1]) {
            minx = minx.min(p[0]);
            miny = miny.min(p[1]);
            maxx = maxx.max(p[0]);
            maxy = maxy.max(p[1]);
        }
    };
    for seg in segs.iter().take(8_000) {
        consider(seg.a);
        consider(seg.b);
    }
    for text in texts.iter().take(2_000) {
        consider(text.at);
    }
    let margin = 15.0;
    let avail_w = (page_w - 2.0 * margin).max(10.0);
    let avail_h = (page_h - 2.0 * margin).max(10.0);
    if !minx.is_finite() {
        return Fit { minx: 0.0, miny: 0.0, scale: 1.0, ox: margin, oy: margin, pt: 72.0 / 25.4 };
    }
    let bw = (maxx - minx).max(1.0);
    let bh = (maxy - miny).max(1.0);
    let scale = (avail_w / bw).min(avail_h / bh).clamp(1.0e-4, 20.0);
    let ox = margin + (avail_w - bw * scale) * 0.5;
    let oy = margin + (avail_h - bh * scale) * 0.5;
    Fit { minx, miny, scale, ox, oy, pt: 72.0 / 25.4 }
}

fn pdf_stroke(kind: DrawKind) -> &'static str {
    match kind {
        DrawKind::Hidden => "0.4 w [1.6 1.0] 0 d\n",
        DrawKind::Section => "0.9 w [] 0 d\n",
        DrawKind::Annotation => "0.45 w [] 0 d\n",
        _ => "0.5 w [] 0 d\n",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_dxf;
    use crate::sketch2d::Geom2;

    fn line(x: f64, kind: DrawKind) -> DrawSeg {
        DrawSeg { a: [x, 0.0], b: [x + 10.0, 4.0], kind }
    }

    /// Feature: fusion360-feature-parity, Property 23: Drawing exports round-trip through public formats
    #[test]
    fn property_23_dxf_and_pdf_round_trip() {
        let mut cases = 0usize;
        for i in 0..100 {
            let kind = if i % 5 == 0 { DrawKind::Hidden } else { DrawKind::Visible };
            let segs = [line(i as f64, kind), DrawSeg { a: [0.0, i as f64], b: [3.0, i as f64 + 1.0], kind: DrawKind::Section }];
            let texts = [DrawText { at: [1.0, 2.0], text: format!("N{i}"), height: 2.5, kind: DrawKind::Annotation }];
            let dxf = write_drawing_dxf(&segs, &texts);
            assert!(dxf.contains("ENTITIES") && dxf.contains("$INSUNITS") && dxf.contains("SC_DASH"), "{dxf}");
            let back = read_dxf(&dxf).expect("dxf");
            let lines = back.iter().filter(|g| matches!(g, Geom2::Line(_, _))).count();
            let labels = back.iter().filter(|g| matches!(g, Geom2::Text { .. })).count();
            assert_eq!(lines, 2, "{back:?}");
            assert_eq!(labels, 1, "{back:?}");
            if let Geom2::Line(a, b) = &back[0] {
                assert!((a.x - i as f64).abs() < 1e-4 && (b.x - i as f64 - 10.0).abs() < 1e-4, "{a:?} {b:?}");
            } else {
                panic!("first entity should be the line");
            }
            let pdf = write_drawing_pdf([297.0, 210.0], &segs, &texts);
            assert!(pdf.starts_with("%PDF-1.4\n"), "{pdf}");
            assert!(pdf.contains(" l ") && pdf.contains("Helvetica") && pdf.contains("xref\n"), "{pdf}");
            assert!(pdf.contains("/Info 6 0 R") && pdf.contains("/Title (SolidCraft)"), "{pdf}");
            let marker = "startxref\n";
            let at = pdf.find(marker).expect("startxref");
            let off: usize = pdf[at + marker.len()..].lines().next().unwrap_or("0").trim().parse().unwrap_or(0);
            assert!(pdf.get(off..).is_some_and(|rest| rest.starts_with("xref\n")), "xref at {off}");
            assert!(pdf.contains(&format!("N{i}")));
            cases += 1;
        }
        assert_eq!(cases, 100);
        let empty = write_drawing_pdf([0.0, 0.0], &[], &[]);
        assert!(empty.contains("%PDF-1.4") && empty.contains(" l "));
        let titled = write_drawing_pdf_titled([420.0, 297.0], "Bracket", &segs_sample(), &[]);
        assert!(titled.contains("/Title (Bracket)") && titled.contains("MediaBox [0 0 1190.55 841.89]"), "{titled}");
        let svg = write_drawing_svg(
            &[DrawSeg { a: [0.0, 0.0], b: [10.0, 0.0], kind: DrawKind::Visible }, DrawSeg { a: [0.0, 1.0], b: [10.0, 1.0], kind: DrawKind::Hidden }],
            &[DrawText { at: [1.0, 2.0], text: "Hi".into(), height: 3.0, kind: DrawKind::Annotation }],
        );
        assert!(svg.contains("id=\"VISIBLE\"") && svg.contains("id=\"HIDDEN\"") && svg.contains("id=\"ANNOTATION\""), "{svg}");
    }

    fn segs_sample() -> [DrawSeg; 1] {
        [line(0.0, DrawKind::Visible)]
    }
}
