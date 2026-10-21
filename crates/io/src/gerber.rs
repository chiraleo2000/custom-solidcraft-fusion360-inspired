//! Gerber RS-274X subset, written from the public Ucamco Gerber Layer Format specification.
//!
//! One file is a board outline plus one copper layer, in millimetres: coordinate format 4.6,
//! absolute notation, linear draws and circular apertures. Arcs, regions, step-and-repeat and
//! clear polarity are refused. PCB autorouting stays deferred; this module only reads and writes
//! copper the caller already placed.

use std::collections::BTreeMap;

use crate::{IoError, Result};

/// Largest Gerber text we read.
const MAX_GERBER_BYTES: usize = 8 << 20;
const MAX_RECORDS: usize = 100_000;
/// Copper segments in one file.
pub const MAX_GERBER_TRACES: usize = 4096;

/// One copper draw, millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct GerberTrace {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub width: f64,
}

/// Board outline and one copper layer, millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct GerberLayer {
    pub width: f64,
    pub height: f64,
    pub traces: Vec<GerberTrace>,
}

/// Write a board as a Gerber RS-274X subset (millimetres, format 4.6, one copper layer).
pub fn write_gerber(layer: &GerberLayer) -> Result<String> {
    if !usable_size(layer.width) || !usable_size(layer.height) {
        return Err(IoError::Invalid("the board size is not usable".into()));
    }
    if layer.traces.len() > MAX_GERBER_TRACES {
        return Err(IoError::Invalid("too many traces".into()));
    }
    let mut widths = Vec::new();
    for trace in &layer.traces {
        check_trace(trace)?;
        if !widths.iter().any(|width: &f64| (*width - trace.width).abs() < 1e-9) {
            widths.push(trace.width);
        }
    }
    if widths.len() > 900 {
        return Err(IoError::Invalid("too many trace widths".into()));
    }
    let mut text = String::new();
    text.push_str("G04 SolidCraft Gerber subset of the Ucamco Gerber Layer Format.*\n");
    text.push_str("G04 PCB autorouting is not supported yet.*\n");
    text.push_str("%FSLAX46Y46*%\n");
    text.push_str("%MOMM*%\n");
    text.push_str("%ADD10C,0.100000*%\n");
    let mut next_code = 11u32;
    let mut codes = Vec::new();
    for width in &widths {
        let code = if (*width - 0.1).abs() < 1e-9 {
            10
        } else {
            let code = next_code;
            next_code += 1;
            text.push_str(&format!("%ADD{code}C,{width:.6}*%\n"));
            code
        };
        codes.push((*width, code));
    }
    text.push_str("D10*\n");
    push_draw(&mut text, 0.0, 0.0, 2)?;
    push_draw(&mut text, layer.width, 0.0, 1)?;
    push_draw(&mut text, layer.width, layer.height, 1)?;
    push_draw(&mut text, 0.0, layer.height, 1)?;
    push_draw(&mut text, 0.0, 0.0, 1)?;
    text.push_str("G04 COPPER*\n");
    let mut current = None;
    for trace in &layer.traces {
        let code = codes
            .iter()
            .find(|(width, _)| (*width - trace.width).abs() < 1e-9)
            .map(|(_, code)| *code)
            .ok_or_else(|| IoError::Invalid("missing aperture".into()))?;
        if current != Some(code) {
            text.push_str(&format!("D{code}*\n"));
            current = Some(code);
        }
        push_draw(&mut text, trace.x0, trace.y0, 2)?;
        push_draw(&mut text, trace.x1, trace.y1, 1)?;
    }
    text.push_str("M02*\n");
    Ok(text)
}

/// Read the subset [`write_gerber`] emits. Other public Gerber commands that this subset does
/// not model are an error, not a silent drop.
pub fn read_gerber(text: &str) -> Result<GerberLayer> {
    let records = records(text)?;
    let mut plot = Plot::default();
    for record in &records {
        if plot.ended {
            break;
        }
        plot.apply(record)?;
    }
    let (width, height) = if plot.outlined {
        (plot.max_x, plot.max_y)
    } else {
        let mut max_x: f64 = 0.0;
        let mut max_y: f64 = 0.0;
        for trace in &plot.traces {
            max_x = max_x.max(trace.x0).max(trace.x1);
            max_y = max_y.max(trace.y0).max(trace.y1);
        }
        (max_x, max_y)
    };
    if !usable_size(width) || !usable_size(height) {
        return Err(IoError::Invalid("the Gerber file has no board outline".into()));
    }
    Ok(GerberLayer { width, height, traces: plot.traces })
}

fn usable_size(mm: f64) -> bool {
    mm.is_finite() && mm > 0.0 && mm < 10_000.0
}

fn check_trace(trace: &GerberTrace) -> Result<()> {
    let nums = [trace.x0, trace.y0, trace.x1, trace.y1, trace.width];
    if nums.iter().any(|n| !n.is_finite()) || trace.width <= 0.0 || trace.width >= 1_000.0 {
        return Err(IoError::Invalid("a trace is not usable".into()));
    }
    if nums[..4].iter().any(|n| n.abs() >= 10_000.0) {
        return Err(IoError::Invalid("a Gerber coordinate does not fit format 4.6".into()));
    }
    Ok(())
}

fn gerber_int(mm: f64) -> Result<i64> {
    if !mm.is_finite() || mm.abs() >= 10_000.0 {
        return Err(IoError::Invalid("a Gerber coordinate does not fit format 4.6".into()));
    }
    let scaled = (mm * 1_000_000.0).round();
    if !scaled.is_finite() || scaled.abs() > i64::MAX as f64 {
        return Err(IoError::Invalid("a Gerber coordinate does not fit format 4.6".into()));
    }
    Ok(scaled as i64)
}

fn push_draw(text: &mut String, x: f64, y: f64, op: u8) -> Result<()> {
    let x = gerber_int(x)?;
    let y = gerber_int(y)?;
    text.push_str(&format!("X{x}Y{y}D0{op}*\n"));
    Ok(())
}

fn push_record(out: &mut Vec<String>, record: &str) -> Result<()> {
    let record = record.trim();
    if record.is_empty() {
        return Ok(());
    }
    if out.len() >= MAX_RECORDS {
        return Err(IoError::Invalid("the Gerber file has too many commands".into()));
    }
    out.push(record.to_string());
    Ok(())
}

/// Graphics commands end at `*`. A parameter block is `%` … `%`, and the `*` inside it only
/// separates commands (the closing `%` is not part of the previous command).
fn records(text: &str) -> Result<Vec<String>> {
    if text.len() > MAX_GERBER_BYTES {
        return Err(IoError::Invalid("the Gerber file is too large".into()));
    }
    if text.chars().any(|c| c == '\0') {
        return Err(IoError::Invalid("the Gerber file is not text".into()));
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut parameter = false;
    for c in text.chars() {
        if c == '%' {
            if parameter {
                for command in cur.split('*') {
                    let command = command.trim();
                    if command.is_empty() {
                        continue;
                    }
                    push_record(&mut out, &format!("%{command}%"))?;
                }
                cur.clear();
                parameter = false;
            } else {
                push_record(&mut out, &cur)?;
                cur.clear();
                parameter = true;
            }
            continue;
        }
        if c == '*' && !parameter {
            push_record(&mut out, &cur)?;
            cur.clear();
            continue;
        }
        cur.push(c);
    }
    if parameter {
        return Err(IoError::Invalid("a Gerber parameter block is not closed".into()));
    }
    if !cur.trim().is_empty() {
        return Err(IoError::Invalid("a Gerber command is not terminated".into()));
    }
    Ok(out)
}

struct Plot {
    x: f64,
    y: f64,
    decimals: u32,
    integer_digits: u32,
    unit: f64,
    unit_seen: bool,
    aperture: Option<f64>,
    apertures: BTreeMap<u32, f64>,
    copper: bool,
    outlined: bool,
    max_x: f64,
    max_y: f64,
    traces: Vec<GerberTrace>,
    ended: bool,
}

impl Default for Plot {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            decimals: 6,
            integer_digits: 4,
            unit: 1.0,
            unit_seen: false,
            aperture: None,
            apertures: BTreeMap::new(),
            copper: false,
            outlined: false,
            max_x: 0.0,
            max_y: 0.0,
            traces: Vec::new(),
            ended: false,
        }
    }
}

impl Plot {
    fn apply(&mut self, record: &str) -> Result<()> {
        let trimmed = record.trim();
        let upper = trimmed.to_ascii_uppercase();
        if upper.starts_with("G04") {
            if upper.contains("COPPER") {
                self.copper = true;
            }
            return Ok(());
        }
        if upper.starts_with('M') {
            return self.mode_m(&upper);
        }
        if upper.starts_with('%') {
            return self.parameter(trimmed.trim_matches('%'));
        }
        if upper.starts_with('G') && !upper.contains('X') && !upper.contains('Y') {
            return self.mode_g(&upper);
        }
        self.plot(trimmed)
    }

    fn mode_m(&mut self, upper: &str) -> Result<()> {
        if upper.starts_with("M02") {
            self.ended = true;
            Ok(())
        } else {
            Err(IoError::Invalid(format!("unsupported Gerber command {upper}")))
        }
    }

    fn mode_g(&mut self, upper: &str) -> Result<()> {
        if upper.starts_with("G36") || upper.starts_with("G37") {
            return Err(IoError::Invalid("not supported yet: Gerber regions".into()));
        }
        if upper.starts_with("G02") || upper.starts_with("G03") {
            return Err(IoError::Invalid("not supported yet: Gerber arcs".into()));
        }
        if upper.starts_with("G70") {
            self.unit = 25.4;
            self.unit_seen = true;
        } else if upper.starts_with("G71") {
            self.unit = 1.0;
            self.unit_seen = true;
        }
        Ok(())
    }

    fn parameter(&mut self, body: &str) -> Result<()> {
        let tag = body.to_ascii_uppercase();
        if tag.starts_with("FS") {
            let (integer_digits, decimals) = parse_fs(body)?;
            self.integer_digits = integer_digits;
            self.decimals = decimals;
            return Ok(());
        }
        if tag.starts_with("MO") {
            self.unit = if tag.contains("IN") {
                25.4
            } else if tag.contains("MM") {
                1.0
            } else {
                return Err(IoError::Invalid("the Gerber unit is not millimetres or inches".into()));
            };
            self.unit_seen = true;
            return Ok(());
        }
        if tag.starts_with("ADD") {
            if !self.unit_seen {
                return Err(IoError::Invalid("the Gerber unit must come before an aperture".into()));
            }
            let (code, diameter) = parse_aperture(body)?;
            self.apertures.insert(code, diameter * self.unit);
            return Ok(());
        }
        if tag.starts_with("TF") || tag.starts_with("TA") || tag.starts_with("TD") || tag.starts_with("TO") || tag.starts_with("LPD") {
            return Ok(());
        }
        if tag.starts_with("LPC") {
            return Err(IoError::Invalid("not supported yet: clear Gerber polarity".into()));
        }
        Err(IoError::Invalid(format!("unsupported Gerber command %{body}%")))
    }

    fn plot(&mut self, record: &str) -> Result<()> {
        let drawn = parse_plot(record, self.integer_digits, self.decimals)?;
        if drawn.arc {
            return Err(IoError::Invalid("not supported yet: Gerber arcs".into()));
        }
        let x = drawn.x.map(|n| n * self.unit).unwrap_or(self.x);
        let y = drawn.y.map(|n| n * self.unit).unwrap_or(self.y);
        if !x.is_finite() || !y.is_finite() || x.abs() >= 10_000.0 || y.abs() >= 10_000.0 {
            return Err(IoError::Invalid("a Gerber coordinate is out of range".into()));
        }
        let Some(code) = drawn.d else {
            self.x = x;
            self.y = y;
            return Ok(());
        };
        if code >= 10 {
            self.aperture = Some(*self.apertures.get(&code).ok_or_else(|| IoError::Invalid(format!("unknown aperture D{code}")))?);
            return Ok(());
        }
        match code {
            2 => {
                self.x = x;
                self.y = y;
            }
            1 => self.draw(x, y)?,
            3 => self.flash(x, y)?,
            _ => return Err(IoError::Invalid(format!("unsupported Gerber D{code}"))),
        }
        Ok(())
    }

    fn draw(&mut self, x: f64, y: f64) -> Result<()> {
        let width = self.aperture.ok_or_else(|| IoError::Invalid("no aperture selected".into()))?;
        if self.copper {
            self.push_trace(self.x, self.y, x, y, width)?;
        } else {
            self.max_x = self.max_x.max(self.x).max(x);
            self.max_y = self.max_y.max(self.y).max(y);
            self.outlined = true;
        }
        self.x = x;
        self.y = y;
        Ok(())
    }

    fn flash(&mut self, x: f64, y: f64) -> Result<()> {
        let width = self.aperture.ok_or_else(|| IoError::Invalid("no aperture selected".into()))?;
        if self.copper {
            self.push_trace(x, y, x, y, width)?;
        }
        self.x = x;
        self.y = y;
        Ok(())
    }

    fn push_trace(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, width: f64) -> Result<()> {
        if self.traces.len() >= MAX_GERBER_TRACES {
            return Err(IoError::Invalid("too many traces".into()));
        }
        self.traces.push(GerberTrace { x0, y0, x1, y1, width });
        Ok(())
    }
}

struct Drawn {
    x: Option<f64>,
    y: Option<f64>,
    d: Option<u32>,
    arc: bool,
}

fn parse_fs(body: &str) -> Result<(u32, u32)> {
    let bytes = body.as_bytes();
    let kind = byte(bytes, 0)?;
    let spec = byte(bytes, 1)?;
    if !kind.eq_ignore_ascii_case(&b'F') || !spec.eq_ignore_ascii_case(&b'S') {
        return Err(IoError::Invalid("the Gerber format specification is incomplete".into()));
    }
    let zero = byte(bytes, 2)?;
    let notation = byte(bytes, 3)?;
    if zero.eq_ignore_ascii_case(&b'T') {
        return Err(IoError::Invalid("not supported yet: trailing-zero Gerber coordinates".into()));
    }
    if !zero.eq_ignore_ascii_case(&b'L') {
        return Err(IoError::Invalid("the Gerber format specification is incomplete".into()));
    }
    if notation.eq_ignore_ascii_case(&b'I') {
        return Err(IoError::Invalid("not supported yet: incremental Gerber coordinates".into()));
    }
    if !notation.eq_ignore_ascii_case(&b'A') || !byte(bytes, 4)?.eq_ignore_ascii_case(&b'X') || !byte(bytes, 7)?.eq_ignore_ascii_case(&b'Y') {
        return Err(IoError::Invalid("the Gerber format specification is incomplete".into()));
    }
    let xi = digit(byte(bytes, 5)?)?;
    let xd = digit(byte(bytes, 6)?)?;
    let yi = digit(byte(bytes, 8)?)?;
    let yd = digit(byte(bytes, 9)?)?;
    if !(1..=6).contains(&xi) || !(2..=6).contains(&xd) || xi != yi || xd != yd {
        return Err(IoError::Invalid("the Gerber format is not a supported X/Y pair".into()));
    }
    Ok((xi, xd))
}

fn byte(bytes: &[u8], index: usize) -> Result<u8> {
    bytes.get(index).copied().ok_or_else(|| IoError::Invalid("the Gerber format specification is incomplete".into()))
}

fn digit(byte: u8) -> Result<u32> {
    if byte.is_ascii_digit() { Ok((byte - b'0') as u32) } else { Err(IoError::Invalid("the Gerber format is not a supported X/Y pair".into())) }
}

fn parse_aperture(body: &str) -> Result<(u32, f64)> {
    let rest = body.get(3..).ok_or_else(|| IoError::Invalid("bad aperture".into()))?;
    let (code_text, after) = take_digits(rest)?;
    let code: u32 = code_text.parse().map_err(|_| IoError::Invalid("bad aperture".into()))?;
    if !(10..1000).contains(&code) {
        return Err(IoError::Invalid("aperture code out of range".into()));
    }
    let mut chars = after.chars();
    let kind = chars.next().ok_or_else(|| IoError::Invalid("bad aperture".into()))?;
    if !kind.eq_ignore_ascii_case(&'C') {
        return Err(IoError::Invalid(format!("not supported yet: Gerber aperture {kind}")));
    }
    let spec = chars.as_str().trim_start_matches(',');
    let diameter_text = spec.split(['X', 'x']).next().unwrap_or("");
    let diameter: f64 = diameter_text.parse().map_err(|_| IoError::Invalid("bad aperture diameter".into()))?;
    if !diameter.is_finite() || diameter <= 0.0 || diameter >= 1_000.0 {
        return Err(IoError::Invalid("aperture diameter is not usable".into()));
    }
    Ok((code, diameter))
}

fn take_digits(text: &str) -> Result<(&str, &str)> {
    let end = text.char_indices().find(|(_, c)| !c.is_ascii_digit()).map(|(i, _)| i).unwrap_or(text.len());
    if end == 0 {
        return Err(IoError::Invalid("bad aperture".into()));
    }
    let (digits, rest) = text.split_at(end);
    Ok((digits, rest))
}

fn parse_plot(record: &str, integer_digits: u32, decimals: u32) -> Result<Drawn> {
    let bytes = record.as_bytes();
    let mut index = 0;
    let mut drawn = Drawn { x: None, y: None, d: None, arc: false };
    while index < bytes.len() {
        let word = byte(bytes, index)?.to_ascii_uppercase();
        index += 1;
        match word {
            b'X' | b'Y' | b'I' | b'J' => {
                let start = index;
                if matches!(bytes.get(index).copied(), Some(b'+' | b'-')) {
                    index += 1;
                }
                let mut dotted = false;
                while let Some(ch) = bytes.get(index).copied() {
                    if ch.is_ascii_digit() {
                        index += 1;
                    } else if ch == b'.' && !dotted {
                        dotted = true;
                        index += 1;
                    } else {
                        break;
                    }
                }
                let token = record.get(start..index).ok_or_else(|| IoError::Invalid("bad coordinate".into()))?;
                if word == b'I' || word == b'J' {
                    drawn.arc = true;
                } else {
                    let value = parse_coord(token, integer_digits, decimals, dotted)?;
                    if word == b'X' { drawn.x = Some(value) } else { drawn.y = Some(value) }
                }
            }
            b'D' | b'G' => {
                let start = index;
                while bytes.get(index).copied().is_some_and(|ch| ch.is_ascii_digit()) {
                    index += 1;
                }
                let token = record.get(start..index).filter(|token| !token.is_empty()).ok_or_else(|| IoError::Invalid("bad D code".into()))?;
                if word == b'G' {
                    if token == "02" || token == "03" {
                        drawn.arc = true;
                    }
                } else {
                    let code: u32 = token.parse().map_err(|_| IoError::Invalid("bad D code".into()))?;
                    drawn.d = Some(code);
                }
            }
            _ => return Err(IoError::Invalid("unsupported Gerber word".into())),
        }
    }
    Ok(drawn)
}

fn parse_coord(token: &str, integer_digits: u32, decimals: u32, dotted: bool) -> Result<f64> {
    if token.is_empty() || token == "+" || token == "-" {
        return Err(IoError::Invalid("missing coordinate".into()));
    }
    if dotted {
        let value: f64 = token.parse().map_err(|_| IoError::Invalid("bad coordinate".into()))?;
        if !value.is_finite() {
            return Err(IoError::Invalid("bad coordinate".into()));
        }
        return Ok(value);
    }
    let negative = token.starts_with('-');
    let digits = token.trim_start_matches(['+', '-']);
    if digits.is_empty() || digits.len() > (integer_digits + decimals) as usize || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(IoError::Invalid("bad coordinate".into()));
    }
    let magnitude: i64 = digits.parse().map_err(|_| IoError::Invalid("bad coordinate".into()))?;
    let scale = 10f64.powi(decimals as i32);
    let signed = if negative { -magnitude } else { magnitude };
    Ok(signed as f64 / scale)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn close(got: f64, want: f64) {
        assert!((got - want).abs() < 1e-5, "{got} vs {want}");
    }

    #[test]
    fn a_board_round_trips_and_says_routing_is_deferred() {
        let mut traces = Vec::new();
        for i in 0..100 {
            let y = 1.0 + i as f64 * 0.4;
            traces.push(GerberTrace { x0: 1.0, y0: y, x1: 8.0 + (i % 3) as f64, y1: y, width: 0.15 + (i % 5) as f64 * 0.05 });
        }
        let layer = GerberLayer { width: 30.0, height: 50.0, traces };
        let text = write_gerber(&layer).unwrap();
        assert!(text.contains("%FSLAX46Y46*%"), "{text}");
        assert!(text.contains("%MOMM*%"), "{text}");
        assert!(text.contains("M02*"), "{text}");
        assert!(text.contains("PCB autorouting is not supported yet"), "{text}");
        let back = read_gerber(&text).unwrap();
        close(back.width, 30.0);
        close(back.height, 50.0);
        assert_eq!(back.traces.len(), 100);
        for (got, want) in back.traces.iter().zip(&layer.traces) {
            close(got.x0, want.x0);
            close(got.y0, want.y0);
            close(got.x1, want.x1);
            close(got.y1, want.y1);
            close(got.width, want.width);
        }
    }

    #[test]
    fn inches_modal_coordinates_and_refused_commands() {
        let text = "\
%FSLAX46Y46*%
%MOIN*%
G04 COPPER*
%ADD10C,0.010000*%
D10*
X0Y0D02*
X1000000D01*
Y2000000D01*
M02*
";
        let layer = read_gerber(text).unwrap();
        assert_eq!(layer.traces.len(), 2, "{layer:?}");
        close(layer.traces[0].x1, 25.4);
        close(layer.traces[0].width, 0.254);
        close(layer.traces[1].x0, 25.4);
        close(layer.traces[1].y1, 50.8);
        let arc = read_gerber("%FSLAX46Y46*%\n%MOMM*%\nG02*\nM02*\n").unwrap_err();
        assert!(arc.to_string().contains("not supported yet"), "{arc}");
        assert!(read_gerber("").is_err());
        assert!(read_gerber("hello").is_err());
        let huge = "X0Y0D01*".repeat(MAX_RECORDS + 1);
        assert!(read_gerber(&huge).unwrap_err().to_string().contains("too many"));
        let clear = read_gerber("%FSLAX46Y46*%\n%MOMM*%\n%LPC*%\nM02*\n").unwrap_err();
        assert!(clear.to_string().contains("not supported yet"), "{clear}");
    }
}
