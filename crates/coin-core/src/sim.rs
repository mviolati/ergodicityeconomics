//! Simulation of every player's path.
//!
//! Players are processed in blocks of 64: one Philox call gives the 64 coins of a block at one
//! round. Per player we keep only integers on the lattice (see [`crate::Lattice`]): final heads,
//! the peak, the first round below the broke threshold and the largest fall, each as
//! (round, heads). The full path of any player can be re-created with [`path`].

use crate::{rng::coins, Game};

/// Players per Philox call. Ranges handed to [`simulate_range`] start at a multiple of this.
pub const BLOCK: u64 = 64;

/// Number of per-player fields in a [`Summary`].
pub const FIELDS: usize = 8;
/// Field index: heads after the last round.
pub const FINAL_K: usize = 0;
/// Field index: round of the highest wealth over rounds 0..=R (first one; 0 = never above start).
pub const PEAK_T: usize = 1;
/// Field index: heads at `PEAK_T`.
pub const PEAK_K: usize = 2;
/// Field index: first round with wealth below the broke threshold; 0 = never.
pub const BROKE_T: usize = 3;
/// Field index: round where the largest fall from a previous peak starts (that peak).
pub const FALL_FROM_T: usize = 4;
/// Field index: heads at `FALL_FROM_T`.
pub const FALL_FROM_K: usize = 5;
/// Field index: round where the largest fall ends (its lowest point). Equal to `FALL_FROM_T`
/// if the player never fell below a previous peak.
pub const FALL_TO_T: usize = 6;
/// Field index: heads at `FALL_TO_T`.
pub const FALL_TO_K: usize = 7;

/// Per-player results: [`FIELDS`] integer arrays, one entry per player (see the field indices).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub fields: [Vec<u32>; FIELDS],
}

impl Summary {
    /// A summary for `n` players, all zero.
    pub fn zeros(n: usize) -> Self {
        Summary { fields: std::array::from_fn(|_| vec![0; n]) }
    }

    /// Number of players.
    pub fn len(&self) -> usize {
        self.fields[0].len()
    }

    /// True if there are no players.
    pub fn is_empty(&self) -> bool {
        self.fields[0].is_empty()
    }

    /// Field `f` of player `i`.
    #[inline]
    pub fn get(&self, f: usize, i: usize) -> u32 {
        self.fields[f][i]
    }

    /// Mutable view of players `range`.
    pub fn slice_mut(&mut self, range: std::ops::Range<usize>) -> SummaryMut<'_> {
        let mut it = self.fields.iter_mut();
        SummaryMut(std::array::from_fn(|_| &mut it.next().expect("FIELDS arrays")[range.clone()]))
    }
}

/// Mutable view of a contiguous range of players in a [`Summary`].
pub struct SummaryMut<'a>(pub [&'a mut [u32]; FIELDS]);

impl<'a> SummaryMut<'a> {
    /// Number of players in the view.
    pub fn len(&self) -> usize {
        self.0[0].len()
    }

    /// True if the view is empty.
    pub fn is_empty(&self) -> bool {
        self.0[0].is_empty()
    }

    /// Splits into the first `n` players and the rest.
    pub fn split_at(self, n: usize) -> (SummaryMut<'a>, SummaryMut<'a>) {
        let mut heads: [&'a mut [u32]; FIELDS] = Default::default();
        let mut tails: [&'a mut [u32]; FIELDS] = Default::default();
        for (f, s) in self.0.into_iter().enumerate() {
            let (a, b) = s.split_at_mut(n);
            heads[f] = a;
            tails[f] = b;
        }
        (SummaryMut(heads), SummaryMut(tails))
    }
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
    let n = out.len();
    assert!(out.0.iter().all(|f| f.len() == n), "summary slices have different lengths");
    let lat = game.lattice();
    let out = out.0;

    const B: usize = BLOCK as usize;
    let mut done = 0usize;
    let mut block = first / BLOCK;
    while done < n {
        let m = (n - done).min(B);
        let mut k = [0u32; B];
        let mut peak = [lat.l0; B];
        let mut peak_at = [(0u32, 0u32); B];
        let mut fall = [0f64; B];
        let mut fall_at = [(0u32, 0u32, 0u32, 0u32); B];
        let mut broke_t = [0u32; B];
        for t in 1..=rounds {
            let bits = coins(game.seed, block, t);
            let base = lat.base(t);
            let row = &mut counts[(t as usize - 1) * width..t as usize * width];
            let lanes = k[..m]
                .iter_mut()
                .zip(peak[..m].iter_mut().zip(peak_at[..m].iter_mut()))
                .zip(fall[..m].iter_mut().zip(fall_at[..m].iter_mut()))
                .zip(broke_t[..m].iter_mut());
            for (j, (((kj, (pk, pa)), (fl, fa)), bt)) in lanes.enumerate() {
                *kj += ((bits >> j) & 1) as u32;
                // SAFETY: *kj <= t <= rounds and the row has rounds + 1 cells.
                unsafe { *row.get_unchecked_mut(*kj as usize) += 1 };
                let l = base + f64::from(*kj) * lat.gap;
                // Selects, not branches: the coin is unpredictable, a branch would mispredict.
                let up = l > *pk;
                *pk = if up { l } else { *pk };
                *pa = if up { (t, *kj) } else { *pa };
                // The fall from the current peak, from its (rounds, heads) only: the same fall
                // always gives the same number, so "largest" and "equal" are exact.
                let f = lat.fall(t - pa.0, *kj - pa.1);
                let deeper = f > *fl;
                *fl = if deeper { f } else { *fl };
                *fa = if deeper { (pa.0, pa.1, t, *kj) } else { *fa };
                *bt = if *bt == 0 && l < lat.broke { t } else { *bt };
            }
        }
        for j in 0..m {
            let i = done + j;
            let (ft, fk, tt, tk) = fall_at[j];
            for (f, v) in [
                (FINAL_K, k[j]),
                (PEAK_T, peak_at[j].0),
                (PEAK_K, peak_at[j].1),
                (BROKE_T, broke_t[j]),
                (FALL_FROM_T, ft),
                (FALL_FROM_K, fk),
                (FALL_TO_T, tt),
                (FALL_TO_K, tk),
            ] {
                out[f][i] = v;
            }
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
            let (head, tail) = rest.split_at(n);
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

    /// The definitions, player by player, from the re-created path.
    pub(crate) fn brute_force(game: &Game, players: usize) -> (Vec<u32>, Summary) {
        let lat = game.lattice();
        let width = game.rounds as usize + 1;
        let mut counts = vec![0u32; density_len(game.rounds)];
        let mut s = Summary::zeros(players);
        for i in 0..players {
            let p = path(game, i as u64);
            let (mut best, mut peak_at, mut broke) = (lat.at(0, 0), (0, 0), 0);
            let (mut fall, mut fall_at) = (0f64, (0, 0, 0, 0));
            for t in 1..=game.rounds {
                let k = p[t as usize];
                counts[(t as usize - 1) * width + k as usize] += 1;
                let l = lat.at(t, k);
                if l > best {
                    (best, peak_at) = (l, (t, k));
                }
                let f = lat.fall(t - peak_at.0, k - peak_at.1);
                if f > fall {
                    (fall, fall_at) = (f, (peak_at.0, peak_at.1, t, k));
                }
                if broke == 0 && l < lat.broke {
                    broke = t;
                }
            }
            let v = [p[game.rounds as usize], peak_at.0, peak_at.1, broke, fall_at.0, fall_at.1, fall_at.2, fall_at.3];
            for (f, x) in v.into_iter().enumerate() {
                s.fields[f][i] = x;
            }
        }
        (counts, s)
    }

    #[test]
    fn fast_simulation_equals_brute_force_from_paths() {
        for (rounds, players) in [(1, 5), (2, 70), (37, 130), (400, 1001)] {
            let game = Game::peters(rounds, 99);
            assert_eq!(simulate(&game, players), brute_force(&game, players), "rounds {rounds}");
        }
    }

    #[test]
    fn largest_fall_is_the_largest_drop_from_a_previous_peak() {
        let game = Game::peters(300, 5);
        let lat = game.lattice();
        let (_, s) = simulate(&game, 500);
        for i in 0..500 {
            let p = path(&game, i as u64);
            let ls: Vec<f64> = (0..=300u32).map(|t| lat.at(t, p[t as usize])).collect();
            let mut worst = 0f64;
            for b in 0..ls.len() {
                for a in 0..b {
                    worst = worst.max(ls[a] - ls[b]);
                }
            }
            let (ft, fk, tt, tk) =
                (s.get(FALL_FROM_T, i), s.get(FALL_FROM_K, i), s.get(FALL_TO_T, i), s.get(FALL_TO_K, i));
            let got = lat.at(ft, fk) - lat.at(tt, tk);
            assert!((got - worst).abs() < 1e-9, "player {i}: {got} vs {worst}");
            assert_eq!(p[ft as usize], fk);
            assert_eq!(p[tt as usize], tk);
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
        for f in 0..FIELDS {
            assert_eq!(small.fields[f][..], big.fields[f][..500], "field {f}");
        }
    }

    #[test]
    fn different_seeds_give_different_players() {
        let (_, a) = simulate(&Game::peters(100, 1), 256);
        let (_, b) = simulate(&Game::peters(100, 2), 256);
        let (_, c) = simulate(&Game::peters(100, 1 << 40), 256);
        assert_ne!(a.fields[FINAL_K], b.fields[FINAL_K]);
        assert_ne!(a.fields[FINAL_K], c.fields[FINAL_K], "the high 32 bits of the seed are used");
    }
}
