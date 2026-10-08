//! Where the overlay appears (docs/DESIGN_SYSTEM.md §3): horizontally centered,
//! top edge at ~20% of the active monitor's work area, always fully inside it.
//! Pure geometry in physical pixels so it is testable without a window system.

/// Fraction of the work-area height above the overlay's top edge.
pub(crate) const TOP_FRACTION: f64 = 0.20;

/// Maximum share of the work-area height the overlay may take (DESIGN_SYSTEM §3: 65–72 %).
/// With the top edge at [`TOP_FRACTION`] the window always fits below it, so growing never
/// moves the search bar.
pub(crate) const MAX_HEIGHT_FRACTION: f64 = 0.72;

/// Rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhysicalRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Top-left physical position for a window of `size` (physical px) in `work_area`.
///
/// The window is clamped inside the work area; if it is larger than the work area
/// it is pinned to the work area's top-left corner.
pub(crate) fn overlay_position(work_area: PhysicalRect, size: (u32, u32)) -> (i32, i32) {
    let (width, height) = (i64::from(size.0), i64::from(size.1));
    let (area_x, area_y) = (i64::from(work_area.x), i64::from(work_area.y));
    let (area_w, area_h) = (i64::from(work_area.width), i64::from(work_area.height));

    let x = area_x + (area_w - width) / 2;
    // Truncation is fine: sub-pixel placement is meaningless.
    #[allow(clippy::cast_possible_truncation)]
    let top_offset = (area_h as f64 * TOP_FRACTION) as i64;
    let y = area_y + top_offset;

    let max_x = (area_x + area_w - width).max(area_x);
    let max_y = (area_y + area_h - height).max(area_y);
    let clamp =
        |v: i64, lo: i64, hi: i64| -> i32 { i32::try_from(v.clamp(lo, hi)).unwrap_or(i32::MAX) };
    (clamp(x, area_x, max_x), clamp(y, area_y, max_y))
}

/// Converts a logical size to physical pixels for a monitor's scale factor.
pub(crate) fn to_physical(logical: (f64, f64), scale: f64) -> (u32, u32) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let px = |v: f64| (v * scale).round().max(0.0) as u32;
    (px(logical.0), px(logical.1))
}

/// Height (logical px) to apply for a `requested` content height on a monitor whose work
/// area is `area_height` physical px at `scale`: at least `min`, at most
/// [`MAX_HEIGHT_FRACTION`] of the work area. Non-finite requests give `min`.
pub(crate) fn clamp_height(requested: f64, min: f64, area_height: u32, scale: f64) -> f64 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let max = (f64::from(area_height) / scale * MAX_HEIGHT_FRACTION).floor();
    if !requested.is_finite() {
        return min;
    }
    requested.min(max).max(min).round()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FHD: PhysicalRect = PhysicalRect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1040, // 1080 minus a 40px taskbar
    };

    #[test]
    fn centered_at_twenty_percent() {
        assert_eq!(overlay_position(FHD, (800, 64)), (560, 208));
    }

    #[test]
    fn respects_secondary_monitor_offsets() {
        // Monitor to the left of the primary, with negative coordinates.
        let left = PhysicalRect {
            x: -2560,
            y: -200,
            width: 2560,
            height: 1400,
        };
        assert_eq!(
            overlay_position(left, (1000, 100)),
            (-2560 + 780, -200 + 280)
        );
    }

    #[test]
    fn clamps_inside_small_work_area() {
        let tiny = PhysicalRect {
            x: 100,
            y: 50,
            width: 600,
            height: 300,
        };
        // Larger than the work area: pinned to its top-left corner.
        assert_eq!(overlay_position(tiny, (800, 400)), (100, 50));
        // Fits horizontally, would overflow vertically: pushed up to stay inside.
        assert_eq!(overlay_position(tiny, (400, 280)), (200, 70));
    }

    #[test]
    fn scales_logical_size() {
        assert_eq!(to_physical((800.0, 64.0), 1.0), (800, 64));
        assert_eq!(to_physical((800.0, 64.0), 1.25), (1000, 80));
        assert_eq!(to_physical((800.0, 64.0), 1.5), (1200, 96));
        assert_eq!(to_physical((800.0, 64.0), 2.0), (1600, 128));
        assert_eq!(to_physical((800.0, 64.0), f64::NAN), (800, 64));
    }

    #[test]
    fn height_is_clamped_to_the_work_area_share() {
        // 1040 px work area at 100 %: max 748 logical.
        assert_eq!(clamp_height(497.0, 64.0, 1040, 1.0), 497.0);
        assert_eq!(clamp_height(2000.0, 64.0, 1040, 1.0), 748.0);
        // 728 px work area at 125 %: 582 logical -> max 419.
        assert_eq!(clamp_height(497.0, 64.0, 728, 1.25), 419.0);
        assert_eq!(clamp_height(10.0, 64.0, 1040, 1.0), 64.0);
        assert_eq!(clamp_height(f64::NAN, 64.0, 1040, 1.0), 64.0);
        // A clamped window still sits at the 20 % line, fully inside.
        let h = clamp_height(5000.0, 64.0, 1040, 1.0);
        let (_, y) = overlay_position(FHD, to_physical((800.0, h), 1.0));
        assert_eq!(y, 208);
    }
}
