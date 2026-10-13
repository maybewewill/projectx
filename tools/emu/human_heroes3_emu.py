"""Unicorn checks of the channel lifecycle and the Blizzard channel duration of Game.dll 1.26a (S35).

ORIGINAL code of the user's Game.dll, run for real (docs/spec/abilities-human-heroes.md, section 14):
  1. Channel duration of Blizzard: the ability method `0x6F1B9E90` (the point-form parameter hand-over) ends by
     storing a real in the property at ability `+0x90` (the "channel time" the cast core arms the `0xD01BD` timer
     with). Rows: level fields cast, DataA -> the stored value (bits).
  2. The two property compares of the cast tail (`0x6F063760`): `0x6F022270` (property differs from a constant, with
     the 0.001 epsilon, false on NaN) and `0x6F022340` (the same without the NaN test). Rows: property bits,
     constant bits -> result.

Usage: GAME_DLL=/path/Game.dll py tools/emu/human_heroes3_emu.py [--dump OUT_DIR] [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import human_heroes2_emu as H2  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
wr, rd, f2u = A.wr, A.rd, A.f2u
DATA = emu.DATA

F_ARGS = 0x6F1B9E90
F_SETUP = 0x6F1B9BB0
F_UNIT_FIN = 0x6F077630
F_PROP_NE = 0x6F022270
F_PROP_NE2 = 0x6F022340
FLOATMINI_VT = 0x6F876D38
VT_BLIZZARD = 0x6F910CAC
CONST = DATA + 0x6300


def rbits(r, small=False):
    k = r.random()
    if k < 0.08:
        return r.choice([0, 0x80000000, 0x7F800000, 0x7FC00000, 0x3F800000, 0xBF800000, 1, 0x7F7FFFFF])
    if k < 0.85:
        return f2u(r.choice([r.uniform(0, 12), float(r.randint(0, 12)), r.uniform(-3, 3), r.uniform(0, 1), 1.0]))
    return r.getrandbits(32)


def install():
    H2.install()
    # the 20-argument effect setup is not under test here: return at once, cleaning its stack arguments
    A.patch_jump(F_SETUP, A.stub_ret(0x50, 0))
    A.patch_jump(F_UNIT_FIN, A.stub_ret(0, 0))
    A.patch_jump(0x6F0419C0, A.stub_ret(4, 0))   # agent handle assignment (needs the agent world)


def check_channel_time(cases, r, rows):
    for _ in range(cases):
        cast, data_a = rbits(r), rbits(r)
        A.make_ability(VT_BLIZZARD, 0x41486262, 75)
        L = A.LVL
        wr(L + 0x04, cast)
        wr(L + 0x20, data_a)
        wr(A.OBJ + 0x9C, 0)
        wr(A.OBJ + 0x90, FLOATMINI_VT)
        wr(A.OBJ + 0x94, 0xDEADBEEF)
        mu.mem_write(H2.OBJ2, b"\0" * 0x400)
        wr(H2.BUFFS, 0x1111)
        wr(L + 0x4C, 2)
        wr(L + 0x50, H2.BUFFS)
        wr(H2.EF, 0x3333)
        wr(L + 0x5C, 1)
        wr(L + 0x60, H2.EF)
        emu.call(F_ARGS, ecx=A.OBJ, stack_args=(H2.OBJ2, f2u(1.0), f2u(2.0)))
        rows.append((cast, data_a, rd(A.OBJ + 0x94)))


def check_prop_compare(cases, r, rows):
    obj = DATA + 0x6400
    consts = [0, 0x7F7FFFFF, 0x3F800000]
    for _ in range(cases):
        prop = rbits(r)
        if r.random() < 0.3:
            prop = r.choice([0, 0x7F7FFFFF, f2u(0.0005), f2u(0.001), f2u(0.0011), 0x80000000])
        c = r.choice(consts) if r.random() < 0.7 else rbits(r)
        mu.mem_write(obj, b"\0" * 0x10)
        wr(obj + 4, prop)
        wr(CONST, c)
        a = emu.call(F_PROP_NE, ecx=obj, edx=CONST) & 0xFF
        b = emu.call(F_PROP_NE2, ecx=obj, edx=CONST) & 0xFF
        rows.append((prop, c, a, b))


F_WAVE_UNIT = 0x6F1AF950
EFFECT = DATA + 0x9000      # fake CEffectBlizzard
EVT = DATA + 0x9400         # its vtable (slots 0x80, 0x84, 0x8C are code stubs)
TARGET = DATA + 0x9800      # fake target unit
BUFFOBJ = DATA + 0x9C00     # fake existing BHbz buff (property at +0xCC, value at +0xD0)
BVT = DATA + 0xA000         # its vtable (slot 0x350 stub)
CAPT = DATA + 0xA400        # capture cells
DPTR = DATA + 0xA480        # the damage cell passed by pointer
CODE = DATA + 0xA800


def asm_stubs():
    """Machine code for the three effect slots and the buff's set-duration slot (records its arguments)."""
    def a32(x):
        return struct.pack("<I", x)
    # slot 0x8C: mov eax,[CAPT+0x20]; ret
    neg = bytes([0xA1]) + a32(CAPT + 0x20) + b"\xC3"
    # slot 0x84 (unit, &interval, &damage): record unit, damage, interval, bump the call counter; return the buff
    s84 = (b"\x8B\x44\x24\x04" + b"\xA3" + a32(CAPT + 0x00)
           + b"\x8B\x44\x24\x0C\x8B\x00" + b"\xA3" + a32(CAPT + 0x04)
           + b"\x8B\x44\x24\x08\x8B\x00" + b"\xA3" + a32(CAPT + 0x08)
           + b"\xA1" + a32(CAPT + 0x0C) + b"\x40" + b"\xA3" + a32(CAPT + 0x0C)
           + b"\xA1" + a32(CAPT + 0x24) + b"\xC2\x0C\x00")
    # slot 0x80: ret 0xC (the optional debuff; the numeric test keeps its durations at 0)
    s80 = b"\xC2\x0C\x00"
    # buff slot 0x350 (set duration, two arguments): mov [CAPT+0x10],1; ret 8
    s350 = b"\xC7\x05" + a32(CAPT + 0x10) + a32(1) + b"\xC2\x08\x00"
    # the smart-pointer assignment 0x6F038450 does a run-time type check through the agent registry; replace it by
    # its effect (store the pointer, add one reference), `ret 4`
    sp = b"\x8B\x44\x24\x04\x89\x01\x85\xC0\x74\x03\xFF\x40\x04\x8B\xC1\xC2\x04\x00"
    mu.mem_write(CODE + 0x200, sp)
    A.patch_jump(0x6F038450, CODE + 0x200)
    mu.mem_write(CODE + 0x240, sp[:-3] + b"\xC2\x08\x00")
    A.patch_jump(0x6F042B20, CODE + 0x240)   # the same for the buff reference (two arguments)
    mu.mem_write(CODE, neg)
    mu.mem_write(CODE + 0x40, s84)
    mu.mem_write(CODE + 0x100, s80)
    mu.mem_write(CODE + 0x140, s350)
    mu.mem_write(EVT, b"\0" * 0x400)
    wr(EVT + 0x8C, CODE)
    wr(EVT + 0x84, CODE + 0x40)
    wr(EVT + 0x80, CODE + 0x100)
    mu.mem_write(BVT, b"\0" * 0x400)
    wr(BVT + 0x350, CODE + 0x140)


def check_wave_unit(cases, r, rows):
    asm_stubs()
    for _ in range(cases):
        n = r.choice([1, 1, 2, 3, 4, 5, 8, 12, r.randint(1, 60)])
        cap = r.choice([0, f2u(0.0), f2u(r.uniform(1, 300)), f2u(r.uniform(-5, 5)), f2u(float(r.randint(1, 200)))]) \
            if r.random() < 0.9 else rbits(r)
        d = rbits(r) if r.random() < 0.15 else f2u(r.choice([r.uniform(1, 80), float(r.randint(1, 60))]))
        building = rbits(r) if r.random() < 0.2 else f2u(r.choice([0.5, 0.25, 1.0, r.uniform(0, 1.5)]))
        structure = r.random() < 0.3
        ward = r.random() < 0.15
        negate = int(r.random() < 0.2)
        has_buff = r.random() < 0.5
        old = rbits(r) if r.random() < 0.15 else f2u(r.choice([r.uniform(0, 80), float(r.randint(0, 60))]))
        mu.mem_write(EFFECT, b"\0" * 0x400)
        wr(EFFECT, EVT)
        wr(EFFECT + 0x80, f2u(1.0))
        wr(EFFECT + 0x94, cap)
        wr(EFFECT + 0xAC, building)
        for off in (0x35C, 0x364):             # the optional debuff durations: properties with value 0.0
            wr(EFFECT + off, FLOATMINI_VT)
            wr(EFFECT + off + 4, 0)
        mu.mem_write(TARGET, b"\0" * 0x400)
        wr(TARGET + 4, 100)                    # reference count
        mu.mem_write(TARGET + 0x33, b"A")      # a hero id: the hero branch needs no helper
        wr(TARGET + 0x5C, 0x10000 if structure else 0)
        wr(TARGET + 0x20, 0x200000 if ward else 0)
        mu.mem_write(BUFFOBJ, b"\0" * 0x200)
        wr(BUFFOBJ, BVT)
        wr(BUFFOBJ + 4, 100)
        wr(BUFFOBJ + 0xCC, FLOATMINI_VT)
        wr(BUFFOBJ + 0xD0, old)
        mu.mem_write(CAPT, b"\0" * 0x40)
        wr(CAPT + 0x20, negate)
        wr(CAPT + 0x24, BUFFOBJ if has_buff else 0)
        dptr = DPTR
        wr(dptr, d)
        emu.call(F_WAVE_UNIT, ecx=EFFECT, stack_args=(TARGET, n, dptr))
        out = [rd(CAPT + 0x04), rd(CAPT + 0x00), rd(CAPT + 0x0C), rd(BUFFOBJ + 0xD0), rd(CAPT + 0x10), rd(CAPT + 0x08)]
        rows.append([n, cap, d, building, int(structure), int(ward), negate, int(has_buff), old] + out)


F_MT_INIT = 0x6F060AE0
F_NORM = 0x6F2552F0
VT_MASS_TELEPORT = 0x6F88BEFC


def check_mt_init(cases, r, rows):
    """Mass Teleport level init `0x6F060AE0`: the channel-time property at `+0x90` gets `DataB` (the base init
    `0x6F05C6C0` is stubbed: it needs the unit-type tables)."""
    A.patch_jump(0x6F05C6C0, A.stub_ret(8, 0))
    for _ in range(cases):
        db = rbits(r)
        A.make_ability(VT_MASS_TELEPORT, 0x41486D74, 100)
        wr(A.LVL + 0x24, db)
        wr(A.LVL + 0x20, rbits(r))
        wr(A.OBJ + 0x90, FLOATMINI_VT)
        wr(A.OBJ + 0x94, 0xDEADBEEF)
        emu.call(F_MT_INIT, ecx=A.OBJ, stack_args=(H2.OBJ2, f2u(1.0)))
        rows.append((db, rd(A.OBJ + 0x94)))


def check_targs_normalise(cases, r, rows):
    """The default-filling normaliser of the flag word `0x6F2552F0` (ECX = word, returns EAX)."""
    words = [0, 0xFFFFFFFF, 0x40303006, 0xFDD1F00E, 0x1, 0x2, 0x4, 0x8, 0x2000, 0x100000, 0x200000]
    for i in range(cases):
        w = words[i] if i < len(words) else (r.getrandbits(32) & r.getrandbits(32) if r.random() < 0.7 else r.getrandbits(32))
        rows.append((w, emu.call(F_NORM, ecx=w)))


F_BANISH = 0x6F1CA2B0   # thiscall(ability), (target unit): the unit effect of Banish
BN_ABIL, BN_TARGET, BN_NEWBUFF, BN_BVT, BN_CAP, BN_CODE = (DATA + 0xD800, DATA + 0xDA00, DATA + 0xDC00, DATA + 0xDE00,
                                                         DATA + 0xDF00, DATA + 0xDF80)


def check_banish(cases, r, rows):
    """Banish's effect `0x6F1CA2B0`: duration (hero test through the real `0x6F1D2660`, then HeroDur or Dur + `+0x9C`), the
    negated DataA and DataB, as they reach the apply slot `0x354` of a new buff. Rows: dur, herodur, DataA, DataB,
    `+0x9C`, hero flag -> the three values read from the argument pointers."""
    # stubs: existing-buff lookup (none), new-buff allocator (our fake object), register on the unit (ret 4)
    A.patch_jump(0x6F0787D0, A.stub_ret(0x14, 0))
    A.patch_jump(0x6F1C8050, A.stub_ret(0, BN_NEWBUFF))
    A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))
    # slot 0x324 (ret 4); slot 0x354: copy the pointees of the five arguments' pointer parameters, ret 0x14
    mu.mem_write(BN_BVT, b"\0" * 0x400)
    mu.mem_write(BN_CODE, b"\xC2\x04\x00")
    wr(BN_BVT + 0x324, BN_CODE)
    c = b""
    for i, off in enumerate((0x8, 0xC, 0x10, 0x14)):     # [esp+4+4*k] k = 1..4 = P2..P5 ... read P2, P4, P5
        pass
    # P2 = [esp+8], P4 = [esp+0x10], P5 = [esp+0x14]; store *P2, *P4, *P5 into capture cells
    code = (b"\x8B\x44\x24\x08\x8B\x00\xA3" + struct.pack("<I", BN_CAP)
            + b"\x8B\x44\x24\x10\x8B\x00\xA3" + struct.pack("<I", BN_CAP + 4)
            + b"\x8B\x44\x24\x14\x8B\x00\xA3" + struct.pack("<I", BN_CAP + 8)
            + b"\xC2\x14\x00")
    mu.mem_write(BN_CODE + 0x20, code)
    wr(BN_BVT + 0x354, BN_CODE + 0x20)
    for _ in range(cases):
        dur, hdur, da, db, extra = rbits(r), rbits(r), rbits(r), rbits(r), rbits(r) if r.random() < 0.3 else 0
        hero = r.random() < 0.5
        A.make_ability(0x6F90F74C, 0x41486E62, 75)
        L = A.LVL
        wr(L + 0x08, dur)
        wr(L + 0x0C, hdur)
        wr(L + 0x20, da)
        wr(L + 0x24, db)
        wr(A.OBJ + 0x9C, extra)
        mu.mem_write(BN_TARGET, b"\0" * 0x200)
        mu.mem_write(BN_TARGET + 0x33, b"A" if hero else b"h")
        wr(BN_TARGET + 4, 100)
        mu.mem_write(BN_NEWBUFF, b"\0" * 0x200)
        wr(BN_NEWBUFF, BN_BVT)
        mu.mem_write(BN_CAP, b"\0" * 0x10)
        emu.call(F_BANISH, ecx=A.OBJ, stack_args=(BN_TARGET,))
        rows.append([dur, hdur, da, db, extra, int(hero), rd(BN_CAP), rd(BN_CAP + 4), rd(BN_CAP + 8)])


F_SLOW = 0x6F18A930     # thiscall(ability), (target unit): Thunder Clap's slow application


def check_thunder_slow(cases, r, rows):
    """Thunder Clap's slow `0x6F18A930` (new-buff path): hero test, `HeroDur` / `Dur + (+0x9C)`, the duration non-zero test,
    movement `-DataC` and attack speed `-DataD` as they reach the apply slot `0x354`. Rows: dur, herodur, DataC, DataD,
    `+0x9C`, hero flag -> applied, the three values."""
    A.patch_jump(0x6F0787D0, A.stub_ret(0x14, 0))
    A.patch_jump(0x6F188B30, A.stub_ret(0, BN_NEWBUFF))
    A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))
    asm_stubs()   # also installs the smart-reference stubs (0x6F038450, 0x6F042B20)
    mu.mem_write(BN_BVT, b"\0" * 0x400)
    mu.mem_write(BN_CODE, b"\xC2\x04\x00")
    wr(BN_BVT + 0x324, BN_CODE)
    code = (b"\x8B\x44\x24\x08\x8B\x00\xA3" + struct.pack("<I", BN_CAP)
            + b"\x8B\x44\x24\x10\x8B\x00\xA3" + struct.pack("<I", BN_CAP + 4)
            + b"\x8B\x44\x24\x14\x8B\x00\xA3" + struct.pack("<I", BN_CAP + 8)
            + b"\xA1" + struct.pack("<I", BN_CAP + 0xC) + b"\x40\xA3" + struct.pack("<I", BN_CAP + 0xC)
            + b"\xC2\x14\x00")
    mu.mem_write(BN_CODE + 0x60, code)   # a new address: Unicorn keeps translated blocks of rewritten code
    wr(BN_BVT + 0x354, BN_CODE + 0x60)
    for _ in range(cases):
        dur, hdur, dc, dd = rbits(r), rbits(r), rbits(r), rbits(r)
        extra = rbits(r) if r.random() < 0.3 else 0
        if r.random() < 0.15:
            dur = 0
        if r.random() < 0.15:
            hdur = 0
        hero = r.random() < 0.5
        A.make_ability(0x6F8F67C4, 0x41487463, 75)
        L = A.LVL
        wr(L + 0x08, dur)
        wr(L + 0x0C, hdur)
        wr(L + 0x28, dc)
        wr(L + 0x2C, dd)
        wr(A.OBJ + 0x9C, extra)
        mu.mem_write(BN_TARGET, b"\0" * 0x200)
        mu.mem_write(BN_TARGET + 0x33, b"A" if hero else b"h")
        wr(BN_TARGET + 4, 100)
        mu.mem_write(BN_NEWBUFF, b"\0" * 0x200)
        wr(BN_NEWBUFF, BN_BVT)
        mu.mem_write(BN_CAP, b"\0" * 0x20)
        emu.call(F_SLOW, ecx=A.OBJ, stack_args=(BN_TARGET,))
        rows.append([dur, hdur, dc, dd, extra, int(hero), rd(BN_CAP + 0xC), rd(BN_CAP), rd(BN_CAP + 4), rd(BN_CAP + 8)])


F_DISPEL = 0x6F079FE0 # thiscall(unit), 9 stack arguments: walks the unit's buff list and removes the selected ones
F_RESOLVE = 0x6F4786B0  # reference -> object
F_REMOVE = 0x6F079CC0   # thiscall(unit), (buff): remove a buff
DUNIT, DBUFF, DVT, DLOG, DCODE = DATA + 0xC200, DATA + 0xCC00, DATA + 0xC500, DATA + 0xC900, DATA + 0xCA00


def check_dispel(cases, r, rows):
    """The dispel walker `0x6F079FE0` (used by Divine Shield and Avatar on apply) against fake buff objects: each buff has
    random predicate answers (vtable slots 0x1D8 magic, 0x1DC physical, 0x1E0 negative, 0x1E4 positive, 0x1E8 aura, 0x32C
    undispellable, 0x330(arg) type test) and a flag word; the removal calls are logged. Rows: 9 arguments, then per buff
    its 8 predicate bits, then the removal mask and the returned count."""
    def getter(off):
        # mov eax,[ecx+off]; ret
        return b"\x8B\x81" + struct.pack("<I", off) + b"\xC3"
    offs = {0x1D8: 0x100, 0x1DC: 0x104, 0x1E0: 0x108, 0x1E4: 0x10C, 0x1E8: 0x110, 0x32C: 0x114}
    mu.mem_write(DVT, b"\0" * 0x400)
    for i, (slot, field) in enumerate(offs.items()):
        mu.mem_write(DCODE + 0x10 * i, getter(field))
        wr(DVT + slot, DCODE + 0x10 * i)
    # slot 0x330 takes one stack argument: mov eax,[ecx+0x118]; ret 4
    mu.mem_write(DCODE + 0x80, b"\x8B\x81" + struct.pack("<I", 0x118) + b"\xC2\x04\x00")
    wr(DVT + 0x330, DCODE + 0x80)
    # the reference resolver 0x6F4786B0: the reference cell holds the object address twice
    A.patch_jump(F_RESOLVE, DCODE + 0xA0)
    mu.mem_write(DCODE + 0xA0, b"\x8B\x01\xC3")           # mov eax,[ecx]; ret
    # the removal 0x6F079CC0: log the buff, unlink it from the unit's list (the walker re-reads the head afterwards)
    def on_remove(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        buff = struct.unpack("<I", bytes(uc.mem_read(esp + 4, 4)))[0]
        ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
        k = rd(DLOG)
        wr(DLOG + 4 + 4 * k, buff)
        wr(DLOG, k + 1)
        head = rd(DUNIT + 0x1DC)
        nxt = rd(buff + 0x24)
        if head == buff:
            wr(DUNIT + 0x1DC, nxt)
            wr(DUNIT + 0x1E0, nxt)
        else:
            cur = head
            while cur != 0xFFFFFFFF and rd(cur + 0x24) != buff:
                cur = rd(cur + 0x24)
            if cur != 0xFFFFFFFF:
                wr(cur + 0x24, nxt)
                wr(cur + 0x28, nxt)
        uc.reg_write(UC_X86_REG_ESP, esp + 8)
        uc.reg_write(UC_X86_REG_EIP, ret)
        uc.reg_write(UC_X86_REG_EAX, 0)
    if not getattr(check_dispel, "hooked", False):
        mu.hook_add(UC_HOOK_CODE, on_remove, begin=F_REMOVE, end=F_REMOVE)
        check_dispel.hooked = True
    for _ in range(cases):
        n = r.randint(0, 5)
        args = [r.getrandbits(1) for _ in range(9)]
        if r.random() < 0.2:
            args = [0, 1, 0, 0, 0, 1, 1, 0, 1]
        mu.mem_write(DUNIT, b"\0" * 0x300)
        mu.mem_write(DLOG, b"\0" * 0x80)
        bits = []
        addrs = [DBUFF + 0x200 * i for i in range(n)]
        for i, b in enumerate(addrs):
            mu.mem_write(b, b"\0" * 0x200)
            wr(b, DVT)
            pb = [r.getrandbits(1) for _ in range(6)] + [r.getrandbits(1), r.getrandbits(1)]  # + 0x330 result, flag 0x2000_0000
            for (slot, field), v in zip(offs.items(), pb[:6]):
                wr(b + field, v)
            wr(b + 0x118, pb[6])
            wr(b + 0x20, 0x20000000 if pb[7] else 0)
            nxt = addrs[i + 1] if i + 1 < n else 0xFFFFFFFF
            wr(b + 0x24, nxt)
            wr(b + 0x28, nxt)
            bits.append(pb)
        first = addrs[0] if n else 0xFFFFFFFF
        wr(DUNIT + 0x1DC, first)
        wr(DUNIT + 0x1E0, first)
        try:
            cnt = emu.call(F_DISPEL, ecx=DUNIT, stack_args=tuple(args))
        except Exception:
            print("dispel failed: args", args, "n", n, "bits", bits)
            raise
        removed = 0
        for j in range(rd(DLOG)):
            removed |= 1 << addrs.index(rd(DLOG + 4 + 4 * j))
        ints = [sum(v << k for k, v in enumerate(pb)) for pb in bits] + [0] * (5 - n)
        rows.append(args + [n] + ints + [removed, cnt])


F_DIFF3 = 0x6F020210   # ECX = out vector (3 reals), EDX = A, stack = B: out = A - B per component
F_SQLEN = 0x6F020000    # ECX = a pair (x, y): stores x*x + y*y at [ECX]
V_A, V_B, V_O = DATA + 0xB400, DATA + 0xB440, DATA + 0xB480


def check_corpse_distance(cases, r, rows):
    """The corpse record's real (`+8`): `0x6F020210` (3D difference of two positions) then `0x6F020000` (squared length
    of the first two components), exactly as `0x6F1C1B90` chains them."""
    for _ in range(cases):
        a = [rbits(r) if r.random() < 0.1 else f2u(r.uniform(-4000, 4000)) for _ in range(3)]
        b = [rbits(r) if r.random() < 0.1 else f2u(r.uniform(-4000, 4000)) for _ in range(3)]
        for i in range(3):
            wr(V_A + 4 * i, a[i])
            wr(V_B + 4 * i, b[i])
            wr(V_O + 4 * i, 0)
        emu.call(F_DIFF3, ecx=V_O, edx=V_A, stack_args=(V_B,))
        d = [rd(V_O + 4 * i) for i in range(3)]
        emu.call(F_SQLEN, ecx=V_O, stack_args=(V_O + 0x20,))   # the second argument is the scratch out pointer
        rows.append(a + b + d + [rd(V_O + 0x20)])


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 35
    r = random.Random(seed)
    install()
    ch, pc, wv = [], [], []
    check_channel_time(cases, r, ch)
    check_prop_compare(cases, r, pc)
    check_wave_unit(cases, r, wv)
    mt, nm, cd, dp, bn, ts = [], [], [], [], [], []
    check_dispel(cases, r, dp)
    check_banish(cases, r, bn)
    check_thunder_slow(cases, r, ts)
    check_mt_init(cases, r, mt)
    check_targs_normalise(cases, r, nm)
    check_corpse_distance(cases, r, cd)
    print("channel time rows %d, property compare rows %d, wave unit rows %d, mt init %d, targs %d"
          % (len(ch), len(pc), len(wv), len(mt), len(nm)))
    for row in mt[:4] + nm[:12]:
        print(" ".join("%08x" % x for x in row))
    if dump:
        os.makedirs(dump, exist_ok=True)
        with open(os.path.join(dump, "channel_time.txt"), "w") as fh:
            for row in ch:
                fh.write(" ".join("%08x" % x for x in row) + "\n")
        with open(os.path.join(dump, "prop_compare.txt"), "w") as fh:
            for row in pc:
                fh.write("%08x %08x %d %d\n" % row)
        with open(os.path.join(dump, "wave_unit.txt"), "w") as fh:
            for row in wv:
                fh.write(" ".join("%08x" % x for x in row) + "\n")
        with open(os.path.join(dump, "mt_init.txt"), "w") as fh:
            for row in mt:
                fh.write(" ".join("%08x" % x for x in row) + "\n")
        with open(os.path.join(dump, "thunder_slow.txt"), "w") as fh:
            for row in ts:
                fh.write(" ".join("%08x" % x for x in row) + "\n")
        with open(os.path.join(dump, "banish.txt"), "w") as fh:
            for row in bn:
                fh.write(" ".join("%08x" % x for x in row) + "\n")
        with open(os.path.join(dump, "dispel.txt"), "w") as fh:
            for row in dp:
                fh.write(" ".join("%x" % x for x in row) + "\n")
        with open(os.path.join(dump, "corpse_distance.txt"), "w") as fh:
            for row in cd:
                fh.write(" ".join("%08x" % x for x in row) + "\n")
        with open(os.path.join(dump, "targs_normalise.txt"), "w") as fh:
            for row in nm:
                fh.write(" ".join("%08x" % x for x in row) + "\n")


if __name__ == "__main__":
    main()
