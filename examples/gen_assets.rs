//! Generates the MSIX / Microsoft Store logo PNGs from the procedural icon.
//!
//!     cargo run --example gen_assets -- <out_dir>
//!
//! Produces the `Assets\*.png` files referenced by `packaging\AppxManifest.xml`
//! (with scale/targetsize qualifiers resolved by makepri) plus a 300x300
//! store-listing logo.

#[path = "../src/icon_art.rs"]
#[allow(dead_code)]
mod icon_art;

use std::path::Path;

use icon_art::Rgba;

/// Renders the icon at `inner` px and centers it on a transparent `w` x `h` canvas.
fn canvas(w: u32, h: u32, inner: u32) -> Vec<Rgba> {
    let icon = icon_art::render(inner);
    let mut out = vec![Rgba::default(); (w * h) as usize];
    let (ox, oy) = ((w - inner) / 2, (h - inner) / 2);
    for y in 0..inner {
        for x in 0..inner {
            out[((y + oy) * w + x + ox) as usize] = icon[(y * inner + x) as usize];
        }
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// Minimal PNG encoder (RGBA8, zlib-compressed, no row filters).
fn png(w: u32, h: u32, px: &[Rgba]) -> Vec<u8> {
    let q = |v: f64| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let mut raw = Vec::with_capacity(((w * 4 + 1) * h) as usize);
    for y in 0..h {
        raw.push(0); // filter: none
        for x in 0..w {
            let p = px[(y * w + x) as usize];
            raw.extend_from_slice(&[q(p.r), q(p.g), q(p.b), q(p.a)]);
        }
    }
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 9);

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

fn write(dir: &Path, name: &str, w: u32, h: u32, inner: u32) {
    let px = canvas(w, h, inner);
    std::fs::write(dir.join(name), png(w, h, &px)).unwrap();
    println!("{name} ({w}x{h})");
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "target/msix/assets".into());
    let dir = Path::new(&out);
    std::fs::create_dir_all(dir).unwrap();

    // App list / taskbar / Start icon. The icon has its own plate, so the
    // "unplated" variants are identical.
    for s in [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256] {
        write(dir, &format!("Square44x44Logo.targetsize-{s}.png"), s, s, s);
        write(dir, &format!("Square44x44Logo.targetsize-{s}_altform-unplated.png"), s, s, s);
        write(dir, &format!("Square44x44Logo.targetsize-{s}_altform-lightunplated.png"), s, s, s);
    }
    for (scale, s) in [(100, 44), (125, 55), (150, 66), (200, 88), (400, 176)] {
        write(dir, &format!("Square44x44Logo.scale-{scale}.png"), s, s, s);
    }
    // Medium tile: icon at ~2/3 of the tile.
    for (scale, s) in [(100, 150), (125, 188), (150, 225), (200, 300), (400, 600)] {
        write(dir, &format!("Square150x150Logo.scale-{scale}.png"), s, s, s * 2 / 3);
    }
    // Wide tile.
    for (scale, w, h) in [(100, 310, 150), (125, 388, 188), (150, 465, 225), (200, 620, 300), (400, 1240, 600)] {
        write(dir, &format!("Wide310x150Logo.scale-{scale}.png"), w, h, h * 2 / 3);
    }
    // Package logo shown by the installer and Store.
    for (scale, s) in [(100, 50), (125, 63), (150, 75), (200, 100), (400, 200)] {
        write(dir, &format!("StoreLogo.scale-{scale}.png"), s, s, s);
    }
    // For the Partner Center listing (not part of the package).
    let listing = dir.parent().unwrap_or(dir).join("listing");
    std::fs::create_dir_all(&listing).unwrap();
    write(&listing, "logo-300x300.png", 300, 300, 300);
}
