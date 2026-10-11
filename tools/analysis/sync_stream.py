"""Structural tests of the 0x22 sync stream (docs/spec/lockstep.md section 13.6).

Input: the TSV written by `cargo run -p w3g --example checksums -- FILE --tsv`
(columns: slot, time_before_ms, increment_ms, next_block, csum_hex). No game data is read here.

Usage: python3 -I tools/analysis/sync_stream.py slots.tsv

Reports (1) how many consecutive pairs show the constant-state signature rotr(D,12) = T ^ T'
(T = time before each slot), (2) the bit-length histogram of rotr(D,12), and (3) for the first record,
whether the value is reachable with registry fold A = 0, Y = 0, T = 0 (solves the GF(2)-affine map in X).
"""
import collections
import sys

M = 0xFFFFFFFF


def rol(x, n):
    return ((x << n) | (x >> (32 - n))) & M


def rotr(x, n):
    return ((x >> n) | (x << (32 - n))) & M


def sync(t, a, x, y):
    """Section 13.3 chain, in machine-code order (X twice, then Y)."""
    v = t ^ rol(a ^ 0x17AB, 3)
    v = rol(v, 3) ^ x
    v = rol(v, 3) ^ x
    v = rol(v, 3) ^ y
    return rol(v, 3)


def solve_x(t, a, y, target):
    """Find X with sync(t, a, X, y) == target (sync is affine in X over GF(2)); None if unreachable."""
    g0 = sync(t, a, 0, y)
    piv = []
    for i in range(32):
        val, comb = sync(t, a, 1 << i, y) ^ g0, 1 << i
        for pv, pc in piv:
            if val ^ pv < val:
                val ^= pv
                comb ^= pc
        if val:
            piv.append((val, comb))
            piv.sort(reverse=True)
    val, comb = target ^ g0, 0
    for pv, pc in piv:
        if val ^ pv < val:
            val ^= pv
            comb ^= pc
    return comb if val == 0 else None


def main():
    rows = [line.split() for line in open(sys.argv[1])]
    times = [int(r[1]) for r in rows]
    csum = [int(r[4], 16) for r in rows]
    pairs = len(csum) - 1
    hits = sum(1 for k in range(pairs) if rotr(csum[k] ^ csum[k + 1], 12) == (times[k] ^ times[k + 1]))
    print("pairs with the constant-state signature:", hits, "of", pairs)
    hist = collections.Counter(rotr(csum[k] ^ csum[k + 1], 12).bit_length() for k in range(pairs))
    print("bit length of rotr(D,12) (min, max):", min(hist), max(hist))
    x = solve_x(0, 0, 0, csum[0])
    print("first record reachable with A=0, Y=0, T=0:", x is not None)


if __name__ == "__main__":
    main()
