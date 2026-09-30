//! Philox4x32-10, the counter-based generator of Salmon, Moraes, Dror and Shaw
//! ("Parallel random numbers: as easy as 1, 2, 3", SC'11), bit-compatible with Random123.
//!
//! A counter-based generator has no state: the output is a pure function of (counter, key).
//! This is what makes every player's path reproducible on its own, on any machine and with any
//! number of threads.

const M0: u32 = 0xD251_1F53;
const M1: u32 = 0xCD9E_8D57;
const W0: u32 = 0x9E37_79B9;
const W1: u32 = 0xBB67_AE85;

#[inline(always)]
fn mulhilo(a: u32, b: u32) -> (u32, u32) {
    let p = u64::from(a) * u64::from(b);
    ((p >> 32) as u32, p as u32)
}

/// One Philox4x32-10 block: 128 random bits for the given counter and key.
#[inline(always)]
pub fn philox4x32_10(mut ctr: [u32; 4], mut key: [u32; 2]) -> [u32; 4] {
    for round in 0..10 {
        if round > 0 {
            key[0] = key[0].wrapping_add(W0);
            key[1] = key[1].wrapping_add(W1);
        }
        let (hi0, lo0) = mulhilo(M0, ctr[0]);
        let (hi1, lo1) = mulhilo(M1, ctr[2]);
        ctr = [hi1 ^ ctr[1] ^ key[0], lo1, hi0 ^ ctr[3] ^ key[1], lo0];
    }
    ctr
}

/// The coins of 64 players at one round: bit `j` is player `block * 64 + j`, 1 = heads.
///
/// Counter = (block low, block high, round, 0), key = (seed low, seed high). Only the first
/// 64 of the 128 output bits are used.
#[inline(always)]
pub fn coins(seed: u64, block: u64, round: u32) -> u64 {
    let o = philox4x32_10([block as u32, (block >> 32) as u32, round, 0], [seed as u32, (seed >> 32) as u32]);
    u64::from(o[0]) | (u64::from(o[1]) << 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known-answer vectors from Random123 (tests/kat_vectors, "philox4x32 10").
    #[test]
    fn matches_random123_known_answers() {
        let cases = [
            ([0u32; 4], [0u32; 2], [0x6627_e8d5, 0xe169_c58d, 0xbc57_ac4c, 0x9b00_dbd8]),
            ([u32::MAX; 4], [u32::MAX; 2], [0x408f_276d, 0x41c8_3b0e, 0xa20b_c7c6, 0x6d54_51fd]),
            (
                [0x243f_6a88, 0x85a3_08d3, 0x1319_8a2e, 0x0370_7344],
                [0xa409_3822, 0x299f_31d0],
                [0xd16c_fe09, 0x94fd_cceb, 0x5001_e420, 0x2412_6ea1],
            ),
        ];
        for (ctr, key, want) in cases {
            assert_eq!(philox4x32_10(ctr, key), want);
        }
    }
}
