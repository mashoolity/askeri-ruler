//! Procedural app icon. Used by `build.rs` to generate `app.ico`, so the repo
//! has no binary art assets. Pure Rust, no Windows dependencies.
//!
//! Design: dark rounded square with a white molar, crossed by a red guide
//! line tilted -12°, a dashed red X at ±45° to it, and a red pivot dot in
//! the middle of the crown.

/// Straight (non-premultiplied) RGBA.
#[derive(Clone, Copy, Default)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba { r: r as f64 / 255.0, g: g as f64 / 255.0, b: b as f64 / 255.0, a: 1.0 }
    }

    /// Source-over compositing of `src` with extra coverage `k` onto `self`.
    fn over(self, src: Rgba, k: f64) -> Rgba {
        let sa = src.a * k.clamp(0.0, 1.0);
        let a = sa + self.a * (1.0 - sa);
        if a <= 0.0 {
            return Rgba::default();
        }
        let mix = |s: f64, d: f64| (s * sa + d * self.a * (1.0 - sa)) / a;
        Rgba { r: mix(src.r, self.r), g: mix(src.g, self.g), b: mix(src.b, self.b), a }
    }
}

const BG: Rgba = Rgba::rgb(0x1F, 0x24, 0x30);
const RED: Rgba = Rgba::rgb(0xFF, 0x2D, 0x2D);
const WHITE: Rgba = Rgba::rgb(0xFF, 0xFF, 0xFF);

fn cov(signed_dist_inside: f64) -> f64 {
    (signed_dist_inside + 0.5).clamp(0.0, 1.0)
}

/// Signed distance to a rounded square (positive inside).
fn rounded_square(x: f64, y: f64, size: f64, radius: f64) -> f64 {
    let half = size / 2.0;
    let (qx, qy) = ((x - half).abs() - (half - radius), (y - half).abs() - (half - radius));
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    radius - (outside + inside)
}

fn in_circle(u: f64, v: f64, cx: f64, cy: f64, r: f64) -> bool {
    (u - cx).powi(2) + (v - cy).powi(2) <= r * r
}

/// Molar silhouette in normalized coordinates (0..1, y down): a crown with
/// two cusps and two tapered roots separated by a rounded arch.
fn inside_tooth(u: f64, v: f64) -> bool {
    const ROOT_TOP: f64 = 0.58;
    const ROOT_TIP: f64 = 0.85;
    // Crown: two cusps on top of a straight-sided body.
    if in_circle(u, v, 0.375, 0.31, 0.155) || in_circle(u, v, 0.625, 0.31, 0.155) {
        return true;
    }
    if (0.22..=0.78).contains(&u) && (0.31..=ROOT_TOP).contains(&v) {
        return true;
    }
    // Roots: the outer edge curves in toward a rounded tip; the gap between
    // the roots opens like an arch (half-width grows with sqrt).
    if (ROOT_TOP..=ROOT_TIP).contains(&v) {
        let t = (v - ROOT_TOP) / (ROOT_TIP - ROOT_TOP);
        let outer = 0.28 - 0.09 * t.powf(1.5); // distance of outer edge from center
        let inner = 0.09 * t.sqrt(); // half-width of the gap
        let d = (u - 0.5).abs();
        if d >= inner && d <= outer {
            return true;
        }
    }
    in_circle(u, v, 0.36, ROOT_TIP, 0.05) || in_circle(u, v, 0.64, ROOT_TIP, 0.05)
}

/// Tooth coverage of a pixel by supersampling.
fn tooth_coverage(x: u32, y: u32, s: f64, n: u32) -> f64 {
    let mut hits = 0;
    for j in 0..n {
        for i in 0..n {
            let u = (x as f64 + (i as f64 + 0.5) / n as f64) / s;
            let v = (y as f64 + (j as f64 + 0.5) / n as f64) / s;
            if inside_tooth(u, v) {
                hits += 1;
            }
        }
    }
    hits as f64 / (n * n) as f64
}

fn lerp(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    Rgba { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: 1.0 }
}

/// Renders the icon at `size` x `size`, row-major top-down.
pub fn render(size: u32) -> Vec<Rgba> {
    let s = size as f64;
    // Pivot sits in the middle of the crown.
    let (cx, cy) = (0.5 * s, 0.45 * s);
    let line_w = (s / 16.0).max(1.4);
    let cross_w = (s / 30.0).max(1.0);
    let dot_r = (s * 0.075).max(1.6);
    let ring = (s / 48.0).max(0.8);
    let dash = (s / 8.0).max(2.0);
    let angle = (-12f64).to_radians();
    let inset = if size <= 24 { 0.0 } else { s / 32.0 };
    let radius = s * 0.22;
    let samples = if size <= 48 { 6 } else { 4 };
    let tooth_top = Rgba::rgb(0xFF, 0xFF, 0xFF);
    let tooth_bottom = Rgba::rgb(0xD9, 0xE0, 0xE8);

    let mut out = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
            let shape = cov(rounded_square(fx - inset, fy - inset, s - 2.0 * inset, radius));
            let mut p = Rgba::default().over(BG, shape);

            let tooth = tooth_coverage(x, y, s, samples);
            if tooth > 0.0 {
                p = p.over(lerp(tooth_top, tooth_bottom, (fy / s - 0.3) / 0.6), tooth * shape);
            }

            let (dx, dy) = (fx - cx, fy - cy);
            let dist_to = |a: f64| (dx * a.sin() - dy * a.cos()).abs();
            let along = |a: f64| dx * a.cos() + dy * a.sin();

            // Diagonal cross: dashed at large sizes, thin solid at 24 px,
            // omitted at the tiniest sizes where it would be clutter.
            let cross_sizes: &[f64] = if size >= 24 { &[45.0, -45.0] } else { &[] };
            for &d in cross_sizes {
                let a = angle + d.to_radians();
                let t = along(a).abs() - (dot_r + ring + s / 24.0);
                let k = if size >= 32 {
                    if t >= 0.0 && t.rem_euclid(dash * 2.0) < dash { 0.95 } else { 0.0 }
                } else if t >= 0.0 {
                    0.75
                } else {
                    0.0
                };
                if k > 0.0 {
                    p = p.over(RED, cov(cross_w / 2.0 - dist_to(a)) * k * shape);
                }
            }
            p = p.over(RED, cov(line_w / 2.0 - dist_to(angle)) * shape);

            let r = (dx * dx + dy * dy).sqrt();
            p = p.over(BG, cov(dot_r + ring - r));
            p = p.over(RED, cov(dot_r - r));
            out.push(p);
        }
    }
    out
}

/// Builds a multi-size .ico file (32-bit BMP entries with alpha).
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let mut images: Vec<Vec<u8>> = Vec::new();
    for &size in sizes {
        let px = render(size);
        let mut img = Vec::new();
        // BITMAPINFOHEADER; height is doubled to include the AND mask.
        img.extend_from_slice(&40u32.to_le_bytes());
        img.extend_from_slice(&(size as i32).to_le_bytes());
        img.extend_from_slice(&((size * 2) as i32).to_le_bytes());
        img.extend_from_slice(&1u16.to_le_bytes());
        img.extend_from_slice(&32u16.to_le_bytes());
        img.extend_from_slice(&[0u8; 24]);
        // XOR bitmap, bottom-up BGRA, straight alpha.
        for y in (0..size).rev() {
            for x in 0..size {
                let p = px[(y * size + x) as usize];
                let q = |v: f64| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                img.extend_from_slice(&[q(p.b), q(p.g), q(p.r), q(p.a)]);
            }
        }
        // AND mask: all zero, rows padded to 32 bits.
        let row = size.div_ceil(32) * 4;
        img.extend(std::iter::repeat_n(0u8, (row * size) as usize));
        images.push(img);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (i, &size) in sizes.iter().enumerate() {
        let dim = if size >= 256 { 0 } else { size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(images[i].len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += images[i].len() as u32;
    }
    for img in images {
        out.extend(img);
    }
    out
}
