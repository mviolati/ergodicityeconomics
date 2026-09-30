//! Round-by-round ensemble quantities, exact from the density matrix.

use crate::{sim::density_len, Game};

/// Lines and counts per round, index `t` in 0..=R. Wealth values are log10 EUR.
#[derive(Clone, Debug, PartialEq)]
pub struct Ensemble {
    /// Number of players.
    pub players: u64,
    /// log10 of the average wealth of all players.
    pub mean: Vec<f64>,
    /// log10 wealth of the median player: the player of rank ceil(P / 2), poorest first.
    pub median: Vec<f64>,
    /// log10 of the expected wealth, start * ((win + lose) / 2)^t.
    pub expected: Vec<f64>,
    /// Players at or above the rich threshold at round t.
    pub rich_now: Vec<u64>,
    /// Players below the broke threshold at round t.
    pub broke_now: Vec<u64>,
    /// Largest number of players in one cell (round, heads), round 0 included (all players).
    pub max_cell: u32,
}

/// Computes the ensemble lines from `counts` (see [`crate::sim::density_len`]).
pub fn ensemble(game: &Game, players: u64, counts: &[u32]) -> Ensemble {
    assert_eq!(counts.len(), density_len(game.rounds));
    assert!(players > 0, "no players");
    let lat = game.lattice();
    let r = game.rounds as usize;
    let mut e = Ensemble {
        players,
        mean: vec![lat.l0; r + 1],
        median: vec![lat.l0; r + 1],
        expected: (0..=game.rounds).map(|t| lat.expected(t)).collect(),
        rich_now: vec![0; r + 1],
        broke_now: vec![0; r + 1],
        max_cell: counts.iter().copied().max().unwrap_or(0).max(players.min(u64::from(u32::MAX)) as u32),
    };
    let rank = players.div_ceil(2);
    for t in 1..=game.rounds {
        let row = &counts[(t as usize - 1) * (r + 1)..][..t as usize + 1];
        let top = (0..=t).rev().find(|&k| row[k as usize] > 0).expect("every round holds every player");
        let peak = lat.at(t, top);
        let (mut sum, mut seen, mut median) = (0f64, 0u64, None);
        for k in 0..=t {
            let n = u64::from(row[k as usize]);
            if n == 0 {
                continue;
            }
            let l = lat.at(t, k);
            // log-sum-exp: shift by the largest term so that nothing overflows.
            sum += n as f64 * 10f64.powf(l - peak);
            seen += n;
            if median.is_none() && seen >= rank {
                median = Some(l);
            }
            if lat.is_rich(t, k) {
                e.rich_now[t as usize] += n;
            }
            if lat.is_broke(t, k) {
                e.broke_now[t as usize] += n;
            }
        }
        assert_eq!(seen, players, "round {t} does not hold every player");
        e.mean[t as usize] = peak + (sum / players as f64).log10();
        e.median[t as usize] = median.expect("rank <= players");
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{path, simulate};

    #[test]
    fn lines_equal_a_direct_computation_over_players() {
        let game = Game::peters(250, 5);
        let players = 777;
        let (counts, _) = simulate(&game, players);
        let e = ensemble(&game, players as u64, &counts);
        let lat = game.lattice();
        let paths: Vec<Vec<u32>> = (0..players as u64).map(|i| path(&game, i)).collect();
        for t in [1u32, 2, 50, 249, 250] {
            let mut ls: Vec<f64> = paths.iter().map(|p| lat.at(t, p[t as usize])).collect();
            ls.sort_by(f64::total_cmp);
            assert_eq!(e.median[t as usize], ls[players.div_ceil(2) - 1], "median t={t}");
            let mean: f64 = ls.iter().map(|l| 10f64.powf(*l)).sum::<f64>() / players as f64;
            assert!((e.mean[t as usize] - mean.log10()).abs() < 1e-12, "mean t={t}");
            let rich = ls.iter().filter(|l| **l >= lat.rich).count() as u64;
            let broke = ls.iter().filter(|l| **l < lat.broke).count() as u64;
            assert_eq!((e.rich_now[t as usize], e.broke_now[t as usize]), (rich, broke), "t={t}");
        }
        assert_eq!(e.expected[10], lat.l0 + 10.0 * (1.05f64).log10());
    }
}
