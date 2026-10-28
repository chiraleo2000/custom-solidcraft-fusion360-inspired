//! A bounded shelf packer for print layouts. No third-party packer.

use crate::Vec2;

const MAX_ITEMS: usize = 256;

/// Place axis-aligned rectangles on a bed, left to right then a new row.
/// Returns the minimum corner of each rectangle, in the same order as `sizes`.
/// Rectangles do not overlap and stay inside the bed. `gap` is the space between them.
///
/// Parts are packed tallest-first (then widest) so short leftovers fill gaps in a row;
/// the returned positions still match the caller's input order.
pub fn pack_shelf(sizes: &[(f64, f64)], bed_w: f64, bed_d: f64, gap: f64) -> Result<Vec<Vec2>, &'static str> {
    if sizes.is_empty() || sizes.len() > MAX_ITEMS {
        return Err("the nest needs 1…256 parts");
    }
    if !(bed_w.is_finite() && bed_d.is_finite() && gap.is_finite()) || bed_w <= 0.0 || bed_d <= 0.0 || gap < 0.0 || gap > bed_w.max(bed_d) {
        return Err("the bed size or gap is not usable");
    }
    for (w, h) in sizes {
        if !(w.is_finite() && h.is_finite()) || *w <= 0.0 || *h <= 0.0 {
            return Err("a part has no printable size");
        }
        if *w > bed_w || *h > bed_d {
            return Err("a part does not fit the bed");
        }
    }

    // Pack in an order that prefers tall, wide parts first, then map back.
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|&a, &b| {
        let (wa, ha) = sizes[a];
        let (wb, hb) = sizes[b];
        hb.total_cmp(&ha).then_with(|| wb.total_cmp(&wa)).then_with(|| a.cmp(&b))
    });

    let mut placed = vec![Vec2::ZERO; sizes.len()];
    let mut x = 0.0;
    let mut y = 0.0;
    let mut row_h = 0.0;
    for &i in &order {
        let (w, h) = sizes[i];
        if x > 0.0 && x + w > bed_w + 1e-9 {
            x = 0.0;
            y += row_h + gap;
            row_h = 0.0;
        }
        if y + h > bed_d + 1e-9 {
            return Err("the parts do not fit the bed");
        }
        placed[i] = Vec2::new(x, y);
        x += w + gap;
        row_h = row_h.max(h);
    }
    Ok(placed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_pack_does_not_overlap_and_stays_on_the_bed() {
        let sizes = [(10.0, 10.0), (10.0, 10.0), (10.0, 8.0)];
        let placed = pack_shelf(&sizes, 24.0, 40.0, 2.0).unwrap();
        assert_eq!(placed.len(), 3);
        for (i, a) in placed.iter().enumerate() {
            let (wa, ha) = sizes[i];
            assert!(a.x >= 0.0 && a.y >= 0.0);
            assert!(a.x + wa <= 24.0 + 1e-9);
            assert!(a.y + ha <= 40.0 + 1e-9);
            for (j, b) in placed.iter().enumerate().skip(i + 1) {
                let (wb, hb) = sizes[j];
                let ox = a.x < b.x + wb - 1e-9 && b.x < a.x + wa - 1e-9;
                let oy = a.y < b.y + hb - 1e-9 && b.y < a.y + ha - 1e-9;
                assert!(!(ox && oy), "overlap {i}@{a:?} {j}@{b:?}");
            }
        }
        assert!(pack_shelf(&[(30.0, 10.0)], 20.0, 20.0, 0.0).is_err());
        assert!(pack_shelf(&[], 10.0, 10.0, 0.0).is_err());
    }

    #[test]
    fn tall_first_packing_keeps_caller_order_and_starts_tall_on_row_one() {
        // Input lists the tall part last; packing still returns slots in that order, with the
        // tall piece on the first shelf row and a short beside it when width allows.
        let sizes = [(10.0, 5.0), (10.0, 5.0), (10.0, 12.0)];
        let placed = pack_shelf(&sizes, 22.0, 20.0, 2.0).unwrap();
        assert_eq!(placed.len(), 3);
        assert!(placed[2].y < 1e-9, "tall part (last input) on row 1: {placed:?}");
        assert!(placed[0].y < 1e-9 || placed[1].y < 1e-9, "a short shares row 1: {placed:?}");
        for (i, p) in placed.iter().enumerate() {
            let (w, h) = sizes[i];
            assert!(p.x + w <= 22.0 + 1e-9 && p.y + h <= 20.0 + 1e-9, "{i} {p:?}");
        }
    }
}
