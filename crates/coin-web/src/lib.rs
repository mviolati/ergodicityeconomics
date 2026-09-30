//! WebAssembly entry points of the page. All computation, text and pixels come from Rust; the
//! page's JavaScript starts workers, copies their bytes into this module, and puts the returned
//! text and pixels on screen.
//!
//! Protocol, per run:
//! 1. each worker: `sim_chunk` on its range of players (ranges start at multiples of 64);
//! 2. main instance: `run_begin`, then for every chunk copy the worker's arrays to `run_slot`
//!    pointers and call `run_add_scratch`, then `run_finish`;
//! 3. `view` (page text), `render` (chart pixels), `hover` (tooltip) as often as needed.
//!    Text results are UTF-8 at `out_ptr()`, length returned by the call.

pub mod view;

use coin_chart::{cells, columns_of_round, render as draw, rows_of_cell, theme, Frame, Layout, Raster, Scene};
use coin_core::{
    ensemble::{ensemble, Ensemble},
    sim::{density_len, path, simulate_range, Summary, SummaryMut, BLOCK, FIELDS},
    stats::{picks, rich_ids, stats, Pick, Stats},
    Game,
};
use std::cell::RefCell;

/// Most players one run accepts.
pub const MAX_PLAYERS: u32 = 2_000_000;
/// Most rich players whose lines are drawn; the legend says when more exist.
pub const MAX_RICH_LINES: usize = 2000;

struct Finished {
    ensemble: Ensemble,
    stats: Stats,
    picks: Vec<Pick>,
    /// Highlighted players: id, palette slot, path (log10 EUR per round).
    highlighted: Vec<(u64, usize, Vec<f64>)>,
    rich_paths: Vec<Vec<f64>>,
    layout: Option<(Layout, Raster)>,
    pixels: Vec<u8>,
    size: (u32, u32),
}

struct Run {
    game: Game,
    counts: Vec<u32>,
    scratch: Vec<u32>,
    summary: Summary,
    sim_ms: f64,
    threads: u32,
    done: Option<Finished>,
}

thread_local! {
    static RUN: RefCell<Option<Run>> = const { RefCell::new(None) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn game_of(rounds: u32, seed_lo: u32, seed_hi: u32) -> Game {
    Game::peters(rounds, u64::from(seed_lo) | (u64::from(seed_hi) << 32))
}

fn put(s: String) -> u32 {
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        o.clear();
        o.extend_from_slice(s.as_bytes());
        o.len() as u32
    })
}

/// Address of the last text result.
#[no_mangle]
pub extern "C" fn out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

/// Allocates `n` zeroed bytes for the caller.
#[no_mangle]
pub extern "C" fn alloc(n: usize) -> *mut u8 {
    let mut v = vec![0u8; n];
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Frees memory from [`alloc`].
///
/// # Safety
/// `p` and `n` must come from one call to [`alloc`].
#[no_mangle]
pub unsafe extern "C" fn dealloc(p: *mut u8, n: usize) {
    drop(Vec::from_raw_parts(p, n, n));
}

/// Worker side: simulates players `first_block * 64 .. + n`. Writes the density to `counts`
/// (rounds * (rounds + 1) u32) and the summary to `fields` (FIELDS * n u32, field-major: field `f`
/// of player `i` at `f * n + i`). Returns 0, or -1 for invalid parameters.
///
/// # Safety
/// Both pointers must address buffers of the sizes above.
#[no_mangle]
pub unsafe extern "C" fn sim_chunk(
    rounds: u32,
    seed_lo: u32,
    seed_hi: u32,
    first_block: u32,
    n: u32,
    counts: *mut u32,
    fields: *mut u32,
) -> i32 {
    let game = game_of(rounds, seed_lo, seed_hi);
    if game.validate().is_err() {
        return -1;
    }
    let n = n as usize;
    let c = std::slice::from_raw_parts_mut(counts, density_len(rounds));
    c.fill(0);
    let all = std::slice::from_raw_parts_mut(fields, FIELDS * n);
    let mut chunks = all.chunks_mut(n.max(1));
    let out = SummaryMut(std::array::from_fn(|_| chunks.next().map_or(&mut [][..], |c| &mut c[..n])));
    simulate_range(&game, u64::from(first_block) * BLOCK, c, out);
    0
}

/// Starts a run. Returns 0, or a negative code if the parameters are out of range.
#[no_mangle]
pub extern "C" fn run_begin(players: u32, rounds: u32, seed_lo: u32, seed_hi: u32) -> i32 {
    let game = game_of(rounds, seed_lo, seed_hi);
    if game.validate().is_err() {
        return -1;
    }
    if players == 0 || players > MAX_PLAYERS {
        return -2;
    }
    RUN.with(|r| {
        *r.borrow_mut() = Some(Run {
            game,
            counts: vec![0; density_len(rounds)],
            scratch: vec![0; density_len(rounds)],
            summary: Summary::zeros(players as usize),
            sim_ms: 0.0,
            threads: 0,
            done: None,
        })
    });
    0
}

/// Where to copy a worker's array: field 0 = density (scratch; then call `run_add_scratch`),
/// fields 1..=FIELDS = summary field `field - 1` (u32) at player `first`. Null for a bad field or
/// offset.
#[no_mangle]
pub extern "C" fn run_slot(field: u32, first: u32) -> *mut u8 {
    RUN.with(|r| {
        let mut r = r.borrow_mut();
        let Some(run) = r.as_mut() else { return std::ptr::null_mut() };
        let (f, i) = (field as usize, first as usize);
        if f == 0 {
            return run.scratch.as_mut_ptr() as *mut u8;
        }
        if f > FIELDS || i >= run.summary.len() {
            return std::ptr::null_mut();
        }
        run.summary.fields[f - 1][i..].as_mut_ptr() as *mut u8
    })
}

/// Adds the scratch density to the run's density and clears it.
#[no_mangle]
pub extern "C" fn run_add_scratch() {
    RUN.with(|r| {
        if let Some(run) = r.borrow_mut().as_mut() {
            for (c, s) in run.counts.iter_mut().zip(run.scratch.iter_mut()) {
                *c += *s;
                *s = 0;
            }
        }
    })
}

/// Computes lines, figures and the players to show. Returns 0, or -1 if no run is open or the
/// density does not hold every player in every round.
#[no_mangle]
pub extern "C" fn run_finish(sim_ms: f64, threads: u32) -> i32 {
    RUN.with(|r| {
        let mut r = r.borrow_mut();
        let Some(run) = r.as_mut() else { return -1 };
        let players = run.summary.len() as u64;
        let width = run.game.rounds as usize + 1;
        for t in 0..run.game.rounds as usize {
            let total: u64 = run.counts[t * width..(t + 1) * width].iter().map(|&n| u64::from(n)).sum();
            if total != players {
                return -1;
            }
        }
        let game = run.game;
        let lat = game.lattice();
        let log_path =
            |id: u64| -> Vec<f64> { path(&game, id).iter().enumerate().map(|(t, &k)| lat.at(t as u32, k)).collect() };
        let e = ensemble(&game, players, &run.counts);
        let st = stats(&game, &run.summary, &e);
        let pk = picks(&game, &run.summary);
        let highlighted = pk.iter().map(|p| (p.id, p.role.slot(), log_path(p.id))).collect();
        let rich_paths = rich_ids(&game, &run.summary).into_iter().take(MAX_RICH_LINES).map(log_path).collect();
        run.sim_ms = sim_ms;
        run.threads = threads;
        run.done = Some(Finished {
            ensemble: e,
            stats: st,
            picks: pk,
            highlighted,
            rich_paths,
            layout: None,
            pixels: Vec::new(),
            size: (0, 0),
        });
        0
    })
}

/// Page text of the finished run (JSON). Returns its length, 0 if there is no finished run.
#[no_mangle]
pub extern "C" fn view() -> u32 {
    let s = RUN.with(|r| {
        let r = r.borrow();
        let run = r.as_ref()?;
        let d = run.done.as_ref()?;
        let shown = d.rich_paths.len();
        Some(view::view_json(&run.game, &run.summary, &d.ensemble, &d.stats, &d.picks, shown, run.sim_ms, run.threads))
    });
    s.map_or(0, put)
}

/// Draws the chart at `css_w` x `css_h` CSS pixels. Returns the RGBA pixels (straight alpha,
/// opaque), size from `render_width` / `render_height`; null if there is no finished run.
#[no_mangle]
pub extern "C" fn render(css_w: f32, css_h: f32, dpr: f32, dark: u32, rich_full: u32) -> *const u8 {
    RUN.with(|r| {
        let mut r = r.borrow_mut();
        let Some(run) = r.as_mut() else { return std::ptr::null() };
        let Some(d) = run.done.as_mut() else { return std::ptr::null() };
        let hl: Vec<(Vec<f64>, usize)> = d.highlighted.iter().map(|(_, slot, p)| (p.clone(), *slot)).collect();
        let scene = Scene {
            game: &run.game,
            counts: &run.counts,
            ensemble: &d.ensemble,
            highlighted: &hl,
            rich_paths: &d.rich_paths,
            rich_full: rich_full != 0,
        };
        let frame = Frame { css_w: css_w.max(200.0), css_h: css_h.max(260.0), dpr: dpr.clamp(1.0, 4.0) };
        let (pm, lay) = draw(&scene, frame, if dark != 0 { &theme::DARK } else { &theme::LIGHT });
        d.layout = Some((lay, lay.raster(frame.dpr)));
        d.size = (pm.width(), pm.height());
        d.pixels = pm.take();
        d.pixels.as_ptr()
    })
}

/// Width of the last rendered chart in device pixels.
#[no_mangle]
pub extern "C" fn render_width() -> u32 {
    RUN.with(|r| r.borrow().as_ref().and_then(|run| run.done.as_ref()).map_or(0, |d| d.size.0))
}

/// Height of the last rendered chart in device pixels.
#[no_mangle]
pub extern "C" fn render_height() -> u32 {
    RUN.with(|r| r.borrow().as_ref().and_then(|run| run.done.as_ref()).map_or(0, |d| d.size.1))
}

/// What the pixel under a CSS point shows: the round, the rounds sharing its column, and the
/// cell (log10 wealth, players) if the point is on the density.
pub struct Probe {
    pub round: u32,
    pub shared: (u32, u32),
    pub cell: Option<(f64, u32)>,
}

/// Applies the density's own ownership rules to the pixel under (x, y). In the main plot the
/// answer is the fullest cell among the rounds that own the column (what the pixel colour shows);
/// in the strip, the round with the most rich players.
pub fn probe(scene: &Scene<'_>, lay: &Layout, ras: &Raster, x: f32, y: f32) -> Option<Probe> {
    let col = ras.col_at(x)?;
    let r = scene.game.rounds;
    let window = r / ras.w.max(1) as u32 + 3;
    let guess = lay.round_at(x);
    let owners: Vec<u32> = (guess.saturating_sub(window)..=(guess + window).min(r))
        .filter(|&t| columns_of_round(lay, ras, t).contains(&col))
        .collect();
    let (&first, &last) = (owners.first()?, owners.last()?);
    let near = |t: u32| (lay.x_of(f64::from(t)) - x).abs();
    let lat = scene.game.lattice();
    if lay.contains(x, y) {
        let row = ras.row_at(y)?;
        let mut best: Option<(u32, f64, u32)> = None; // (round, level, players)
        for &t in &owners {
            for (k, n) in cells(scene, t) {
                if rows_of_cell(lay, ras, &lat, t, k).contains(&row) {
                    let better = match best {
                        None => true,
                        Some((bt, _, bn)) => n > bn || (n == bn && near(t) < near(bt)),
                    };
                    if better {
                        best = Some((t, lat.at(t, k), n));
                    }
                }
            }
        }
        let round =
            best.map_or_else(|| *owners.iter().min_by(|a, b| near(**a).total_cmp(&near(**b))).unwrap(), |b| b.0);
        return Some(Probe { round, shared: (first, last), cell: best.map(|b| (b.1, b.2)) });
    }
    if lay.in_strip(x, y) {
        let rich = &scene.ensemble.rich_now;
        let round = *owners
            .iter()
            .max_by(|a, b| rich[**a as usize].cmp(&rich[**b as usize]).then(near(**b).total_cmp(&near(**a))))
            .unwrap();
        return Some(Probe { round, shared: (first, last), cell: None });
    }
    None
}

/// Tooltip at CSS point (x, y) of the last rendered chart (JSON). Returns its length; the JSON
/// is `{"inside":false}` outside the plot and the strip.
#[no_mangle]
pub extern "C" fn hover(x: f32, y: f32) -> u32 {
    let s = RUN.with(|r| {
        let r = r.borrow();
        let run = r.as_ref()?;
        let d = run.done.as_ref()?;
        let (lay, ras) = d.layout?;
        let scene = Scene {
            game: &run.game,
            counts: &run.counts,
            ensemble: &d.ensemble,
            highlighted: &[],
            rich_paths: &[],
            rich_full: false,
        };
        Some(match probe(&scene, &lay, &ras, x, y) {
            None => "{\"inside\":false}".to_string(),
            Some(p) => view::hover_json(&run.game, &d.ensemble, &lay, &d.highlighted, &p),
        })
    });
    s.map_or(0, put)
}

/// The opening paragraph for the default game (used by the build to fill the page).
pub fn default_lede() -> String {
    view::lede(&Game::peters(1000, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> String {
        OUT.with(|o| String::from_utf8(o.borrow().clone()).unwrap())
    }

    /// Runs the page protocol natively with the given chunks.
    fn run_chunks(players: u32, rounds: u32, seed: u32, chunks: &[(u32, u32)]) {
        assert_eq!(run_begin(players, rounds, seed, 0), 0);
        for &(first, n) in chunks.iter().rev() {
            let mut counts = vec![0u32; density_len(rounds)];
            let mut fields = vec![0u32; FIELDS * n as usize];
            let rc = unsafe { sim_chunk(rounds, seed, 0, first / 64, n, counts.as_mut_ptr(), fields.as_mut_ptr()) };
            assert_eq!(rc, 0);
            unsafe {
                std::ptr::copy_nonoverlapping(counts.as_ptr(), run_slot(0, 0) as *mut u32, counts.len());
                run_add_scratch();
                for f in 0..FIELDS {
                    let src = &fields[f * n as usize..][..n as usize];
                    std::ptr::copy_nonoverlapping(src.as_ptr(), run_slot(f as u32 + 1, first) as *mut u32, n as usize);
                }
            }
        }
        assert_eq!(run_finish(1.0, 1), 0);
    }

    #[test]
    fn chunked_run_gives_the_same_view_as_one_chunk() {
        let mut views = Vec::new();
        for chunks in [vec![(0u32, 3001u32)], vec![(0, 1024), (1024, 1024), (2048, 953)]] {
            run_chunks(3001, 300, 2022, &chunks);
            view();
            views.push(text());
        }
        assert_eq!(views[0], views[1]);
        assert!(views[0].contains("\"tiles\""));
    }

    #[test]
    fn a_missing_chunk_is_detected() {
        assert_eq!(run_begin(200, 50, 1, 0), 0);
        assert_eq!(run_finish(0.0, 1), -1);
    }

    #[test]
    fn hover_reports_what_the_pixel_shows() {
        run_chunks(20_000, 1000, 7, &[(0, 20_000)]);
        for (w, h, dpr) in
            [(600.0f32, 420.0f32, 1.0f32), (1000.0, 560.0, 2.0), (733.0, 411.0, 1.25), (260.0, 320.0, 1.0)]
        {
            assert!(!render(w, h, dpr, 0, 0).is_null());
            RUN.with(|r| {
                let r = r.borrow();
                let run = r.as_ref().unwrap();
                let d = run.done.as_ref().unwrap();
                let (lay, ras) = d.layout.unwrap();
                let scene = Scene {
                    game: &run.game,
                    counts: &run.counts,
                    ensemble: &d.ensemble,
                    highlighted: &[],
                    rich_paths: &[],
                    rich_full: false,
                };
                let grid = coin_chart::density_grid(&scene, &lay, &ras);
                let mut checked = 0;
                for c in (0..ras.w).step_by(7) {
                    for row in (0..ras.h).step_by(5) {
                        // CSS point at the centre of device pixel (c, row).
                        let x = (ras.ox as f32 + c as f32 + 0.5) / dpr;
                        let y = (ras.oy as f32 + row as f32 + 0.5) / dpr;
                        let p = probe(&scene, &lay, &ras, x, y).expect("inside");
                        let shown = grid[row * ras.w + c];
                        assert_eq!(p.cell.map_or(0, |c| c.1), shown, "pixel ({c},{row}) at {w}x{h}@{dpr}");
                        checked += 1;
                    }
                }
                assert!(checked > 100);
            });
        }
    }

    #[test]
    fn render_and_hover_work_after_a_run() {
        run_chunks(1000, 100, 7, &[(0, 1000)]);
        assert!(!render(600.0, 400.0, 2.0, 1, 0).is_null());
        assert_eq!((render_width(), render_height()), (1200, 800));
        hover(300.0, 100.0);
        let h = text();
        assert!(h.starts_with("{\"inside\":true") && h.contains("Round"), "{h}");
        hover(1.0, 1.0);
        assert_eq!(text(), "{\"inside\":false}");
    }
}
