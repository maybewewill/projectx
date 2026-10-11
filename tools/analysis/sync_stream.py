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


def implied_fold(value, t, x, y):
    """Invert section 13.3: the registry fold A that gives `value` for the clock t and world words x, y.

    The map from A to the value is a bijection (rol, xor with constants), so every record implies exactly one A
    once T, X and Y are fixed. Inverse of sync(): v3 = rotr(value,3); v2 = rotr(v3^Y,3); v1 = rotr(v2^X,3);
    v0 = rotr(v1^X,3); c = v0^T; A = rotr(c,3)^0x17AB.
    """
    v3 = rotr(value, 3)
    v2 = rotr(v3 ^ y, 3)
    v1 = rotr(v2 ^ x, 3)
    v0 = rotr(v1 ^ x, 3)
    c = v0 ^ t
    return rotr(c, 3) ^ 0x17AB


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
    # Start-state candidate: T = 0, X = 0 (world+0x1C zero-filled at allocation), Y = 0 (world+0x50 = 0 by
    # FUN_6f46b120). Then the first record implies one registry fold A. A = 0 means an empty registry.
    for k in range(min(3, len(csum))):
        a = implied_fold(csum[k], 0, 0, 0)
        print("record %d: implied A with T=X=Y=0: %08x" % (k, a))


if __name__ == "__main__":
    main()
