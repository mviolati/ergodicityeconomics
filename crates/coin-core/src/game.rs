//! The game and its wealth lattice.
//!
//! After `t` rounds with `k` heads a player has `start * win^k * lose^(t-k)`. In log10 this is
//! a lattice: `log10(start) + t * log10(lose) + k * (log10(win) - log10(lose))`. The whole program
//! stores only the integers `(t, k)` and converts them with [`Lattice::at`], so every number on
//! screen comes from one formula.

/// Parameters of one run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Game {
    /// Wealth factor on heads (1.5 = +50%).
    pub win: f64,
    /// Wealth factor on tails (0.6 = -40%).
    pub lose: f64,
    /// Starting wealth, EUR.
    pub start: f64,
    /// "Rich" threshold, EUR (reached = wealth >= rich).
    pub rich: f64,
    /// "Broke" threshold, EUR (fallen = wealth < broke).
    pub broke: f64,
    /// Number of rounds.
    pub rounds: u32,
    /// Seed of the coins.
    pub seed: u64,
}

impl Game {
    /// Largest supported number of rounds (the density matrix has rounds * (rounds + 1) cells).
    pub const MAX_ROUNDS: u32 = 2000;

    /// Peters' example: +50% / -40%, 100 EUR start, thresholds 1 billion EUR and 1 EUR.
    pub fn peters(rounds: u32, seed: u64) -> Self {
        Game { win: 1.5, lose: 0.6, start: 100.0, rich: 1e9, broke: 1.0, rounds, seed }
    }

    /// Checks that the parameters describe a game this program can simulate and show.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.rounds >= 1 && self.rounds <= Self::MAX_ROUNDS) {
            return Err(format!("rounds must be in 1..={}", Self::MAX_ROUNDS));
        }
        let finite = [self.win, self.lose, self.start, self.rich, self.broke];
        if finite.iter().any(|x| !x.is_finite() || *x <= 0.0) {
            return Err("win, lose, start, rich and broke must be positive and finite".into());
        }
        if self.win <= self.lose {
            return Err("win must be larger than lose".into());
        }
        if !(self.broke <= self.start && self.start < self.rich) {
            return Err("thresholds must satisfy broke <= start < rich".into());
        }
        // Threshold tests are done in f64. They are exact only if no reachable wealth is within
        // rounding error of a threshold, so reject games where one is.
        let lat = self.lattice();
        for t in 1..=self.rounds {
            for k in 0..=t {
                let l = lat.at(t, k);
                if [lat.rich, lat.broke, lat.l0].iter().any(|thr| (l - thr).abs() <= 1e-9) {
                    return Err(format!("a reachable wealth (round {t}, {k} heads) is on a threshold"));
                }
            }
        }
        Ok(())
    }

    /// The log10 lattice of this game.
    pub fn lattice(&self) -> Lattice {
        let lb = self.lose.log10();
        let lw = self.win.log10();
        Lattice {
            l0: self.start.log10(),
            lb,
            lw,
            gap: lw - lb,
            rich: self.rich.log10(),
            broke: self.broke.log10(),
            expected_step: (0.5 * (self.win + self.lose)).log10(),
        }
    }
}

/// log10 wealth lattice and thresholds, precomputed once per run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lattice {
    /// log10 of the starting wealth.
    pub l0: f64,
    /// log10 of the tails factor.
    pub lb: f64,
    /// log10 of the heads factor.
    pub lw: f64,
    /// Distance between neighbour lattice points: log10(win / lose).
    pub gap: f64,
    /// log10 of the rich threshold.
    pub rich: f64,
    /// log10 of the broke threshold.
    pub broke: f64,
    /// log10 of the expected factor per round, (win + lose) / 2.
    pub expected_step: f64,
}

impl Lattice {
    /// log10 wealth with no heads after `t` rounds. `at(t, k) == base(t) + k * gap`, bit for bit.
    #[inline(always)]
    pub fn base(&self, t: u32) -> f64 {
        self.l0 + f64::from(t) * self.lb
    }

    /// log10 wealth (EUR) after `t` rounds with `k` heads. The one conversion used everywhere.
    #[inline(always)]
    pub fn at(&self, t: u32, k: u32) -> f64 {
        self.base(t) + f64::from(k) * self.gap
    }

    /// Size of a fall, in decades (log10 units), over `rounds` rounds with `heads` heads:
    /// `-(heads * log10(win) + tails * log10(lose))`. It depends only on (rounds, heads), so the
    /// same fall always gives the same number wherever it happens.
    #[inline(always)]
    pub fn fall(&self, rounds: u32, heads: u32) -> f64 {
        f64::from(rounds - heads) * -self.lb - f64::from(heads) * self.lw
    }

    /// log10 of the expected wealth after `t` rounds.
    pub fn expected(&self, t: u32) -> f64 {
        self.l0 + f64::from(t) * self.expected_step
    }

    /// True if the lattice point is at or above the rich threshold.
    #[inline(always)]
    pub fn is_rich(&self, t: u32, k: u32) -> bool {
        self.at(t, k) >= self.rich
    }

    /// True if the lattice point is below the broke threshold.
    #[inline(always)]
    pub fn is_broke(&self, t: u32, k: u32) -> bool {
        self.at(t, k) < self.broke
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Threshold tests are done in f64. They are exact only if no lattice point lies within
    /// rounding error of a threshold. Check every lattice point the program can produce.
    #[test]
    fn no_lattice_point_is_ambiguous_at_a_threshold() {
        let lat = Game::peters(Game::MAX_ROUNDS, 0).lattice();
        let mut closest = f64::INFINITY;
        for t in 1..=Game::MAX_ROUNDS {
            for k in 0..=t {
                let l = lat.at(t, k);
                for thr in [lat.rich, lat.broke, lat.l0] {
                    closest = closest.min((l - thr).abs());
                }
            }
        }
        // f64 error of `at` is below 1e-12 for |l| < 1000; demand a margin 1000 times larger.
        assert!(closest > 1e-9, "a lattice point is {closest:e} from a threshold");
    }

    /// Comparisons between lattice points (the peak, the ranking of players) are exact on every
    /// platform only if two different points are never within rounding error of each other.
    /// Two points of different rounds or heads always differ; check by how much.
    #[test]
    fn distinct_lattice_points_are_far_apart() {
        let lat = Game::peters(Game::MAX_ROUNDS, 0).lattice();
        let mut all: Vec<f64> = (0..=Game::MAX_ROUNDS).flat_map(|t| (0..=t).map(move |k| lat.at(t, k))).collect();
        all.sort_by(f64::total_cmp);
        let closest = all.windows(2).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min);
        assert!(closest > 1e-9, "two lattice points are only {closest:e} apart");
    }

    /// Falls are compared as numbers from [`Lattice::fall`]. Two different falls must never be
    /// within rounding error of each other.
    #[test]
    fn distinct_falls_are_far_apart() {
        let lat = Game::peters(Game::MAX_ROUNDS, 0).lattice();
        let mut all: Vec<f64> = (0..=Game::MAX_ROUNDS).flat_map(|r| (0..=r).map(move |h| lat.fall(r, h))).collect();
        all.sort_by(f64::total_cmp);
        let closest = all.windows(2).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min);
        assert!(closest > 1e-9, "two different falls are only {closest:e} apart");
    }

    #[test]
    fn base_plus_gap_is_the_same_number_as_at() {
        let lat = Game::peters(1000, 0).lattice();
        for t in [1, 7, 500, 1000] {
            for k in 0..=t {
                assert_eq!((lat.base(t) + f64::from(k) * lat.gap).to_bits(), lat.at(t, k).to_bits());
            }
        }
    }

    #[test]
    fn validate_rejects_bad_games() {
        assert!(Game::peters(1000, 1).validate().is_ok());
        assert!(Game::peters(0, 1).validate().is_err());
        assert!(Game::peters(Game::MAX_ROUNDS + 1, 1).validate().is_err());
        assert!(Game { win: 0.5, ..Game::peters(10, 1) }.validate().is_err());
        assert!(Game { broke: 200.0, ..Game::peters(10, 1) }.validate().is_err());
        // 100 * 1.5^2 = 225 EUR is reachable after 2 heads: a threshold there is ambiguous.
        assert!(Game { rich: 225.0, ..Game::peters(10, 1) }.validate().is_err());
        assert!(Game { rich: 225.0, ..Game::peters(1, 1) }.validate().is_ok(), "not reachable in 1 round");
        assert!(Game { rich: 1e4, ..Game::peters(500, 1) }.validate().is_ok());
    }
}
