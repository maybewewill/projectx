"""Unicorn numeric cores of the remaining item abilities of Game.dll 1.26a (docs/spec/abilities-items.md, task S41).

Companion of item_cores_emu.py (same method: ORIGINAL code on fake objects, the callees that need the object model are
stubbed and recorded, the soft-float library and the level data getters run for real). Rows go to
crates/sim/tests/item_ability_vectors2.txt and are recomputed by crates/sim/tests/item_ability_vectors2.rs.

Row kinds (hex unless noted, `|` separates input from output):
  GR  cool ignore                  | called cool_arg   group cooldown start at the end of a cast (0x6F05C840): the
                                                          inventory routine 0x6F0E3F30 is called with the ability's Cool
                                                          unless ignoreCD zeroed it or it is not above 0.0
  VI  code hero dur herodur extra  | dur               item buff effects AIvi (0x6F13C180) and AIvu (0x6F13C270):
                                                          the duration handed to the buff apply (code 0 = AIvi, 1 = AIvu)
  RL  v88 illusion misc            | called rate       Arel apply 0x6F0FB0C0: the life regeneration modifier (0x6F02DC60)
                                                          gets the ability value unless an illusion owner is blocked
  RM  typeregen x v88              | rate              AIrm rate 0x6F0E64E0: mul(add(type regenMana, hero int term), DataA)
  RH  misc inttotal                | x                 hero int regen term 0x6F2082C0: mul(Misc IntRegenBonus, I2R(int))

Usage: GAME_DLL=/path/Game.dll py tools/emu/item_cores2_emu.py OUT.txt [CASES] [SEED]
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
D = C.D
w32 = C.w32
r32 = C.r32
rand_real = C.rand_real
T.ALLOW.append((0x6F01FD40, 0x6F01FD60))  # the soft-float compare used by the cast tail
T.ALLOW.append((0x6F6EF000, 0x6F6F1000))

ITEMOBJ = D + 0x8000
INV = D + 0x8400
TYPE = D + 0x8800
HERO_TEST = 0x6F1D2660
FIND_BUFF = 0x6F0787D0
FAKE = 0x6F0D6500 + 0x10000  # an address nothing maps: the stubbed buff apply slot


# ---------------------------------------------------------------- GR: group cooldown start (cast end 0x6F05C840)
ENDCAST_ITEM = 0x6F05C840
VT_HEAL = 0x6F8C9F64


def case_gr(r):
    cool = rand_real(r)
    ignore = r.choice([0, 1])
    T.setup(VT_HEAL)
    mu.mem_write(ITEMOBJ, bytes(0x100))
    mu.mem_write(INV, bytes(0x100))
    rec = bytearray(bytes(mu.mem_read(T.REC, 0x68)))
    struct.pack_into('<I', rec, 0x14, cool)
    mu.mem_write(T.REC, bytes(rec))
    w32(T.ABIL + 0xA0, 0x100001)
    w32(T.ABIL + 0xA4, 0x100001)
    w32(ITEMOBJ + 0x30, TYPE)
    got = {}

    def inv_start(uc, c, d, st):
        got['called'] = (r32(st[1]),)
        return 0

    vt = lambda off: r32(VT_HEAL + off)
    stub_fn = {
        0x6F4786B0: lambda *x: ITEMOBJ,
        0x6F2B8DA0: lambda *x: ignore,
        0x6F0DB030: lambda *x: 0x41496E76,
        0x6F0787D0: lambda *x: INV,
        0x6F0E3F30: inv_start,
        vt(0x394): lambda *x: 1,
        vt(0x314): lambda *x: 1,
    }
    T.trace_call(ENDCAST_ITEM, T.ABIL, (), stub_fn=stub_fn, span=0x300)
    if 'called' in got:
        return 'GR %x %x | 1 %x' % (cool, ignore, got['called'][0])
    return 'GR %x %x | 0 0' % (cool, ignore)


# ---------------------------------------------------------------- VI: item buff effects with the hero duration rule
def case_vi(r):
    which = r.choice([0, 1])
    hero = r.choice([0, 1])
    dur, herodur, extra = (rand_real(r) for _ in range(3))
    vt, fn = [(0x6F8DCDF4, 0x6F13C180), (0x6F8DD224, 0x6F13C270)][which]
    T.setup(vt)
    rec = bytearray(bytes(mu.mem_read(T.REC, 0x68)))
    struct.pack_into('<I', rec, 0x08, dur)
    struct.pack_into('<I', rec, 0x0C, herodur)
    mu.mem_write(T.REC, bytes(rec))
    w32(T.ABIL + 0x9C, extra)
    mu.mem_write(C.BUFF, bytes(0x200))
    mu.mem_write(C.BVT, bytes(0x800))
    w32(C.BUFF, C.BVT)
    w32(C.BVT + 0x350, FAKE)
    got = {}

    def apply(uc, c, d, st):
        got['dur'] = r32(st[1])
        return 0

    T.trace_call(fn, T.ABIL, (T.UNIT,), stub_fn={HERO_TEST: lambda *x: hero, FIND_BUFF: lambda *x: C.BUFF, FAKE: apply},
                 span=0x200, cleanup={FAKE: 8})
    return 'VI %x %x %x %x %x | %x' % (which, hero, dur, herodur, extra, got.get('dur', 0xdeadbeef))


# ---------------------------------------------------------------- RL / RM / RH: the regeneration items
OUTP = D + 0x6000
VT_AREL = 0x6F880434
VT_AIRM = 0x6F8BA7FC


def case_rl(r):
    v88 = rand_real(r)
    illusion = r.choice([0, 1])
    misc = r.choice([0, 1])
    T.setup(VT_AREL)
    w32(T.ABIL + 0x88, v88)
    w32(T.UNIT + 0x5C, 0x40000000 if illusion else 0)
    got = {}

    def modifier(uc, c, d, st):
        got['rate'] = r32(st[1])
        return 0

    T.trace_call(0x6F0FB0C0, T.ABIL, (), stub_fn={0x6F009F90: lambda *x: misc, 0x6F02DC60: modifier}, span=0x200,
                 cleanup={0x6F02DC60: 12}, into=(0x6F0F4840,))
    if 'rate' in got:
        return 'RL %x %x %x | 1 %x' % (v88, illusion, misc, got['rate'])
    return 'RL %x %x %x | 0 0' % (v88, illusion, misc)


def case_rm(r):
    typeregen, x, v88 = rand_real(r), rand_real(r), rand_real(r)
    T.setup(VT_AIRM)
    w32(T.ABIL + 0x88, v88)
    w32(OUTP, 0xdeadbeef)

    def hero_term(uc, c, d, st):
        w32(st[0], x)
        return 0

    def type_regen(uc, c, d, st):
        w32(c, typeregen)
        return c

    T.trace_call(0x6F0E64E0, T.ABIL, (OUTP,), stub_fn={0x6F2082C0: hero_term, 0x6F29BF20: type_regen}, span=0x200,
                 cleanup={0x6F2082C0: 4})
    return 'RM %x %x %x | %x' % (typeregen, x, v88, r32(OUTP))


def case_rh(r):
    misc = rand_real(r)
    inttotal = r.choice([0, 1, 5, 20, 99, r.randint(-5, 200)])
    T.setup(VT_AIRM)
    w32(OUTP, 0xdeadbeef)

    def misc_real(uc, c, d, st):
        w32(c, misc)
        return c

    T.trace_call(0x6F2082C0, T.ABIL, (OUTP,), stub_fn={0x6F009EE0: misc_real, 0x6F0DB3A0: lambda *x: inttotal & 0xffffffff},
                 span=0x100, cleanup={0x6F009EE0: 8})
    return 'RH %x %x | %x' % (misc, inttotal & 0xffffffff, r32(OUTP))


GENS = [('GR', case_gr), ('VI', case_vi), ('RL', case_rl), ('RM', case_rm), ('RH', case_rh)]


def main():
    out = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 300
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    r = random.Random(seed)
    rows = []
    for name, fn in GENS:
        for _ in range(cases):
            rows.append(fn(r))
    with open(out, 'w') as fh:
        fh.write('\n'.join(rows) + '\n')
    print(len(rows), 'rows')


if __name__ == '__main__':
    main()
