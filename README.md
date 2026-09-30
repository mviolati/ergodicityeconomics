# Ergodicity Economics: the coin game

Each round, every player tosses a coin. Heads: the wealth of the player increases by 50%. Tails: it
decreases by 40%. All players start with 100 €. The expected value increases by 5% each round, but the
typical player loses almost everything. This is the example of Fig. 2 in Ole Peters,
[*The ergodicity problem in economics*](https://rdcu.be/cS2t3) (Nature Physics, 2019). Emanuel Derman
states the same point in [a tweet](https://twitter.com/EmanuelDerman/status/1532473709239455745).

This project simulates the full path of every player and shows all of them in one chart. The code is
Rust: simulation, text, chart pixels and the build of the web page. The only other code is the small
JavaScript in the page that moves bytes between workers, WebAssembly and the screen, and an independent
Python reference used by one test.

![1,000,000 players, 1,000 rounds, seed 2022](docs/chart.png)

*1,000,000 players, 1,000 rounds, seed 2022 (`cargo xtask png --players 1000000 --seed 2022 --width 1000
--height 620 --dpr 1 --out docs/chart.png`). 277 players reach 1 billion € at least once. At most 43 of
them are above 1 billion € in the same round (strip under the chart). 243 of the 277 end below 1 €.*

## How to read the chart

| Mark | Meaning |
|---|---|
| Grey background | Number of players in each cell (round × wealth level). A cell that stands out more from the background holds more players. The scale is logarithmic; the page shows it with numbers. If more than one round or level falls on one pixel, the pixel shows the fullest cell. Between two neighbouring occupied levels there is no gap, and no cell crosses a threshold line. |
| Orange strip under the chart | The number of players with at least 1 billion € in each round. If more than one round falls on one pixel column, the bar shows the highest of them; the tooltip gives the value of each round. |
| Thin orange lines | The other players who reached 1 billion € (the thick lines are not drawn twice), by default only while they are above it. Players on the same level in the same round have the same wealth, so their lines coincide: count with the strip, not with the lines. An option on the page shows their full paths. |
| Thick coloured lines | Up to three real players of the run, coloured by role. Blue: the richest at the end. Orange: among the players who reached 1 billion €, the one who ended lowest (if nobody reached it: the deepest crash in proportion, i.e. the largest division of wealth from a previous peak, not the largest loss in euros). Green: the first player below 1 €. Each role is decided over all players; ties are stated. |
| Solid / dotted / dashed line | Median player (rank ⌈P/2⌉ from the poorest) / mean of all players / expected value. |

## Design

- **Every path is reproducible.** The coin of player `i` at round `t` is one bit of Philox4x32-10
  (counter `(i / 64, t)`, key = seed). Philox is counter-based: any path is a function of `(seed, i)` only.
  The result is the same on every device and with any number of threads.
- **Exact numbers.** For each player the simulation stores integers only: heads at the end, round and heads
  of the peak, first round below 1 €, start and end of the largest fall. One function (`Lattice::at`)
  converts `(round, heads)` to wealth, and one (`Lattice::fall`) gives the size of a fall from its
  (rounds, heads). Tests check every lattice point and every fall up to 2,000 rounds: two different values
  are never closer than 1e-9 (log10), and no lattice point is closer than 1e-9 to a threshold. So every
  comparison in `f64` is exact, also with a different `log10` on another platform.
- **Values are truncated, not rounded.** A shown value is never above the true value by more than 1e-12
  (relative; slack for log/pow round-trip error). No reachable wealth is that close to a threshold, so a
  player below 1 billion € never shows as "1,00 mld €".

## Layout

| Path | Contents |
|---|---|
| `crates/coin-core` | Philox4x32-10, simulation, per-player summaries, density, ensemble lines, headline figures, player roles, Italian number format |
| `crates/coin-core/tests/reference.py` | Independent pure-Python reference (exact fractions) that generates `golden.txt` |
| `crates/coin-chart` | Chart renderer (tiny-skia, IBM Plex Sans outlines), palette, density rasterisation |
| `crates/coin-web` | WebAssembly entry points, page text (JSON), tooltip |
| `web/index.html` | Page template: markup, CSS, and the JavaScript glue |
| `xtask` | `web`, `png` and `bench` commands |

## Commands

Requirements: Rust 1.80 or later, and the WebAssembly target:

```sh
rustup target add wasm32-unknown-unknown
```

| Command | Result |
|---|---|
| `cargo test --workspace --release` | All tests |
| `cargo xtask web` | `dist/index.html` (one file: page and WebAssembly module, built with source paths mapped to `/src` and `/cargo`) and `dist/fragment.html` (the same page without the document skeleton) |
| `cargo xtask png --players 1000000 --seed 2022 --theme dark --out target/chart.png` | The chart as PNG, rendered natively (unknown or invalid options stop with a message) |
| `cargo xtask bench` | Native simulation speed |
| `python3 crates/coin-core/tests/reference.py > crates/coin-core/tests/golden.txt` | Regenerates the reference values |

To open the page, serve `dist/` with any static server, for example `python3 -m http.server -d dist`.
The page loads its text fonts from Google Fonts when it can, and otherwise uses the system fonts. The
chart does not depend on them: its font is inside the WebAssembly module. The page lists the licences of
all third-party code it contains.

## Verification

The tests check:

1. Philox4x32-10 against the Random123 known-answer vectors.
2. The simulation against the independent Python reference, which compares wealth as exact fractions
   (520 players, 4 games; 8 players reach 1 billion €).
3. The fast simulation, the largest fall, the density, the ensemble lines and the headline figures against
   brute-force recomputation from re-created paths, also with a low "rich" threshold so that every rich code
   path runs.
4. Same results with 1 to 7 threads; adding players does not change the first ones; the high 32 bits of
   the seed are used.
5. Density against Binomial(t, 1/2) (chi-square, 3 seeds, 5 rounds); no correlation between neighbour rounds
   or neighbour players.
6. Rasterisation: each pixel matches its definition; the bands of a round have no holes and never cross a
   threshold; every occupied cell is visible at every tested size (also with a device pixel ratio of 1.25);
   isolated cells are not enlarged; the frame does not cover data; the strip reaches the true maximum; the
   tooltip reports exactly the cell the pixel shows.
7. Every player role and its tie count are true (150 games); the median rank, the first round of the
   maximum and every check of `Game::validate` are pinned; the page texts are compared as exact strings
   (singular and plural); JSON escaping.

## Speed

One measurement in this project's development container (4 cores), 1,000 rounds. Times change with the
machine and its load. Browser simulation time is measured inside the workers.

| Players | Native, 4 threads (`cargo xtask bench`) | Browser (Chromium, WebAssembly, 4 workers) |
|---|---|---|
| 10,000 | 0.03 s | 0.04 s simulation, 0.04 s chart |
| 100,000 | 0.15 s | 0.26 s simulation, 0.05 s chart |
| 1,000,000 | 1.55 s | 2.2 s simulation, 0.12 s chart |

## License

Apache License 2.0 (`LICENSE.md`). IBM Plex Sans: SIL Open Font License 1.1
(`crates/coin-chart/assets/IBMPlexSans-LICENSE.txt`).
