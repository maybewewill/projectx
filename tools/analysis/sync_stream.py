"""Structural tests of the 0x22 sync stream (docs/spec/lockstep.md sections 13 and 15).

Superseded by `w3boot --replay FILE.w3g --check-sync` (crates/engine/src/sync_check.rs), which also folds in the
net checksum of the received blocks. This script keeps the arithmetic of the value in Python (corrected in
section 15: X once, a fourth term R) and the old constant-state test, for quick experiments on the TSV written by
`cargo run -p w3g --example checksums -- FILE --tsv` (columns: slot, time_before_ms, increment_ms, next_block, csum_hex).

Usage: python3 -I tools/analysis/sync_stream.py slots.tsv
"""
import collections
import sys

M = 0xFFFFFFFF


def rol(x, n):
    return ((x << n) | (x >> (32 - n))) & M


def rotr(x, n):
    return ((x >> n) | (x << (32 - n))) & M


def sync(t, a, x, y, r):
    """Section 15.0 chain, in machine-code order: rol(rol(rol(rol(T ^ c, 3) ^ X, 3) ^ Y, 3) ^ R, 3)."""
    v = t ^ rol(a ^ 0x17AB, 3)
    v = rol(v, 3) ^ x
    v = rol(v, 3) ^ y
    v = rol(v, 3) ^ r
    return rol(v, 3)


def implied_fold(value, t, x, y, r):
    """The registry fold A that gives `value` for the clock t, world words x, y and generator hash r."""
    v3 = rotr(value, 3) ^ r
    v2 = rotr(v3, 3) ^ y
    v1 = rotr(v2, 3) ^ x
    v0 = rotr(v1, 3)
    c = v0 ^ t
    return rotr(c, 3) ^ 0x17AB


def main():
    rows = [line.split() for line in open(sys.argv[1])]
    times = [int(r[1]) for r in rows]
    csum = [int(r[4], 16) for r in rows if len(r) > 4]
    pairs = len(csum) - 1
    hits = sum(1 for k in range(pairs) if rotr(csum[k] ^ csum[k + 1], 12) == (times[k] ^ times[k + 1]))
    print("pairs with the constant-state signature:", hits, "of", pairs)
    hist = collections.Counter(rotr(csum[k] ^ csum[k + 1], 12).bit_length() for k in range(pairs))
    print("bit length of rotr(D,12) (min, max):", min(hist), max(hist))
    print("(the record stream also depends on the net checksum; use w3boot --check-sync)")


if __name__ == "__main__":
    main()
