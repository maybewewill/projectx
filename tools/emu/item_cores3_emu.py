"""Unicorn numeric cores of the item abilities of Game.dll 1.26a, task S53 (docs/spec/abilities-items.md section 17).

Companion of item_cores_emu.py / item_cores2_emu.py (same method: ORIGINAL code on fake objects, callees that need the
object model are stubbed and recorded, the soft-float library and the level data getters run for real). Rows go to
crates/sim/tests/item_ability_vectors3.txt and are recomputed by crates/sim/tests/item_ability_vectors3.rs.

Row kinds (hex unless noted, `|` separates input from output):
  CT  dataA dataB dur extra          | called tod dur    Moonstone (AIct) 0x6F109EB0: the time of day DataA + DataB / 60 and the
                                                          duration extra + Dur handed to the effect start 0x6F104570 (only when
                                                          the duration is above 0)
  PV  hero dur herodur extra A B     | dur p2 p3        Potion of Vampirism (AIpv) 0x6F109F70: the buff apply arguments
                                                          (duration by the hero rule, then DataB, then DataA)
  VP  mech rel misc1 vimm imm misc2 gate victill ratio amount | healed heal fx
                                                        the vampire buff's damage handler 0x6F0FB7D0
  FB  targs area v88 armed recflags  | called area value count flags class victim_skip deliver
                                                        Orb of Fire (AIfb) 0x6F0F2CD0: the area body arguments
  ZB  hero dur herodur ill           | dur_or_none      Orb of Freezing (AIzb) 0x6F108E20: the stun duration (hero rule)
  CR  hero dur herodur ill dataA     | dur amount       Orb of Corruption (AIcb) 0x6F109B40: duration and armor amount
  LB  armed vict ill found blocked   | ran              Orb of Lightning (old) (AIlb) 0x6F0F2F70: runs the AIlp ability's effect
  MS  n v0 .. v(n-1)                 | bonus            the unit's flat move speed bonus 0x6F077590 over n abilities
  TN  cx cy ux uy best haveBest first | replaced best  the Town Portal destination callback 0x6F0264B0 (nearest candidate)

Usage: GAME_DLL=/path/Game.dll py tools/emu/item_cores3_emu.py OUT.txt [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import item_cores_emu as C  # noqa: E402
import item_cores2_emu as C2  # noqa: E402

T = C.T
mu = C.mu
D = C.D
w32 = C.w32
r32 = C.r32
rand_real = C.rand_real
fbits = C.fbits
T.ALLOW.append((0x6F01FD00, 0x6F01FD60))
T.ALLOW.append((0x6F100880, 0x6F100890))
T.ALLOW.append((0x6F0FECC0, 0x6F0FECE0))
T.ALLOW.append((0x6F0FF220, 0x6F0FF230))
T.ALLOW.append((0x6F6EE000, 0x6F6F1000))

HERO_TEST = C2.HERO_TEST
FIND_BUFF = C2.FIND_BUFF
FAKE = C2.FAKE
VICTIM = C2.VICTIM
RECORD = C2.RECORD
AMOUNT = C2.AMOUNT


def setrec(**kw):
    """Level record fields: dur, herodur, area, a..i (bits)."""
    rec = bytearray(bytes(mu.mem_read(T.REC, 0x68)))
    offs = {'dur': 0x08, 'herodur': 0x0C, 'area': 0x18}
    for i, n in enumerate('abcdefghi'):
        offs[n] = 0x20 + 4 * i
    for k, v in kw.items():
        struct.pack_into('<I', rec, offs[k], v & 0xffffffff)
    mu.mem_write(T.REC, bytes(rec))


# ---------------------------------------------------------------- CT: Moonstone
VT_AICT = 0x6F8C1EA4
START = 0x6F104570


def case_ct(r):
    a, b, dur, extra = (rand_real(r) for _ in range(4))
    T.setup(VT_AICT)
    setrec(a=a, b=b, dur=dur)
    w32(T.ABIL + 0x9C, extra)
    got = {}

    def start(uc, c, d, st):
        got['tod'] = r32(c)
        got['dur'] = r32(d)
        return 0

    T.trace_call(0x6F109EB0, T.ABIL, (), stub_fn={START: start}, span=0x200)
    if 'tod' in got:
        return 'CT %x %x %x %x | 1 %x %x' % (a, b, dur, extra, got['tod'], got['dur'])
    return 'CT %x %x %x %x | 0 0 0' % (a, b, dur, extra)


# ---------------------------------------------------------------- PV: Potion of Vampirism
VT_AIPV = 0x6F8C8E14


def case_pv(r):
    hero = r.choice([0, 1])
    dur, herodur, extra, a, b = (rand_real(r) for _ in range(5))
    T.setup(VT_AIPV)
    setrec(dur=dur, herodur=herodur, a=a, b=b)
    w32(T.ABIL + 0x9C, extra)
    mu.mem_write(C.BUFF, bytes(0x200))
    mu.mem_write(C.BVT, bytes(0x800))
    w32(C.BUFF, C.BVT)
    w32(C.BVT + 0x358, FAKE)
    got = {}

    def apply(uc, c, d, st):
        got['dur'] = r32(st[1])
        got['p2'] = r32(st[2])
        got['p3'] = r32(st[3])
        return 0

    T.trace_call(0x6F109F70, T.ABIL, (T.UNIT,), stub_fn={HERO_TEST: lambda *x: hero, FIND_BUFF: lambda *x: C.BUFF, FAKE: apply},
                 span=0x300, cleanup={FAKE: 16})
    return 'PV %x %x %x %x %x %x | %x %x %x' % (hero, dur, herodur, extra, a, b, got.get('dur', 0xdeadbeef),
                                                got.get('p2', 0xdeadbeef), got.get('p3', 0xdeadbeef))


# ---------------------------------------------------------------- VP: the vampire buff damage handler
VT_BIPV = 0x6F8BC16C


def case_vp(r):
    mech = r.choice([0, 0, 0, 1])
    rel = r.choice([0, 1, 1, 1])
    misc1 = r.choice([0, 1])
    vimm = r.choice([0, 0, 0, 1])
    imm = r.choice([0, 0, 0, 1])
    misc2 = r.choice([0, 1])
    gate = r.choice([0, 0, 1])
    ownill = r.choice([0, 0, 1])
    ratio = rand_real(r)
    amount = rand_real(r)
    T.setup(VT_BIPV)
    w32(T.ABIL + 0xD0, ratio)
    mu.mem_write(VICTIM, bytes(0x300))
    w32(VICTIM, T.UVT)
    w32(VICTIM + 0x248, 8 if mech else 0)
    w32(VICTIM + 0x5C, 0x40000000 if ownill else 0)
    w32(AMOUNT, amount)
    got = {}

    def life(uc, c, d, st):
        got['heal'] = r32(st[1])
        return 0

    def fx(uc, c, d, st):
        got['fx'] = 1
        return 0

    def resolve(uc, c, d, st):
        w32(c, VICTIM)
        return c

    def misc(uc, c, d, st):
        # the key at 0x6F8B4A5C is read first (edx), the illusion key 0x6F876B18 second
        return misc1 if d == 0x6F8B4A5C else misc2

    T.trace_call(0x6F0FB7D0, T.ABIL, (RECORD, 0, AMOUNT),
                 stub_fn={0x6F038450: resolve, 0x6F285B20: lambda *x: rel, 0x6F009F90: misc, 0x6F28D750: lambda *x: imm,
                          0x6F277C20: life, 0x6F053040: fx, 0x6F0F57A0: lambda *x: gate, T.RETSTUB: lambda *x: vimm},
                 span=0x300, cleanup={0x6F038450: 4, 0x6F285B20: 4, 0x6F009F90: 4, 0x6F28D750: 8, 0x6F277C20: 8, 0x6F053040: 4,
                                      T.RETSTUB: 0})
    return 'VP %x %x %x %x %x %x %x %x %x %x | %d %x %d' % (mech, rel, misc1, vimm, imm, misc2, gate, ownill, ratio, amount,
                                                           1 if 'heal' in got else 0, got.get('heal', 0), got.get('fx', 0))


# ---------------------------------------------------------------- FB: Orb of Fire post-hit handler
VT_AIFB = 0x6F8BECEC
VVT = D + 0x9C00
S_POS = D + 0xA400
S_DELIVER = D + 0xA420
ESP = None


def stack_words(uc, n):
    from unicorn.x86_const import UC_X86_REG_ESP
    esp = uc.reg_read(UC_X86_REG_ESP)
    return struct.unpack('<%dI' % n, bytes(uc.mem_read(esp, 4 * n)))


def case_fb(r):
    targs = r.choice([0, 0, 0x4C0000, r.getrandbits(32)])
    area = rand_real(r)
    v88 = rand_real(r)
    armed = r.choice([0, 1, 1, 1])
    recflags = r.choice([0, 0x100, r.getrandbits(32)])
    T.setup(VT_AIFB, flags=0x100 if armed else 0)
    setrec(area=area)
    w32(T.ABIL + 0x4C, targs)
    w32(T.ABIL + 0x88, v88)
    mu.mem_write(VICTIM, bytes(0x300))
    w32(VICTIM, VVT)
    for i in range(0x200):
        w32(VVT + 4 * i, T.RETSTUB)
    w32(VVT + 0xB8, S_POS)
    w32(VVT + 0x120, S_DELIVER)
    mu.mem_write(RECORD, bytes(0x40))
    w32(RECORD + 0x0C, recflags)
    got = {}

    def resolve(uc, c, d, st):
        w32(c, VICTIM)
        return c

    def area_call(uc, c, d, st):
        w = stack_words(uc, 12)
        got['area'] = (r32(w[0]), r32(w[0] + 4), w[1], w[5], w[2], w[6] == T.UNIT, w[7], w[8] == 0x6F0DC570, w[9] == VICTIM, w[10], w[11])
        return 0

    def deliver(uc, c, d, st):
        got['deliver'] = (st[2], st[1] == RECORD)
        return 0

    T.trace_call(0x6F0F2CD0, T.ABIL, (VICTIM, RECORD),
                 stub_fn={0x6F0F2BE0: lambda *x: 1, 0x6F4743A0: lambda *x: 0, 0x6F2C31D0: area_call, 0x6F282F60: lambda *x: VICTIM,
                          0x6F038450: resolve, S_DELIVER: deliver, S_POS: lambda *x: 0},
                 span=0x400, cleanup={0x6F0F2BE0: 8, 0x6F038450: 4, 0x6F2C31D0: 0x30, S_DELIVER: 16, S_POS: 4, 0x6F4743A0: 0, 0x6F282F60: 0})
    out = 'FB %x %x %x %x %x |' % (targs, area, v88, armed, recflags)
    if 'area' in got:
        sa, sv, cnt, flags, cls, unit_is, m1, cb_ok, ctx_ok, w10, w11 = got['area']
        out += ' 1 %x %x %x %x %x %d %x %d %d %x %x' % (sa, sv, cnt, flags, cls, int(unit_is), m1, int(cb_ok), int(ctx_ok), w10, w11)
    else:
        out += ' 0 0 0 0 0 0 0 0 0 0 0 0'
    if 'deliver' in got:
        out += ' %x %d' % (got['deliver'][0], int(got['deliver'][1]))
    else:
        out += ' - -'
    return out


# ---------------------------------------------------------------- ZB / CR: the buff orbs (hero-rule duration)
VT_AIZB = 0x6F8BF01C
VT_AICB = 0x6F8C0764


def orb_buff_case(r, vt, fn, slot, extra_stubs, cleanup_extra, setup_rec, apply_args=4):
    hero = r.choice([0, 1])
    dur, herodur = rand_real(r), rand_real(r)
    ownill = r.choice([0, 0, 1])
    armed = r.choice([0, 1, 1, 1])
    T.setup(vt, flags=0x100 if armed else 0)
    setup_rec(dur, herodur)
    w32(T.UNIT + 0x5C, 0x40000000 if ownill else 0)
    mu.mem_write(VICTIM, bytes(0x300))
    w32(VICTIM, VVT)
    for i in range(0x200):
        w32(VVT + 4 * i, T.RETSTUB)
    w32(VVT + 0x120, S_DELIVER)
    mu.mem_write(C.BUFF, bytes(0x200))
    mu.mem_write(C.BVT, bytes(0x800))
    w32(C.BUFF, C.BVT)
    w32(C.BVT + slot, FAKE)
    mu.mem_write(RECORD, bytes(0x40))
    got = {}

    def resolve(uc, c, d, st):
        w32(c, VICTIM)
        return c

    def apply(uc, c, d, st):
        third = r32(st[2]) if (D <= st[2] < D + 0x10000 or 0x100000 <= st[2] < 0x110000) and st[2] != T.UNIT else int(st[2] == T.UNIT)
        got['apply'] = [r32(st[1]), third, st[3] if apply_args == 4 else 0]
        return 0

    stubs = {0x6F0F2BE0: lambda *x: 1, 0x6F282F60: lambda *x: VICTIM, 0x6F038450: resolve, HERO_TEST: lambda *x: hero,
             0x6F0787D0: lambda *x: C.BUFF, FAKE: apply}
    stubs.update(extra_stubs)
    cl = {0x6F0F2BE0: 8, 0x6F038450: 4, 0x6F0787D0: 20, FAKE: 4 * apply_args, S_DELIVER: 16}
    stubs[S_DELIVER] = lambda *x: 0
    cl.update(cleanup_extra)
    w32(RECORD + 0x0C, 0x100)
    T.trace_call(fn, T.ABIL, (VICTIM, RECORD), stub_fn=stubs, span=0x400, cleanup=cl)
    return hero, dur, herodur, ownill, armed, got


def case_zb(r):
    def rec(dur, herodur):
        setrec(dur=dur, herodur=herodur)

    hero, dur, herodur, ownill, armed, got = orb_buff_case(r, VT_AIZB, 0x6F108E20, 0x360, {}, {}, rec)
    out = 'ZB %x %x %x %x %x |' % (hero, dur, herodur, ownill, armed)
    if 'apply' in got:
        out += ' 1 %x %x %x' % tuple(got['apply'])
    else:
        out += ' 0 0 0 0'
    return out


def case_cr(r):
    a, b = rand_real(r), rand_real(r)

    def rec(dur, herodur):
        setrec(dur=dur, herodur=herodur, a=a, b=b)

    hero, dur, herodur, ownill, armed, got = orb_buff_case(r, VT_AICB, 0x6F109B40, 0x358, {}, {}, rec, apply_args=3)
    out = 'CR %x %x %x %x %x %x %x |' % (hero, dur, herodur, ownill, armed, a, b)
    if 'apply' in got:
        out += ' 1 %x %x %x' % tuple(got['apply'])
    else:
        out += ' 0 0 0 0'
    return out


# ---------------------------------------------------------------- LB: Orb of Lightning (old) runs the AIlp ability
VT_AILB = 0x6F8BF67C
LPA = D + 0xB000
LVT = D + 0xB100
S_BLK = D + 0xA440
S_EFF = D + 0xA460


def case_lb(r):
    armed = r.choice([0, 1, 1, 1])
    vict = r.choice([0, 1, 1, 1])
    ownill = r.choice([0, 0, 1])
    found = r.choice([0, 1, 1, 1])
    blocked = r.choice([0, 0, 1])
    T.setup(VT_AILB, flags=0x100 if armed else 0)
    w32(T.UNIT + 0x5C, 0x40000000 if ownill else 0)
    mu.mem_write(VICTIM, bytes(0x300))
    w32(VICTIM, VVT)
    for i in range(0x200):
        w32(VVT + 4 * i, T.RETSTUB)
    w32(VVT + 0x120, S_DELIVER)
    mu.mem_write(LPA, bytes(0x100))
    w32(LPA, LVT)
    for i in range(0x200):
        w32(LVT + 4 * i, T.RETSTUB)
    w32(LVT + 0x42C, S_BLK)
    w32(LVT + 0x424, S_EFF)
    mu.mem_write(RECORD, bytes(0x40))
    w32(RECORD + 0x0C, 0x100)
    got = {}

    def eff(uc, c, d, st):
        got['eff'] = (c == LPA, st[0])
        return 0

    def deliver(uc, c, d, st):
        got['deliver'] = 1
        return 0

    ret = T.trace_call(0x6F0F2F70, T.ABIL, (VICTIM, RECORD),
                       stub_fn={0x6F0F2BE0: lambda *x: 1, 0x6F282F60: lambda *x: VICTIM if vict else 0,
                                0x6F0787D0: lambda *x: LPA if found else 0, S_BLK: lambda *x: blocked, S_EFF: eff,
                                S_DELIVER: deliver},
                       span=0x200, cleanup={0x6F0F2BE0: 8, 0x6F0787D0: 20, S_BLK: 4, S_EFF: 4, S_DELIVER: 16})[0]
    return 'LB %x %x %x %x %x | %d %d %d' % (armed, vict, ownill, found, blocked, 1 if 'eff' in got else 0,
                                           1 if 'deliver' in got else 0, ret & 0xff)


# ---------------------------------------------------------------- MS: the flat move speed bonus (max over abilities)
UNITX = D + 0xC000
ABOBJ = D + 0xC400
AVT = D + 0xE000
ABSTRIDE = 0x100


def case_ms(r):
    n = r.choice([0, 1, 1, 2, 2, 3, 4, 5])
    vals = [rand_real(r) for _ in range(n)]
    mu.mem_write(UNITX, bytes(0x300))
    mu.mem_write(AVT, bytes(0x800))
    for i in range(0x200):
        w32(AVT + 4 * i, 0x6F0FF220)
    objs = []
    for i in range(n):
        o = ABOBJ + i * ABSTRIDE
        mu.mem_write(o, bytes(ABSTRIDE))
        w32(o, AVT)
        w32(o + 0x88, vals[i])
        if i + 1 < n:
            w32(o + 0x24, i + 2)
            w32(o + 0x28, i + 2)
        else:
            w32(o + 0x24, 0xFFFFFFFF)
            w32(o + 0x28, 0xFFFFFFFF)
        objs.append(o)
    if n:
        w32(UNITX + 0x1DC, 1)
        w32(UNITX + 0x1E0, 1)
    else:
        w32(UNITX + 0x1DC, 0xFFFFFFFF)
        w32(UNITX + 0x1E0, 0xFFFFFFFF)
    outp = D + 0xD000
    w32(outp, 0xdeadbeef)

    def resolve(uc, c, d, st):
        lo = r32(c)
        idx = lo
        if lo == 0xFFFFFFFF or idx < 1 or idx > n:
            return 0
        return objs[idx - 1]

    T.trace_call(0x6F077590, UNITX, (outp,), stub_fn={0x6F4786B0: resolve}, span=0x80, cleanup={0x6F4786B0: 0})
    return 'MS %d%s | %x' % (n, ''.join(' %x' % v for v in vals), r32(outp))


# ---------------------------------------------------------------- DD: the passive defense damage handler (AIdd, Defend)
VT_AIDD = 0x6F895574


def case_dd(r):
    row = r.choice([0, 1, 2, 3, 4, 5, 6, r.getrandbits(3)])
    armed = r.choice([0, 1])
    a, e, g, h = (rand_real(r) for _ in range(4))
    one = 0x3F800000
    if r.random() < 0.4:
        g = one if r.random() < 0.5 else g
        h = one if r.random() < 0.5 else h
    if r.random() < 0.3:
        e = one
    amt1, amt2 = rand_real(r), rand_real(r)
    T.setup(VT_AIDD, flags=0x200 if armed else 0)
    setrec(a=a, e=e, g=g, h=h)
    mu.mem_write(RECORD, bytes(0x40))
    w32(RECORD + 0x20, row)
    w32(RECORD + 0x10, amt1)
    w32(RECORD + 0x18, amt2)
    got = {}

    def fx(uc, c, d, st):
        got['fx'] = 1
        return 0

    T.trace_call(0x6F098F90, T.ABIL, (RECORD,), stub_fn={0x6F052FB0: fx}, span=0x200)
    return 'DD %x %x %x %x %x %x %x %x | %x %x %d %x' % (row, armed, a, e, g, h, amt1, amt2, r32(RECORD + 0x10),
                                                         r32(RECORD + 0x18), got.get('fx', 0), r32(T.ABIL + 0x20) & 0x200)


# ---------------------------------------------------------------- SB: Spell Effect Bonus (AIsb) proc roll
VT_AISB = 0x6F8C0DF4
T.ALLOW.append((0x6F0E5080, 0x6F0E5200))
T.ALLOW.append((0x6F1D21F0, 0x6F1D25E6))  # ends before the hero test 0x6F1D2660
T.ALLOW.append((0x6F01FD00, 0x6F01FDA0))


def case_sb(r):
    typechar = r.choice([0x48, 0x41, 0x5A, 0x40, 0x5B, 0x6E, 0x6F, 0x4F, r.getrandbits(8)])
    flags248 = r.choice([0, 4, 0x8004, r.getrandbits(32)])
    pct = lambda: r.choice([0, 5, 10, 15, 20, 25, 35, 50, 75, 100, r.randint(0, 100), r.random() * 130 - 10])
    chances = []
    for _ in range(3):
        v = pct()
        chances.append(fbits(float(v)))
    counts = [r.choice([1, 1, 1, 2, 3, 5, 10, 21, r.randint(1, 40), 0, -1]) for _ in range(3)]
    draw = r.choice([fbits(r.random() * 0.5), fbits(r.random() * 0.1), fbits(r.random()), 0, 0x3F800000, 0x7FC00000])
    T.setup(VT_AISB, flags=0)
    setrec(b=chances[0], c=chances[1], d=chances[2])
    for i, cnt in enumerate(counts):
        w32(T.ABIL + 0xD0 + 4 * i, cnt)
    # the ability's own vtable: slot 0x268 (enabled) answers yes
    for i in range(0x200):
        pass
    mu.mem_write(VICTIM, bytes(0x300))
    w32(VICTIM, T.UVT)
    mu.mem_write(VICTIM + 0x33, bytes([typechar & 0xff]))
    w32(VICTIM + 0x248, flags248)
    got = {}

    def roll(uc, c, d, st):
        w32(c, draw)
        return c

    vt = r32(T.ABIL)
    # slot 0x268 of the ability is reached through the vtable of the class: stub by address
    slot268 = r32(vt + 0x268)
    T.trace_call(0x6F0F3DD0, T.ABIL, (VICTIM, 0, 0),
                 stub_fn={slot268: lambda *x: 1, 0x6F26D590: lambda *x: 0, 0x6F282F60: lambda *x: VICTIM,
                          0x6F28D750: lambda *x: 0, 0x6F2853D0: roll},
                 span=0x200, cleanup={slot268: 4, 0x6F26D590: 8, 0x6F28D750: 8, 0x6F2853D0: 0})
    armed = (r32(T.ABIL + 0x20) >> 10) & 1
    return 'SB %x %x %x %x %x %x %x %x %x | %d %x %x %x' % (
        typechar & 0xff, flags248, chances[0], chances[1], chances[2], counts[0] & 0xffffffff, counts[1] & 0xffffffff,
        counts[2] & 0xffffffff, draw, armed, r32(T.ABIL + 0xD0), r32(T.ABIL + 0xD4), r32(T.ABIL + 0xD8))


# ---------------------------------------------------------------- IL: Wand of Illusion (AIil) apply arguments
VT_AIIL = 0x6F8E0D5C
ILLU = D + 0xB800
S_APPLY = D + 0xA480
S_SLOT324 = D + 0xA4A0
S_EC = D + 0xA4C0
S_D4 = D + 0xA4E0


def case_il(r):
    hero = r.choice([0, 1])
    dur, herodur, extra, a, b = (rand_real(r) for _ in range(5))
    made = r.choice([0, 1, 1, 1])
    T.setup(VT_AIIL)
    setrec(dur=dur, herodur=herodur, a=a, b=b)
    w32(T.ABIL + 0x9C, extra)
    mu.mem_write(VICTIM, bytes(0x300))
    w32(VICTIM, T.UVT)
    mu.mem_write(C.BUFF, bytes(0x200))
    mu.mem_write(C.BVT, bytes(0x800))
    w32(C.BUFF, C.BVT)
    w32(C.BVT + 0x364, S_APPLY)
    w32(C.BVT + 0x324, S_SLOT324)
    w32(T.UVT + 0xEC, S_EC)
    w32(T.UVT + 0xD4, S_D4)
    got = {}

    def apply(uc, c, d, st):
        got['apply'] = (r32(st[1]), r32(st[2]), r32(st[3]), st[4] == T.UNIT)
        return 0

    T.trace_call(0x6F13DFD0, T.ABIL, (VICTIM,),
                 stub_fn={HERO_TEST: lambda *x: hero, 0x6F127E00: lambda *x: ILLU if made else 0, 0x6F13A670: lambda *x: C.BUFF,
                          S_APPLY: apply, S_SLOT324: lambda *x: 0, S_EC: lambda *x: 0, S_D4: lambda *x: 0},
                 span=0x200, cleanup={S_APPLY: 20, S_SLOT324: 4, S_EC: 8, S_D4: 4, 0x6F1D26B0: 4, 0x6F127E00: 4, 0x6F13A670: 0, 0x6F079CA0: 4,
                                      0x6F052BC0: 8, 0x6F299390: 4})
    out = 'IL %x %x %x %x %x %x %d |' % (hero, dur, herodur, extra, a, b, made)
    if 'apply' in got:
        out += ' 1 %x %x %x %d' % got['apply']
    else:
        out += ' 0 0 0 0 0'
    return out


GENS = [('CT', case_ct), ('PV', case_pv), ('VP', case_vp), ('FB', case_fb), ('ZB', case_zb), ('CR', case_cr), ('LB', case_lb),
        ('MS', case_ms), ('DD', case_dd), ('SB', case_sb), ('IL', case_il)]


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
