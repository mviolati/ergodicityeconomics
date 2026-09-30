//! Peters' coin game with a full, reproducible path for every player.
//!
//! Each round a player's wealth is multiplied by `win` (heads) or `lose` (tails). The coin of
//! player `i` at round `t` is one bit of Philox4x32-10(seed, i / 64, t), so any path can be
//! re-created from `(seed, i)` alone, on any machine, with any number of threads.
//!
//! - [`sim`]: simulation, per-player summaries, the density matrix, path re-creation.
//! - [`ensemble`]: mean, median, expected value and threshold counts per round.
//! - [`stats`]: headline figures and the players the chart singles out.
//! - [`fmt`]: Italian number formatting that never rounds a value across a threshold.

pub mod ensemble;
pub mod fmt;
pub mod game;
pub mod rng;
pub mod sim;
pub mod stats;

pub use game::{Game, Lattice};
