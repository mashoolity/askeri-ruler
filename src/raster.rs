//! Anti-aliased line and dot rasterization.
//!
//! Shapes produce `(pixel_index, coverage)` pairs where coverage is 0..=255.
//! Coverage comes from the distance between each pixel center and the shape
//! edge (a 1 px wide linear falloff), which gives smooth edges at any angle
//! while keeping axis-aligned lines with an even width perfectly crisp.
//!
//! Geometry is in pixel-edge coordinates: pixel (x, y) covers [x, x+1) x [y, y+1).

pub type Coverage = (u32, u8);

/// Center point of the shapes for a pivot pixel. For even widths the center
/// sits on a pixel corner so a horizontal/vertical line covers whole pixels.
pub fn shape_center(px: f64, py: f64, width: f64) -> (f64, f64) {
    let off = if (width.round() as i64) % 2 == 0 { 1.0 } else { 0.5 };
    (px.floor() + off, py.floor() + off)
}

fn alpha(c: f64) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Dash pattern for a line, measured along the line from the center point.
/// The pattern is mirrored on both sides of the center, and nothing is drawn
/// within `start` pixels of it (so dashes leave room around the pivot dot).
#[derive(Clone, Copy, Debug)]
pub struct Dash {
    pub on: f64,
    pub off: f64,
    pub start: f64,
}

/// Coverage (0..=1) of the dash pattern at distance `t` along the line.
fn dash_coverage(t: f64, d: &Dash) -> f64 {
    let t = t.abs() - d.start;
    if t < 0.0 {
        return (0.5 + t).clamp(0.0, 1.0);
    }
    let period = d.on + d.off;
    let m = t.rem_euclid(period);
    // Signed distance to the nearest dash edge: positive inside a dash.
    let signed = if m < d.on {
        m.min(d.on - m)
    } else {
        -(m - d.on).min(period - m)
    };
    (0.5 + signed).clamp(0.0, 1.0)
}

/// Appends the pixels of an infinite line through (`cx`, `cy`) at `angle`
/// with the given `width`, clipped to a `w` x `h` surface. With `dash`, the
/// line is drawn as a mirrored dash pattern around the center point.
#[allow(clippy::too_many_arguments)]
pub fn line_pixels(
    w: i32,
    h: i32,
    cx: f64,
    cy: f64,
    angle: f64,
    width: f64,
    dash: Option<&Dash>,
    out: &mut Vec<Coverage>,
) {
    if w <= 0 || h <= 0 || width <= 0.0 {
        return;
    }
    let hw = width / 2.0;
    let (sin, cos) = angle.sin_cos();
    let mut emit = |x: i32, y: i32, perp: f64| {
        let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
        let mut c = hw + 0.5 - perp;
        if c <= 0.0 {
            return;
        }
        if let Some(d) = dash {
            let t = (fx - cx) * cos + (fy - cy) * sin;
            c = c.min(1.0) * dash_coverage(t, d);
        }
        let a = alpha(c);
        if a > 0 {
            out.push((y as u32 * w as u32 + x as u32, a));
        }
    };
    if cos.abs() >= sin.abs() {
        // Mostly horizontal: walk columns, cover a vertical span per column.
        let slope = sin / cos;
        let k = cos.abs(); // perpendicular distance = vertical distance * |cos|
        let reach = (hw + 0.5) / k;
        for x in 0..w {
            let yc = cy + (x as f64 + 0.5 - cx) * slope;
            let y0 = ((yc - reach).floor() as i32).max(0);
            let y1 = ((yc + reach).ceil() as i32).min(h - 1);
            for y in y0..=y1 {
                emit(x, y, ((y as f64 + 0.5) - yc).abs() * k);
            }
        }
    } else {
        // Mostly vertical: walk rows, cover a horizontal span per row.
        let inv = cos / sin;
        let k = sin.abs();
        let reach = (hw + 0.5) / k;
        for y in 0..h {
            let xc = cx + (y as f64 + 0.5 - cy) * inv;
            let x0 = ((xc - reach).floor() as i32).max(0);
            let x1 = ((xc + reach).ceil() as i32).min(w - 1);
            for x in x0..=x1 {
                emit(x, y, ((x as f64 + 0.5) - xc).abs() * k);
            }
        }
    }
}

/// Appends the pixels of a filled anti-aliased disc.
pub fn dot_pixels(w: i32, h: i32, cx: f64, cy: f64, diameter: f64, out: &mut Vec<Coverage>) {
    let r = diameter / 2.0;
    let x0 = ((cx - r - 1.0).floor() as i32).max(0);
    let x1 = ((cx + r + 1.0).ceil() as i32).min(w - 1);
    let y0 = ((cy - r - 1.0).floor() as i32).max(0);
    let y1 = ((cy + r + 1.0).ceil() as i32).min(h - 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let a = alpha(r + 0.5 - (dx * dx + dy * dy).sqrt());
            if a > 0 {
                out.push((y as u32 * w as u32 + x as u32, a));
            }
        }
    }
}

/// Lines closer than this to parallel or perpendicular get no bisector.
pub const BISECTOR_TOLERANCE_DEG: f64 = 0.5;

/// Bisector of the obtuse (bigger) angle between two infinite lines given by
/// a point and an angle each. Returns (intersection x, intersection y, angle).
///
/// `None` when the lines are (nearly) parallel — there is no angle — or
/// (nearly) perpendicular, where both angles are 90° and the bisector would
/// be ambiguous.
pub fn bisector(p1: (f64, f64), a1: f64, p2: (f64, f64), a2: f64) -> Option<(f64, f64, f64)> {
    let delta = normalize_angle(a2 - a1); // (-90°, 90°]: the acute angle, signed
    let tol = BISECTOR_TOLERANCE_DEG.to_radians();
    if delta.abs() < tol || (std::f64::consts::FRAC_PI_2 - delta.abs()).abs() < tol {
        return None;
    }
    let (u1, u2) = ((a1.cos(), a1.sin()), (a2.cos(), a2.sin()));
    let cross = |a: (f64, f64), b: (f64, f64)| a.0 * b.1 - a.1 * b.0;
    let t = cross((p2.0 - p1.0, p2.1 - p1.1), u2) / cross(u1, u2);
    // a1 + delta/2 bisects the acute angle; the obtuse bisector is perpendicular to it.
    let obtuse = a1 + delta / 2.0 + std::f64::consts::FRAC_PI_2;
    Some((p1.0 + t * u1.0, p1.1 + t * u1.1, normalize_angle(obtuse)))
}

/// Normalizes an angle to the range (-90°, 90°]. A line is symmetric, so
/// angles 180° apart describe the same line.
pub fn normalize_angle(a: f64) -> f64 {
    use std::f64::consts::{FRAC_PI_2, PI};
    let mut a = a % PI;
    if a > FRAC_PI_2 {
        a -= PI;
    } else if a <= -FRAC_PI_2 {
        a += PI;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    fn alpha_at(v: &[Coverage], idx: u32) -> u8 {
        v.iter().filter(|c| c.0 == idx).map(|c| c.1).max().unwrap_or(0)
    }

    #[test]
    fn horizontal_2px_line_is_crisp() {
        let (cx, cy) = shape_center(3.0, 2.0, 2.0);
        let mut v = Vec::new();
        line_pixels(10, 6, cx, cy, 0.0, 2.0, None, &mut v);
        v.sort();
        let want: Vec<Coverage> = (20..40).map(|i| (i, 255)).collect();
        assert_eq!(v, want, "only rows 2 and 3, fully opaque");
    }

    #[test]
    fn vertical_2px_line_is_crisp() {
        let (cx, cy) = shape_center(3.0, 2.0, 2.0);
        let mut v = Vec::new();
        line_pixels(10, 5, cx, cy, FRAC_PI_2, 2.0, None, &mut v);
        assert_eq!(v.len(), 10);
        assert!(v.iter().all(|&(i, a)| a == 255 && (i % 10 == 3 || i % 10 == 4)));
    }

    #[test]
    fn line_covers_pivot_at_any_angle() {
        let (cx, cy) = shape_center(57.0, 33.0, 2.0);
        for deg in [-80.0f64, -45.0, -10.0, 0.0, 7.0, 30.0, 45.0, 60.0, 89.0] {
            let mut v = Vec::new();
            line_pixels(200, 100, cx, cy, deg.to_radians(), 2.0, None, &mut v);
            assert!(alpha_at(&v, 33 * 200 + 57) >= 200, "angle {deg}");
        }
    }

    #[test]
    fn diagonal_is_antialiased() {
        let (cx, cy) = shape_center(25.0, 25.0, 2.0);
        let mut v = Vec::new();
        line_pixels(50, 50, cx, cy, 10f64.to_radians(), 2.0, None, &mut v);
        assert!(v.iter().any(|&(_, a)| a > 0 && a < 255), "has partial pixels");
    }

    #[test]
    fn dot_is_round() {
        let mut v = Vec::new();
        let (cx, cy) = shape_center(10.0, 10.0, 2.0); // (11, 11)
        dot_pixels(30, 30, cx, cy, 8.0, &mut v);
        // Pixels next to the center are fully covered.
        assert_eq!(alpha_at(&v, 10 * 30 + 10), 255);
        assert_eq!(alpha_at(&v, 11 * 30 + 11), 255);
        // Bounding-box corner of an 8 px disc is outside the circle.
        assert_eq!(alpha_at(&v, 7 * 30 + 7), 0);
        // Edge midpoint (4 px left of center) is partially covered.
        let edge = alpha_at(&v, 11 * 30 + 7);
        assert!(edge > 0 && edge < 255);
        // Area is roughly pi * r^2.
        let area: f64 = v.iter().map(|&(_, a)| a as f64 / 255.0).sum();
        assert!((area - PI * 16.0).abs() < 2.0, "area {area}");
    }

    #[test]
    fn dashed_line_has_gap_at_center_then_dashes() {
        let dash = Dash { on: 6.0, off: 6.0, start: 10.0 };
        let (cx, cy) = shape_center(50.0, 10.0, 2.0); // (51, 11): rows 10 and 11
        let mut v = Vec::new();
        line_pixels(101, 20, cx, cy, 0.0, 2.0, Some(&dash), &mut v);
        let row = |x: u32| alpha_at(&v, 10 * 101 + x);
        // Clear zone around the center.
        for x in 41..61 {
            assert_eq!(row(x), 0, "x {x} should be empty");
        }
        // First dash on each side: 10..16 px from center (x = 61..67, 35..41).
        for x in 62..66 {
            assert_eq!(row(x), 255, "x {x} right dash");
        }
        for x in 36..40 {
            assert_eq!(row(x), 255, "x {x} left dash");
        }
        // Then a gap: 16..22 px from center (x = 67..73).
        for x in 68..72 {
            assert_eq!(row(x), 0, "x {x} gap");
        }
    }

    #[test]
    fn dash_coverage_values() {
        let d = Dash { on: 4.0, off: 4.0, start: 0.0 };
        assert_eq!(dash_coverage(2.0, &d), 1.0);
        assert_eq!(dash_coverage(-2.0, &d), 1.0);
        assert_eq!(dash_coverage(6.0, &d), 0.0);
        assert_eq!(dash_coverage(4.0, &d), 0.5);
    }

    #[test]
    fn clipped_at_edges() {
        let mut v = Vec::new();
        dot_pixels(10, 10, 0.0, 0.0, 8.0, &mut v);
        assert!(v.iter().all(|&(i, _)| i < 100));
    }

    #[test]
    fn bisector_of_obtuse_angle() {
        let deg = |d: f64| d.to_radians();
        // Horizontal through (0,0) and 60° through (10,0): meet at (10,0). The
        // angles are 60° and 120°; the 120° one is bisected at 30° + 90° = 120° ≡ -60°.
        let (x, y, a) = bisector((0.0, 0.0), 0.0, (10.0, 0.0), deg(60.0)).unwrap();
        assert!((x - 10.0).abs() < 1e-9 && y.abs() < 1e-9);
        assert!((a - deg(-60.0)).abs() < 1e-9);
        // 0° and 120°: the 120° angle between them is bisected at 60°.
        let (_, _, a) = bisector((0.0, 0.0), 0.0, (0.0, 0.0), deg(120.0)).unwrap();
        assert!((a - deg(60.0)).abs() < 1e-9);
        // Order of the lines does not matter.
        let (_, _, a) = bisector((0.0, 0.0), deg(120.0), (0.0, 0.0), 0.0).unwrap();
        assert!((normalize_angle(a - deg(60.0))).abs() < 1e-9);
        // The bisector makes equal angles (> 45°) with both lines.
        for (a1, a2) in [(0.0, 30.0), (-20.0, 50.0), (10.0, 95.0)] {
            let (_, _, b) = bisector((0.0, 0.0), deg(a1), (0.0, 0.0), deg(a2)).unwrap();
            let d1 = normalize_angle(b - deg(a1)).abs();
            let d2 = normalize_angle(b - deg(a2)).abs();
            assert!((d1 - d2).abs() < 1e-9 && d1 > deg(45.0), "{a1} {a2}");
        }
        // Intersection away from both pivots.
        let (x, y, _) = bisector((0.0, 0.0), deg(30.0), (10.0, 0.0), deg(-30.0)).unwrap();
        assert!((x - 5.0).abs() < 1e-9 && (y - 5.0 * deg(30.0).tan()).abs() < 1e-9);
    }

    #[test]
    fn no_bisector_for_parallel_or_perpendicular() {
        let deg = |d: f64| d.to_radians();
        assert!(bisector((0.0, 0.0), deg(10.0), (5.0, 5.0), deg(10.0)).is_none());
        assert!(bisector((0.0, 0.0), deg(10.0), (5.0, 5.0), deg(190.2)).is_none());
        assert!(bisector((0.0, 0.0), 0.0, (5.0, 5.0), deg(90.0)).is_none());
        assert!(bisector((0.0, 0.0), deg(20.0), (5.0, 5.0), deg(-70.3)).is_none());
        assert!(bisector((0.0, 0.0), 0.0, (5.0, 5.0), deg(89.0)).is_some());
    }

    #[test]
    fn normalize() {
        assert!((normalize_angle(PI) - 0.0).abs() < 1e-9);
        assert!((normalize_angle(3.0 * FRAC_PI_4) - (-FRAC_PI_4)).abs() < 1e-9);
        assert!((normalize_angle(-FRAC_PI_2) - FRAC_PI_2).abs() < 1e-9);
    }
}
