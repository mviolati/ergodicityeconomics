//! Cross-checks against an independent implementation and against the law of fair coins.

use coin_core::{
    ensemble::ensemble,
    sim::{density_len, simulate, simulate_range, Summary, FIELDS, PEAK_K, PEAK_T},
    Game,
};

#[test]
fn equals_independent_reference_implementation() {
    let text = include_str!("golden.txt");
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    let (mut cases, mut rich_seen) = (0, 0);
    while let Some(head) = lines.next() {
        let h: Vec<u64> =
            head.strip_prefix("case ").expect("case line").split(' ').map(|x| x.parse().unwrap()).collect();
        let (seed, rounds, count) = (h[0], h[1] as u32, h[2] as usize);
        let rows: Vec<Vec<u64>> = (0..count)
            .map(|_| lines.next().expect("player line").split(' ').map(|x| x.parse().unwrap()).collect())
            .collect();
        let game = Game::peters(rounds, seed);
        let lat = game.lattice();
        let players = rows.iter().map(|r| r[0]).max().unwrap() as usize + 1;
        // Simulate only the blocks that hold the listed players (ranges start at multiples of 64).
        for row in &rows {
            let first = row[0] / 64 * 64;
            let n = (players - first as usize).min(64);
            let mut counts = vec![0u32; density_len(rounds)];
            let mut s = Summary::zeros(n);
            simulate_range(&game, first, &mut counts, s.slice_mut(0..n));
            let i = (row[0] - first) as usize;
            let mut got: Vec<u64> = vec![row[0]];
            got.extend((0..FIELDS).map(|f| u64::from(s.get(f, i))));
            got.push(u64::from(lat.is_rich(s.get(PEAK_T, i), s.get(PEAK_K, i))));
            assert_eq!(got, *row, "seed {seed} rounds {rounds} player {}", row[0]);
            rich_seen += row[9];
        }
        cases += 1;
    }
    assert_eq!(cases, 4);
    assert!(rich_seen >= 8, "the reference must include players who reach 1 billion EUR");
}

/// Pearson chi-square statistic of observed counts against Binomial(t, 1/2), cells with an
/// expected count below 5 pooled into their neighbours. Returns (statistic, degrees of freedom).
fn chi_square_binomial(obs: &[u32], t: u32, n: u64) -> (f64, usize) {
    let mut p = vec![0f64; t as usize + 1];
    // log C(t, k) - t log 2, stable for t up to thousands
    let mut logc = 0f64;
    for (k, pk) in p.iter_mut().enumerate() {
        if k > 0 {
            logc += ((t as usize - k + 1) as f64).ln() - (k as f64).ln();
        }
        *pk = (logc - f64::from(t) * std::f64::consts::LN_2).exp();
    }
    // Pool neighbouring cells until each expected count is at least 5; a small tail joins the
    // last full cell.
    let mut cells: Vec<(f64, f64)> = Vec::new();
    let (mut o_acc, mut e_acc) = (0f64, 0f64);
    for k in 0..=t as usize {
        o_acc += f64::from(obs[k]);
        e_acc += p[k] * n as f64;
        if e_acc >= 5.0 {
            cells.push((o_acc, e_acc));
            (o_acc, e_acc) = (0.0, 0.0);
        }
    }
    if let Some(last) = cells.last_mut() {
        last.0 += o_acc;
        last.1 += e_acc;
    }
    let stat = cells.iter().map(|(o, e)| (o - e).powi(2) / e).sum();
    (stat, cells.len().saturating_sub(1))
}

#[test]
fn density_follows_the_binomial_law() {
    // Deterministic seeds: the test either always passes or always fails. The bound is the
    // 99.99% quantile of chi-square (Wilson-Hilferty), so a correct generator passes.
    let players = 200_000u64;
    for seed in [1u64, 2022, 1 << 33] {
        let game = Game::peters(300, seed);
        let (counts, _) = simulate(&game, players as usize);
        let w = 301;
        for t in [1u32, 2, 10, 100, 300] {
            let row = &counts[(t as usize - 1) * w..][..t as usize + 1];
            let (stat, df) = chi_square_binomial(row, t, players);
            let d = df as f64;
            let z = 3.719; // 99.99%
            let bound = d * (1.0 - 2.0 / (9.0 * d) + z * (2.0 / (9.0 * d)).sqrt()).powi(3);
            assert!(stat < bound, "seed {seed} round {t}: chi2 {stat:.1} > {bound:.1} (df {df})");
        }
        let e = ensemble(&game, players, &counts);
        assert_eq!(e.median.len(), 301);
    }
}

#[test]
fn neighbouring_players_and_rounds_are_uncorrelated() {
    // Coins of player i at round t and t+1, and of players i and i+1 at the same round.
    let game = Game::peters(400, 99);
    let n = 64 * 400;
    let bits = |i: u64, t: u32| (coin_core::rng::coins(game.seed, i / 64, t) >> (i % 64)) & 1;
    let (mut same_t, mut same_i, mut total) = (0i64, 0i64, 0i64);
    for i in 0..n as u64 {
        for t in 1..400u32 {
            let b = bits(i, t) as i64 * 2 - 1;
            same_t += b * (bits(i, t + 1) as i64 * 2 - 1);
            if i + 1 < n as u64 {
                same_i += b * (bits(i + 1, t) as i64 * 2 - 1);
            }
            total += 1;
        }
    }
    // Each sum is +-1 terms; under independence its sd is sqrt(total). 5 sd bound.
    let bound = 5.0 * (total as f64).sqrt();
    assert!((same_t as f64).abs() < bound, "round-to-round correlation {same_t} vs {bound}");
    assert!((same_i as f64).abs() < bound, "player-to-player correlation {same_i} vs {bound}");
}
