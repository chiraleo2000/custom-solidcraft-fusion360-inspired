//! Colour map for a scalar result. Low values are blue, high values are red.
//! The ramp is SolidCraft's own. It does not change a shell or a mesh.

use crate::{Rgb, png};

/// `t` = 0 is blue, `t` = 1 is red, with a green peak in the middle.
/// A non-finite `t` is drawn as the low colour.
pub fn result_colour(t: f64) -> Rgb {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let red = t * 255.0;
    let blue = (1.0 - t) * 255.0;
    let green = (1.0 - (t * 2.0 - 1.0).abs()) * 180.0;
    Rgb(byte(red), byte(green), byte(blue))
}

/// Map `value` between `min` and `max` onto [`result_colour`].
/// A zero span, or a non-finite value, is the middle colour.
pub fn result_colour_value(value: f64, min: f64, max: f64) -> Rgb {
    let span = max - min;
    if !value.is_finite() || !span.is_finite() || span.abs() <= 1e-15 {
        return result_colour(0.5);
    }
    result_colour((value - min) / span)
}

fn byte(value: f64) -> u8 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= 255.0 {
        255
    } else {
        value.round() as u8
    }
}

/// A horizontal strip of the samples, one 8×16 cell each, as a PNG.
/// Empty, non-finite, or over-long sample lists return `None`.
pub fn result_ramp_png(samples: &[f64]) -> Option<Vec<u8>> {
    if samples.is_empty() || samples.len() > 64 {
        return None;
    }
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for sample in samples {
        if !sample.is_finite() {
            return None;
        }
        min = min.min(*sample);
        max = max.max(*sample);
    }
    let width = samples.len().saturating_mul(8);
    let height = 16usize;
    if width == 0 || width > 4096 {
        return None;
    }
    let mut rgba = vec![0u8; width.saturating_mul(height).saturating_mul(4)];
    for (index, sample) in samples.iter().enumerate() {
        let colour = result_colour_value(*sample, min, max);
        for y in 0..height {
            for x in 0..8 {
                let pixel = (y * width + index * 8 + x) * 4;
                let Some(slot) = rgba.get_mut(pixel..pixel + 4) else { continue };
                if let Some(red) = slot.first_mut() {
                    *red = colour.0;
                }
                if let Some(green) = slot.get_mut(1) {
                    *green = colour.1;
                }
                if let Some(blue) = slot.get_mut(2) {
                    *blue = colour.2;
                }
                if let Some(alpha) = slot.get_mut(3) {
                    *alpha = 255;
                }
            }
        }
    }
    png(width, height, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ramp_runs_from_blue_to_red() {
        let low = result_colour(0.0);
        let high = result_colour(1.0);
        assert_eq!(low, Rgb(0, 0, 255));
        assert_eq!(high, Rgb(255, 0, 0));
        assert_eq!(result_colour(f64::NAN), low);
        let mid = result_colour_value(1.0, 1.0, 1.0);
        assert_eq!(mid, result_colour(0.5));
        let bytes = result_ramp_png(&[0.0, 30.0, 60.0]).expect("png");
        assert!(bytes.starts_with(b"\x89PNG"));
        assert!(result_ramp_png(&[]).is_none());
        assert!(result_ramp_png(&[f64::NAN]).is_none());
    }
}
