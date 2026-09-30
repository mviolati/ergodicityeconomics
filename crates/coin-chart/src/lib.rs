//! Pure-Rust renderer of the coin-game chart.
//!
//! What the chart shows, and how each mark maps to data:
//! - Density: one cell per (round, heads). A pixel shows the LARGEST player count among the cells
//!   it covers (max-pooling), on a log colour scale from 1 player to the fullest cell. Pooling by
//!   maximum keeps single players visible when many rounds share one pixel column. The legend
//!   gives the scale in numbers ([`legend_ticks`]).
//! - Thin orange lines: every player who reached the rich threshold, one line per player, so the
//!   number of lines is the number in the headline figure.
//! - Thick coloured lines: the players singled out by `coin_core::stats::picks`.
//! - Black/white lines: median player (solid), mean of all players (dotted), expected value (dashed).

pub mod text;
pub mod theme;

use coin_core::{ensemble::Ensemble, Game};
use text::Align;
use theme::{Rgb, Theme};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Rect, Stroke,
    StrokeDash, Transform,
};

/// Everything the chart draws. Wealth values are log10 EUR, one per round 0..=R.
pub struct Scene<'a> {
    pub game: &'a Game,
    /// Density matrix, see `coin_core::sim::density_len`.
    pub counts: &'a [u32],
    pub ensemble: &'a Ensemble,
    /// Highlighted players: path and palette slot (0..3).
    pub highlighted: &'a [(Vec<f64>, usize)],
    /// Paths of the players who reached the rich threshold.
    pub rich_paths: &'a [Vec<f64>],
    /// Draw the rich players' whole paths (true) or only the part at or above the rich line.
    pub rich_full: bool,
}

/// Canvas size in CSS pixels and the device pixel ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub css_w: f32,
    pub css_h: f32,
    pub dpr: f32,
}

impl Frame {
    /// Width and height of the pixel buffer.
    pub fn device_size(&self) -> (u32, u32) {
        ((self.css_w * self.dpr).round().max(1.0) as u32, (self.css_h * self.dpr).round().max(1.0) as u32)
    }
}

/// Plot area and the data <-> CSS pixel mapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// log10 EUR at the bottom and top edge of the plot.
    pub ymin: f64,
    pub ymax: f64,
    pub rounds: u32,
}

impl Layout {
    /// CSS x of round `t` (a real number: round cells span t - 0.5 .. t + 0.5).
    pub fn x_of(&self, t: f64) -> f32 {
        self.x + (t / f64::from(self.rounds)) as f32 * self.w
    }
    /// CSS y of log10 wealth `l`.
    pub fn y_of(&self, l: f64) -> f32 {
        self.y + ((self.ymax - l) / (self.ymax - self.ymin)) as f32 * self.h
    }
    /// Nearest round at CSS x, clamped to 0..=R.
    pub fn round_at(&self, x: f32) -> u32 {
        let t = ((x - self.x) / self.w * self.rounds as f32).round();
        t.clamp(0.0, self.rounds as f32) as u32
    }
    /// log10 wealth at CSS y.
    pub fn wealth_at(&self, y: f32) -> f64 {
        self.ymax - f64::from((y - self.y) / self.h) * (self.ymax - self.ymin)
    }
    /// True if the CSS point is inside the plot.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

const MARGIN_LEFT: f32 = 58.0;
const MARGIN_RIGHT: f32 = 14.0;
const MARGIN_TOP: f32 = 12.0;
const MARGIN_BOTTOM: f32 = 30.0;

/// Computes the plot area and a y range that holds every drawn mark and both thresholds.
pub fn layout(scene: &Scene<'_>, frame: Frame) -> Layout {
    let game = scene.game;
    let lat = game.lattice();
    let r = game.rounds;
    let width = r as usize + 1;
    let half = lat.gap / 2.0;
    let (mut lo, mut hi) = (lat.broke.min(lat.l0), lat.rich.max(lat.l0));
    for t in 1..=r {
        let row = &scene.counts[(t as usize - 1) * width..][..t as usize + 1];
        if let Some(k) = row.iter().position(|&n| n > 0) {
            lo = lo.min(lat.at(t, k as u32) - half);
        }
        if let Some(k) = row.iter().rposition(|&n| n > 0) {
            hi = hi.max(lat.at(t, k as u32) + half);
        }
    }
    let e = scene.ensemble;
    let lines = e.mean.iter().chain(&e.median).chain(&e.expected);
    let paths = scene.highlighted.iter().flat_map(|(p, _)| p.iter()).chain(scene.rich_paths.iter().flatten());
    for &v in lines.chain(paths) {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let pad = (hi - lo) * 0.02;
    Layout {
        x: MARGIN_LEFT,
        y: MARGIN_TOP,
        w: (frame.css_w - MARGIN_LEFT - MARGIN_RIGHT).max(10.0),
        h: (frame.css_h - MARGIN_TOP - MARGIN_BOTTOM).max(10.0),
        ymin: lo - pad,
        ymax: hi + pad,
        rounds: r,
    }
}

/// Occupied cells of round `t`: (heads, players). Round 0 is one cell holding every player.
fn cells(scene: &Scene<'_>, t: u32) -> Vec<(u32, u32)> {
    if t == 0 {
        return vec![(0, scene.ensemble.players.min(u64::from(u32::MAX)) as u32)];
    }
    let width = scene.game.rounds as usize + 1;
    let row = &scene.counts[(t as usize - 1) * width..][..t as usize + 1];
    row.iter().enumerate().filter(|(_, &n)| n > 0).map(|(k, &n)| (k as u32, n)).collect()
}

/// Device columns that show round `t`: the columns whose centre lies in the round's span
/// [t - 0.5, t + 0.5); if there is none (several rounds per column), the column that holds t.
pub fn columns_of_round(lay: &Layout, dpr: f32, dev_w: usize, t: u32) -> std::ops::Range<usize> {
    let x = |t: f64| f64::from(lay.x_of(t) - lay.x) * f64::from(dpr);
    let (a, b) = (x(f64::from(t) - 0.5), x(f64::from(t) + 0.5));
    let c0 = (a - 0.5).ceil().max(0.0) as usize;
    let c1 = ((b - 0.5).ceil().max(0.0) as usize).min(dev_w);
    if c0 < c1 {
        c0..c1
    } else {
        let c = (x(f64::from(t)).floor().max(0.0) as usize).min(dev_w - 1);
        c..c + 1
    }
}

/// Device rows that show cell (t, k): the rows whose centre lies in the cell's band. The band is
/// at(t, k) -+ gap / 2 (the bands of one round tile the axis), cut at the rich and broke
/// thresholds so that a cell never paints on the other side of a threshold line. If no row
/// centre falls in the band, the row that holds at(t, k).
pub fn rows_of_cell(scene: &Scene<'_>, lay: &Layout, dpr: f32, dev_h: usize, t: u32, k: u32) -> std::ops::Range<usize> {
    let lat = scene.game.lattice();
    let l = lat.at(t, k);
    let (mut lo, mut hi) = (l - lat.gap / 2.0, l + lat.gap / 2.0);
    for thr in [lat.rich, lat.broke] {
        if l >= thr {
            lo = lo.max(thr);
        } else {
            hi = hi.min(thr);
        }
    }
    let y = |v: f64| f64::from(lay.y_of(v) - lay.y) * f64::from(dpr);
    let (top, bottom) = (y(hi), y(lo));
    let r0 = (top - 0.5).ceil().max(0.0) as usize;
    let r1 = ((bottom - 0.5).ceil().max(0.0) as usize).min(dev_h);
    if r0 < r1 {
        r0..r1
    } else {
        let r = (y(l).floor().max(0.0) as usize).min(dev_h - 1);
        r..r + 1
    }
}

/// Density in device pixels of the plot area (row-major, `dev_w * dev_h`): the number of players
/// in the (round, heads) cell that the pixel shows; 0 = no player. Each round owns whole columns
/// and each cell owns whole rows (see [`columns_of_round`], [`rows_of_cell`]), so a cell is
/// neither enlarged nor skipped. Where several rounds or cells share one pixel, the pixel shows
/// the fullest of them.
pub fn density_grid(scene: &Scene<'_>, lay: &Layout, dpr: f32) -> (Vec<u32>, usize, usize) {
    let dev_w = (lay.w * dpr).round().max(1.0) as usize;
    let dev_h = (lay.h * dpr).round().max(1.0) as usize;
    let mut grid = vec![0u32; dev_w * dev_h];
    for t in 0..=scene.game.rounds {
        let cols = columns_of_round(lay, dpr, dev_w, t);
        for (k, n) in cells(scene, t) {
            for y in rows_of_cell(scene, lay, dpr, dev_h, t, k) {
                for cell in &mut grid[y * dev_w..][cols.clone()] {
                    *cell = (*cell).max(n);
                }
            }
        }
    }
    (grid, dev_w, dev_h)
}

/// Position in [0, 1] of `n` players on the log colour scale that ends at `max_cell`.
pub fn scale_position(n: u32, max_cell: u32) -> f64 {
    if max_cell <= 1 || n <= 1 {
        return 0.0;
    }
    (f64::from(n).ln() / f64::from(max_cell).ln()).min(1.0)
}

/// Legend ticks of the density scale: (players, position in [0, 1]) for 1, 10, 100, ... and
/// the fullest cell.
pub fn legend_ticks(max_cell: u32) -> Vec<(u32, f64)> {
    let mut out = Vec::new();
    let mut v = 1u32;
    while v < max_cell {
        out.push((v, scale_position(v, max_cell)));
        v = match v.checked_mul(10) {
            Some(x) => x,
            None => break,
        };
    }
    out.push((max_cell.max(1), 1.0));
    out
}

fn paint(c: Rgb, alpha: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.0, c.1, c.2, alpha);
    p.anti_alias = true;
    p
}

fn stroke(width: f32, dash: Option<(f32, f32)>) -> Stroke {
    Stroke {
        width,
        line_cap: if dash.is_some() { LineCap::Butt } else { LineCap::Round },
        line_join: LineJoin::Round,
        dash: dash.and_then(|(on, off)| StrokeDash::new(vec![on, off], 0.0)),
        ..Stroke::default()
    }
}

fn polyline(lay: &Layout, values: &[f64]) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for (t, &v) in values.iter().enumerate() {
        let (x, y) = (lay.x_of(t as f64), lay.y_of(v));
        if t == 0 {
            pb.move_to(x, y)
        } else {
            pb.line_to(x, y)
        }
    }
    pb.finish()
}

/// How a label is drawn.
#[derive(Clone, Copy)]
struct Label {
    size: f32,
    align: Align,
    color: Rgb,
    /// Outline in this colour behind the text, to keep it readable over data.
    halo: Option<Rgb>,
}

fn text(pm: &mut Pixmap, tf: Transform, s: &str, x: f32, y: f32, style: Label) {
    let Label { size, align, color, halo } = style;
    if let Some(p) = text::path(s, x, y, size, align) {
        if let Some(h) = halo {
            pm.stroke_path(&p, &paint(h, 255), &stroke(3.0, None), tf, None);
        }
        pm.fill_path(&p, &paint(color, 255), FillRule::Winding, tf, None);
    }
}

/// "10^e €" as base, raised exponent and euro sign, right-aligned at `x`.
fn power_label(pm: &mut Pixmap, tf: Transform, e: i64, x: f32, y: f32, color: Rgb) {
    let exp = if e < 0 { format!("−{}", -e) } else { e.to_string() };
    let euro = "\u{2009}€";
    let we = text::width(euro, 12.0);
    let wx = text::width(&exp, 9.0);
    let label = |size| Label { size, align: Align::Right, color, halo: None };
    text(pm, tf, euro, x, y, label(12.0));
    text(pm, tf, &exp, x - we, y - 5.0, label(9.0));
    text(pm, tf, "10", x - we - wx, y, label(12.0));
}

/// Draws the chart. The result is opaque, so its premultiplied bytes are also straight RGBA.
pub fn render(scene: &Scene<'_>, frame: Frame, theme: &Theme) -> (Pixmap, Layout) {
    let (dw, dh) = frame.device_size();
    let mut pm = Pixmap::new(dw, dh).expect("non-zero size");
    let s = theme.surface;
    pm.fill(Color::from_rgba8(s.0, s.1, s.2, 255));
    let lay = layout(scene, frame);
    let tf = Transform::from_scale(frame.dpr, frame.dpr);
    let lat = scene.game.lattice();

    // Grid lines at powers of ten, at most 8 of them.
    let span = lay.ymax - lay.ymin;
    let step = [1i64, 2, 5, 10, 20, 50, 100, 200, 500].into_iter().find(|s| span / *s as f64 <= 8.0).unwrap_or(1000);
    let mut e = (lay.ymin / step as f64).ceil() as i64 * step;
    let mut decades = Vec::new();
    while (e as f64) <= lay.ymax {
        decades.push(e);
        e += step;
    }
    let rule = paint(theme.rule, 255);
    for &d in &decades {
        let y = lay.y_of(d as f64);
        let mut pb = PathBuilder::new();
        pb.move_to(lay.x, y);
        pb.line_to(lay.x + lay.w, y);
        if let Some(p) = pb.finish() {
            pm.stroke_path(&p, &rule, &stroke(1.0, None), tf, None);
        }
    }

    // Density, written straight into the pixels.
    let (grid, gw, gh) = density_grid(scene, &lay, frame.dpr);
    let max_cell = scene.ensemble.max_cell;
    let lut: Vec<Rgb> = (0..=255).map(|i| theme.ramp(f64::from(i) / 255.0)).collect();
    let (ox, oy) = ((lay.x * frame.dpr).round() as usize, (lay.y * frame.dpr).round() as usize);
    let pixels = pm.pixels_mut();
    for gy in 0..gh {
        let py = oy + gy;
        if py >= dh as usize {
            break;
        }
        for gx in 0..gw {
            let n = grid[gy * gw + gx];
            let px = ox + gx;
            if n == 0 || px >= dw as usize {
                continue;
            }
            let c = lut[(scale_position(n, max_cell) * 255.0).round() as usize];
            pixels[py * dw as usize + px] = PremultipliedColorU8::from_rgba(c.0, c.1, c.2, 255).expect("opaque colour");
        }
    }

    // Everything else is clipped to the plot.
    let mut clip = Mask::new(dw, dh).expect("non-zero size");
    if let Some(r) = Rect::from_xywh(lay.x, lay.y, lay.w, lay.h) {
        clip.fill_path(&PathBuilder::from_rect(r), FillRule::Winding, false, tf);
    }
    let rich_paint = paint(theme.players[1], if theme.dark { 150 } else { 130 });
    let mut rich_clip = Mask::new(dw, dh).expect("non-zero size");
    let top = if scene.rich_full { lay.y + lay.h } else { lay.y_of(lat.rich) };
    if let Some(r) = Rect::from_xywh(lay.x, lay.y, lay.w, top - lay.y) {
        rich_clip.fill_path(&PathBuilder::from_rect(r), FillRule::Winding, false, tf);
    }
    for p in scene.rich_paths {
        if let Some(path) = polyline(&lay, p) {
            pm.stroke_path(&path, &rich_paint, &stroke(1.0, None), tf, Some(&rich_clip));
        }
    }
    let muted = paint(theme.muted, 255);
    for l in [lat.rich, lat.broke] {
        let y = lay.y_of(l);
        let mut pb = PathBuilder::new();
        pb.move_to(lay.x, y);
        pb.line_to(lay.x + lay.w, y);
        if let Some(p) = pb.finish() {
            pm.stroke_path(&p, &muted, &stroke(1.0, Some((3.0, 3.0))), tf, Some(&clip));
        }
    }
    let ink = paint(theme.ink, 255);
    let en = scene.ensemble;
    let casing = paint(theme.surface, 255);
    for (values, width, dash) in
        [(&en.expected, 1.4, Some((7.0, 4.0))), (&en.mean, 1.6, Some((1.5, 3.0))), (&en.median, 1.6, None)]
    {
        if let Some(p) = polyline(&lay, values) {
            // A surface-coloured casing keeps the line readable over any density.
            pm.stroke_path(&p, &casing, &stroke(width + 2.0, dash), tf, Some(&clip));
            pm.stroke_path(&p, &ink, &stroke(width, dash), tf, Some(&clip));
        }
    }
    for (p, slot) in scene.highlighted {
        if let Some(path) = polyline(&lay, p) {
            pm.stroke_path(&path, &casing, &stroke(4.0, None), tf, Some(&clip));
            pm.stroke_path(&path, &paint(theme.players[*slot % 3], 255), &stroke(2.0, None), tf, Some(&clip));
        }
    }

    // Frame, labels.
    if let Some(r) = Rect::from_xywh(lay.x + 0.5, lay.y + 0.5, lay.w - 1.0, lay.h - 1.0) {
        pm.stroke_path(&PathBuilder::from_rect(r), &rule, &stroke(1.0, None), tf, None);
    }
    for (l, label) in [
        (lat.rich, format!("{} €", rich_label(scene.game.rich))),
        (lat.broke, format!("{} €", rich_label(scene.game.broke))),
    ] {
        let style = Label { size: 11.5, align: Align::Right, color: theme.muted, halo: Some(theme.surface) };
        text(&mut pm, tf, &label, lay.x + lay.w - 4.0, lay.y_of(l) - 4.0, style);
    }
    for &d in &decades {
        power_label(&mut pm, tf, d, lay.x - 8.0, lay.y_of(d as f64) + 4.0, theme.muted);
    }
    let base = lay.y + lay.h + 18.0;
    for i in 0..=4u32 {
        let t = (u64::from(scene.game.rounds) * u64::from(i) / 4) as u32;
        let align = match i {
            0 => Align::Left,
            4 => Align::Right,
            _ => Align::Center,
        };
        let style = Label { size: 12.0, align, color: theme.muted, halo: None };
        text(&mut pm, tf, &coin_core::fmt::int(u64::from(t)), lay.x_of(f64::from(t)), base, style);
    }
    let style = Label { size: 12.0, align: Align::Right, color: theme.muted, halo: None };
    text(&mut pm, tf, "round", lay.x - 8.0, base, style);
    (pm, lay)
}

/// "1 miliardo", "1 milione", "1" (EUR) for a threshold value.
pub fn rich_label(v: f64) -> String {
    if v == 1e9 {
        "1 miliardo".into()
    } else if v == 1e6 {
        "1 milione".into()
    } else if v == 1.0 {
        "1".into()
    } else {
        coin_core::fmt::eur(v.log10()).trim_end_matches(" €").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coin_core::{ensemble::ensemble, sim::simulate};

    fn scene_parts(rounds: u32, players: usize) -> (Game, Vec<u32>, Ensemble) {
        let game = Game::peters(rounds, 11);
        let (counts, _) = simulate(&game, players);
        let e = ensemble(&game, players as u64, &counts);
        (game, counts, e)
    }

    fn scene<'a>(game: &'a Game, counts: &'a [u32], e: &'a Ensemble) -> Scene<'a> {
        Scene { game, counts, ensemble: e, highlighted: &[], rich_paths: &[], rich_full: false }
    }

    const FRAMES: [Frame; 4] = [
        Frame { css_w: 272.0, css_h: 240.0, dpr: 1.0 }, // several rounds per column
        Frame { css_w: 700.0, css_h: 420.0, dpr: 2.0 },
        Frame { css_w: 1040.0, css_h: 560.0, dpr: 1.0 },
        Frame { css_w: 400.0, css_h: 320.0, dpr: 3.0 },
    ];

    /// Reference: the definition, pixel by pixel.
    #[test]
    fn density_grid_matches_its_definition() {
        let (game, counts, e) = scene_parts(60, 3000);
        let sc = scene(&game, &counts, &e);
        for frame in FRAMES {
            let lay = layout(&sc, frame);
            let (grid, w, h) = density_grid(&sc, &lay, frame.dpr);
            let mut want = vec![0u32; w * h];
            for t in 0..=game.rounds {
                for c in columns_of_round(&lay, frame.dpr, w, t) {
                    for (k, n) in cells(&sc, t) {
                        for r in rows_of_cell(&sc, &lay, frame.dpr, h, t, k) {
                            want[r * w + c] = want[r * w + c].max(n);
                        }
                    }
                }
            }
            assert_eq!(grid, want, "{frame:?}");
        }
    }

    #[test]
    fn every_round_owns_a_column_and_columns_are_not_shared_when_there_is_room() {
        let (game, counts, e) = scene_parts(1000, 500);
        let sc = scene(&game, &counts, &e);
        for frame in FRAMES {
            let lay = layout(&sc, frame);
            let w = (lay.w * frame.dpr).round() as usize;
            let mut owners = vec![0u32; w];
            for t in 0..=1000 {
                let cols = columns_of_round(&lay, frame.dpr, w, t);
                assert!(!cols.is_empty(), "round {t} has no column in {frame:?}");
                for c in cols {
                    owners[c] += 1;
                }
            }
            if w as u32 > 1000 {
                assert!(owners.iter().all(|&o| o == 1), "a column shows two rounds although each round has a column");
            }
            assert!(owners.iter().all(|&o| o >= 1), "an empty column");
        }
    }

    #[test]
    fn every_occupied_cell_is_visible() {
        let (game, counts, e) = scene_parts(1000, 2000);
        let sc = scene(&game, &counts, &e);
        for frame in FRAMES {
            let lay = layout(&sc, frame);
            let (grid, w, h) = density_grid(&sc, &lay, frame.dpr);
            for t in 1..=1000u32 {
                for (k, n) in cells(&sc, t) {
                    let seen = columns_of_round(&lay, frame.dpr, w, t)
                        .any(|c| rows_of_cell(&sc, &lay, frame.dpr, h, t, k).any(|r| grid[r * w + c] >= n));
                    assert!(seen, "cell t={t} k={k} not visible in {frame:?}");
                }
            }
        }
    }

    #[test]
    fn no_cell_paints_on_the_wrong_side_of_a_threshold() {
        let game = Game::peters(400, 2022);
        let (counts, _) = simulate(&game, 200_000);
        let e = ensemble(&game, 200_000, &counts);
        let sc = scene(&game, &counts, &e);
        let lat = game.lattice();
        for frame in FRAMES {
            let lay = layout(&sc, frame);
            let h = (lay.h * frame.dpr).round() as usize;
            let y = |v: f64| f64::from(lay.y_of(v) - lay.y) * f64::from(frame.dpr);
            for t in 1..=400u32 {
                for (k, _) in cells(&sc, t) {
                    let rows = rows_of_cell(&sc, &lay, frame.dpr, h, t, k);
                    if rows.len() == 1 && (rows.start as f64 + 0.5) > y(lat.at(t, k) - lat.gap / 2.0) {
                        continue; // sub-pixel band placed at its own value: allowed
                    }
                    for thr in [lat.rich, lat.broke] {
                        let line = y(thr);
                        for r in rows.clone() {
                            let centre = r as f64 + 0.5;
                            if lat.at(t, k) >= thr {
                                assert!(centre <= line + 1e-9, "t={t} k={k}: rich cell below its line");
                            } else {
                                assert!(centre >= line - 1e-9, "t={t} k={k}: cell above a line it does not reach");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn isolated_cells_are_not_enlarged() {
        let (game, counts, e) = scene_parts(200, 1000);
        let sc = scene(&game, &counts, &e);
        let frame = Frame { css_w: 1040.0, css_h: 560.0, dpr: 2.0 };
        let lay = layout(&sc, frame);
        let lat = game.lattice();
        let px_per_gap = lat.gap / (lay.ymax - lay.ymin) * f64::from(lay.h * frame.dpr);
        let h = (lay.h * frame.dpr).round() as usize;
        for t in [50u32, 120, 200] {
            for (k, _) in cells(&sc, t) {
                let rows = rows_of_cell(&sc, &lay, frame.dpr, h, t, k).len() as f64;
                assert!(rows <= px_per_gap.ceil(), "cell t={t} k={k} got {rows} rows for a {px_per_gap:.2}-row band");
            }
        }
    }

    #[test]
    fn layout_holds_every_mark_and_maps_back() {
        let (game, counts, e) = scene_parts(300, 5000);
        let path: Vec<f64> = (0..=300).map(|t| 2.0 + f64::from(t) * 0.05).collect();
        let hl = vec![(path, 0usize)];
        let scene =
            Scene { game: &game, counts: &counts, ensemble: &e, highlighted: &hl, rich_paths: &[], rich_full: false };
        let lay = layout(&scene, Frame { css_w: 900.0, css_h: 500.0, dpr: 1.0 });
        assert!(lay.ymax >= 17.0 && lay.ymax >= e.expected[300] && lay.ymin <= 0.0);
        for t in [0u32, 1, 150, 300] {
            assert_eq!(lay.round_at(lay.x_of(f64::from(t))), t);
        }
        assert!((lay.wealth_at(lay.y_of(3.25)) - 3.25).abs() < 1e-4);
    }

    #[test]
    fn legend_ticks_are_increasing_and_end_at_the_fullest_cell() {
        let t = legend_ticks(500_733);
        assert_eq!(t.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 10, 100, 1000, 10_000, 100_000, 500_733]);
        assert!(t.windows(2).all(|w| w[1].1 > w[0].1));
        assert_eq!(legend_ticks(1), vec![(1, 1.0)]);
        assert_eq!(scale_position(1, 500_733), 0.0);
    }

    #[test]
    fn renders_at_any_size() {
        let (game, counts, e) = scene_parts(100, 500);
        let scene = scene(&game, &counts, &e);
        for (w, h, dpr) in [(280.0, 320.0, 1.0), (1040.0, 560.0, 2.0), (400.0, 300.0, 3.0)] {
            let (pm, _) = render(&scene, Frame { css_w: w, css_h: h, dpr }, &theme::LIGHT);
            assert_eq!(pm.width(), (w * dpr) as u32);
            assert!(pm.pixels().iter().all(|p| p.alpha() == 255), "chart must be opaque");
        }
    }
}
