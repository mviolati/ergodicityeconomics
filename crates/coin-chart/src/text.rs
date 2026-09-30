//! Text as vector paths: glyph outlines of IBM Plex Sans (SIL Open Font License 1.1, see
//! `assets/IBMPlexSans-LICENSE.txt`) filled with tiny-skia.

use ab_glyph::{Font, FontRef, OutlineCurve};
use std::sync::OnceLock;
use tiny_skia::{Path, PathBuilder};

static FONT_BYTES: &[u8] = include_bytes!("../assets/IBMPlexSans-Regular.ttf");

fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(FONT_BYTES).expect("embedded font is valid"))
}

/// Horizontal anchor of a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Width of `text` at `size` px.
pub fn width(text: &str, size: f32) -> f32 {
    let f = font();
    let scale = size / f.units_per_em().expect("font has units per em");
    let mut w = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let id = f.glyph_id(c);
        if let Some(p) = prev {
            w += f.kern_unscaled(p, id);
        }
        w += f.h_advance_unscaled(id);
        prev = Some(id);
    }
    w * scale
}

/// Outline of `text` at `size` px, with its baseline at `y` and anchored at `x`.
pub fn path(text: &str, x: f32, y: f32, size: f32, align: Align) -> Option<Path> {
    let f = font();
    let scale = size / f.units_per_em().expect("font has units per em");
    let mut pen = match align {
        Align::Left => x,
        Align::Center => x - width(text, size) / 2.0,
        Align::Right => x - width(text, size),
    };
    let mut pb = PathBuilder::new();
    let mut prev = None;
    for c in text.chars() {
        let id = f.glyph_id(c);
        if let Some(p) = prev {
            pen += f.kern_unscaled(p, id) * scale;
        }
        if let Some(outline) = f.outline(id) {
            let map = |p: ab_glyph::Point| (pen + p.x * scale, y - p.y * scale);
            let mut last: Option<(f32, f32)> = None;
            for curve in &outline.curves {
                let (start, end) = match curve {
                    OutlineCurve::Line(a, b) | OutlineCurve::Quad(a, _, b) | OutlineCurve::Cubic(a, _, _, b) => {
                        (map(*a), map(*b))
                    }
                };
                if last.map_or(true, |l| (l.0 - start.0).abs() > 1e-4 || (l.1 - start.1).abs() > 1e-4) {
                    if last.is_some() {
                        pb.close();
                    }
                    pb.move_to(start.0, start.1);
                }
                match curve {
                    OutlineCurve::Line(_, _) => pb.line_to(end.0, end.1),
                    OutlineCurve::Quad(_, c1, _) => {
                        let c = map(*c1);
                        pb.quad_to(c.0, c.1, end.0, end.1);
                    }
                    OutlineCurve::Cubic(_, c1, c2, _) => {
                        let (a, b) = (map(*c1), map(*c2));
                        pb.cubic_to(a.0, a.1, b.0, b.1, end.0, end.1);
                    }
                }
                last = Some(end);
            }
            if last.is_some() {
                pb.close();
            }
        }
        pen += f.h_advance_unscaled(id) * scale;
        prev = Some(id);
    }
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_character_we_draw_exists_in_the_font() {
        let f = font();
        // Every text the chart draws (see render): digits, separators, units, titles.
        let texts = [
            "0123456789.,−",
            "\u{2009}€",
            "10",
            "round",
            "1 mld €",
            "1 €",
            "Giocatori sopra 1 mld €, round per round",
            "Sopra 1 mld €, per round",
            "Giocatori sopra 1 mld €: nessuno, in nessun round",
            "Sopra 1 mld €: nessuno",
            "1,00 € 999 mln € 8,4 × 10",
        ];
        for c in texts.iter().flat_map(|t| t.chars()) {
            assert_ne!(f.glyph_id(c).0, 0, "missing glyph for {c:?}");
        }
    }

    #[test]
    fn right_aligned_text_ends_at_the_anchor() {
        let p = path("1 miliardo €", 100.0, 50.0, 12.0, Align::Right).unwrap();
        let b = p.bounds();
        assert!(b.right() <= 100.5 && b.right() > 95.0, "right edge {}", b.right());
    }
}
