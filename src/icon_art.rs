//! Procedural app icon. Used by `build.rs` to generate `app.ico`, so the repo
//! has no binary art assets. Pure Rust, no Windows dependencies.
//!
//! Design: dark rounded square, a red guide line tilted -20° through the
//! center, a faint dashed X at ±45° to it, and a red pivot dot with a white
//! outline.

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

/// Renders the icon at `size` x `size`, row-major top-down.
pub fn render(size: u32) -> Vec<Rgba> {
    let s = size as f64;
    let c = s / 2.0;
    let line_w = (s / 14.0).max(1.6);
    let cross_w = (s / 28.0).max(1.0);
    let dot_r = (s * 0.13).max(2.2);
    let ring = (s / 32.0).max(1.0);
    let dash = (s / 9.0).max(2.0);
    let angle = (-20f64).to_radians();
    let inset = if size <= 24 { 0.0 } else { s / 32.0 };
    let radius = s * 0.22;

    let mut out = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
            let shape = cov(rounded_square(fx - inset, fy - inset, s - 2.0 * inset, radius));
            let mut p = Rgba::default().over(BG, shape);

            let (dx, dy) = (fx - c, fy - c);
            let dist_to = |a: f64| (dx * a.sin() - dy * a.cos()).abs();
            let along = |a: f64| dx * a.cos() + dy * a.sin();

            if size >= 32 {
                for d in [45f64, -45.0] {
                    let a = angle + d.to_radians();
                    let t = along(a).abs() - (dot_r + ring + s / 20.0);
                    let on = t >= 0.0 && t.rem_euclid(dash * 2.0) < dash;
                    if on {
                        p = p.over(RED, cov(cross_w / 2.0 - dist_to(a)) * 0.55 * shape);
                    }
                }
            }
            p = p.over(RED, cov(line_w / 2.0 - dist_to(angle)) * shape);

            let r = (dx * dx + dy * dy).sqrt();
            p = p.over(WHITE, cov(dot_r + ring - r));
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
