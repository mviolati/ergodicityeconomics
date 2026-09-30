//! Headline figures and the players the chart singles out.

use crate::{ensemble::Ensemble, sim::Summary, Game};

/// Headline figures of one run. All counts are players.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    pub players: u64,
    /// Players whose wealth was at or above the rich threshold in at least one round.
    pub rich_ever: u64,
    /// Of `rich_ever`: players below the broke threshold after the last round.
    pub rich_ever_broke_at_end: u64,
    /// Largest number of players at or above the rich threshold in the same round.
    pub rich_most_at_once: u64,
    /// First round where `rich_most_at_once` happens (0 if nobody was ever rich).
    pub rich_most_at_once_round: u32,
    /// Players at or above the rich threshold after the last round.
    pub rich_at_end: u64,
    /// Players never below the broke threshold.
    pub never_broke: u64,
    /// Players with more than the starting wealth after the last round.
    pub above_start_at_end: u64,
}

/// Computes the headline figures.
pub fn stats(game: &Game, s: &Summary, e: &Ensemble) -> Stats {
    let lat = game.lattice();
    let r = game.rounds;
    let mut st = Stats {
        players: s.len() as u64,
        rich_ever: 0,
        rich_ever_broke_at_end: 0,
        rich_most_at_once: 0,
        rich_most_at_once_round: 0,
        rich_at_end: 0,
        never_broke: 0,
        above_start_at_end: 0,
    };
    for i in 0..s.len() {
        let fin = lat.at(r, s.final_k[i]);
        if lat.is_rich(s.peak_t[i], s.peak_k[i]) {
            st.rich_ever += 1;
            if fin < lat.broke {
                st.rich_ever_broke_at_end += 1;
            }
        }
        st.rich_at_end += u64::from(fin >= lat.rich);
        st.never_broke += u64::from(s.broke_t[i] == 0);
        st.above_start_at_end += u64::from(fin > lat.l0);
    }
    for (t, &n) in e.rich_now.iter().enumerate() {
        if n > st.rich_most_at_once {
            st.rich_most_at_once = n;
            st.rich_most_at_once_round = t as u32;
        }
    }
    st
}

/// Why a player is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Highest wealth after the last round.
    RichestAtEnd,
    /// Reached the rich threshold, ended below the broke threshold (the lowest such ending).
    RichThenBroke,
    /// Reached the rich threshold; the lowest ending among them, still at or above broke.
    RichThenLowest,
    /// Nobody reached the rich threshold: the largest fall from a peak.
    BiggestFall,
    /// Earliest round below the broke threshold.
    FirstBroke,
}

/// A player the chart singles out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    pub id: u64,
    pub role: Role,
    /// Other players with exactly the same score for this role (the lowest id is shown).
    pub ties: u64,
}

/// Picks up to three different players. Every role is decided over ALL players, so its name is
/// true as written: the richest at the end; among players who reached the rich threshold, the one
/// who ended lowest (or, if nobody got rich, the largest fall from a peak); the first to fall
/// below the broke threshold. Ties go to the lowest id not already shown; `ties` counts the
/// other players with the same score. A role whose winners are all already shown is left out.
pub fn picks(game: &Game, s: &Summary) -> Vec<Pick> {
    let lat = game.lattice();
    let n = s.len();
    let mut out: Vec<Pick> = Vec::new();

    // All players with the largest key, in id order.
    let winners = |key: &dyn Fn(usize) -> Option<i128>| -> Vec<u64> {
        let mut top: Option<i128> = None;
        let mut ids = Vec::new();
        for i in 0..n {
            let Some(v) = key(i) else { continue };
            match top {
                Some(w) if v < w => {}
                Some(w) if v == w => ids.push(i as u64),
                _ => {
                    top = Some(v);
                    ids.clear();
                    ids.push(i as u64);
                }
            }
        }
        ids
    };
    let take = |ids: Vec<u64>, role: &dyn Fn(u64) -> Role, out: &mut Vec<Pick>| -> bool {
        match ids.iter().find(|id| out.iter().all(|p| p.id != **id)) {
            Some(&id) => {
                out.push(Pick { id, role: role(id), ties: ids.len() as u64 - 1 });
                true
            }
            None => false,
        }
    };

    take(winners(&|i| Some(i128::from(s.final_k[i]))), &|_| Role::RichestAtEnd, &mut out);
    let rich = |i: usize| lat.is_rich(s.peak_t[i], s.peak_k[i]);
    let lowest_rich = winners(&|i| rich(i).then(|| -i128::from(s.final_k[i])));
    let broke_at_end = |id: u64| lat.is_broke(game.rounds, s.final_k[id as usize]);
    if lowest_rich.is_empty() {
        // Drawdown is an f64 >= 0; for such values the bit pattern orders like the value.
        let fall = winners(&|i| (s.drawdown[i] > 0.0).then(|| i128::from(s.drawdown[i].to_bits())));
        take(fall, &|_| Role::BiggestFall, &mut out);
    } else {
        let role = |id: u64| if broke_at_end(id) { Role::RichThenBroke } else { Role::RichThenLowest };
        take(lowest_rich, &role, &mut out);
    }
    take(winners(&|i| (s.broke_t[i] > 0).then(|| -i128::from(s.broke_t[i]))), &|_| Role::FirstBroke, &mut out);
    out
}

/// Ids of all players who reached the rich threshold, in id order.
pub fn rich_ids(game: &Game, s: &Summary) -> Vec<u64> {
    let lat = game.lattice();
    (0..s.len()).filter(|&i| lat.is_rich(s.peak_t[i], s.peak_k[i])).map(|i| i as u64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ensemble::ensemble, sim::simulate};

    #[test]
    fn stats_equal_a_direct_count_over_paths() {
        let game = Game::peters(600, 2022);
        let players = 20_000;
        let (counts, s) = simulate(&game, players);
        let e = ensemble(&game, players as u64, &counts);
        let st = stats(&game, &s, &e);
        let lat = game.lattice();
        let (mut rich, mut rich_broke, mut never, mut up) = (0, 0, 0, 0);
        for i in 0..players as u64 {
            let p = crate::sim::path(&game, i);
            let ls: Vec<f64> = (0..=game.rounds).map(|t| lat.at(t, p[t as usize])).collect();
            let fin = *ls.last().unwrap();
            if ls.iter().any(|l| *l >= 9.0) {
                rich += 1;
                rich_broke += u64::from(fin < 0.0);
            }
            never += u64::from(ls.iter().all(|l| *l >= 0.0));
            up += u64::from(fin > 2.0);
        }
        assert_eq!(
            (st.rich_ever, st.rich_ever_broke_at_end, st.never_broke, st.above_start_at_end),
            (rich, rich_broke, never, up)
        );
        assert_eq!(st.rich_most_at_once, *e.rich_now.iter().max().unwrap());
        assert!(st.rich_most_at_once <= st.rich_ever);
        assert_eq!(rich_ids(&game, &s).len() as u64, st.rich_ever);
    }

    #[test]
    fn picks_are_distinct_and_their_names_are_true() {
        for seed in 0..40u64 {
            for (rounds, players) in [(100, 1000), (1000, 3000)] {
                let game = Game::peters(rounds, seed);
                let (_, s) = simulate(&game, players);
                let lat = game.lattice();
                let ps = picks(&game, &s);
                for (a, p) in ps.iter().enumerate() {
                    assert!(ps[a + 1..].iter().all(|q| q.id != p.id), "duplicate pick");
                    let i = p.id as usize;
                    let fin = lat.at(rounds, s.final_k[i]);
                    let rich = lat.is_rich(s.peak_t[i], s.peak_k[i]);
                    match p.role {
                        Role::RichestAtEnd => assert!(s.final_k.iter().all(|k| *k <= s.final_k[i])),
                        Role::RichThenBroke | Role::RichThenLowest => {
                            assert!(rich);
                            assert_eq!(p.role == Role::RichThenBroke, fin < lat.broke);
                            let lowest = (0..s.len())
                                .filter(|&j| lat.is_rich(s.peak_t[j], s.peak_k[j]))
                                .map(|j| s.final_k[j])
                                .min();
                            assert_eq!(lowest, Some(s.final_k[i]), "not the lowest ending among the rich");
                        }
                        Role::BiggestFall => {
                            assert!(!(0..s.len()).any(|j| lat.is_rich(s.peak_t[j], s.peak_k[j])));
                            assert!(s.drawdown.iter().all(|d| *d <= s.drawdown[i]));
                        }
                        Role::FirstBroke => {
                            assert!(s.broke_t[i] > 0 && s.broke_t.iter().all(|b| *b == 0 || *b >= s.broke_t[i]))
                        }
                    }
                }
            }
        }
    }
}
