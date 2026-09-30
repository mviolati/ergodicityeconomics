//! Simulation of every player's path.
//!
//! Players are processed in blocks of 64: one Philox call gives the 64 coins of a block at one
//! round. Per player we keep only integers on the lattice (see [`crate::Lattice`]): final heads,
//! the round and heads of the peak, the first round below the broke threshold. The full path
//! of any player can be re-created with [`path`].

use crate::{rng::coins, Game};

/// Players per Philox call. Ranges handed to [`simulate_range`] start at a multiple of this.
pub const BLOCK: u64 = 64;

/// Per-player results, one entry per player (struct of arrays).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// Heads after the last round.
    pub final_k: Vec<u32>,
    /// Round of the highest wealth over rounds 0..=R (first one if tied; 0 = never above start).
    pub peak_t: Vec<u32>,
    /// Heads at `peak_t`.
    pub peak_k: Vec<u32>,
    /// First round with wealth below the broke threshold; 0 = never.
    pub broke_t: Vec<u32>,
    /// Largest fall from a previous peak, in log10 units (decades).
    pub drawdown: Vec<f64>,
}

impl Summary {
    /// A summary for `n` players, all zero.
    pub fn zeros(n: usize) -> Self {
        Summary {
            final_k: vec![0; n],
            peak_t: vec![0; n],
            peak_k: vec![0; n],
            broke_t: vec![0; n],
            drawdown: vec![0.0; n],
        }
    }

    /// Number of players.
    pub fn len(&self) -> usize {
        self.final_k.len()
    }

    /// True if there are no players.
    pub fn is_empty(&self) -> bool {
        self.final_k.is_empty()
    }

    /// Mutable view of players `range`.
    pub fn slice_mut(&mut self, range: std::ops::Range<usize>) -> SummaryMut<'_> {
        SummaryMut {
            final_k: &mut self.final_k[range.clone()],
            peak_t: &mut self.peak_t[range.clone()],
            peak_k: &mut self.peak_k[range.clone()],
            broke_t: &mut self.broke_t[range.clone()],
            drawdown: &mut self.drawdown[range],
        }
    }
}

/// Mutable view of a contiguous range of players in a [`Summary`].
pub struct SummaryMut<'a> {
    pub final_k: &'a mut [u32],
    pub peak_t: &'a mut [u32],
    pub peak_k: &'a mut [u32],
    pub broke_t: &'a mut [u32],
    pub drawdown: &'a mut [f64],
}

/// Cells of the density matrix: `counts[(t - 1) * (R + 1) + k]` = players with `k` heads after
/// round `t`, for `t` in 1..=R.
pub fn density_len(rounds: u32) -> usize {
    rounds as usize * (rounds as usize + 1)
}

/// Simulates the players `first .. first + out.len()`, where `first` is a multiple of
/// [`BLOCK`]. Adds their density to `counts` and writes their summaries to `out`.
pub fn simulate_range(game: &Game, first: u64, counts: &mut [u32], out: SummaryMut<'_>) {
    assert_eq!(first % BLOCK, 0, "a range must start at a block boundary");
    let rounds = game.rounds;
    let width = rounds as usize + 1;
    assert_eq!(counts.len(), density_len(rounds), "density matrix has the wrong size");
    let n = out.final_k.len();
    assert!(
        out.peak_t.len() == n && out.peak_k.len() == n && out.broke_t.len() == n && out.drawdown.len() == n,
        "summary slices have different lengths"
    );
    let lat = game.lattice();

    let mut done = 0usize;
    let mut block = first / BLOCK;
    while done < n {
        let m = (n - done).min(BLOCK as usize);
        let mut k = [0u32; BLOCK as usize];
        let mut peak = [lat.l0; BLOCK as usize];
        let mut peak_t = [0u32; BLOCK as usize];
        let mut peak_k = [0u32; BLOCK as usize];
        let mut drawdown = [0f64; BLOCK as usize];
        let mut broke_t = [0u32; BLOCK as usize];
        for t in 1..=rounds {
            let bits = coins(game.seed, block, t);
            let base = lat.base(t);
            let row = &mut counts[(t as usize - 1) * width..t as usize * width];
            let lanes = k[..m]
                .iter_mut()
                .zip(peak[..m].iter_mut())
                .zip(peak_t[..m].iter_mut().zip(peak_k[..m].iter_mut()))
                .zip(drawdown[..m].iter_mut().zip(broke_t[..m].iter_mut()));
            for (j, (((kj, pk), (pt, pkk)), (dd, bt))) in lanes.enumerate() {
                *kj += ((bits >> j) & 1) as u32;
                // SAFETY: *kj <= t <= rounds and the row has rounds + 1 cells.
                unsafe { *row.get_unchecked_mut(*kj as usize) += 1 };
                let l = base + f64::from(*kj) * lat.gap;
                // Selects, not branches: the coin is unpredictable, a branch would mispredict.
                let up = l > *pk;
                *pk = if up { l } else { *pk };
                *pt = if up { t } else { *pt };
                *pkk = if up { *kj } else { *pkk };
                *dd = dd.max(*pk - l);
                *bt = if *bt == 0 && l < lat.broke { t } else { *bt };
            }
        }
        for j in 0..m {
            let i = done + j;
            out.final_k[i] = k[j];
            out.peak_t[i] = peak_t[j];
            out.peak_k[i] = peak_k[j];
            out.broke_t[i] = broke_t[j];
            out.drawdown[i] = drawdown[j];
        }
        done += m;
        block += 1;
    }
}

/// Simulates `players` players on one thread.
pub fn simulate(game: &Game, players: usize) -> (Vec<u32>, Summary) {
    let mut counts = vec![0u32; density_len(game.rounds)];
    let mut summary = Summary::zeros(players);
    simulate_range(game, 0, &mut counts, summary.slice_mut(0..players));
    (counts, summary)
}

/// Simulates `players` players on `threads` threads. The result does not depend on `threads`.
#[cfg(not(target_arch = "wasm32"))]
pub fn simulate_parallel(game: &Game, players: usize, threads: usize) -> (Vec<u32>, Summary) {
    let threads = threads.max(1);
    let blocks = players.div_ceil(BLOCK as usize);
    let per = blocks.div_ceil(threads).max(1) * BLOCK as usize;
    let mut summary = Summary::zeros(players);
    let mut counts = vec![0u32; density_len(game.rounds)];
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        let mut rest = summary.slice_mut(0..players);
        let mut first = 0usize;
        while first < players {
            let n = per.min(players - first);
            let (head, tail) = split(rest, n);
            rest = tail;
            let start = first as u64;
            handles.push(scope.spawn(move || {
                let mut local = vec![0u32; density_len(game.rounds)];
                simulate_range(game, start, &mut local, head);
                local
            }));
            first += n;
        }
        for h in handles {
            for (c, l) in counts.iter_mut().zip(h.join().expect("simulation thread panicked")) {
                *c += l;
            }
        }
    });
    (counts, summary)
}

#[cfg(not(target_arch = "wasm32"))]
fn split(s: SummaryMut<'_>, n: usize) -> (SummaryMut<'_>, SummaryMut<'_>) {
    let (a0, b0) = s.final_k.split_at_mut(n);
    let (a1, b1) = s.peak_t.split_at_mut(n);
    let (a2, b2) = s.peak_k.split_at_mut(n);
    let (a3, b3) = s.broke_t.split_at_mut(n);
    let (a4, b4) = s.drawdown.split_at_mut(n);
    (
        SummaryMut { final_k: a0, peak_t: a1, peak_k: a2, broke_t: a3, drawdown: a4 },
        SummaryMut { final_k: b0, peak_t: b1, peak_k: b2, broke_t: b3, drawdown: b4 },
    )
}

/// Heads of player `id` after each round: `out[t]` for t in 0..=R (`out[0] = 0`).
pub fn path(game: &Game, id: u64) -> Vec<u32> {
    let (block, lane) = (id / BLOCK, id % BLOCK);
    let mut out = Vec::with_capacity(game.rounds as usize + 1);
    let mut k = 0u32;
    out.push(0);
    for t in 1..=game.rounds {
        k += ((coins(game.seed, block, t) >> lane) & 1) as u32;
        out.push(k);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute_force(game: &Game, players: usize) -> (Vec<u32>, Summary) {
        let lat = game.lattice();
        let width = game.rounds as usize + 1;
        let mut counts = vec![0u32; density_len(game.rounds)];
        let mut s = Summary::zeros(players);
        for i in 0..players {
            let p = path(game, i as u64);
            let (mut best, mut bt, mut bk, mut dd, mut broke) = (lat.at(0, 0), 0, 0, 0f64, 0);
            for t in 1..=game.rounds {
                let k = p[t as usize];
                counts[(t as usize - 1) * width + k as usize] += 1;
                let l = lat.at(t, k);
                if l > best {
                    (best, bt, bk) = (l, t, k);
                }
                dd = dd.max(best - l);
                if broke == 0 && l < lat.broke {
                    broke = t;
                }
            }
            s.final_k[i] = p[game.rounds as usize];
            (s.peak_t[i], s.peak_k[i], s.broke_t[i], s.drawdown[i]) = (bt, bk, broke, dd);
        }
        (counts, s)
    }

    #[test]
    fn fast_simulation_equals_brute_force_from_paths() {
        for (rounds, players) in [(1, 5), (37, 130), (400, 1001)] {
            let game = Game::peters(rounds, 99);
            assert_eq!(simulate(&game, players), brute_force(&game, players), "rounds {rounds}");
        }
    }

    #[test]
    fn result_does_not_depend_on_threads() {
        let game = Game::peters(300, 7);
        let one = simulate(&game, 3001);
        for threads in [2, 3, 4, 7] {
            assert_eq!(simulate_parallel(&game, 3001, threads), one, "threads {threads}");
        }
    }

    #[test]
    fn adding_players_keeps_the_first_ones() {
        let game = Game::peters(200, 3);
        let (_, small) = simulate(&game, 500);
        let (_, big) = simulate(&game, 2000);
        assert_eq!(small.final_k[..], big.final_k[..500]);
        assert_eq!(small.peak_t[..], big.peak_t[..500]);
        assert_eq!(small.broke_t[..], big.broke_t[..500]);
    }

    #[test]
    fn different_seeds_give_different_players() {
        let (_, a) = simulate(&Game::peters(100, 1), 256);
        let (_, b) = simulate(&Game::peters(100, 2), 256);
        let (_, c) = simulate(&Game::peters(100, 1 << 40), 256);
        assert_ne!(a.final_k, b.final_k);
        assert_ne!(a.final_k, c.final_k, "the high 32 bits of the seed are used");
    }
}
