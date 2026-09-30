//! Headline figures and the players the chart singles out.

use crate::{
    ensemble::Ensemble,
    sim::{Summary, BROKE_T, FALL_FROM_K, FALL_FROM_T, FALL_TO_K, FALL_TO_T, FINAL_K, PEAK_K, PEAK_T},
    Game, Lattice,
};

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

/// True if player `i` reached the rich threshold in some round.
pub fn is_rich_ever(lat: &Lattice, s: &Summary, i: usize) -> bool {
    lat.is_rich(s.get(PEAK_T, i), s.get(PEAK_K, i))
}

/// The largest fall of player `i` as (rounds, heads) and its size in decades.
pub fn largest_fall(lat: &Lattice, s: &Summary, i: usize) -> (u32, u32, f64) {
    let rounds = s.get(FALL_TO_T, i) - s.get(FALL_FROM_T, i);
    let heads = s.get(FALL_TO_K, i) - s.get(FALL_FROM_K, i);
    (rounds, heads, lat.fall(rounds, heads))
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
        let fin = lat.at(r, s.get(FINAL_K, i));
        if is_rich_ever(&lat, s, i) {
            st.rich_ever += 1;
            if fin < lat.broke {
                st.rich_ever_broke_at_end += 1;
            }
        }
        st.rich_at_end += u64::from(fin >= lat.rich);
        st.never_broke += u64::from(s.get(BROKE_T, i) == 0);
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

/// Why a player is shown. The chart colour follows the role, not the position in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Highest wealth after the last round.
    RichestAtEnd,
    /// Among players who reached the rich threshold: the lowest ending, and it is below broke.
    RichThenBroke,
    /// Among players who reached the rich threshold: the lowest ending, at or above broke.
    RichThenLowest,
    /// Nobody reached the rich threshold: the largest fall from a previous peak.
    BiggestFall,
    /// Earliest round below the broke threshold.
    FirstBroke,
}

impl Role {
    /// Palette slot of the role: 0 richest (blue), 1 a fall from the top (orange), 2 first
    /// below broke (green).
    pub fn slot(self) -> usize {
        match self {
            Role::RichestAtEnd => 0,
            Role::RichThenBroke | Role::RichThenLowest | Role::BiggestFall => 1,
            Role::FirstBroke => 2,
        }
    }
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
/// who ended lowest (or, if nobody got rich, the largest fall from a previous peak); the first to
/// fall below the broke threshold. Every score is an exact function of integers. Ties go to the
/// lowest id not already shown; `ties` counts the other players with the same score. A role
/// whose winners are all already shown is left out.
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
    let take = |ids: Vec<u64>, role: &dyn Fn(u64) -> Role, out: &mut Vec<Pick>| {
        if let Some(&id) = ids.iter().find(|id| out.iter().all(|p| p.id != **id)) {
            out.push(Pick { id, role: role(id), ties: ids.len() as u64 - 1 });
        }
    };

    take(winners(&|i| Some(i128::from(s.get(FINAL_K, i)))), &|_| Role::RichestAtEnd, &mut out);
    let lowest_rich = winners(&|i| is_rich_ever(&lat, s, i).then(|| -i128::from(s.get(FINAL_K, i))));
    if lowest_rich.is_empty() {
        // A fall is ranked by its exact (rounds, heads). Different falls differ by more than
        // 1e-9 decades (game.rs test), so ordering them by `Lattice::fall` is exact, and equal
        // falls give bit-identical numbers.
        let fall = winners(&|i| {
            let (_, _, f) = largest_fall(&lat, s, i);
            (f > 0.0).then(|| i128::from(f.to_bits()))
        });
        take(fall, &|_| Role::BiggestFall, &mut out);
    } else {
        let role = |id: u64| {
            if lat.is_broke(game.rounds, s.get(FINAL_K, id as usize)) {
                Role::RichThenBroke
            } else {
                Role::RichThenLowest
            }
        };
        take(lowest_rich, &role, &mut out);
    }
    let first_broke = winners(&|i| (s.get(BROKE_T, i) > 0).then(|| -i128::from(s.get(BROKE_T, i))));
    take(first_broke, &|_| Role::FirstBroke, &mut out);
    out
}

/// Ids of all players who reached the rich threshold, in id order.
pub fn rich_ids(game: &Game, s: &Summary) -> Vec<u64> {
    let lat = game.lattice();
    (0..s.len()).filter(|&i| is_rich_ever(&lat, s, i)).map(|i| i as u64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ensemble::ensemble,
        sim::{path, simulate},
    };

    /// A game where many players pass a lower "rich" threshold, so every rich code path runs.
    fn low_rich(rounds: u32, seed: u64) -> Game {
        Game { rich: 1e4, ..Game::peters(rounds, seed) }
    }

    #[test]
    fn stats_equal_a_direct_count_over_paths() {
        for game in [Game::peters(600, 2022), low_rich(300, 4)] {
            let players = 20_000;
            let (counts, s) = simulate(&game, players);
            let e = ensemble(&game, players as u64, &counts);
            let st = stats(&game, &s, &e);
            let lat = game.lattice();
            let (mut rich, mut rich_broke, mut never, mut up, mut rich_end) = (0, 0, 0, 0, 0);
            for i in 0..players as u64 {
                let p = path(&game, i);
                let ls: Vec<f64> = (0..=game.rounds).map(|t| lat.at(t, p[t as usize])).collect();
                let fin = *ls.last().unwrap();
                if ls.iter().any(|l| *l >= lat.rich) {
                    rich += 1;
                    rich_broke += u64::from(fin < 0.0);
                }
                rich_end += u64::from(fin >= lat.rich);
                never += u64::from(ls.iter().all(|l| *l >= 0.0));
                up += u64::from(fin > 2.0);
            }
            let got = (st.rich_ever, st.rich_ever_broke_at_end, st.never_broke, st.above_start_at_end, st.rich_at_end);
            assert_eq!(got, (rich, rich_broke, never, up, rich_end), "{game:?}");
            assert_eq!(st.rich_most_at_once, *e.rich_now.iter().max().unwrap());
            assert_eq!(e.rich_now[st.rich_most_at_once_round as usize], st.rich_most_at_once);
            assert!(st.rich_most_at_once <= st.rich_ever);
            assert_eq!(rich_ids(&game, &s).len() as u64, st.rich_ever);
        }
        // The low threshold really exercises the rich paths.
        let (_, s) = simulate(&low_rich(300, 4), 20_000);
        assert!(rich_ids(&low_rich(300, 4), &s).len() > 1000);
    }

    #[test]
    fn most_at_once_reports_the_first_round_of_the_maximum() {
        let game = Game::peters(3, 0);
        let players = 5;
        let (counts, s) = simulate(&game, players);
        let mut e = ensemble(&game, players as u64, &counts);
        e.rich_now = vec![0, 5, 3, 5];
        let st = stats(&game, &s, &e);
        assert_eq!((st.rich_most_at_once, st.rich_most_at_once_round), (5, 1));
    }

    /// Recomputes each role from its definition with a direct scan, ties included.
    #[test]
    fn picks_are_distinct_and_their_names_and_ties_are_true() {
        let mut seen = std::collections::HashSet::new();
        let mut games = Vec::new();
        for seed in 0..30u64 {
            games.push((Game::peters(100, seed), 1000));
            games.push((Game::peters(1000, seed), 3000));
            games.push((low_rich(250, seed), 2000));
            games.push((low_rich(2, seed), 5));
            games.push((low_rich(30, seed), 2000)); // short: the lowest rich ending is above 1 EUR
        }
        for (game, players) in games {
            let (_, s) = simulate(&game, players);
            let lat = game.lattice();
            let ps = picks(&game, &s);
            let n = s.len();
            let rich: Vec<usize> = (0..n).filter(|&j| is_rich_ever(&lat, &s, j)).collect();
            for (a, p) in ps.iter().enumerate() {
                seen.insert(format!("{:?}", p.role));
                assert!(ps[a + 1..].iter().all(|q| q.id != p.id), "duplicate pick");
                let i = p.id as usize;
                let fin = |j: usize| s.get(FINAL_K, j);
                // (players with the winning score, the score is winning)
                let (group, ok): (Vec<usize>, bool) = match p.role {
                    Role::RichestAtEnd => {
                        let best = (0..n).map(fin).max().unwrap();
                        ((0..n).filter(|&j| fin(j) == best).collect(), fin(i) == best)
                    }
                    Role::RichThenBroke | Role::RichThenLowest => {
                        let low = rich.iter().map(|&j| fin(j)).min().unwrap();
                        let broke = lat.at(game.rounds, fin(i)) < lat.broke;
                        let role_ok = (p.role == Role::RichThenBroke) == broke;
                        (rich.iter().copied().filter(|&j| fin(j) == low).collect(), role_ok && fin(i) == low)
                    }
                    Role::BiggestFall => {
                        let f = |j: usize| {
                            let (r, h, _) = largest_fall(&lat, &s, j);
                            (r - h, h)
                        };
                        // Exact comparison of falls as rationals is not needed: equal (tails, heads)
                        // means equal falls, and the largest value is checked below.
                        let best = (0..n).map(|j| largest_fall(&lat, &s, j).2).fold(0.0, f64::max);
                        let group: Vec<usize> = (0..n).filter(|&j| f(j) == f(i)).collect();
                        (group, rich.is_empty() && largest_fall(&lat, &s, i).2 == best && best > 0.0)
                    }
                    Role::FirstBroke => {
                        let first = (0..n).map(|j| s.get(BROKE_T, j)).filter(|b| *b > 0).min().unwrap();
                        ((0..n).filter(|&j| s.get(BROKE_T, j) == first).collect(), s.get(BROKE_T, i) == first)
                    }
                };
                assert!(ok, "{:?} is not true for player {i} in {game:?}", p.role);
                assert_eq!(p.ties, group.len() as u64 - 1, "{:?} ties in {game:?}", p.role);
                let earlier_free = group.iter().find(|&&j| ps[..a].iter().all(|q| q.id != j as u64));
                assert_eq!(earlier_free, Some(&i), "not the lowest free id");
            }
        }
        for role in ["RichestAtEnd", "RichThenBroke", "RichThenLowest", "BiggestFall", "FirstBroke"] {
            assert!(seen.contains(role), "role {role} never exercised");
        }
    }
}
