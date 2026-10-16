"""Unicorn checks of the buff-giving unit spells of Game.dll 1.26a (task V1, docs/spec/abilities-unit-spells.md).

The ORIGINAL effect slot of the ability, the original buff-creation helper, the original buff apply slot and the original
"holder -> sink" routines run for real on fake objects; only the allocator of the new buff, the lookup of an existing
buff, the holder property objects (a set-value stub that stores the pointee of its argument, value at holder + 4) and the final sinks (which record their
arguments) are stubs.

Checks (rows are printed as hex words, see each function):
  bloodlust : effect 0x6F1CB080 -> CBuffBloodlust slot 0x35C (0x6F1C2870) -> 0x6F0D6460 -> sink routines 0x6F0D59A0 /
              0x6F0D59F0 -> the attack-speed sink 0x6F0C9090 and the movement sink 0x6F202FF0.
  innerfire : effect 0x6F13BFA0 -> 0x6F13BE70 -> CBuffInnerFire slot 0x354 (0x6F12CA40) -> the four helpers 0x6F1236F0
              (attack bonus), 0x6F1238B0 (armor sink 0x6F2769C0), 0x6F123920 / 0x6F123A00 (regeneration modifiers).

Usage: GAME_DLL=/path/Game.dll py tools/emu/spell_buff_emu.py [--dump OUT_DIR] [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402

mu = emu.mu
wr, rd, f2u = A.wr, A.rd, A.f2u

M = 0x00400000
mu.mem_map(M, 0x10000)
CAP = M + 0x0000
BUFF = M + 0x1000
FVT = M + 0x1800
HVT_A = M + 0x1C00     # holder vtable: slot 0 = set value (stores the pointee of the argument)
HVT_B = M + 0x1C20     # holder vtable: slot 0 = set value from a pointer
TGT = M + 0x2000
ATK = M + 0x2400
_code = [M + 0x3000]

VT_BLOODLUST_ABIL = 0x6F90EE4C
VT_INNERFIRE_ABIL = 0x6F8DC54C
VT_BLOODLUST_BUFF = 0x6F9032D4
VT_INNERFIRE_BUFF = 0x6F8A1AEC


def a32(x):
    return struct.pack("<I", x & 0xFFFFFFFF)


def new_code(code):
    """Writes machine code to a fresh address (Unicorn keeps translated blocks of rewritten code)."""
    a = _code[0]
    mu.mem_write(a, code)
    _code[0] += (len(code) + 0x1F) & ~0x1F
    return a


def rbits(r):
    k = r.random()
    if k < 0.08:
        return r.choice([0, 0x80000000, 0x7F800000, 0x7FC00000, 0x3F800000, 0xBF800000, 1, 0x7F7FFFFF])
    if k < 0.85:
        return f2u(r.choice([r.uniform(0, 12), float(r.randint(0, 12)), r.uniform(-3, 3), r.uniform(0, 1), 1.0,
                             r.uniform(0, 100), 0.1, 0.25, 0.4]))
    return r.getrandbits(32)


def sink(addr, nbytes, slot):
    """Records the first argument's pointee into CAP[slot] and counts the call in CAP[slot + 4]."""
    code = (b"\x8B\x44\x24\x04\x8B\x00" + b"\xA3" + a32(CAP + slot)
            + b"\xFF\x05" + a32(CAP + slot + 4) + b"\xC2" + struct.pack("<H", nbytes))
    A.patch_jump(addr, new_code(code))


def install():
    A.setup_stubs()
    # smart reference assignment helpers (run-time agent registry type checks), as in human_heroes3_emu
    sp = b"\x8B\x44\x24\x04\x89\x01\x85\xC0\x74\x03\xFF\x40\x04\x8B\xC1\xC2\x04\x00"
    A.patch_jump(0x6F038450, new_code(sp))
    A.patch_jump(0x6F042B20, new_code(sp[:-3] + b"\xC2\x08\x00"))
    A.patch_jump(0x6F0787D0, A.stub_ret(0x14, 0))      # lookup of an existing buff: none
    A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))         # register the buff on the unit
    A.patch_jump(0x6F26FE20, A.stub_ret(0, 0))         # unit stat refresh
    A.patch_jump(0x6F0766A0, A.stub_ret(0, 0))         # "has the hero ability" reference walk: no
    # sinks: first argument is a pointer to the real; record it
    sink(0x6F202FF0, 4, 0x10)      # movement multiplier sink
    sink(0x6F0C9090, 8, 0x20)      # attack speed sink
    sink(0x6F2769C0, 4, 0x30)      # armor sink
    # the regeneration modifier creators: (out slot, &value, &zero) -> record *value, clear the out slot, ret 0xC
    for fn, slot in ((0x6F02DC60, 0x40), (0x6F02DD00, 0x50)):
        code = (b"\x8B\x44\x24\x08\x8B\x00" + b"\xA3" + a32(CAP + slot)
                + b"\xFF\x05" + a32(CAP + slot + 4)
                + b"\x8B\x44\x24\x04\xC7\x00\x00\x00\x00\x00" + b"\xC2\x0C\x00")
        A.patch_jump(fn, new_code(code))
    A.patch_jump(0x6F029580, A.stub_ret(4, 0))         # store the modifier on the buff
    # holder vtables
    mu.mem_write(HVT_A, b"\0" * 0x40)
    wr(HVT_A, new_code(b"\x8B\x44\x24\x04\x8B\x00\x89\x41\x04\xC2\x08\x00"))
    wr(HVT_B, new_code(b"\x8B\x44\x24\x04\x8B\x00\x89\x41\x04\xC2\x08\x00"))


def make_target(hero):
    mu.mem_write(TGT, b"\0" * 0x400)
    mu.mem_write(TGT + 0x33, b"A" if hero else b"h")
    wr(TGT + 4, 100)
    wr(TGT + 0x1E8, ATK)
    wr(TGT + 0x1EC, ATK)           # the movement ability pointer (only tested for non-zero)


def make_buff(vt, holders_a, holders_b, overrides):
    """A fake buff with a copy of the real class vtable (selected slots replaced) and inline holders."""
    mu.mem_write(BUFF, b"\0" * 0x400)
    mu.mem_write(FVT, bytes(mu.mem_read(vt, 0x400)))
    for off, target in overrides.items():
        wr(FVT + off, target)
    wr(BUFF, FVT)
    wr(BUFF + 0x30, TGT)
    for h in holders_a:
        wr(BUFF + h, HVT_A)
    for h in holders_b:
        wr(BUFF + h, HVT_B)


def call(fn, **kw):
    """emu.call with a diagnostic on a fault (EIP, registers)."""
    try:
        return emu.call(fn, **kw)
    except Exception:
        from unicorn.x86_const import UC_X86_REG_EIP, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP
        regs = {n: mu.reg_read(c) for n, c in (("eip", UC_X86_REG_EIP), ("ecx", UC_X86_REG_ECX), ("edx", UC_X86_REG_EDX),
                                              ("esi", UC_X86_REG_ESI), ("edi", UC_X86_REG_EDI))}
        sys.stderr.write("fault in %#x: %s\n" % (fn, {k: hex(v) for k, v in regs.items()}))
        esp = mu.reg_read(UC_X86_REG_ESP)
        words = struct.unpack("<16I", bytes(mu.mem_read(esp, 64)))
        sys.stderr.write("stack: %s\n" % " ".join("%x" % w for w in words))
        raise


def clear_cap():
    mu.mem_write(CAP, b"\0" * 0x200)


def level(L, dur, hdur, data):
    wr(L + 0x08, dur)
    wr(L + 0x0C, hdur)
    for i, v in enumerate(data):
        wr(L + 0x20 + 4 * i, v)


def slot_capture2():
    """A buff slot (stdcall, three arguments) that records its first two at CAP + 0x80."""
    return new_code(b"\x8B\x44\x24\x04\xA3" + a32(CAP + 0x80) + b"\x8B\x44\x24\x08\x8B\x00\xA3" + a32(CAP + 0x84)
                    + b"\xC2\x0C\x00")


def check_bloodlust(cases, r, rows):
    """Rows: dur herodur DataA DataB DataC extra hero | slot-0x320 arg1 arg2 | movement sink value, calls | attack speed
    sink value, calls."""
    ret0 = A.stub_ret(0, 0)
    s320 = slot_capture2()
    s324 = A.stub_ret(4, 0)
    A.patch_jump(0x6F1C87A0, A.stub_ret(0, BUFF))
    for _ in range(cases):
        dur, hdur, da, db, dc = rbits(r), rbits(r), rbits(r), rbits(r), rbits(r)
        extra = rbits(r) if r.random() < 0.3 else 0
        hero = r.random() < 0.5
        A.make_ability(VT_BLOODLUST_ABIL, 0x41626C6F, 75)
        level(A.LVL, dur, hdur, [da, db, dc])
        wr(A.OBJ + 0x9C, extra)
        make_target(hero)
        make_buff(VT_BLOODLUST_BUFF, (0xCC, 0xD4, 0xFC, 0x104), (), {0x320: s320, 0x324: s324, 0x360: ret0})
        clear_cap()
        call(0x6F1CB080, ecx=A.OBJ, stack_args=(TGT,))
        rows.append([dur, hdur, da, db, dc, extra, int(hero), rd(CAP + 0x80), rd(CAP + 0x84),
                     rd(CAP + 0x10), rd(CAP + 0x14), rd(CAP + 0x20), rd(CAP + 0x24)])


def u32(x):
    return x & 0xFFFFFFFF


def check_innerfire(cases, r, rows):
    """Rows: dur herodur DataA DataB DataC DataD DataE extra hero | count0 sides0 bonus0 mask0 count1 sides1 bonus1 mask1 |
    slot-0x320 arg1 arg2 | attack counter +0xAC/+0xB0 deltas | stored amounts +0xE0/+0xE4 | armor sink value, calls |
    life regen value, calls | mana regen value, calls."""
    ret0 = A.stub_ret(0, 0)
    s320 = slot_capture2()
    s324 = A.stub_ret(4, 0)
    A.patch_jump(0x6F139970, A.stub_ret(0, BUFF))
    masks = [0x80, 1, 0x40, 0x100, 2, 0, 0xFFFF, 0x1C0, 0x7F, 0x181]
    for _ in range(cases):
        dur, hdur, da, db, dc, dd, de = (rbits(r) for _ in range(7))
        if r.random() < 0.5:
            da = f2u(r.choice([0.1, 0.05, 0.15, 0.25, r.uniform(-0.5, 0.5), 1.0, r.uniform(0, 2)]))
        if r.random() < 0.15:
            dd = 0
        if r.random() < 0.15:
            de = 0
        extra = rbits(r) if r.random() < 0.3 else 0
        hero = r.random() < 0.5
        weap = []
        for _w in range(2):
            count = r.choice([0, 1, 1, 2, 3, 5, r.randint(-3, 9), r.getrandbits(32)]) if r.random() < 0.95 else r.getrandbits(32)
            sides = r.choice([0, 2, 4, 6, 6, 8, 12, r.randint(-3, 20)])
            bonus = r.choice([0, 5, 10, 20, 50, r.randint(-20, 120), -1])
            mask = r.choice(masks) if r.random() < 0.8 else r.getrandbits(32)
            weap.append((u32(count), u32(sides), u32(bonus), u32(mask)))
        A.make_ability(VT_INNERFIRE_ABIL, 0x41696E66, 75)
        level(A.LVL, dur, hdur, [da, db, dc, dd, de])
        wr(A.OBJ + 0x9C, extra)
        make_target(hero)
        mu.mem_write(ATK, b"\0" * 0x400)
        wr(ATK + 0x30, TGT)
        for w, (count, sides, bonus, mask) in enumerate(weap):
            wr(ATK + 0x88 + 4 * w, count)
            wr(ATK + 0x94 + 4 * w, sides)
            wr(ATK + 0xA0 + 4 * w, bonus)
            wr(ATK + 0x218 + 4 * w, mask)
        base0, base1 = r.randint(0, 5), r.randint(0, 5)
        wr(ATK + 0xAC, base0)
        wr(ATK + 0xB0, base1)
        make_buff(VT_INNERFIRE_BUFF, (0xCC, 0xD4, 0xE8, 0xF0, 0xF8), (), {0x320: s320, 0x324: s324})
        clear_cap()
        call(0x6F13BFA0, ecx=A.OBJ, stack_args=(TGT,))
        rows.append([dur, hdur, da, db, dc, dd, de, extra, int(hero)]
                    + [x for w in weap for x in w]
                    + [rd(CAP + 0x80), rd(CAP + 0x84), u32(rd(ATK + 0xAC) - base0), u32(rd(ATK + 0xB0) - base1),
                       rd(BUFF + 0xE0), rd(BUFF + 0xE4), rd(CAP + 0x30), rd(CAP + 0x34), rd(CAP + 0x40), rd(CAP + 0x44),
                       rd(CAP + 0x50), rd(CAP + 0x54)])


VT_CRIPPLE_ABIL = 0x6F914394
VT_CRIPPLE_BUFF = 0x6F89EDCC


def random_weapons(r):
    masks = [0x80, 1, 0x40, 0x100, 2, 0, 0xFFFF, 0x1C0, 0x7F, 0x181]
    weap = []
    for _w in range(2):
        count = r.choice([0, 1, 1, 2, 3, 5, r.randint(-3, 9), r.getrandbits(32)]) if r.random() < 0.95 else r.getrandbits(32)
        sides = r.choice([0, 2, 4, 6, 6, 8, 12, r.randint(-3, 20)])
        bonus = r.choice([0, 5, 10, 20, 50, r.randint(-20, 120), -1])
        mask = r.choice(masks) if r.random() < 0.8 else r.getrandbits(32)
        weap.append((u32(count), u32(sides), u32(bonus), u32(mask)))
    return weap


def make_attack(weap, base0, base1):
    mu.mem_write(ATK, b"\0" * 0x400)
    wr(ATK + 0x30, TGT)
    for w, (count, sides, bonus, mask) in enumerate(weap):
        wr(ATK + 0x88 + 4 * w, count)
        wr(ATK + 0x94 + 4 * w, sides)
        wr(ATK + 0xA0 + 4 * w, bonus)
        wr(ATK + 0x218 + 4 * w, mask)
    wr(ATK + 0xAC, base0)
    wr(ATK + 0xB0, base1)


def check_cripple(cases, r, rows):
    """Rows: dur herodur DataA DataB DataC extra hero | weapons (4 words each) | slot-0x320 arg1 arg2 | movement sink value,
    calls | attack speed sink value, calls | attack counter deltas | stored amounts (+0xFC, +0x100) | holder +0xF4 value."""
    ret0 = A.stub_ret(0, 0)
    s320 = slot_capture2()
    s324 = A.stub_ret(4, 0)
    A.patch_jump(0x6F1C9090, A.stub_ret(0, BUFF))
    for _ in range(cases):
        dur, hdur, da, db, dc = (rbits(r) for _ in range(5))
        if r.random() < 0.5:
            dc = f2u(r.choice([0.1, 0.25, 0.5, 0.75, r.uniform(0, 1), r.uniform(-1, 2)]))
        extra = rbits(r) if r.random() < 0.3 else 0
        hero = r.random() < 0.5
        weap = random_weapons(r)
        A.make_ability(VT_CRIPPLE_ABIL, 0x41637269, 75)
        level(A.LVL, dur, hdur, [da, db, dc])
        wr(A.OBJ + 0x9C, extra)
        make_target(hero)
        base0, base1 = r.randint(0, 5), r.randint(0, 5)
        make_attack(weap, base0, base1)
        make_buff(VT_CRIPPLE_BUFF, (0xCC, 0xD4, 0xF0), (), {0x320: s320, 0x324: s324})
        wr(BUFF + 0xF8, 2)
        clear_cap()
        call(0x6F1CC450, ecx=A.OBJ, stack_args=(TGT,))
        rows.append([dur, hdur, da, db, dc, extra, int(hero)] + [x for w in weap for x in w]
                    + [rd(CAP + 0x80), rd(CAP + 0x84), rd(CAP + 0x10), rd(CAP + 0x14), rd(CAP + 0x20), rd(CAP + 0x24),
                       u32(rd(ATK + 0xAC) - base0), u32(rd(ATK + 0xB0) - base1), rd(BUFF + 0xFC), rd(BUFF + 0x100),
                       rd(BUFF + 0xF4)])


def check_command_aura(cases, r, rows):
    """The attack-bonus routine of the Command / Trueshot aura buffs `0x6F1ACC40` (real) on a fake buff. Rows: fraction
    flags(bits 8..10) slots | weapon 0 (count sides bonus targs kind) | weapon 1 | counter deltas | stored amounts +0xE4/+0xE8."""
    kinds = [1, 1, 2, 3, 4, 5, 6, 7, 8, 0, 9, r.randint(0, 12)]
    for _ in range(cases):
        frac = rbits(r)
        if r.random() < 0.5:
            frac = f2u(r.choice([0.05, 0.1, 0.15, 0.2, 0.25, 0.5, r.uniform(-0.5, 0.5), 1.0, 2.0, r.uniform(0, 3)]))
        flags = r.getrandbits(3) << 8
        slots = r.choice([1, 2, 2, 2])
        weap = random_weapons(r)
        make_target(False)
        base0, base1 = r.randint(0, 5), r.randint(0, 5)
        make_attack(weap, base0, base1)
        for w in range(2):
            wr(ATK + 0xDC + 4 * w, r.choice(kinds))
        mu.mem_write(BUFF, b"\0" * 0x400)
        wr(BUFF + 0x20, flags)
        wr(BUFF + 0x30, TGT)
        wr(BUFF + 0xD0, frac)
        wr(BUFF + 0xE0, slots)
        call(0x6F1ACC40, ecx=BUFF)
        rows.append([frac, flags, slots] + [x for w, t in zip(weap, (0, 1)) for x in w + (rd(ATK + 0xDC + 4 * t),)]
                    + [u32(rd(ATK + 0xAC) - base0), u32(rd(ATK + 0xB0) - base1), rd(BUFF + 0xE4), rd(BUFF + 0xE8)])


VT_AVATAR_ABIL = 0x6F90998C
VT_AVATAR_BUFF = 0x6F9039A4
UVT = M + 0x2C00


def check_avatar(cases, r, rows):
    """Avatar's cast slot `0x3C0` (`0x6F1CA180`, real) -> `CBuffAvatar` apply slot 214 (`0x6F1A03B0`, real) -> the stat sink
    helpers `0x6F196A40` (armor), `0x6F196A60` (life, mode 3), `0x6F196A80` (damage counter, real `0x6F0C6C10`). Rows:
    dur extra DataA DataB DataC DataD | set-duration args (owner, duration) | armor sink value, calls | life sink value,
    mode, calls | counter deltas | holders +0xD4 +0xDC, integer +0xE4, holder +0xE8 | immunity calls, argument."""
    A.patch_jump(0x6F1B69E0, A.stub_ret(0, 0))
    A.patch_jump(0x6F4778F0, A.stub_ret(0x14, 0))
    A.patch_jump(0x6F1C7F80, A.stub_ret(0, BUFF))
    A.patch_jump(0x6F4786B0, A.stub_ret(0, 0))
    A.patch_jump(0x6F1A1400, A.stub_ret(4, 0))
    A.patch_jump(0x6F052B60, A.stub_ret(8, 0))
    A.patch_jump(0x6F27B460, A.stub_ret(4, 0))
    # set duration (owner, &duration): record both
    code = (b"\x8B\x44\x24\x04\xA3" + a32(CAP + 0x90) + b"\x8B\x44\x24\x08\x8B\x00\xA3" + a32(CAP + 0x94)
            + b"\xC2\x08\x00")
    A.patch_jump(0x6F0D28F0, new_code(code))
    # the life sink 0x6F2AC450 (ecx = unit; &value, mode)
    code = (b"\x8B\x44\x24\x04\x8B\x00\xA3" + a32(CAP + 0x60) + b"\x8B\x44\x24\x08\xA3" + a32(CAP + 0x64)
            + b"\xFF\x05" + a32(CAP + 0x68) + b"\xC2\x08\x00")
    A.patch_jump(0x6F2AC450, new_code(code))
    # the magic immunity counter 0x6F07A3A0 (ecx = unit; flag)
    code = b"\x8B\x44\x24\x04\xA3" + a32(CAP + 0x74) + b"\xFF\x05" + a32(CAP + 0x70) + b"\xC2\x04\x00"
    A.patch_jump(0x6F07A3A0, new_code(code))
    mu.mem_write(UVT, b"\0" * 0x400)
    ret12 = A.stub_ret(0xC, 0)
    wr(UVT + 0x8C, ret12)
    wr(UVT + 0x90, ret12)
    for _ in range(cases):
        dur, hdur, extra = rbits(r), rbits(r), (rbits(r) if r.random() < 0.3 else 0)
        da, db, dc, dd = (rbits(r) for _ in range(4))
        if r.random() < 0.5:
            dc = f2u(float(r.randint(-5, 15)))
        A.make_ability(VT_AVATAR_ABIL, 0x41486176, 75)
        level(A.LVL, dur, hdur, [da, db, dc, dd])
        wr(A.OBJ + 0x9C, extra)
        wr(A.UNIT, UVT)
        wr(A.UNIT + 0x1E8, ATK)
        weap = random_weapons(r)
        base0, base1 = r.randint(0, 5), r.randint(0, 5)
        make_attack(weap, base0, base1)
        wr(ATK + 0x30, A.UNIT)
        make_buff(VT_AVATAR_BUFF, (0xD4, 0xDC, 0xE8), (), {0x320: A.stub_ret(0xC, 0), 0x324: A.stub_ret(4, 0)})
        wr(BUFF + 0x30, A.UNIT)
        clear_cap()
        call(0x6F1CA180, ecx=A.OBJ)
        rows.append([dur, extra, da, db, dc, dd, rd(CAP + 0x90), rd(CAP + 0x94), rd(CAP + 0x30), rd(CAP + 0x34),
                     rd(CAP + 0x60), rd(CAP + 0x64), rd(CAP + 0x68), u32(rd(ATK + 0xAC) - base0), u32(rd(ATK + 0xB0) - base1),
                     rd(BUFF + 0xD8), rd(BUFF + 0xE0), rd(BUFF + 0xE4), rd(BUFF + 0xEC), rd(CAP + 0x70), rd(CAP + 0x74)])


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 51
    r = random.Random(seed)
    install()
    out = {}
    out["bloodlust"] = []
    check_bloodlust(cases, r, out["bloodlust"])
    out["innerfire"] = []
    check_innerfire(cases, r, out["innerfire"])
    out["cripple"] = []
    check_cripple(cases, r, out["cripple"])
    out["command_aura"] = []
    check_command_aura(cases, r, out["command_aura"])
    out["avatar"] = []
    check_avatar(cases, r, out["avatar"])
    for name, rows in out.items():
        print("%s rows %d" % (name, len(rows)))
        for row in rows[:4]:
            print(" ".join("%08x" % x for x in row))
    if dump:
        os.makedirs(dump, exist_ok=True)
        for name, rows in out.items():
            with open(os.path.join(dump, name + ".txt"), "w") as fh:
                for row in rows:
                    fh.write(" ".join("%08x" % x for x in row) + "\n")


if __name__ == "__main__":
    main()
