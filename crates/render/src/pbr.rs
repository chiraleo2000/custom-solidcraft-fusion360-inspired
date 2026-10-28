//! Local metallic-roughness shading. The model is SolidCraft's own: a diffuse term that fades
//! as metalness rises, and a highlight that tightens as roughness falls. No environment image
//! is loaded. Named lights are procedural; an unknown name is not a light.

use solidcraft_geom::Vec3;

/// A procedural studio light. Colour channels are 0..1, intensity is a multiplier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StudioLight {
    pub color: [f64; 3],
    pub intensity: f64,
}

const STUDIOS: &[(&str, [f64; 3], f64)] = &[
    ("studio", [1.0, 1.0, 1.0], 1.0),
    ("neutral", [1.0, 1.0, 1.0], 1.0),
    ("white", [1.0, 1.0, 1.0], 1.0),
    ("warm", [1.0, 0.86, 0.72], 1.15),
    ("cool", [0.78, 0.88, 1.0], 1.05),
];

/// A procedural environment by name. `None` means the name is not one we ship.
pub fn studio(name: &str) -> Option<StudioLight> {
    let key = name.trim();
    if key.is_empty() || key.len() > 200 {
        return None;
    }
    STUDIOS.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(key)).map(|(_, color, intensity)| StudioLight { color: *color, intensity: *intensity })
}

/// Names of the procedural studio lights SolidCraft ships (no image files).
pub fn studio_names() -> &'static [&'static str] {
    &["studio", "neutral", "white", "warm", "cool"]
}

/// Shade one fragment. `metalness` is 0 dielectric .. 1 metal. `roughness` is 0 smooth .. 1 rough.
/// `tint` and `intensity` scale the studio key. Non-finite inputs become a dark fragment.
pub(crate) fn shade_pbr(normal: Vec3, view_dir: Vec3, base: [f64; 3], metalness: f64, roughness: f64, tint: [f64; 3], intensity: f64) -> [f64; 3] {
    if !normal.is_finite() || !view_dir.is_finite() {
        return [0.0, 0.0, 0.0];
    }
    let metalness = if metalness.is_finite() { metalness.clamp(0.0, 1.0) } else { 0.0 };
    let roughness = if roughness.is_finite() { roughness.clamp(0.0, 1.0) } else { 0.5 };
    let intensity = if intensity.is_finite() { intensity.clamp(0.0, 8.0) } else { 1.0 };
    let tint = tint.map(|c| if c.is_finite() { c.clamp(0.0, 4.0) } else { 1.0 });
    let n = if normal.dot(view_dir) < 0.0 { -normal } else { normal };
    let key = (view_dir * 0.8 + Vec3::new(-0.3, 0.2, 0.6)).normalized().unwrap_or(view_dir);
    let ndl = n.dot(key).max(0.0);
    let ndv = n.dot(view_dir).max(0.0);
    let half = (key + view_dir).normalized().unwrap_or(view_dir);
    let ndh = n.dot(half).max(0.0);
    let alpha = (roughness * roughness).clamp(0.045, 1.0);
    let power = (2.0 / (alpha * alpha) - 2.0).clamp(1.0, 256.0);
    let spec = ndh.powf(power);
    let fresnel = (1.0 - ndv).clamp(0.0, 1.0).powi(5);
    let mut out = [0.0; 3];
    for i in 0..3 {
        let base_i = if base[i].is_finite() { base[i].clamp(0.0, 1.0) } else { 0.0 };
        let f0 = 0.04 * (1.0 - metalness) + base_i * metalness;
        let spec_i = f0 + (1.0 - f0) * fresnel;
        let diffuse = base_i * (1.0 - metalness) * (0.18 + 0.82 * ndl);
        let highlight = spec_i * spec * (0.2 + 0.8 * ndl) * (1.2 - 0.85 * roughness);
        let lit = (diffuse + highlight) * tint[i] * intensity;
        out[i] = if lit.is_finite() { lit.clamp(0.0, 1.0) } else { 0.0 };
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn unknown_environments_are_absent_and_named_ones_are_procedural() {
        assert!(studio("").is_none());
        assert!(studio("night-hdri").is_none());
        assert!(studio("studio").is_some_and(|s| s.intensity == 1.0));
        let warm = studio("Warm").expect("warm");
        assert!(warm.color[0] > warm.color[2]);
        assert_eq!(studio_names().len(), 5);
        for name in studio_names() {
            assert!(studio(name).is_some(), "{name}");
        }
    }

    /// Feature: fusion360-feature-parity, Property 24: Metalness and roughness change shaded pixels
    #[test]
    fn property_24_metalness_and_roughness_change_the_shade() {
        let normal = Vec3::new(0.15, 0.35, 0.92).normalized().expect("normal");
        let view = Vec3::new(0.1, 0.2, 1.0).normalized().expect("view");
        let base = [0.72, 0.38, 0.22];
        let tint = [1.0, 1.0, 1.0];
        let reference = shade_pbr(normal, view, base, 0.0, 0.5, tint, 1.0);
        let mut distinct = 0u32;
        let mut seen = [[0u8; 3]; 64];
        for i in 0..100 {
            let metalness = i as f64 / 99.0;
            let roughness = ((i * 7) % 100) as f64 / 99.0;
            let col = shade_pbr(normal, view, base, metalness, roughness, tint, 1.0);
            assert!(col.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)), "{i} {col:?}");
            if (metalness - 0.0).abs() > 1e-9 || (roughness - 0.5).abs() > 0.05 {
                let delta = col.iter().zip(reference).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
                assert!(delta > 1e-3, "{i} metal {metalness} rough {roughness} stayed {col:?} vs {reference:?}");
            }
            let key = col.map(|c| (c * 16.0).round().clamp(0.0, 15.0) as u8);
            if !seen.iter().take(distinct as usize).any(|s| *s == key) {
                if let Some(slot) = seen.get_mut(distinct as usize) {
                    *slot = key;
                }
                distinct += 1;
            }
        }
        assert!(distinct > 8, "only {distinct} shades");
        let metal = shade_pbr(normal, view, base, 1.0, 0.12, tint, 1.0);
        let matte = shade_pbr(normal, view, base, 0.0, 0.95, tint, 1.0);
        let delta = metal.iter().zip(matte).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(delta > 0.05, "metal {metal:?} matte {matte:?}");
        let warm = shade_pbr(normal, view, base, 0.0, 0.5, [1.0, 0.7, 0.5], 1.2);
        assert!(warm.iter().zip(reference).any(|(a, b)| (a - b).abs() > 0.02));
    }
}
