"""Independent reference for crates/coin-core/tests/golden.txt.

Written from the Random123 specification of Philox4x32-10 and from the game rules, not from the
Rust code. Wealth is compared as exact fractions: after t rounds with k heads it is
100 * 1.5^k * 0.6^(t-k) = 100 * 3^t / (2^k * 5^(t-k)).

Run: python3 crates/coin-core/tests/reference.py > crates/coin-core/tests/golden.txt
"""
import random
from fractions import Fraction

MASK = 0xFFFFFFFF


def philox(c, k):
    c, k = list(c), list(k)
    for r in range(10):
        if r:
            k[0] = (k[0] + 0x9E3779B9) & MASK
            k[1] = (k[1] + 0xBB67AE85) & MASK
        p0 = 0xD2511F53 * c[0]
        p1 = 0xCD9E8D57 * c[2]
        c = [((p1 >> 32) ^ c[1] ^ k[0]) & MASK, p1 & MASK, ((p0 >> 32) ^ c[3] ^ k[1]) & MASK, p0 & MASK]
    return c


assert philox([0] * 4, [0] * 2) == [0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8]


def heads(seed, player, t):
    blk, lane = divmod(player, 64)
    o = philox([blk & MASK, blk >> 32, t, 0], [seed & MASK, seed >> 32])
    return ((o[0] | (o[1] << 32)) >> lane) & 1


def wealth(t, k):
    return Fraction(100 * 3**t, 2**k * 5 ** (t - k))


def player(seed, i, rounds):
    k = 0
    peak, peak_at = Fraction(100), (0, 0)
    fall, fall_at = Fraction(1), (0, 0, 0, 0)  # ratio peak / wealth; 1 = no fall
    broke = 0
    for t in range(1, rounds + 1):
        k += heads(seed, i, t)
        w = wealth(t, k)
        if w > peak:
            peak, peak_at = w, (t, k)
        if peak_at != (t, k):
            ratio = wealth(*peak_at) / w
            if ratio > fall:
                fall, fall_at = ratio, (peak_at[0], peak_at[1], t, k)
        if broke == 0 and w < 1:
            broke = t
    rich = int(peak >= 10**9)
    return [i, k, peak_at[0], peak_at[1], broke, *fall_at, rich]


print("# Reference values from reference.py (independent pure-Python implementation).")
print("# case <seed> <rounds> <number of players>, then one line per player:")
print("# id final_k peak_t peak_k broke_t fall_from_t fall_from_k fall_to_t fall_to_k rich")
rng = random.Random(1)
cases = [
    (2022, 100, list(range(300))),
    ((1 << 40) + 7, 60, list(range(130))),
    (7, 400, list(range(70))),
    # Players who reach 1 billion EUR in the 1,000,000-player run of seed 2022 (found with the Rust
    # code, values computed here), plus random players of the same run.
    (2022, 1000, [4457, 12514, 13804, 14604, 15312, 24609, 27108, 27920] + rng.sample(range(1_000_000), 12)),
]
for seed, rounds, ids in cases:
    print(f"case {seed} {rounds} {len(ids)}")
    for i in ids:
        print(" ".join(map(str, player(seed, i, rounds))))
