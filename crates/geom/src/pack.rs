//! A bounded shelf packer for print layouts. No third-party packer.

use crate::Vec2;

const MAX_ITEMS: usize = 256;

/// Place axis-aligned rectangles on a bed, left to right then a new row.
/// Returns the minimum corner of each rectangle, in the same order.
/// Rectangles do not overlap and stay inside the bed. `gap` is the space between them.
pub fn pack_shelf(sizes: &[(f64, f64)], bed_w: f64, bed_d: f64, gap: f64) -> Result<Vec<Vec2>, &'static str> {
    if sizes.is_empty() || sizes.len() > MAX_ITEMS {
        return Err("the nest needs 1…256 parts");
    }
    if !(bed_w.is_finite() && bed_d.is_finite() && gap.is_finite()) || bed_w <= 0.0 || bed_d <= 0.0 || gap < 0.0 || gap > bed_w.max(bed_d) {
        return Err("the bed size or gap is not usable");
    }
    let mut out = Vec::with_capacity(sizes.len());
    let mut x = 0.0;
    let mut y = 0.0;
    let mut row_h = 0.0;
    for (w, h) in sizes {
        if !(w.is_finite() && h.is_finite()) || *w <= 0.0 || *h <= 0.0 {
            return Err("a part has no printable size");
        }
        if *w > bed_w || *h > bed_d {
            return Err("a part does not fit the bed");
        }
        if x > 0.0 && x + w > bed_w + 1e-9 {
            x = 0.0;
            y += row_h + gap;
            row_h = 0.0;
        }
        if y + h > bed_d + 1e-9 {
            return Err("the parts do not fit the bed");
        }
        out.push(Vec2::new(x, y));
        x += w + gap;
        row_h = row_h.max(*h);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_pack_does_not_overlap_and_stays_on_the_bed() {
        let sizes = [(10.0, 10.0), (10.0, 10.0), (10.0, 8.0)];
        let placed = pack_shelf(&sizes, 24.0, 40.0, 2.0).unwrap();
        assert_eq!(placed.len(), 3);
        assert!(placed[1].x >= placed[0].x + 10.0);
        assert!(placed[2].y >= 10.0);
        for (p, (w, h)) in placed.iter().zip(sizes) {
            assert!(p.x >= 0.0 && p.y >= 0.0);
            assert!(p.x + w <= 24.0 + 1e-9);
            assert!(p.y + h <= 40.0 + 1e-9);
        }
        assert!(pack_shelf(&[(30.0, 10.0)], 20.0, 20.0, 0.0).is_err());
        assert!(pack_shelf(&[], 10.0, 10.0, 0.0).is_err());
    }
}
