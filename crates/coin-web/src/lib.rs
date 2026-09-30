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

use coin_chart::{columns_of_round, render as draw, rows_of_cell, theme, Frame, Layout, Scene};
use coin_core::{
    ensemble::{ensemble, Ensemble},
    sim::{density_len, path, simulate_range, Summary, SummaryMut, BLOCK},
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
    highlighted: Vec<(u64, Vec<f64>)>,
    rich_paths: Vec<Vec<f64>>,
    layout: Option<(Layout, f32, usize, usize)>,
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

/// Worker side: simulates players `first_block * 64 .. + n` and writes into caller buffers.
///
/// # Safety
/// Every pointer must address a buffer of the right length: `counts` rounds*(rounds+1) u32,
/// the four u32 arrays and `drawdown` (f64) n elements each.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sim_chunk(
    rounds: u32,
    seed_lo: u32,
    seed_hi: u32,
    first_block: u32,
    n: u32,
    counts: *mut u32,
    final_k: *mut u32,
    peak_t: *mut u32,
    peak_k: *mut u32,
    broke_t: *mut u32,
    drawdown: *mut f64,
) -> i32 {
    let game = game_of(rounds, seed_lo, seed_hi);
    if game.validate().is_err() {
        return -1;
    }
    let n = n as usize;
    let c = std::slice::from_raw_parts_mut(counts, density_len(rounds));
    c.fill(0);
    let out = SummaryMut {
        final_k: std::slice::from_raw_parts_mut(final_k, n),
        peak_t: std::slice::from_raw_parts_mut(peak_t, n),
        peak_k: std::slice::from_raw_parts_mut(peak_k, n),
        broke_t: std::slice::from_raw_parts_mut(broke_t, n),
        drawdown: std::slice::from_raw_parts_mut(drawdown, n),
    };
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

/// Where to copy a worker's array: field 0 = density (scratch, then `run_add_scratch`),
/// 1 final_k, 2 peak_t, 3 peak_k, 4 broke_t (u32), 5 drawdown (f64), at player `first`.
/// Returns null for a bad field or offset.
#[no_mangle]
pub extern "C" fn run_slot(field: u32, first: u32) -> *mut u8 {
    RUN.with(|r| {
        let mut r = r.borrow_mut();
        let Some(run) = r.as_mut() else { return std::ptr::null_mut() };
        let i = first as usize;
        if field > 0 && i >= run.summary.len() {
            return std::ptr::null_mut();
        }
        let s = &mut run.summary;
        match field {
            0 => run.scratch.as_mut_ptr() as *mut u8,
            1 => s.final_k[i..].as_mut_ptr() as *mut u8,
            2 => s.peak_t[i..].as_mut_ptr() as *mut u8,
            3 => s.peak_k[i..].as_mut_ptr() as *mut u8,
            4 => s.broke_t[i..].as_mut_ptr() as *mut u8,
            5 => s.drawdown[i..].as_mut_ptr() as *mut u8,
            _ => std::ptr::null_mut(),
        }
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
        let highlighted = pk.iter().map(|p| (p.id, log_path(p.id))).collect();
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
        Some(view::view_json(
            &run.game,
            &run.summary,
            &d.ensemble,
            &d.stats,
            &d.picks,
            d.rich_paths.len(),
            run.sim_ms,
            run.threads,
        ))
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
        let hl: Vec<(Vec<f64>, usize)> = d.highlighted.iter().enumerate().map(|(i, (_, p))| (p.clone(), i)).collect();
        let scene = Scene {
            game: &run.game,
            counts: &run.counts,
            ensemble: &d.ensemble,
            highlighted: &hl,
            rich_paths: &d.rich_paths,
            rich_full: rich_full != 0,
        };
        let frame = Frame { css_w: css_w.max(200.0), css_h: css_h.max(200.0), dpr: dpr.clamp(1.0, 4.0) };
        let (pm, lay) = draw(&scene, frame, if dark != 0 { &theme::DARK } else { &theme::LIGHT });
        let dev_w = (lay.w * frame.dpr).round().max(1.0) as usize;
        let dev_h = (lay.h * frame.dpr).round().max(1.0) as usize;
        d.layout = Some((lay, frame.dpr, dev_w, dev_h));
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

/// Tooltip at CSS point (x, y) of the last rendered chart (JSON). Returns its length; the JSON
/// is `{"inside":false}` outside the plot.
#[no_mangle]
pub extern "C" fn hover(x: f32, y: f32) -> u32 {
    let s = RUN.with(|r| {
        let r = r.borrow();
        let run = r.as_ref()?;
        let d = run.done.as_ref()?;
        let (lay, dpr, dev_w, dev_h) = d.layout?;
        if !lay.contains(x, y) {
            return Some("{\"inside\":false}".to_string());
        }
        let game = &run.game;
        let hl: Vec<(Vec<f64>, usize)> = Vec::new();
        let scene = Scene {
            game,
            counts: &run.counts,
            ensemble: &d.ensemble,
            highlighted: &hl,
            rich_paths: &[],
            rich_full: false,
        };
        // The device pixel under the pointer, and the rounds that own its column.
        let col = (((x - lay.x) * dpr).floor().max(0.0) as usize).min(dev_w - 1);
        let row = (((y - lay.y) * dpr).floor().max(0.0) as usize).min(dev_h - 1);
        let guess = lay.round_at(x);
        let lo = guess.saturating_sub(8);
        let hi = (guess + 8).min(game.rounds);
        let owners: Vec<u32> = (lo..=hi).filter(|&t| columns_of_round(&lay, dpr, dev_w, t).contains(&col)).collect();
        let round =
            owners.iter().copied().min_by_key(|&t| (lay.x_of(f64::from(t)) - x).abs().to_bits()).unwrap_or(guess);
        let shared = match (owners.first(), owners.last()) {
            (Some(&a), Some(&b)) => Some((a, b)),
            _ => None,
        };
        // The cell of that round that owns the pixel row, if any.
        let lat = game.lattice();
        let width = game.rounds as usize + 1;
        let cell = if round == 0 {
            rows_of_cell(&scene, &lay, dpr, dev_h, 0, 0).contains(&row).then_some((lat.l0, d.ensemble.players as u32))
        } else {
            let counts = &run.counts[(round as usize - 1) * width..][..round as usize + 1];
            (0..=round)
                .find(|&k| counts[k as usize] > 0 && rows_of_cell(&scene, &lay, dpr, dev_h, round, k).contains(&row))
                .map(|k| (lat.at(round, k), counts[k as usize]))
        };
        Some(view::hover_json(game, &d.ensemble, &lay, &d.highlighted, round, shared, cell))
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

    /// The protocol the page uses, run natively: chunks in any order give the same page.
    #[test]
    fn chunked_run_gives_the_same_view_as_one_chunk() {
        let (players, rounds) = (3001u32, 300u32);
        let mut views = Vec::new();
        for chunks in [vec![(0u32, 3001u32)], vec![(0, 1024), (1024, 1024), (2048, 953)]] {
            assert_eq!(run_begin(players, rounds, 2022, 0), 0);
            for &(first, n) in chunks.iter().rev() {
                let mut counts = vec![0u32; density_len(rounds)];
                let mut s = Summary::zeros(n as usize);
                let r = unsafe {
                    sim_chunk(
                        rounds,
                        2022,
                        0,
                        first / 64,
                        n,
                        counts.as_mut_ptr(),
                        s.final_k.as_mut_ptr(),
                        s.peak_t.as_mut_ptr(),
                        s.peak_k.as_mut_ptr(),
                        s.broke_t.as_mut_ptr(),
                        s.drawdown.as_mut_ptr(),
                    )
                };
                assert_eq!(r, 0);
                unsafe {
                    std::ptr::copy_nonoverlapping(counts.as_ptr(), run_slot(0, 0) as *mut u32, counts.len());
                    std::ptr::copy_nonoverlapping(s.final_k.as_ptr(), run_slot(1, first) as *mut u32, n as usize);
                    std::ptr::copy_nonoverlapping(s.peak_t.as_ptr(), run_slot(2, first) as *mut u32, n as usize);
                    std::ptr::copy_nonoverlapping(s.peak_k.as_ptr(), run_slot(3, first) as *mut u32, n as usize);
                    std::ptr::copy_nonoverlapping(s.broke_t.as_ptr(), run_slot(4, first) as *mut u32, n as usize);
                    std::ptr::copy_nonoverlapping(s.drawdown.as_ptr(), run_slot(5, first) as *mut f64, n as usize);
                }
                run_add_scratch();
            }
            assert_eq!(run_finish(1.0, 1), 0);
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
    fn render_and_hover_work_after_a_run() {
        assert_eq!(run_begin(1000, 100, 7, 0), 0);
        let mut counts = vec![0u32; density_len(100)];
        let mut s = Summary::zeros(1000);
        unsafe {
            sim_chunk(
                100,
                7,
                0,
                0,
                1000,
                counts.as_mut_ptr(),
                s.final_k.as_mut_ptr(),
                s.peak_t.as_mut_ptr(),
                s.peak_k.as_mut_ptr(),
                s.broke_t.as_mut_ptr(),
                s.drawdown.as_mut_ptr(),
            );
            std::ptr::copy_nonoverlapping(counts.as_ptr(), run_slot(0, 0) as *mut u32, counts.len());
            std::ptr::copy_nonoverlapping(s.final_k.as_ptr(), run_slot(1, 0) as *mut u32, 1000);
            std::ptr::copy_nonoverlapping(s.peak_t.as_ptr(), run_slot(2, 0) as *mut u32, 1000);
            std::ptr::copy_nonoverlapping(s.peak_k.as_ptr(), run_slot(3, 0) as *mut u32, 1000);
            std::ptr::copy_nonoverlapping(s.broke_t.as_ptr(), run_slot(4, 0) as *mut u32, 1000);
            std::ptr::copy_nonoverlapping(s.drawdown.as_ptr(), run_slot(5, 0) as *mut f64, 1000);
        }
        run_add_scratch();
        assert_eq!(run_finish(0.0, 1), 0);
        assert!(!render(600.0, 400.0, 2.0, 1, 0).is_null());
        assert_eq!((render_width(), render_height()), (1200, 800));
        hover(300.0, 200.0);
        let h = text();
        assert!(h.starts_with("{\"inside\":true") && h.contains("Round"), "{h}");
        hover(1.0, 1.0);
        assert_eq!(text(), "{\"inside\":false}");
    }
}
