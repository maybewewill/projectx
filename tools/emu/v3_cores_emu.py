"""Unicorn verification cores of wave V3 (invisibility, item and creep abilities of Game.dll 1.26a).

Same method as item_cores*_emu.py: the ORIGINAL code runs on fake objects, callees that need the object model are
stubbed and recorded, the soft-float library runs for real. Rows go to crates/sim/tests/v3_vectors.txt and are recomputed
by crates/sim/tests/v3_vectors.rs against the Rust model. Row kinds (hex, `|` separates input from output):

  IC  count flags fade       | count' flags' class     invisibility counter increment 0x6F28D7D0 (class: N notify only,
                                                          I immediate flag set, R ramp (fade > 0), - nothing)
  ID  count flags fade       | count' flags' class     decrement 0x6F28D9C0 (flag cleared at once; class I/R/-)
  VG  flags arg owner mask det | pass                  viewer gate 0x6F3A15F0
  DA  fl5c immune hero dur herodur extra A B C | lc L mc M bc armor dur
                                                        Scroll of Protection (AIda) area callback 0x6F0B61D0: the life
                                                        and mana adder calls and the buff apply (Bdef) arguments

  RR  amount x | amount'                              damage record reduce 0x6F255790 (first amount)
  MS  recflags srcnull share per amount mana mana' | events
                                                        Mana Shield damage hook 0x6F126DD0 (events: R remove buff, Dx reduce
                                                        the record by x, Sx spend x mana; mana' = the mana after the spend)

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


# ---------------------------------------------------------------- DA: AIda area callback
import item_trace_emu as _tt  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EBX  # noqa: E402
T.ALLOW.append((0x6F02F800, 0x6F02F900))
DA_VT = int(_tt.factory_row('AIda')['vtable'], 16)
LIFE_ADD, MANA_ADD = 0x6F277C20, 0x6F279140


def setrec(**kw):
    rec = bytearray(bytes(mu.mem_read(T.REC, 0x68)))
    offs = {'dur': 0x08, 'herodur': 0x0C, 'area': 0x18}
    for i, n in enumerate('abcdefghi'):
        offs[n] = 0x20 + 4 * i
    for k, v in kw.items():
        struct.pack_into('<I', rec, offs[k], v & 0xffffffff)
    mu.mem_write(T.REC, bytes(rec))


def case_da(r):
    T.setup(DA_VT)
    cand = T.D + 0x5000
    mu.mem_write(cand, bytes(0x400))
    w32(cand, T.UVT)
    fl5c = r.choice([0, 0, 0x40000000, r.getrandbits(32)])
    w32(cand + 0x5C, fl5c)
    immune = r.choice([0, 0, 0, 1])
    hero = r.choice([0, 1])
    dur, herodur, extra, a, b, c = (rand_real(r) for _ in range(6))
    if r.random() < 0.2:
        b = r.choice([0, 0x80000000, 0x7FC00000, fbits(-5.0)])
    if r.random() < 0.2:
        c = r.choice([0, 0x80000000, 0x7FC00000, fbits(-5.0)])
    setrec(dur=dur, herodur=herodur, a=a, b=b, c=c)
    w32(T.ABIL + 0x9C, extra)
    got = {}

    def life(uc, ecx, edx, st):
        got['lc'] = (ecx == cand, r32(st[1]))
        return 0

    def mana(uc, ecx, edx, st):
        got['mc'] = (ecx == cand, r32(st[1]))
        return 0

    def apply(uc, ecx, edx, st):
        ebx = uc.reg_read(UC_X86_REG_EBX)
        got['bc'] = (r32(ebx), r32(st[1]))
        return 0

    T.trace_call(0x6F0B61D0, cand, (), edx=T.ABIL,
                 stub_fn={LIFE_ADD: life, MANA_ADD: mana, 0x6F28D750: lambda *x: immune, 0x6F1D2660: lambda *x: hero,
                          0x6F0B60F0: apply},
                 span=0x200, cleanup={0x6F0B60F0: 8, 0x6F042B20: 8})
    out = 'DA %x %x %x %x %x %x %x %x %x |' % (fl5c, immune, hero, dur, herodur, extra, a, b, c)
    for k in ('lc', 'mc'):
        if k in got:
            assert got[k][0], 'adder target is the candidate'
            out += ' 1 %x' % got[k][1]
        else:
            out += ' 0 0'
    if 'bc' in got:
        out += ' 1 %x %x' % got['bc']
    else:
        out += ' 0 0 0'
    return out


# ---------------------------------------------------------------- MS: the Mana Shield damage hook
T.ALLOW.append((0x6F022500, 0x6F022600))
MS_HOOK = 0x6F126DD0
PROP_GET, SMART_PTR, REDUCE, MANA_CHANGE, REMOVE_BUFF = 0x6F4773A0, 0x6F038450, 0x6F255790, 0x6F283DB0, 0x6F079CC0


def nice(r):
    k = r.random()
    if k < 0.55:
        return fbits(r.choice([0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 0.25, 10.0, 100.0, 7.0, 33.0, r.uniform(0.1, 20.0), r.uniform(1, 400)]))
    return rand_real(r)


def case_ms(r):
    prep()
    BUFF, REC, UNITO, SMART = T.D + 0x4000, T.D + 0x4800, T.D + 0x4C00, T.D + 0x5000
    for a in (BUFF, REC, UNITO, SMART):
        mu.mem_write(a, bytes(0x400))
    flags = r.choice([0, 0, 0, 0x10, 0x11, r.getrandbits(8)])
    srcnull = r.choice([0, 0, 0, 0, 1])
    share, per, amount, mana = nice(r), nice(r), nice(r), nice(r)
    after = r.choice([0, nice(r), nice(r), 0x80000000, 0x7FC00000, fbits(1e-9)])
    if r.random() < 0.15:
        per = r.choice([0, 0x80000000])
    if r.random() < 0.1:
        amount = 0
    w32(BUFF + 0xD8, share)
    w32(BUFF + 0xD0, per)
    w32(BUFF + 0x30, UNITO)
    w32(REC + 0x10, amount)
    w32(REC + 0xC, flags)
    w32(REC, 0x1234)
    ev = []
    calls = [0]

    def smart(uc, c, d, st):
        w32(c, 0 if srcnull else SMART + 0x10)
        return c

    def getter(uc, c, d, st):
        w32(st[0], mana if calls[0] == 0 else after)
        calls[0] += 1
        return st[0]

    def red(uc, c, d, st):
        ev.append('D%x' % r32(st[0]))
        return 0

    def spend(uc, c, d, st):
        ev.append('S%x' % r32(st[1]))
        return 0

    def rm(uc, c, d, st):
        ev.append('R')
        return 0

    T.trace_call(MS_HOOK, BUFF, (REC,), stub_fn={SMART_PTR: smart, PROP_GET: getter, REDUCE: red, MANA_CHANGE: spend, REMOVE_BUFF: rm},
                 span=0x300, cleanup={SMART_PTR: 4, REDUCE: 8, MANA_CHANGE: 12, REMOVE_BUFF: 4})
    return 'MS %x %d %x %x %x %x %x | %s' % (flags, srcnull, share, per, amount, mana, after, ' '.join(ev) or '-')


def case_rr(r):
    prep()
    REC, XA = T.D + 0x4800, T.D + 0x4A00
    mu.mem_write(REC, bytes(0x40))
    amount = nice(r)
    x = r.choice([amount, nice(r), nice(r), 0, 0x80000000, 0x7FC00000])
    w32(REC + 0x10, amount)
    w32(REC + 0x18, r.choice([0, nice(r)]))
    w32(XA, x)
    w32(XA + 4, 0)
    T.trace_call(REDUCE, REC, (XA, XA + 4), span=0x100)
    return 'RR %x %x | %x' % (amount, x, r32(REC + 0x10))


GENS = [('IC', case_ic), ('ID', case_id), ('VG', case_vg), ('DA', case_da), ('MS', case_ms), ('RR', case_rr)]


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
