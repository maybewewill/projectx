"""Differential checks of the string/number natives against the original Game.dll (Unicorn).

Covers the two natives whose bodies can run without imports:

- R2I: the float-to-int helper at 0x6f6eec70 (the native's whole body is a call to it).
- StringHash: the MPQ-style string hash in Game.dll at 0x6f68cc10, with its crypt table built by
  0x6f68cb00. The native's own body calls the Storm import (ordinal 590), which is not in the upload,
  so this checks the same algorithm as Game.dll's copy, not Storm's code itself.

Prints the observed input/output pairs as Rust literals, to embed in crates/jass/tests/. Run with:
    GAME_DLL=/path/to/Game.dll python3 tools/emu/verify_strings.py
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from emu import *  # noqa: E402,F401  (maps Game.dll, provides mu, call, DATA)

M = 0xFFFFFFFF
R2I_FN = 0x6F6EEC70
CRYPT_GEN = 0x6F68CB00
MPQ_HASH = 0x6F68CC10
TABLE = DATA + 0x4000  # 0x1400 bytes of crypt table
FLT = DATA + 0x200
STR = DATA + 0x8000


def s32(x):
    return ((x + 2**31) % 2**32) - 2**31


# --- R2I -----------------------------------------------------------------------------------------

def r2i_model(bits):
    """Transcription of FUN_6f6eec70 (the rules the Rust `r2i` implements)."""
    neg = (bits >> 31) != 0
    e = (bits >> 23) & 0xFF
    if e < 0x7F:
        return 0
    if e > 0x9D:
        return -(2**31) if neg else 2**31 - 1
    m = (bits & 0x7FFFFF) | 0x800000
    mag = (m >> (0x96 - e)) if e <= 0x96 else ((m << (e - 0x96)) & M)
    v = mag if mag < 2**31 else mag - 2**32
    return s32(-v) if neg else v


def r2i_original(bits):
    mu.mem_write(FLT, struct.pack('<I', bits & M))
    return s32(call(R2I_FN, ecx=FLT))


def check_r2i(n_random=20000):
    edges = [0x00000000, 0x80000000, 0x3F800000, 0xBF800000, 0x3F7FFFFF, 0x3FC00000, 0xBFC00000,
             0x40000000, 0x4B000000, 0x4AFFFFFF, 0x4F000000, 0x4EFFFFFF, 0xCF000000, 0xCEFFFFFF,
             0x7F800000, 0xFF800000, 0x7FC00000, 0xFFC00000, 0x00000001, 0x007FFFFF, 0x3F000000,
             0x41200000, 0x42C80000, 0xC2C80000, 0x7F7FFFFF, 0xFF7FFFFF]
    rnd = random.Random(1234)
    cases = edges + [rnd.getrandbits(32) for _ in range(n_random)]
    # Random exponents near the interesting boundaries (0x7f, 0x96, 0x9d).
    for e in (0x7E, 0x7F, 0x80, 0x96, 0x97, 0x9C, 0x9D, 0x9E):
        for _ in range(200):
            cases.append((rnd.getrandbits(1) << 31) | (e << 23) | rnd.getrandbits(23))
    bad = 0
    for b in cases:
        got = r2i_original(b)
        exp = r2i_model(b)
        if got != exp:
            bad += 1
            if bad <= 10:
                print('R2I MISMATCH bits=%08x orig=%d model=%d' % (b, got, exp))
    print('R2I: %d cases, %d mismatches' % (len(cases), bad))
    return bad


def emit_r2i_pairs():
    pick = [0x3F800000, 0xBF800000, 0x3F7FFFFF, 0x3FC00000, 0xBFC00000, 0x3F000000, 0x40000000,
            0x41200000, 0xC2C80000, 0x42C80000, 0x4AFFFFFF, 0x4B000000, 0x4EFFFFFF, 0x4F000000,
            0xCEFFFFFF, 0xCF000000, 0x7F800000, 0xFF800000, 0x7FC00000, 0xFFC00000, 0x00000001,
            0x80000000, 0x007FFFFF, 0x7F7FFFFF, 0xFF7FFFFF, 0x46FFFE00, 0x4E7FFFFF, 0xC6FFFE00,
            0x3EFFFFFF, 0xBEFFFFFF, 0x4000B333, 0xC0490FDB, 0x5D5E0B6B, 0x00000000]
    print('// R2I pairs (f32 bits, result):')
    for b in pick:
        print('    (0x%08x, %d),' % (b, r2i_original(b)))


# --- StringHash ----------------------------------------------------------------------------------

def build_crypt_table_model():
    t = [0] * 0x500
    seed = 0x00100001
    for i in range(0x100):
        idx = i
        for _ in range(5):
            seed = (seed * 125 + 3) % 0x2AAAAB
            a = (seed & 0xFFFF) << 16
            seed = (seed * 125 + 3) % 0x2AAAAB
            t[idx] = a | (seed & 0xFFFF)
            idx += 0x100
    return t


def mpq_hash_model(name, hash_type, table):
    seed1, seed2 = 0x7FED7FED, 0xEEEEEEEE
    for ch in name:
        c = ch
        if 0x61 <= c <= 0x7A:
            c -= 0x20
        if c == 0x2F:
            c = 0x5C
        seed1 = table[hash_type * 0x100 + c] ^ ((seed1 + seed2) & M)
        seed2 = (c + seed1 + seed2 + (seed2 << 5) + 3) & M
    return seed1


def check_crypt_table():
    call(CRYPT_GEN, ecx=TABLE)
    raw = bytes(mu.mem_read(TABLE, 0x1400))
    got = list(struct.unpack('<%dI' % 0x500, raw))
    exp = build_crypt_table_model()
    same = got == exp
    print('crypt table (0x6f68cb00) == model: %s; first entry 0x%08x' % (same, got[0]))
    return same, exp


def emit_hash_pairs(table):
    names = [b'', b'a', b'A', b'abc', b'Abc', b'ABC', b'units\\human', b'units/human',
             b'Units\\Human', b'Hello World', b'TRIGSTR_001', b'CAbilityBlizzard', b'war3map.j',
             b'ab/cd', b'ab\\cd', b'0123456789', b'!@#$%^&*()', b'_', b'z', b'Z', b'{}[]',
             b'the quick brown fox jumps over the lazy dog', b'x' * 70, b'Ab' * 17,
             bytes(range(0x20, 0x7F))]
    bad = 0
    rows = []
    for name in names:
        if len(name) == 0:
            continue
        mu.mem_write(STR, name + b'\0')
        for t in (0, 1, 2, 3):
            if t != 0 and name not in (b'abc', b'units\\human', b'Hello World'):
                continue
            got = call(MPQ_HASH, ecx=TABLE, stack_args=[STR, t]) & M
            exp = mpq_hash_model(name, t, table)
            if got != exp:
                bad += 1
                print('HASH MISMATCH %r type %d orig=%08x model=%08x' % (name, t, got, exp))
            rows.append((name, t, got))
    print('StringHash (Game.dll MPQ hash): %d cases, %d mismatches' % (len(rows), bad))
    print('// StringHash pairs (name, hash type, Game.dll 0x6f68cc10 result):')
    for name, t, h in rows:
        if len(name) <= 40 and all(0x20 <= c < 0x7F for c in name):
            print('    (b"%s", %d, 0x%08x),' % (name.decode().replace('\\', '\\\\'), t, h))
    return bad


if __name__ == '__main__':
    failures = check_r2i()
    _, table = check_crypt_table()
    failures += emit_hash_pairs(table)
    emit_r2i_pairs()
    print('TOTAL MISMATCHES:', failures)
    sys.exit(1 if failures else 0)
