//! Tray icons drawn at runtime (anti-aliased signed-distance shapes), so every state has a
//! crisp, consistent glyph without shipping a sprite sheet.

use tauri::image::Image;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Recording,
    Paused,
    Idle,
    Error,
}

const SIZE: u32 = 32;

type Rgb = (u8, u8, u8);
const EMBER: Rgb = (0xE2, 0x5B, 0x3C);
const STONE: Rgb = (0x8C, 0x86, 0x7D);
const AMBER: Rgb = (0xE0, 0x9A, 0x2B);
const PAPER: Rgb = (0xFB, 0xF8, 0xF2);

pub fn tray(glyph: Glyph) -> Image<'static> {
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    let c = SIZE as f32 / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let disc = circle(p, (c, c), 14.5);
            let (bg, ring_only) = match glyph {
                Glyph::Recording => (EMBER, false),
                Glyph::Paused => (STONE, false),
                Glyph::Error => (AMBER, false),
                Glyph::Idle => (STONE, true),
            };
            let bg_d = if ring_only { disc.abs() - 1.4 } else { disc };
            let mut a_bg = cover(bg_d);
            let fg_d = match glyph {
                // Three waveform bars.
                Glyph::Recording | Glyph::Idle => bar(p, c - 6.0, 4.5)
                    .min(bar(p, c, 8.0))
                    .min(bar(p, c + 6.0, 5.5)),
                Glyph::Paused => bar(p, c - 3.5, 6.5).min(bar(p, c + 3.5, 6.5)),
                Glyph::Error => rrect(p, (c, c - 2.5), (1.6, 6.0), 1.6).min(circle(p, (c, c + 7.0), 1.9)),
            };
            let a_fg = cover(fg_d);
            let (r, g, b, a) = if ring_only {
                // Hollow: ring + bars in the stone colour, transparent inside.
                a_bg = a_bg.max(a_fg);
                (bg.0, bg.1, bg.2, a_bg)
            } else {
                let a_fg = a_fg * cover(disc);
                let mix = |f: u8, b: u8| (f as f32 * a_fg + b as f32 * (1.0 - a_fg)).round() as u8;
                (mix(PAPER.0, bg.0), mix(PAPER.1, bg.1), mix(PAPER.2, bg.2), a_bg)
            };
            let i = ((y * SIZE + x) * 4) as usize;
            px[i] = r;
            px[i + 1] = g;
            px[i + 2] = b;
            px[i + 3] = (a * 255.0).round() as u8;
        }
    }
    Image::new_owned(px, SIZE, SIZE)
}

fn cover(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

fn circle(p: (f32, f32), c: (f32, f32), r: f32) -> f32 {
    ((p.0 - c.0).powi(2) + (p.1 - c.1).powi(2)).sqrt() - r
}

fn rrect(p: (f32, f32), c: (f32, f32), half: (f32, f32), r: f32) -> f32 {
    let qx = (p.0 - c.0).abs() - half.0 + r;
    let qy = (p.1 - c.1).abs() - half.1 + r;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// Vertical rounded bar centred on the icon's horizontal midline.
fn bar(p: (f32, f32), x: f32, half_h: f32) -> f32 {
    rrect(p, (x, SIZE as f32 / 2.0), (1.6, half_h), 1.6)
}
