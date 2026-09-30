//! Pure-Rust renderer of the coin-game chart.
//!
//! What the chart shows, and how each mark maps to data:
//! - Density: one cell per (round, heads). Each round owns whole pixel columns and each cell whole
//!   pixel rows ([`columns_of_round`], [`rows_of_cell`]); where several rounds or cells share one
//!   pixel, the pixel shows the fullest of them. Colour: log scale from 1 player to the fullest
//!   cell; the page prints the scale in numbers ([`legend_ticks`]). Cells never paint across a
//!   threshold line, and the cells of one round tile the axis without holes ([`band`]).
//! - Strip under the plot: the number of players at or above the rich threshold in each round
//!   (same columns as the density; where several rounds share a column, the largest of them).
//! - Thin orange lines: the players who reached the rich threshold, by default only where they are
//!   at or above it; highlighted players are not drawn twice. Players on the same level in the
//!   same round have the same wealth, so their lines coincide: the strip gives the count.
//! - Thick coloured lines: the players singled out by `coin_core::stats::picks`, coloured by role.
//! - Ink lines: median player (solid), mean of all players (dotted), expected value (dashed).

pub mod text;
pub mod theme;

use coin_core::{ensemble::Ensemble, Game, Lattice};
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
    /// Highlighted players: path and palette slot (0..3, from `Role::slot`).
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

/// Plot areas (CSS pixels) and the data <-> CSS mapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Main plot.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Strip with the number of rich players per round (same x and w as the plot).
    pub strip_y: f32,
    pub strip_h: f32,
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
    /// True if the CSS point is inside the main plot.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    /// True if the CSS point is inside the strip.
    pub fn in_strip(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.strip_y && y < self.strip_y + self.strip_h
    }
    /// Device-pixel grid of the main plot.
    pub fn raster(&self, dpr: f32) -> Raster {
        Raster::new(self.x, self.y, self.w, self.h, dpr)
    }
    /// Device-pixel grid of the strip.
    pub fn strip_raster(&self, dpr: f32) -> Raster {
        Raster::new(self.x, self.strip_y, self.w, self.strip_h, dpr)
    }
}

/// A plot area in device pixels, with an integer origin so that pixels, hover and data agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Raster {
    /// Device pixel of the top-left corner.
    pub ox: usize,
    pub oy: usize,
    /// Size in device pixels.
    pub w: usize,
    pub h: usize,
    pub dpr: f32,
}

impl Raster {
    fn new(x: f32, y: f32, w: f32, h: f32, dpr: f32) -> Raster {
        let ox = (x * dpr).round().max(0.0) as usize;
        let oy = (y * dpr).round().max(0.0) as usize;
        let x1 = ((x + w) * dpr).round() as usize;
        let y1 = ((y + h) * dpr).round() as usize;
        Raster { ox, oy, w: x1.saturating_sub(ox).max(1), h: y1.saturating_sub(oy).max(1), dpr }
    }
    /// CSS x as a device x relative to the raster (not rounded).
    pub fn dx(&self, css_x: f32) -> f64 {
        f64::from(css_x) * f64::from(self.dpr) - self.ox as f64
    }
    /// CSS y as a device y relative to the raster (not rounded).
    pub fn dy(&self, css_y: f32) -> f64 {
        f64::from(css_y) * f64::from(self.dpr) - self.oy as f64
    }
    /// Device column under CSS x, if inside.
    pub fn col_at(&self, css_x: f32) -> Option<usize> {
        let c = self.dx(css_x).floor();
        (c >= 0.0 && (c as usize) < self.w).then_some(c as usize)
    }
    /// Device row under CSS y, if inside.
    pub fn row_at(&self, css_y: f32) -> Option<usize> {
        let r = self.dy(css_y).floor();
        (r >= 0.0 && (r as usize) < self.h).then_some(r as usize)
    }
}

const MARGIN_LEFT: f32 = 58.0;
const MARGIN_RIGHT: f32 = 52.0;
const MARGIN_TOP: f32 = 12.0;
/// Space between the plot and the strip (holds the strip title).
const STRIP_GAP: f32 = 26.0;
const STRIP_H: f32 = 48.0;
/// Space under the strip (holds the round labels).
const AXIS_H: f32 = 26.0;

/// Computes the plot areas and a y range that holds every drawn mark and both thresholds.
pub fn layout(scene: &Scene<'_>, frame: Frame) -> Layout {
    let game = scene.game;
    let lat = game.lattice();
    let r = game.rounds;
    let width = r as usize + 1;
    let (mut lo, mut hi) = (lat.broke.min(lat.l0), lat.rich.max(lat.l0));
    for t in 1..=r {
        let row = &scene.counts[(t as usize - 1) * width..][..t as usize + 1];
        if let Some(k) = row.iter().position(|&n| n > 0) {
            lo = lo.min(band(&lat, t, k as u32).0);
        }
        if let Some(k) = row.iter().rposition(|&n| n > 0) {
            hi = hi.max(band(&lat, t, k as u32).1);
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
    let h = (frame.css_h - MARGIN_TOP - STRIP_GAP - STRIP_H - AXIS_H).max(40.0);
    Layout {
        x: MARGIN_LEFT,
        y: MARGIN_TOP,
        w: (frame.css_w - MARGIN_LEFT - MARGIN_RIGHT).max(40.0),
        h,
        strip_y: MARGIN_TOP + h + STRIP_GAP,
        strip_h: STRIP_H,
        ymin: lo - pad,
        ymax: hi + pad,
        rounds: r,
    }
}

/// Occupied cells of round `t`: (heads, players). Round 0 is one cell holding every player.
pub fn cells(scene: &Scene<'_>, t: u32) -> Vec<(u32, u32)> {
    if t == 0 {
        return vec![(0, scene.ensemble.players.min(u64::from(u32::MAX)) as u32)];
    }
    let width = scene.game.rounds as usize + 1;
    let row = &scene.counts[(t as usize - 1) * width..][..t as usize + 1];
    row.iter().enumerate().filter(|(_, &n)| n > 0).map(|(k, &n)| (k as u32, n)).collect()
}

/// The band of log10 wealth that cell (t, k) paints: halfway to the neighbouring levels, except
/// where a threshold lies between two neighbouring levels, where the band ends at the threshold.
/// So the bands of one round tile the axis without holes and never cross a threshold.
pub fn band(lat: &Lattice, t: u32, k: u32) -> (f64, f64) {
    // The edge between levels k and k + 1 is computed by one expression, so that neighbouring
    // bands share it bit for bit.
    let edge = |half: f64| lat.base(t) + half * lat.gap;
    let l = lat.at(t, k);
    let (mut lo, mut hi) = (edge(f64::from(k) - 0.5), edge(f64::from(k) + 0.5));
    for thr in [lat.rich, lat.broke] {
        if l >= thr {
            let below = (k > 0).then(|| lat.at(t, k - 1));
            match below {
                Some(b) if b < thr => lo = thr,
                None => lo = lo.max(thr),
                _ => {}
            }
        } else {
            let above = (k < t).then(|| lat.at(t, k + 1));
            match above {
                Some(a) if a >= thr => hi = thr,
                None => hi = hi.min(thr),
                _ => {}
            }
        }
    }
    (lo, hi)
}

/// Device columns that show round `t`: the columns whose centre lies in the round's span
/// [t - 0.5, t + 0.5); if there is none (several rounds per column), the column that holds t.
pub fn columns_of_round(lay: &Layout, ras: &Raster, t: u32) -> std::ops::Range<usize> {
    let x = |t: f64| ras.dx(lay.x_of(t));
    let (a, b) = (x(f64::from(t) - 0.5), x(f64::from(t) + 0.5));
    let c0 = (a - 0.5).ceil().max(0.0) as usize;
    let c1 = ((b - 0.5).ceil().max(0.0) as usize).min(ras.w);
    if c0 < c1 {
        c0..c1
    } else {
        let c = (x(f64::from(t)).floor().max(0.0) as usize).min(ras.w - 1);
        c..c + 1
    }
}

/// Device rows that show cell (t, k): the rows whose centre lies in the cell's [`band`]. If no row
/// centre falls in the band, the row that holds at(t, k), moved by one row if its centre is on the
/// other side of a threshold line.
pub fn rows_of_cell(lay: &Layout, ras: &Raster, lat: &Lattice, t: u32, k: u32) -> std::ops::Range<usize> {
    let (lo, hi) = band(lat, t, k);
    let y = |v: f64| ras.dy(lay.y_of(v));
    let (top, bottom) = (y(hi), y(lo));
    let r0 = (top - 0.5).ceil().max(0.0) as usize;
    let r1 = ((bottom - 0.5).ceil().max(0.0) as usize).min(ras.h);
    if r0 < r1 {
        return r0..r1;
    }
    let l = lat.at(t, k);
    let mut r = (y(l).floor().max(0.0) as usize).min(ras.h - 1);
    for thr in [lat.rich, lat.broke] {
        let centre = r as f64 + 0.5;
        if l >= thr && centre > y(thr) && r > 0 {
            r -= 1;
        } else if l < thr && centre < y(thr) && r + 1 < ras.h {
            r += 1;
        }
    }
    r..r + 1
}

/// Density in device pixels of the main plot (row-major, `ras.w * ras.h`): the number of players
/// in the cell that the pixel shows (the fullest, if several share it); 0 = no player.
pub fn density_grid(scene: &Scene<'_>, lay: &Layout, ras: &Raster) -> Vec<u32> {
    let lat = scene.game.lattice();
    let mut grid = vec![0u32; ras.w * ras.h];
    for t in 0..=scene.game.rounds {
        let cols = columns_of_round(lay, ras, t);
        for (k, n) in cells(scene, t) {
            for y in rows_of_cell(lay, ras, &lat, t, k) {
                for cell in &mut grid[y * ras.w..][cols.clone()] {
                    *cell = (*cell).max(n);
                }
            }
        }
    }
    grid
}

/// Rich players per device column of the strip: the largest `rich_now` among the rounds that own
/// the column.
pub fn strip_counts(scene: &Scene<'_>, lay: &Layout, ras: &Raster) -> Vec<u64> {
    let mut out = vec![0u64; ras.w];
    for t in 0..=scene.game.rounds {
        let n = scene.ensemble.rich_now[t as usize];
        for c in columns_of_round(lay, ras, t) {
            out[c] = out[c].max(n);
        }
    }
    out
}

/// Position in [0, 1] of `n` players on the log colour scale that ends at `max_cell`.
pub fn scale_position(n: u32, max_cell: u32) -> f64 {
    if max_cell <= 1 {
        // One player in all: every occupied cell is the fullest cell.
        return 1.0;
    }
    if n <= 1 {
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

/// Round labels under the strip: 0, R/4, R/2, 3R/4, R without duplicates, and without the middle
/// ones that would touch a neighbour. (text, x, alignment).
fn round_ticks(lay: &Layout) -> Vec<(String, f32, Align)> {
    let r = lay.rounds;
    let mut ts: Vec<u32> = (0..=4u32).map(|i| (u64::from(r) * u64::from(i) / 4) as u32).collect();
    ts.dedup();
    let last = *ts.last().expect("at least round 0");
    let span = |t: u32, label: &str| {
        let (x, w) = (lay.x_of(f64::from(t)), text::width(label, 12.0));
        if t == 0 {
            (x, x + w)
        } else if t == last {
            (x - w, x)
        } else {
            (x - w / 2.0, x + w / 2.0)
        }
    };
    let last_label = coin_core::fmt::int(u64::from(last));
    let last_left = span(last, &last_label).0;
    let mut out = Vec::new();
    let mut right = f32::NEG_INFINITY;
    for &t in &ts {
        let label = coin_core::fmt::int(u64::from(t));
        let (a, b) = span(t, &label);
        let is_end = t == 0 || t == last;
        if is_end || (a >= right + 6.0 && b <= last_left - 6.0) {
            let align = if t == 0 {
                Align::Left
            } else if t == last {
                Align::Right
            } else {
                Align::Center
            };
            right = b;
            out.push((label, lay.x_of(f64::from(t)), align));
        }
    }
    out
}

/// Long name of a threshold for text: "1 miliardo", "1 milione", "1" (EUR).
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

/// Short name of a threshold for the chart margin: "1 mld €", "1 €".
pub fn short_label(v: f64) -> String {
    if v == 1e9 {
        "1 mld €".into()
    } else if v == 1.0 {
        "1 €".into()
    } else {
        coin_core::fmt::eur(v.log10())
    }
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

fn hline(x0: f32, x1: f32, y: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x0, y);
    pb.line_to(x1, y);
    pb.finish()
}

fn rect_mask(dw: u32, dh: u32, tf: Transform, x: f32, y: f32, w: f32, h: f32) -> Mask {
    let mut m = Mask::new(dw, dh).expect("non-zero size");
    if let Some(r) = Rect::from_xywh(x, y, w, h.max(0.0)) {
        m.fill_path(&PathBuilder::from_rect(r), FillRule::Winding, false, tf);
    }
    m
}

/// How a label is drawn.
#[derive(Clone, Copy)]
struct Label {
    size: f32,
    align: Align,
    color: Rgb,
}

fn text(pm: &mut Pixmap, tf: Transform, s: &str, x: f32, y: f32, style: Label) {
    if let Some(p) = text::path(s, x, y, style.size, style.align) {
        pm.fill_path(&p, &paint(style.color, 255), FillRule::Winding, tf, None);
    }
}

/// "10^e €" as base, raised exponent and euro sign, right-aligned at `x`.
fn power_label(pm: &mut Pixmap, tf: Transform, e: i64, x: f32, y: f32, color: Rgb) {
    let exp = if e < 0 { format!("−{}", -e) } else { e.to_string() };
    let euro = "\u{2009}€";
    let we = text::width(euro, 12.0);
    let wx = text::width(&exp, 9.0);
    let label = |size| Label { size, align: Align::Right, color };
    text(pm, tf, euro, x, y, label(12.0));
    text(pm, tf, &exp, x - we, y - 5.0, label(9.0));
    text(pm, tf, "10", x - we - wx, y, label(12.0));
}

/// Device-pixel rectangles (x, y, w, h) of the frame around a raster: `th` pixels just outside it.
pub fn frame_rects(ras: &Raster, th: usize) -> [(usize, usize, usize, usize); 4] {
    let (x0, y0) = (ras.ox.saturating_sub(th), ras.oy.saturating_sub(th));
    let full_w = ras.ox + ras.w + th - x0;
    [
        (x0, y0, full_w, ras.oy - y0),       // top
        (x0, ras.oy + ras.h, full_w, th),    // bottom
        (x0, ras.oy, ras.ox - x0, ras.h),    // left
        (ras.ox + ras.w, ras.oy, th, ras.h), // right
    ]
}

fn put(pixels: &mut [PremultipliedColorU8], dw: usize, x: usize, y: usize, c: Rgb) {
    if x < dw && y * dw + x < pixels.len() {
        pixels[y * dw + x] = PremultipliedColorU8::from_rgba(c.0, c.1, c.2, 255).expect("opaque colour");
    }
}

/// Which parts of the chart to draw (all of them for the page; single parts for tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layers {
    pub grid: bool,
    pub density: bool,
    pub strip: bool,
    pub thresholds: bool,
    pub rich: bool,
    pub ensemble: bool,
    pub highlighted: bool,
    pub frames: bool,
    pub labels: bool,
}

impl Layers {
    pub const ALL: Layers = Layers {
        grid: true,
        density: true,
        strip: true,
        thresholds: true,
        rich: true,
        ensemble: true,
        highlighted: true,
        frames: true,
        labels: true,
    };
    pub const NONE: Layers = Layers {
        grid: false,
        density: false,
        strip: false,
        thresholds: false,
        rich: false,
        ensemble: false,
        highlighted: false,
        frames: false,
        labels: false,
    };
}

/// Decades that get a grid line and a label: powers of ten in the y range, at most 8 of them.
pub fn decades(lay: &Layout) -> Vec<i64> {
    let span = lay.ymax - lay.ymin;
    let step = [1i64, 2, 5, 10, 20, 50, 100, 200, 500].into_iter().find(|s| span / *s as f64 <= 8.0).unwrap_or(1000);
    let mut e = (lay.ymin / step as f64).ceil() as i64 * step;
    let mut out = Vec::new();
    while (e as f64) <= lay.ymax {
        out.push(e);
        e += step;
    }
    out
}

/// Height in device pixels of the strip bar for `n` rich players: proportional to `n / max`,
/// at least one pixel for any player.
pub fn bar_height(n: u64, max: u64, h: usize) -> usize {
    if n == 0 || max == 0 {
        return 0;
    }
    (((n as f64 / max as f64) * h as f64).round() as usize).clamp(1, h)
}

/// Draws the chart. The result is opaque, so its premultiplied bytes are also straight RGBA.
pub fn render(scene: &Scene<'_>, frame: Frame, theme: &Theme) -> (Pixmap, Layout) {
    render_layers(scene, frame, theme, Layers::ALL)
}

/// Draws the selected parts of the chart, in this order: grid, density, strip, thresholds, rich
/// players, ensemble lines, highlighted players, frames, labels.
pub fn render_layers(scene: &Scene<'_>, frame: Frame, theme: &Theme, layers: Layers) -> (Pixmap, Layout) {
    let (dw, dh) = frame.device_size();
    let mut pm = Pixmap::new(dw, dh).expect("non-zero size");
    let s = theme.surface;
    pm.fill(Color::from_rgba8(s.0, s.1, s.2, 255));
    let lay = layout(scene, frame);
    let ras = lay.raster(frame.dpr);
    let sras = lay.strip_raster(frame.dpr);
    let tf = Transform::from_scale(frame.dpr, frame.dpr);
    let lat = scene.game.lattice();
    let rule = paint(theme.rule, 255);
    let decs = decades(&lay);
    let strip = strip_counts(scene, &lay, &sras);
    let strip_max = strip.iter().copied().max().unwrap_or(0);

    if layers.grid {
        for &d in &decs {
            if let Some(p) = hline(lay.x, lay.x + lay.w, lay.y_of(d as f64)) {
                pm.stroke_path(&p, &rule, &stroke(1.0, None), tf, None);
            }
        }
    }

    // Density and strip, written straight into the pixels.
    if layers.density || layers.strip {
        let pixels = pm.pixels_mut();
        if layers.density {
            let grid = density_grid(scene, &lay, &ras);
            let max_cell = scene.ensemble.max_cell;
            let lut: Vec<Rgb> = (0..=255).map(|i| theme.ramp(f64::from(i) / 255.0)).collect();
            for gy in 0..ras.h {
                for gx in 0..ras.w {
                    let n = grid[gy * ras.w + gx];
                    if n > 0 {
                        let c = lut[(scale_position(n, max_cell) * 255.0).round() as usize];
                        put(pixels, dw as usize, ras.ox + gx, ras.oy + gy, c);
                    }
                }
            }
        }
        if layers.strip {
            for (c, &n) in strip.iter().enumerate() {
                let h = bar_height(n, strip_max, sras.h);
                for r in sras.h - h..sras.h {
                    put(pixels, dw as usize, sras.ox + c, sras.oy + r, theme.players[1]);
                }
            }
        }
    }

    // Everything else in the plot is clipped to its device pixels.
    let clip = rect_mask(dw, dh, Transform::identity(), ras.ox as f32, ras.oy as f32, ras.w as f32, ras.h as f32);
    let casing = paint(theme.surface, 255);
    if layers.thresholds {
        for l in [lat.rich, lat.broke] {
            if let Some(p) = hline(lay.x, lay.x + lay.w, lay.y_of(l)) {
                // One pixel, alternating ink and surface: visible on any density, and too thin to
                // read as a gap in the data. Drawn before the players, so it hides none of them.
                pm.stroke_path(&p, &casing, &stroke(1.0, None), tf, Some(&clip));
                pm.stroke_path(&p, &paint(theme.ink, 255), &stroke(1.0, Some((4.0, 4.0))), tf, Some(&clip));
            }
        }
    }
    if layers.rich {
        let line_dev = ras.dy(lay.y_of(lat.rich));
        let rich_bottom = if scene.rich_full { ras.h as f64 } else { line_dev };
        let rich_clip =
            rect_mask(dw, dh, Transform::identity(), ras.ox as f32, ras.oy as f32, ras.w as f32, rich_bottom as f32);
        let rich_paint = paint(theme.players[1], if theme.dark { 150 } else { 130 });
        for p in scene.rich_paths {
            if let Some(path) = polyline(&lay, p) {
                pm.stroke_path(&path, &rich_paint, &stroke(1.0, None), tf, Some(&rich_clip));
            }
        }
        if !scene.rich_full {
            // A player who crossed the line by less than a pixel still gets one pixel per round
            // above it, just above the line.
            let line_row = line_dev.floor();
            if line_row >= 1.0 {
                let pixels = pm.pixels_mut();
                for p in scene.rich_paths {
                    for (t, &v) in p.iter().enumerate() {
                        if v < lat.rich {
                            continue;
                        }
                        let col = columns_of_round(&lay, &ras, t as u32).start;
                        let row = ras.dy(lay.y_of(v)).floor().clamp(0.0, line_row - 1.0) as usize;
                        put(pixels, dw as usize, ras.ox + col, ras.oy + row, theme.players[1]);
                    }
                }
            }
        }
    }
    let ink = paint(theme.ink, 255);
    let en = scene.ensemble;
    if layers.ensemble {
        for (values, width, dash) in
            [(&en.expected, 1.4, Some((7.0, 4.0))), (&en.mean, 1.6, Some((1.5, 3.0))), (&en.median, 1.6, None)]
        {
            if let Some(p) = polyline(&lay, values) {
                // A surface-coloured casing keeps the line readable over any density.
                pm.stroke_path(&p, &casing, &stroke(width + 2.0, dash), tf, Some(&clip));
                pm.stroke_path(&p, &ink, &stroke(width, dash), tf, Some(&clip));
            }
        }
    }
    if layers.highlighted {
        for (p, slot) in scene.highlighted {
            if let Some(path) = polyline(&lay, p) {
                pm.stroke_path(&path, &casing, &stroke(4.0, None), tf, Some(&clip));
                pm.stroke_path(&path, &paint(theme.players[*slot % 3], 255), &stroke(2.0, None), tf, Some(&clip));
            }
        }
    }

    // Frames in whole device pixels just outside the plot and the strip: no data pixel is covered.
    if layers.frames {
        let th = (frame.dpr.round() as usize).max(1);
        let pixels = pm.pixels_mut();
        for r in [&ras, &sras] {
            for (x, y, w, h) in frame_rects(r, th) {
                for yy in y..y + h {
                    for xx in x..x + w {
                        put(pixels, dw as usize, xx, yy, theme.rule);
                    }
                }
            }
        }
    }

    // Labels, all outside the data areas.
    if layers.labels {
        let muted = |size, align| Label { size, align, color: theme.muted };
        for (l, v) in [(lat.rich, scene.game.rich), (lat.broke, scene.game.broke)] {
            text(&mut pm, tf, &short_label(v), lay.x + lay.w + 5.0, lay.y_of(l) + 4.0, muted(11.0, Align::Left));
        }
        for &d in &decs {
            power_label(&mut pm, tf, d, lay.x - 8.0, lay.y_of(d as f64) + 4.0, theme.muted);
        }
        let rich = short_label(scene.game.rich);
        let titles = if strip_max > 0 {
            [format!("Giocatori sopra {rich}, round per round"), format!("Sopra {rich}, per round")]
        } else {
            [format!("Giocatori sopra {rich}: nessuno, in nessun round"), format!("Sopra {rich}: nessuno")]
        };
        let title = titles.iter().find(|t| text::width(t, 12.0) <= lay.w).unwrap_or(&titles[1]);
        let title_style = Label { size: 12.0, align: Align::Left, color: theme.ink };
        text(&mut pm, tf, title, lay.x, lay.strip_y - 8.0, title_style);
        if strip_max > 0 {
            let max_txt = coin_core::fmt::int(strip_max);
            text(&mut pm, tf, &max_txt, lay.x - 8.0, lay.strip_y + 9.0, muted(11.0, Align::Right));
            text(&mut pm, tf, "0", lay.x - 8.0, lay.strip_y + lay.strip_h, muted(11.0, Align::Right));
        }
        let base = lay.strip_y + lay.strip_h + 18.0;
        for (label, x, align) in round_ticks(&lay) {
            text(&mut pm, tf, &label, x, base, muted(12.0, align));
        }
        text(&mut pm, tf, "round", lay.x - 8.0, base, muted(12.0, Align::Right));
    }
    (pm, lay)
}

#[cfg(test)]
mod tests {
    use super::*;
    use coin_core::{ensemble::ensemble, sim::simulate};

    fn parts(game: Game, players: usize) -> (Game, Vec<u32>, Ensemble) {
        let (counts, _) = simulate(&game, players);
        let e = ensemble(&game, players as u64, &counts);
        (game, counts, e)
    }

    fn scene<'a>(game: &'a Game, counts: &'a [u32], e: &'a Ensemble) -> Scene<'a> {
        Scene { game, counts, ensemble: e, highlighted: &[], rich_paths: &[], rich_full: false }
    }

    const FRAMES: [Frame; 5] = [
        Frame { css_w: 272.0, css_h: 320.0, dpr: 1.0 }, // several rounds per column
        Frame { css_w: 700.0, css_h: 420.0, dpr: 2.0 },
        Frame { css_w: 1040.0, css_h: 560.0, dpr: 1.0 },
        Frame { css_w: 400.0, css_h: 320.0, dpr: 3.0 },
        Frame { css_w: 733.0, css_h: 411.0, dpr: 1.25 }, // fractional ratio
    ];

    /// Independent reference, pixel by pixel from the definitions (without columns_of_round /
    /// rows_of_cell): a column shows the round whose span holds its centre, plus any round whose
    /// span holds no column centre and whose own x falls in the column; a row shows the occupied
    /// cell whose band holds its centre, plus any cell whose band holds no row centre and whose value
    /// falls in the row (moved off a threshold line); the pixel keeps the fullest.
    #[test]
    fn density_grid_matches_its_definition() {
        let (game, counts, e) = parts(Game::peters(60, 11), 3000);
        let sc = scene(&game, &counts, &e);
        let lat = game.lattice();
        for frame in FRAMES {
            let lay = layout(&sc, frame);
            let ras = lay.raster(frame.dpr);
            let grid = density_grid(&sc, &lay, &ras);
            let dpr = f64::from(frame.dpr);
            // device x (relative) -> rounds, device y (relative) -> log10 wealth
            let round_at = |dx: f64| ((dx + ras.ox as f64) / dpr - f64::from(lay.x)) / f64::from(lay.w) * 60.0;
            let wealth_at = |dy: f64| {
                lay.ymax - ((dy + ras.oy as f64) / dpr - f64::from(lay.y)) / f64::from(lay.h) * (lay.ymax - lay.ymin)
            };
            let dev_y = |v: f64| (f64::from(lay.y_of(v)) * dpr) - ras.oy as f64;
            let dev_x = |t: f64| (f64::from(lay.x_of(t)) * dpr) - ras.ox as f64;
            let mut want = vec![0u32; ras.w * ras.h];
            for t in 0..=60u32 {
                let span = (f64::from(t) - 0.5, f64::from(t) + 0.5);
                let mut cols: Vec<usize> = (0..ras.w)
                    .filter(|&c| {
                        let u = round_at(c as f64 + 0.5);
                        u >= span.0 && u < span.1
                    })
                    .collect();
                if cols.is_empty() {
                    cols.push((dev_x(f64::from(t)).floor().max(0.0) as usize).min(ras.w - 1));
                }
                for (k, n) in cells(&sc, t) {
                    let (lo, hi) = band(&lat, t, k);
                    let mut rows: Vec<usize> = (0..ras.h)
                        .filter(|&r| {
                            let v = wealth_at(r as f64 + 0.5);
                            v >= lo && v < hi
                        })
                        .collect();
                    if rows.is_empty() {
                        let l = lat.at(t, k);
                        let mut r = (dev_y(l).floor().max(0.0) as usize).min(ras.h - 1);
                        for thr in [lat.rich, lat.broke] {
                            let centre = r as f64 + 0.5;
                            if l >= thr && centre > dev_y(thr) && r > 0 {
                                r -= 1;
                            } else if l < thr && centre < dev_y(thr) && r + 1 < ras.h {
                                r += 1;
                            }
                        }
                        rows.push(r);
                    }
                    for &c in &cols {
                        for &r in &rows {
                            want[r * ras.w + c] = want[r * ras.w + c].max(n);
                        }
                    }
                }
            }
            let diff = grid.iter().zip(&want).filter(|(a, b)| a != b).count();
            // Pixel centres that sit exactly on a band edge may fall either way by rounding.
            assert!(diff * 1000 <= grid.len(), "{diff} of {} pixels differ in {frame:?}", grid.len());
        }
    }

    #[test]
    fn bands_tile_each_round_without_holes_and_respect_thresholds() {
        for game in [Game::peters(2000, 0), Game { rich: 1e4, ..Game::peters(500, 0) }] {
            let lat = game.lattice();
            for t in 1..=game.rounds {
                for k in 0..t {
                    let (a, b) = (band(&lat, t, k), band(&lat, t, k + 1));
                    assert_eq!(a.1, b.0, "hole or overlap between levels {k} and {} at round {t}", k + 1);
                }
                for k in 0..=t {
                    let (lo, hi) = band(&lat, t, k);
                    let l = lat.at(t, k);
                    assert!(lo < l + 1e-12 && l < hi + 1e-12 && lo < hi, "band misses its level");
                    for thr in [lat.rich, lat.broke] {
                        assert!(if l >= thr { lo >= thr } else { hi <= thr }, "band crosses a threshold");
                    }
                }
            }
        }
    }

    #[test]
    fn every_round_owns_a_column_and_columns_are_not_shared_when_there_is_room() {
        let (game, counts, e) = parts(Game::peters(1000, 11), 500);
        let sc = scene(&game, &counts, &e);
        for frame in FRAMES {
            let lay = layout(&sc, frame);
            let ras = lay.raster(frame.dpr);
            let mut owners = vec![0u32; ras.w];
            for t in 0..=1000 {
                let cols = columns_of_round(&lay, &ras, t);
                assert!(!cols.is_empty(), "round {t} has no column in {frame:?}");
                for c in cols {
                    owners[c] += 1;
                }
            }
            if ras.w > 1001 {
                assert!(owners.iter().all(|&o| o == 1), "a column shows two rounds although each round has a column");
            }
            assert!(owners.iter().all(|&o| o >= 1), "an empty column");
        }
    }

    #[test]
    fn every_occupied_cell_is_visible() {
        let (game, counts, e) = parts(Game::peters(1000, 11), 2000);
        let sc = scene(&game, &counts, &e);
        let lat = game.lattice();
        for frame in FRAMES {
            let lay = layout(&sc, frame);
            let ras = lay.raster(frame.dpr);
            let grid = density_grid(&sc, &lay, &ras);
            for t in 0..=1000u32 {
                for (k, n) in cells(&sc, t) {
                    let rows = rows_of_cell(&lay, &ras, &lat, t, k);
                    let seen = columns_of_round(&lay, &ras, t).any(|c| rows.clone().any(|r| grid[r * ras.w + c] >= n));
                    assert!(seen, "cell t={t} k={k} not visible in {frame:?}");
                    // ... and at its own place, not clamped to an edge.
                    let (lo, hi) = band(&lat, t, k);
                    let (top, bottom) = (ras.dy(lay.y_of(hi)), ras.dy(lay.y_of(lo)));
                    assert!(
                        rows.start as f64 >= top - 1.0 && rows.end as f64 <= bottom + 1.0,
                        "cell t={t} k={k} misplaced"
                    );
                }
            }
        }
    }

    #[test]
    fn no_cell_paints_on_the_wrong_side_of_a_threshold() {
        let (game, counts, e) = parts(Game::peters(400, 2022), 200_000);
        let sc = scene(&game, &counts, &e);
        let lat = game.lattice();
        for frame in FRAMES {
            let lay = layout(&sc, frame);
            let ras = lay.raster(frame.dpr);
            for t in 1..=400u32 {
                for (k, _) in cells(&sc, t) {
                    for thr in [lat.rich, lat.broke] {
                        let line = ras.dy(lay.y_of(thr));
                        for r in rows_of_cell(&lay, &ras, &lat, t, k) {
                            let centre = r as f64 + 0.5;
                            if lat.at(t, k) >= thr {
                                assert!(centre <= line + 1e-9, "t={t} k={k}: cell below its line");
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
        let (game, counts, e) = parts(Game::peters(200, 11), 1000);
        let sc = scene(&game, &counts, &e);
        let lat = game.lattice();
        let frame = Frame { css_w: 1040.0, css_h: 560.0, dpr: 2.0 };
        let lay = layout(&sc, frame);
        let ras = lay.raster(frame.dpr);
        let px_per_decade = ras.h as f64 / (lay.ymax - lay.ymin);
        for t in [50u32, 120, 200] {
            for (k, _) in cells(&sc, t) {
                let (lo, hi) = band(&lat, t, k);
                let rows = rows_of_cell(&lay, &ras, &lat, t, k).len() as f64;
                let want = (hi - lo) * px_per_decade;
                assert!(rows <= want.ceil().max(1.0), "cell t={t} k={k}: {rows} rows for a {want:.2}-row band");
            }
        }
    }

    #[test]
    fn strip_counts_are_the_rich_players_of_the_owning_rounds() {
        let game = Game { rich: 1e4, ..Game::peters(300, 3) };
        let (game, counts, e) = parts(game, 5000);
        let sc = scene(&game, &counts, &e);
        for frame in FRAMES {
            let lay = layout(&sc, frame);
            let ras = lay.strip_raster(frame.dpr);
            let strip = strip_counts(&sc, &lay, &ras);
            for t in 0..=300u32 {
                for c in columns_of_round(&lay, &ras, t) {
                    assert!(strip[c] >= e.rich_now[t as usize]);
                }
            }
            assert_eq!(strip.iter().max(), e.rich_now.iter().max(), "the strip reaches the true maximum");
        }
    }

    #[test]
    fn layout_holds_every_mark_and_maps_back() {
        let (game, counts, e) = parts(Game::peters(300, 11), 5000);
        let path: Vec<f64> = (0..=300).map(|t| 2.0 + f64::from(t) * 0.05).collect();
        let hl = vec![(path, 0usize)];
        let sc =
            Scene { game: &game, counts: &counts, ensemble: &e, highlighted: &hl, rich_paths: &[], rich_full: false };
        let lay = layout(&sc, Frame { css_w: 900.0, css_h: 500.0, dpr: 1.0 });
        assert!(lay.ymax >= 17.0 && lay.ymax >= e.expected[300] && lay.ymin <= 0.0);
        for t in [0u32, 1, 150, 300] {
            assert_eq!(lay.round_at(lay.x_of(f64::from(t))), t);
        }
        assert!(lay.strip_y > lay.y + lay.h && lay.strip_y + lay.strip_h < 500.0);
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
    fn frames_lie_outside_the_rasters() {
        let (game, counts, e) = parts(Game::peters(100, 11), 500);
        let sc = scene(&game, &counts, &e);
        for frame in FRAMES {
            let lay = layout(&sc, frame);
            for ras in [lay.raster(frame.dpr), lay.strip_raster(frame.dpr)] {
                for (x, y, w, h) in frame_rects(&ras, (frame.dpr.round() as usize).max(1)) {
                    let inside_x = x < ras.ox + ras.w && x + w > ras.ox;
                    let inside_y = y < ras.oy + ras.h && y + h > ras.oy;
                    assert!(!(inside_x && inside_y), "frame rect {:?} overlaps {ras:?}", (x, y, w, h));
                }
            }
        }
    }

    #[test]
    fn round_labels_are_distinct_and_do_not_touch() {
        let (game, counts, e) = parts(Game::peters(1000, 11), 100);
        let sc = scene(&game, &counts, &e);
        for w in [200.0f32, 266.0, 320.0, 400.0, 1000.0] {
            let lay = layout(&sc, Frame { css_w: w, css_h: 400.0, dpr: 1.0 });
            let ticks = round_ticks(&lay);
            let mut spans: Vec<(f32, f32)> = ticks
                .iter()
                .map(|(l, x, a)| {
                    let tw = text::width(l, 12.0);
                    match a {
                        Align::Left => (*x, x + tw),
                        Align::Right => (x - tw, *x),
                        Align::Center => (x - tw / 2.0, x + tw / 2.0),
                    }
                })
                .collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert!(spans.windows(2).all(|p| p[1].0 >= p[0].1), "labels touch at width {w}: {ticks:?}");
        }
        for r in 1..4 {
            let (game, counts, e) = parts(Game::peters(r, 1), 10);
            let lay = layout(&scene(&game, &counts, &e), Frame { css_w: 600.0, css_h: 400.0, dpr: 1.0 });
            let labels: Vec<String> = round_ticks(&lay).into_iter().map(|t| t.0).collect();
            let mut unique = labels.clone();
            unique.dedup();
            assert_eq!(labels, unique, "duplicate round labels for {r} rounds");
        }
    }

    fn rgb(p: PremultipliedColorU8) -> Rgb {
        Rgb(p.red(), p.green(), p.blue())
    }

    fn only(f: impl Fn(&mut Layers)) -> Layers {
        let mut l = Layers::NONE;
        f(&mut l);
        l
    }

    #[test]
    fn a_single_player_is_drawn_with_the_colour_its_legend_shows() {
        assert_eq!(legend_ticks(1), vec![(1, 1.0)]);
        let (game, counts, e) = parts(Game::peters(50, 3), 1);
        let sc = scene(&game, &counts, &e);
        let (pm, _) = render_layers(&sc, FRAMES[1], &theme::LIGHT, only(|l| l.density = true));
        let painted: Vec<Rgb> = pm.pixels().iter().map(|p| rgb(*p)).filter(|c| *c != theme::LIGHT.surface).collect();
        assert!(!painted.is_empty());
        assert!(
            painted.iter().all(|c| *c == theme::LIGHT.ramp(1.0)),
            "one player must use the colour of the legend's only tick"
        );
    }

    /// Every density pixel has the colour the legend gives for its player count.
    #[test]
    fn density_pixels_use_the_legend_colours() {
        let (game, counts, e) = parts(Game::peters(300, 5), 20_000);
        let sc = scene(&game, &counts, &e);
        for (frame, th) in [(FRAMES[1], theme::LIGHT), (FRAMES[4], theme::DARK)] {
            let (pm, lay) = render_layers(&sc, frame, &th, only(|l| l.density = true));
            let ras = lay.raster(frame.dpr);
            let grid = density_grid(&sc, &lay, &ras);
            for gy in 0..ras.h {
                for gx in 0..ras.w {
                    let n = grid[gy * ras.w + gx];
                    let got = rgb(pm.pixels()[(ras.oy + gy) * pm.width() as usize + ras.ox + gx]);
                    let want = if n == 0 {
                        th.surface
                    } else {
                        th.ramp((scale_position(n, e.max_cell) * 255.0).round() / 255.0)
                    };
                    assert_eq!(got, want, "pixel ({gx},{gy}) with {n} players");
                }
            }
        }
    }

    #[test]
    fn strip_bars_have_the_height_of_their_count() {
        let (game, counts, e) = parts(Game { rich: 1e4, ..Game::peters(300, 3) }, 5000);
        let sc = scene(&game, &counts, &e);
        for frame in FRAMES {
            let (pm, lay) = render_layers(&sc, frame, &theme::LIGHT, only(|l| l.strip = true));
            let ras = lay.strip_raster(frame.dpr);
            let counts = strip_counts(&sc, &lay, &ras);
            let max = *counts.iter().max().unwrap();
            for (c, &n) in counts.iter().enumerate() {
                let filled = (0..ras.h)
                    .filter(|r| {
                        rgb(pm.pixels()[(ras.oy + r) * pm.width() as usize + ras.ox + c]) == theme::LIGHT.players[1]
                    })
                    .count();
                let want = if n == 0 { 0 } else { ((n as f64 / max as f64 * ras.h as f64).round() as usize).max(1) };
                assert_eq!(filled, want, "column {c}: {n} of max {max} in {frame:?}");
            }
        }
    }

    /// Rows (device, whole canvas) that hold at least one pixel different from the surface.
    fn painted_rows(pm: &Pixmap, x0: usize, x1: usize, surface: Rgb) -> Vec<usize> {
        let w = pm.width() as usize;
        (0..pm.height() as usize).filter(|&y| (x0..x1).any(|x| rgb(pm.pixels()[y * w + x]) != surface)).collect()
    }

    #[test]
    fn grid_lines_sit_on_their_decades_and_threshold_lines_on_their_thresholds() {
        let (game, counts, e) = parts(Game::peters(300, 5), 5000);
        let sc = scene(&game, &counts, &e);
        let lat = game.lattice();
        for frame in FRAMES {
            for (layer, values) in [
                (only(|l| l.grid = true), decades(&layout(&sc, frame)).iter().map(|d| *d as f64).collect::<Vec<_>>()),
                (only(|l| l.thresholds = true), vec![lat.rich, lat.broke]),
            ] {
                let (pm, lay) = render_layers(&sc, frame, &theme::LIGHT, layer);
                let ras = lay.raster(frame.dpr);
                let rows = painted_rows(&pm, ras.ox, ras.ox + ras.w, theme::LIGHT.surface);
                let at: Vec<f64> = values.iter().map(|v| f64::from(lay.y_of(*v)) * f64::from(frame.dpr)).collect();
                for &r in &rows {
                    assert!(
                        at.iter().any(|y| (r as f64 + 0.5 - y).abs() <= 1.0 + f64::from(frame.dpr) / 2.0),
                        "row {r} painted away from every line in {frame:?}"
                    );
                }
                for y in &at {
                    assert!(
                        rows.iter().any(|&r| (r as f64 + 0.5 - y).abs() <= 1.0),
                        "no line at device y {y} in {frame:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn rich_players_are_drawn_only_above_the_line_and_each_leaves_a_mark() {
        let game = Game::peters(1000, 2022);
        let players = 200_000;
        let (counts, s) = simulate(&game, players);
        let e = ensemble(&game, players as u64, &counts);
        let lat = game.lattice();
        let paths: Vec<Vec<f64>> = coin_core::stats::rich_ids(&game, &s)
            .iter()
            .map(|&id| coin_core::sim::path(&game, id).iter().enumerate().map(|(t, &k)| lat.at(t as u32, k)).collect())
            .collect();
        assert!(paths.len() >= 20, "need rich players, got {}", paths.len());
        for frame in [FRAMES[2], FRAMES[4], FRAMES[0]] {
            let sc = Scene {
                game: &game,
                counts: &counts,
                ensemble: &e,
                highlighted: &[],
                rich_paths: &paths,
                rich_full: false,
            };
            let (pm, lay) = render_layers(&sc, frame, &theme::LIGHT, only(|l| l.rich = true));
            let ras = lay.raster(frame.dpr);
            let line = ras.oy as f64 + ras.dy(lay.y_of(lat.rich));
            let rows = painted_rows(&pm, ras.ox, ras.ox + ras.w, theme::LIGHT.surface);
            assert!(rows.iter().all(|&r| (r as f64) < line), "orange below the 1 mld line in {frame:?}");
            for (i, p) in paths.iter().enumerate() {
                let one = [p.clone()];
                let sc1 = Scene { rich_paths: &one, ..sc };
                let (pm1, _) = render_layers(&sc1, frame, &theme::LIGHT, only(|l| l.rich = true));
                assert!(
                    pm1.pixels().iter().any(|q| rgb(*q) != theme::LIGHT.surface),
                    "rich player {i} leaves no pixel in {frame:?}"
                );
            }
            // With full paths, orange also appears below the line.
            let full = Scene { rich_full: true, ..sc };
            let (pmf, _) = render_layers(&full, frame, &theme::LIGHT, only(|l| l.rich = true));
            let rows_f = painted_rows(&pmf, ras.ox, ras.ox + ras.w, theme::LIGHT.surface);
            assert!(rows_f.iter().any(|&r| (r as f64) > line + 2.0));
        }
    }

    /// The median is solid, the mean dotted: sampled along each line, the median is inked almost
    /// everywhere and the mean clearly less.
    #[test]
    fn line_styles_match_the_legend() {
        let (game, counts, e) = parts(Game::peters(1000, 5), 20_000);
        let sc = scene(&game, &counts, &e);
        let frame = Frame { css_w: 1000.0, css_h: 600.0, dpr: 2.0 };
        let (pm, lay) = render_layers(&sc, frame, &theme::LIGHT, only(|l| l.ensemble = true));
        let w = pm.width() as usize;
        let inked = |values: &[f64]| {
            let mut hit = 0;
            let mut total = 0;
            let mut x = lay.x_of(300.0);
            while x < lay.x_of(1000.0) {
                let t = f64::from((x - lay.x) / lay.w) * 1000.0;
                let (t0, f) = (t.floor() as usize, t.fract());
                let v = values[t0] * (1.0 - f) + values[(t0 + 1).min(1000)] * f;
                let (px, py) = ((x * frame.dpr) as usize, (lay.y_of(v) * frame.dpr) as usize);
                let c = rgb(pm.pixels()[py * w + px]);
                hit += usize::from(c.0 < 128);
                total += 1;
                x += 0.37;
            }
            hit as f64 / total as f64
        };
        let (median, mean) = (inked(&e.median), inked(&e.mean));
        assert!(median > 0.9, "median line not solid: {median:.2}");
        assert!(mean < median - 0.2, "mean line not dotted: {mean:.2} vs {median:.2}");
    }

    #[test]
    fn layout_holds_the_band_of_every_occupied_cell() {
        let (game, counts, e) = parts(Game::peters(1000, 9), 20_000);
        let sc = scene(&game, &counts, &e);
        let lat = game.lattice();
        let lay = layout(&sc, FRAMES[2]);
        for t in 1..=1000u32 {
            for (k, _) in cells(&sc, t) {
                let (lo, hi) = band(&lat, t, k);
                assert!(lo >= lay.ymin && hi <= lay.ymax, "cell ({t},{k}) outside the y range");
            }
        }
    }

    #[test]
    fn frame_does_not_cover_the_first_and_last_columns() {
        let (game, counts, e) = parts(Game::peters(100, 11), 500);
        let sc = scene(&game, &counts, &e);
        for frame in FRAMES {
            let (pm, lay) = render(&sc, frame, &theme::LIGHT);
            let ras = lay.raster(frame.dpr);
            let grid = density_grid(&sc, &lay, &ras);
            let row = (0..ras.h).find(|&r| grid[r * ras.w] > 0 && grid[r * ras.w + ras.w - 1] > 0);
            if let Some(r) = row {
                let rule = theme::LIGHT.rule;
                for c in [0, ras.w - 1] {
                    let p = pm.pixels()[(ras.oy + r) * pm.width() as usize + ras.ox + c];
                    assert!(
                        (p.red(), p.green(), p.blue()) != (rule.0, rule.1, rule.2),
                        "frame drawn over density column {c} in {frame:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn renders_at_any_size() {
        let (game, counts, e) = parts(Game::peters(100, 11), 500);
        let sc = scene(&game, &counts, &e);
        for (w, h, dpr) in [(200.0, 320.0, 1.0), (1040.0, 560.0, 2.0), (400.0, 300.0, 3.0), (733.0, 411.0, 1.25)] {
            let (pm, _) = render(&sc, Frame { css_w: w, css_h: h, dpr }, &theme::DARK);
            assert_eq!(pm.width(), (w * dpr).round() as u32);
            assert!(pm.pixels().iter().all(|p| p.alpha() == 255), "chart must be opaque");
        }
    }
}
