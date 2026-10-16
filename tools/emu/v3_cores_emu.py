"""Unicorn verification cores of wave V3 (invisibility, item and creep abilities of Game.dll 1.26a).

Same method as item_cores*_emu.py: the ORIGINAL code runs on fake objects, callees that need the object model are
stubbed and recorded, the soft-float library runs for real. Rows go to crates/sim/tests/v3_vectors.txt and are recomputed
by crates/sim/tests/v3_vectors.rs against the Rust model. Row kinds (hex, `|` separates input from output):

  IC  count flags fade       | count' flags' class     invisibility counter increment 0x6F28D7D0 (class: N notify only,
                                                          I immediate flag set, R ramp (fade > 0), - nothing)
  ID  count flags fade       | count' flags' class     decrement 0x6F28D9C0 (flag cleared at once; class I/R/-)
  VG  flags arg owner mask det | pass                  viewer gate 0x6F3A15F0

Usage: GAME_DLL=/path/Game.dll py tools/emu/v3_cores_emu.py OUT.txt [CASES] [SEED] [KINDS]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import item_cores_emu as C  # noqa: E402

T = C.T
mu = C.mu
w32 = C.w32
r32 = C.r32
rand_real = C.rand_real
fbits = C.fbits
T.ALLOW.append((0x6F6EE000, 0x6F6F1000))

STUB_B = T.RETSTUB + 0x20   # a second RET stub with its own address (so the stub table can tell slots apart)


def prep():
    T.setup(0x6F8C9F64)
    mu.mem_write(STUB_B, b'\x31\xC0\xC3')


# ---------------------------------------------------------------- IC / ID: the invisibility counter
INC, DEC = 0x6F28D7D0, 0x6F28D9C0
LETTER = {0x6F26F970: 'N', 0x6F4E8700: 'M', 0x6F477550: 'U', 0x6F2AC3A0: 'K', 0x6F477350: 'P', 0x6F4D3510: 'Q'}
SET_NOW = 0x6F284950

FADES = [0, 0x80000000, 0x3F800000, 0x3F19999A, 0x40000000, 0xBF800000, 0x7FC00000, 0x7F800000, 0x00000001, 0x3DCCCCCD]


def counter_case(r, fn):
    prep()
    count = r.choice([0, 0, 0, 0, 1, 1, 2, -1, r.randint(-2, 5)])
    flags = r.choice([0, 0x1000000, r.getrandbits(32), r.getrandbits(32) & ~0x1000000])
    fade = r.choice(FADES) if r.random() < 0.6 else rand_real(r)
    mu.mem_write(T.UNIT, bytes(0x200))
    w32(T.UNIT, T.UVT)
    w32(T.UNIT + 0x114, count & 0xffffffff)
    w32(T.UNIT + 0x5C, flags)
    w32(T.D + 0x4000, fade)
    seq = []

    def rec(t):
        def f(uc, c, d, st):
            seq.append(LETTER.get(t, '?'))
            return 0
        return f

    stubs = {t: rec(t) for t in LETTER}
    stubs[0x6F26D950] = lambda *a: r.choice([0, 1])
    # 0x6F284950 (set the flag now) runs for real; its own callees are stubbed
    for t in (0x6F02C960, 0x6F03ED80, 0x6F2AB310, 0x6F421E20, 0x6F424CE0, 0x6F333230, 0x6F332700, 0x6F3A1650):
        stubs[t] = lambda *a: 0
    T.trace_call(fn, T.UNIT, (T.D + 0x4000,), stub_fn=stubs, into=(SET_NOW,), span=0x400, cleanup={T.RETSTUB: 8})
    cnt = r32(T.UNIT + 0x114)
    fl = r32(T.UNIT + 0x5C)
    s = ''.join(seq)
    if 'U' in s:
        cls = 'R'
    elif 'P' in s:
        cls = 'I'
    elif 'N' in s:
        cls = 'N'
    else:
        cls = '-'
    return cnt, fl, cls, (count & 0xffffffff, flags, fade)


def case_ic(r):
    cnt, fl, cls, (c0, f0, fd) = counter_case(r, INC)
    return 'IC %x %x %x | %x %x %s' % (c0, f0, fd, cnt, fl, cls)


def case_id(r):
    cnt, fl, cls, (c0, f0, fd) = counter_case(r, DEC)
    return 'ID %x %x %x | %x %x %s' % (c0, f0, fd, cnt, fl, cls)


# ---------------------------------------------------------------- VG: the viewer gate
GATE = 0x6F3A15F0
DET = 0x6F27A380


def case_vg(r):
    prep()
    flags = r.choice([0, 0x1000000, r.getrandbits(32), 0x1000000 | r.getrandbits(32)])
    arg = r.choice([0, 1, 2, r.getrandbits(32)])
    owner = r.choice([r.randint(0, 15), r.randint(0, 15), r.randint(0, 40), 0xFFFFFFFF, 12])
    mask = r.choice([r.getrandbits(16), r.getrandbits(32), 1 << (owner & 31), 0])
    det = r.choice([0, r.getrandbits(32), r.getrandbits(16), mask])
    mu.mem_write(T.UNIT, bytes(0x200))
    w32(T.UNIT, T.UVT)
    w32(T.UNIT + 0x5C, flags)
    w32(T.UVT + 0xEC, STUB_B)
    ret = T.trace_call(GATE, 0, (T.UNIT, mask, arg), stub_fn={STUB_B: lambda *a: owner, DET: lambda *a: det},
                       span=0x80, cleanup={DET: 4, STUB_B: 0})[0]
    return 'VG %x %x %x %x %x | %x' % (flags, arg, owner, mask, det, ret & 0xffffffff)


GENS = [('IC', case_ic), ('ID', case_id), ('VG', case_vg)]


def main():
    out = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 300
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    only = sys.argv[4].split(',') if len(sys.argv) > 4 else None
    r = random.Random(seed)
    rows = []
    for name, fn in GENS:
        if only and name not in only:
            continue
        for _ in range(cases):
            rows.append(fn(r))
    with open(out, 'w') as fh:
        fh.write('\n'.join(rows) + '\n')
    print(len(rows), 'rows')


if __name__ == '__main__':
    main()
