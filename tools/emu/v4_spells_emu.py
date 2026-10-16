"""Unicorn checks of the melee unit spells of Game.dll 1.26a (task V4, docs/spec/abilities-unit-spells.md section 13).

The ORIGINAL effect / target-rule / autocast-filter functions run on fake objects (the pattern of spell_buff_emu.py);
only world-dependent callees are stubs that read per-case switches or record their arguments. Rows are dumped as hex
words and re-computed by crates/sim/tests/v4_spell_vectors.rs with the Rust model.

Usage: GAME_DLL=/path/Game.dll py tools/emu/v4_spells_emu.py [--dump DIR] --only NAME [CASES] [SEED]
(each check patches functions of the image for good, so run one check per process.)
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import spell_buff_emu as S  # noqa: E402
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402

mu = emu.mu
wr, rd, f2u = A.wr, A.rd, A.f2u
a32, new_code, level, rbits, u32 = S.a32, S.new_code, S.level, S.rbits, S.u32
CAP, BUFF, TGT, M = S.CAP, S.BUFF, S.TGT, S.M
VARS = M + 0xC000        # per-case switches read by stubs


def misc_real_stub(addr, var):
    """Misc real reader: ecx = out, edx = section, stack (key, default), retn 8. Writes the float at VARS+var."""
    A.patch_jump(addr, new_code(b"\xA1" + a32(VARS + var) + b"\x89\x01\x89\xC8\xC2\x08\x00"))


def misc_int_stub(addr, var):
    """Misc integer reader: ecx = section, edx = key, stack (default), retn 4. Returns the int at VARS+var."""
    A.patch_jump(addr, new_code(b"\xA1" + a32(VARS + var) + b"\xC2\x04\x00"))


def ret_var(addr, nbytes, var):
    code = b"\xA1" + a32(VARS + var) + ((b"\xC2" + struct.pack("<H", nbytes)) if nbytes else b"\xC3")
    A.patch_jump(addr, new_code(code))


def code_record_arg(argidx, slot, nbytes, deref=True, eax=0):
    """Code that records stack argument `argidx` (0 based; deref: its pointee) at CAP+slot, counts the call at
    CAP+slot+4 and returns eax."""
    code = b"\x8B\x44\x24" + bytes([4 + 4 * argidx])
    if deref:
        code += b"\x8B\x00"
    code += b"\xA3" + a32(CAP + slot) + b"\xFF\x05" + a32(CAP + slot + 4)
    code += b"\xB8" + a32(eax) + b"\xC2" + struct.pack("<H", nbytes)
    return new_code(code)


VT_HEAL_ABIL = 0x6F8C9674
ABIL_HEAL = 0x41686561


def check_heal_effect(cases, r, rows):
    """Heal effect slot 0x424 = 0x6F108280. Rows: DataA cool ethereal bonus existing | amount handed to the life adder,
    calls | marker duration, calls (slot 0x350 existing / 0x354 created)."""
    S.install()
    misc_real_stub(0x6F009EE0, 0)
    A.patch_jump(0x6F277C20, new_code(b"\x8B\x44\x24\x08\x8B\x00\xA3" + a32(CAP + 0x30) + b"\xFF\x05" + a32(CAP + 0x34)
                                      + b"\xC2\x08\x00"))
    A.patch_jump(0x6F0787D0, new_code(b"\xA1" + a32(VARS + 8) + b"\xC2\x14\x00"))
    A.patch_jump(0x6F106C00, A.stub_ret(0, BUFF))
    A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))
    s350 = code_record_arg(1, 0x40, 8)
    s354 = code_record_arg(1, 0x50, 0xC)
    s324 = A.stub_ret(4, 0)
    for _ in range(cases):
        da = rbits(r)
        cool = f2u(r.choice([0.0, 1.0, 5.0, r.uniform(0, 20), r.uniform(-2, 2)])) if r.random() < 0.85 else rbits(r)
        eth = r.choice([0, 0, 1, 2, -1, r.randint(-5, 5)])
        bonus = rbits(r)
        existing = r.random() < 0.4
        A.make_ability(VT_HEAL_ABIL, ABIL_HEAL, 75)
        level(A.LVL, 0, 0, [da])
        wr(A.LVL + 0x14, cool)
        wr(A.OBJ + 0x90, S.HVT_A)
        S.make_target(False)
        wr(TGT + 0x158, u32(eth))
        mu.mem_write(S.FVT, b"\0" * 0x400)
        wr(S.FVT + 0x324, s324)
        wr(S.FVT + 0x350, s350)
        wr(S.FVT + 0x354, s354)
        mu.mem_write(BUFF, b"\0" * 0x200)
        wr(BUFF, S.FVT)
        S.clear_cap()
        wr(VARS + 0, bonus)
        wr(VARS + 8, BUFF if existing else 0)
        S.call(0x6F108280, ecx=A.OBJ, stack_args=(TGT,))
        rows.append([da, cool, u32(eth), bonus, int(existing), rd(CAP + 0x30), rd(CAP + 0x34), rd(CAP + 0x40),
                     rd(CAP + 0x44), rd(CAP + 0x50), rd(CAP + 0x54)])


def check_heal_target(cases, r, rows):
    """Heal slot 0x3F0 = 0x6F0F0290 (target rule). Rows: can_restore illusion life max has_marker pair0 pair1 resolved
    ownerptr | error code."""
    S.install()
    misc_int_stub(0x6F009F90, 0)
    A.patch_jump(0x6F0787D0, new_code(b"\xA1" + a32(VARS + 8) + b"\xC2\x14\x00"))
    A.patch_jump(0x6F4786B0, new_code(b"\xA1" + a32(VARS + 12) + b"\xC3"))
    # slot 0x12C(out) returns a pointer to the life value; slot 0x130(out) (retn 4: the pushed pointer stays for the
    # soft subtract) returns a pointer to the max life: deficit = max - life
    s12c = new_code(b"\xB8" + a32(VARS + 16) + b"\xC2\x04\x00")
    s130 = new_code(b"\xB8" + a32(VARS + 20) + b"\xC2\x04\x00")
    for _ in range(cases):
        can = r.random() < 0.5
        ill = r.random() < 0.5
        life = rbits(r)
        mx = rbits(r)
        if r.random() < 0.5:
            mx = f2u(r.choice([100.0, 500.0, 1000.0]))
            life = f2u(r.choice([100.0, 99.5, 99.0, 1.0, 0.0, 250.0, 499.0, 500.0, 999.25]))
        marker = r.random() < 0.6
        pair0 = r.choice([0xFFFFFFFF, 0xFFFFFFFF, 0x100000, 7])
        pair1 = r.choice([0xFFFFFFFF, 0xFFFFFFFF, 0x2000, 0x100000])
        resolved = r.choice([0, 0x1234, 0x5678])
        ownerp = r.choice([0, 0, 0x1234, 0x9999])
        S.make_target(False)
        wr(TGT + 0x5C, 0x40000000 if ill else 0)
        mu.mem_write(S.FVT, b"\0" * 0x400)
        wr(TGT, S.FVT)
        wr(S.FVT + 0x12C, s12c)
        wr(S.FVT + 0x130, s130)
        wr(VARS + 16, life)
        wr(VARS + 20, mx)
        mu.mem_write(BUFF, b"\0" * 0x200)
        wr(BUFF + 0xCC, pair0)
        wr(BUFF + 0xD0, pair1)
        A.make_ability(VT_HEAL_ABIL, ABIL_HEAL, 75)
        wr(A.OBJ + 0x30, ownerp)
        wr(VARS + 0, 1 if can else 0)
        wr(VARS + 8, BUFF if marker else 0)
        wr(VARS + 12, resolved)
        A.patch_jump(A.F_OWNER, A.stub_ret(0, 0x1234))
        res = S.call(0x6F0F0290, ecx=A.OBJ, stack_args=(TGT,))
        rows.append([int(can), int(ill), life, mx, int(marker), pair0, pair1, resolved, ownerp, res & 0xFFFFFFFF])


def check_heal_filter(cases, r, rows):
    """Heal autocast filter slot 0x450 = 0x6F0F0370. Rows: base_ok get_autocast illusion ally regen has_marker pair0 pair1
    resolved ownerptr | result."""
    S.install()
    ret_var(0x6F030730, 8, 0)
    misc_int_stub(0x6F009F90, 4)
    ret_var(0x6F285B80, 4, 8)
    A.patch_jump(0x6F29BE20, new_code(b"\xA1" + a32(VARS + 12) + b"\x89\x01\x8B\xC1\xC3"))
    A.patch_jump(0x6F0787D0, new_code(b"\xA1" + a32(VARS + 16) + b"\xC2\x14\x00"))
    A.patch_jump(0x6F4786B0, new_code(b"\xA1" + a32(VARS + 20) + b"\xC3"))
    for _ in range(cases):
        base_ok = r.random() < 0.8
        getac = r.random() < 0.5
        ill = r.random() < 0.4
        ally = r.random() < 0.8
        regen = rbits(r) if r.random() < 0.5 else f2u(r.choice([0.0, -0.25, 1.0, -1.0, 2.5]))
        marker = r.random() < 0.5
        pair0 = r.choice([0xFFFFFFFF, 0xFFFFFFFF, 0x100000, 7])
        pair1 = r.choice([0xFFFFFFFF, 0xFFFFFFFF, 0x2000, 0x100000])
        resolved = r.choice([0, 0x1234, 0x5678])
        ownerp = r.choice([0, 0, 0x1234, 0x9999])
        S.make_target(False)
        wr(TGT + 0x5C, 0x40000000 if ill else 0)
        wr(TGT + 0x30, 0x55)
        mu.mem_write(BUFF, b"\0" * 0x200)
        wr(BUFF + 0xCC, pair0)
        wr(BUFF + 0xD0, pair1)
        A.make_ability(VT_HEAL_ABIL, ABIL_HEAL, 75)
        wr(A.OBJ + 0x30, ownerp)
        A.patch_jump(A.F_OWNER, A.stub_ret(0, 0x1234))
        wr(VARS + 0, int(base_ok))
        wr(VARS + 4, int(getac))
        wr(VARS + 8, int(ally))
        wr(VARS + 12, regen)
        wr(VARS + 16, BUFF if marker else 0)
        wr(VARS + 20, resolved)
        res = S.call(0x6F0F0370, ecx=A.OBJ, stack_args=(TGT, 0))
        rows.append([int(base_ok), int(getac), int(ill), int(ally), regen, int(marker), pair0, pair1, resolved, ownerp,
                     res & 0xFF])


def recorder(base, spec, nbytes, eax=0, regs=False, counter=0x1F8):
    """Stub that records stack arguments (spec: list of (argidx, deref)) as consecutive dwords at CAP+base, optionally
    ecx and edx first, counts the call at CAP+0x1F8 and returns eax (callee pops nbytes)."""
    code = b""
    off = base
    if regs:
        code += b"\x89\x0D" + a32(CAP + off) + b"\x89\x15" + a32(CAP + off + 4)
        off += 8
    for argidx, deref in spec:
        code += b"\x8B\x44\x24" + bytes([4 + 4 * argidx])
        if deref:
            code += b"\x8B\x00"
        code += b"\xA3" + a32(CAP + off)
        off += 4
    code += b"\xFF\x05" + a32(CAP + counter) + b"\xB8" + a32(eax) + b"\xC2" + struct.pack("<H", nbytes)
    return new_code(code)


WORKER_SPEC = [(0, False), (1, False), (2, False), (3, False), (4, False), (5, False), (6, False), (7, True), (8, True),
               (9, False)]
VTU = M + 0x2C00         # fake owner unit vtable (slot 0xEC)
VTT = M + 0x2E00         # fake target unit vtable (slot 0xEC)
CTX = M + 0x2F00         # callback context: [0] = ability


def worker_row():
    """The recorded worker call: ecx, edx, then ten stack arguments (pointer arguments dereferenced)."""
    return [rd(CAP + 0x100 + 4 * i) for i in range(12)]


def unit_vtables():
    mu.mem_write(VTU, b"\0" * 0x200)
    mu.mem_write(VTT, b"\0" * 0x200)
    wr(VTU + 0xEC, A.stub_ret(0, 0x77))
    wr(VTT + 0xEC, A.stub_ret(0, 0))


def check_dispel_args(cases, r, rows):
    """Dispel Magic's per-unit callback 0x6F0EF210 (ecx = unit, edx = context {ability}) -> the worker 0x6F0EEF00 (recorded).
    Rows: DataA DataB | ecx edx target a2 ability pos neg a6 a7 damage mana flags."""
    S.install()
    A.patch_jump(0x6F2AB540, A.stub_ret(0, 0))
    A.patch_jump(0x6F0EEF00, recorder(0x100, WORKER_SPEC, 0x28, regs=True))
    unit_vtables()
    for _ in range(cases):
        da, db = rbits(r), rbits(r)
        A.make_ability(0x6F8C3A24, 0x41646973, 75)
        level(A.LVL, rbits(r), rbits(r), [da, db])
        wr(A.OBJ + 0x30, A.UNIT)
        wr(A.UNIT, VTU)
        S.make_target(False)
        wr(TGT, VTT)
        wr(CTX, A.OBJ)
        S.clear_cap()
        S.call(0x6F0EF210, ecx=TGT, edx=CTX)
        rows.append([da, db] + worker_row())


def check_purge_args(cases, r, rows):
    """Purge effect 0x6F107E10 (ecx = ability, arg = target): the worker call (recorded), the gate, the hero rule and the
    buff apply 0x354 (recorded). Rows: DataA..DataF dur herodur flag100 allied hero existing | worker (12 words) |
    apply: target dur owner A B D E | apply calls | removal calls."""
    S.install()
    A.patch_jump(0x6F0EEF00, recorder(0x100, WORKER_SPEC, 0x28, regs=True))
    unit_vtables()
    ret_var(0x6F285AC0, 4, 0)
    ret_var(0x6F1D2660, 0, 4)
    A.patch_jump(0x6F0787D0, new_code(b"\xA1" + a32(VARS + 8) + b"\xC2\x14\x00"))
    A.patch_jump(0x6F079CC0, recorder(0x1D0, [(0, False)], 4, counter=0x1F4))
    A.patch_jump(0x6F106A60, A.stub_ret(0, BUFF))
    A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))
    A.patch_jump(0x6F2835A0, A.stub_ret(0x10, 0))
    A.patch_jump(0x6F0405D0, A.stub_ret(0xC, 0))
    s354 = recorder(0x180, [(0, False), (1, True), (2, False), (3, True), (4, True), (5, True), (6, True)], 0x1C)
    for _ in range(cases):
        data = [rbits(r) for _ in range(6)]
        dur, hdur = rbits(r), rbits(r)
        f100 = r.random() < 0.2
        allied = r.random() < 0.3
        hero = r.random() < 0.5
        existing = r.random() < 0.4
        A.make_ability(0x6F8C46CC, 0x41707267, 75)
        level(A.LVL, dur, hdur, data)
        wr(A.OBJ + 0x30, A.UNIT)
        wr(A.UNIT, VTU)
        S.make_target(False)
        wr(TGT, VTT)
        wr(TGT + 0x5C, 0x100 if f100 else 0)
        mu.mem_write(S.FVT, b"\0" * 0x400)
        wr(S.FVT + 0x324, A.stub_ret(4, 0))
        wr(S.FVT + 0x354, s354)
        mu.mem_write(BUFF, b"\0" * 0x200)
        wr(BUFF, S.FVT)
        S.clear_cap()
        wr(VARS + 0, int(allied))
        wr(VARS + 4, int(hero))
        wr(VARS + 8, BUFF if existing else 0)
        S.call(0x6F107E10, ecx=A.OBJ, stack_args=(TGT,))
        rows.append(data + [dur, hdur, int(f100), int(allied), int(hero), int(existing)] + worker_row()
                    + [rd(CAP + 0x180 + 4 * i) for i in range(7)] + [rd(CAP + 0x1F8), rd(CAP + 0x1F4)])


def check_abolish_args(cases, r, rows):
    """Abolish Magic effect 0x6F0EF0F0 (ecx = ability, arg = target): the worker call (recorded). Rows: misc_smart flags
    DataC DataI v4 DataA DataB | worker (12 words)."""
    S.install()
    A.patch_jump(0x6F0EEF00, recorder(0x100, WORKER_SPEC, 0x28, regs=True))
    unit_vtables()
    ret_var(0x6F285B00, 4, 0)
    misc_int_stub(0x6F009F90, 4)
    for _ in range(cases):
        misc = r.random() < 0.3
        flags = r.choice([0, 0x200000, 0x200000, 0x1, 0x200001, r.getrandbits(32)])
        dc = rbits(r) if r.random() < 0.5 else r.choice([0, 0x80000000, f2u(1.0), 0x7FC00000])
        di = r.choice([0, 0, 1, 0x80000000, 0x3F800000, r.getrandbits(32)])
        v4 = r.random() < 0.5
        da, db = rbits(r), rbits(r)
        A.make_ability(0x6F8C35AC, 0x41616D64, 75)
        level(A.LVL, rbits(r), rbits(r), [da, db, dc, 0, 0, 0, 0, 0, rbits(r)])
        wr(A.DEF + 0x40, di)
        wr(A.OBJ + 0x30, A.UNIT)
        wr(A.OBJ + 0x20, flags)
        wr(A.UNIT, VTU)
        S.make_target(False)
        wr(TGT, VTT)
        S.clear_cap()
        wr(VARS + 0, int(v4))
        wr(VARS + 4, int(misc))
        S.call(0x6F0EF0F0, ecx=A.OBJ, stack_args=(TGT,))
        rows.append([int(misc), flags, dc, di, int(v4), da, db] + worker_row())


def check_abolish_filter(cases, r, rows):
    """Abolish Magic autocast filter slot 0x450 = 0x6F0E2B90 (ecx = ability, args candidate, 0). Stubs: base filter, ally test,
    the buff scan 0x6F076D00 (recorded: ecx and nine stack arguments, returns 1). Rows: base_ok ownerflags cand248 ally
    reqLevel | result | scan calls | ecx | a2..a10."""
    S.install()
    ret_var(0x6F030730, 8, 0)
    ret_var(0x6F285B80, 4, 8)
    A.patch_jump(0x6F076D00, recorder(0x100, [(i, False) for i in range(9)], 0x24, eax=1, regs=True))
    for _ in range(cases):
        base_ok = r.random() < 0.8
        ownerflags = r.choice([0, 4, 4, 0, r.getrandbits(32)])
        cand248 = r.choice([0, 0x200, 0x200, 4, r.getrandbits(32)])
        ally = r.random() < 0.5
        req = r.choice([0, 0, 1, 2, 5, r.getrandbits(32)])
        A.make_ability(0x6F8C35AC, 0x41616D64, 75)
        wr(A.OBJ + 0x30, A.UNIT)
        wr(A.UNIT + 0x5C, ownerflags)
        wr(A.DEF + 0x40, req)
        S.make_target(False)
        wr(TGT + 0x248, cand248)
        S.clear_cap()
        wr(VARS + 0, int(base_ok))
        wr(VARS + 8, int(ally))
        res = S.call(0x6F0E2B90, ecx=A.OBJ, stack_args=(TGT, 0))
        rows.append([int(base_ok), ownerflags, cand248, int(ally), req, res & 0xFF, rd(CAP + 0x1F8)]
                    + [rd(CAP + 0x100 + 4 * i) for i in range(11)])


VT_POLY_ABIL = 0x6F8A922C
ABIL_POLY = 0x41706C79
AVT = M + 0x8000         # a writable copy of an ability vtable


def copy_vtable(vt, overrides):
    mu.mem_write(AVT, bytes(mu.mem_read(vt, 0x600)))
    for off, target in overrides.items():
        wr(AVT + off, target)
    wr(A.OBJ, AVT)


def check_polymorph_target(cases, r, rows):
    """Polymorph slot 0x3F0 = 0x6F11C230. Rows: DataA creep_over summoned | code."""
    S.install()
    ret_var(0x6F1D2630, 0, 0)
    for _ in range(cases):
        da = rbits(r) if r.random() < 0.4 else f2u(float(r.choice([0, 1, 5, 5, 6, -1, 3])))
        over = r.random() < 0.5
        summ = r.random() < 0.5
        A.make_ability(VT_POLY_ABIL, ABIL_POLY, 75)
        level(A.LVL, 0, 0, [da])
        S.make_target(False)
        wr(TGT + 0x248, 4 if summ else r.choice([0, 0x200, 8]))
        wr(VARS + 0, int(over))
        res = S.call(0x6F11C230, ecx=A.OBJ, stack_args=(TGT,))
        rows.append([da, int(over), int(summ), res & 0xFFFFFFFF])


def check_polymorph_effect(cases, r, rows):
    """Polymorph effect 0x6F12F560. Rows: dur herodur illusion hero movement | slot 0x124 calls, amount | pick args (level, movement)
    calls | slot 0x43C: target, &dur value, type, calls."""
    S.install()
    A.patch_jump(0x6F2835A0, A.stub_ret(0x10, 0))
    A.patch_jump(0x6F0405D0, A.stub_ret(0xC, 0))
    ret_var(0x6F1D2660, 0, 4)
    A.patch_jump(0x6F12DAA0, recorder(0x120, [(0, False), (1, False)], 8, eax=0x7777, counter=0x1E0))
    s43c = recorder(0x140, [(0, False), (1, True), (2, False)], 0x10, counter=0x1E4)
    s124 = recorder(0x160, [(0, True)], 4, counter=0x1E8)
    for _ in range(cases):
        dur, hdur = rbits(r), rbits(r)
        ill = r.random() < 0.3
        hero = r.random() < 0.5
        mv = r.choice([1, 2, 4, 8, 0x10, 0x20, 0])
        A.make_ability(VT_POLY_ABIL, ABIL_POLY, 75)
        level(A.LVL, dur, hdur, [rbits(r)])
        copy_vtable(VT_POLY_ABIL, {0x43C: s43c})
        S.make_target(False)
        wr(TGT + 0x5C, 0x40000000 if ill else 0)
        wr(TGT + 0x1FC, mv)
        mu.mem_write(S.FVT, b"\0" * 0x400)
        wr(TGT, S.FVT)
        wr(S.FVT + 0x124, s124)
        S.clear_cap()
        wr(VARS + 4, int(hero))
        S.call(0x6F12F560, ecx=A.OBJ, stack_args=(TGT,))
        rows.append([dur, hdur, int(ill), int(hero), mv, rd(CAP + 0x1E8), rd(CAP + 0x160), rd(CAP + 0x1E0), rd(CAP + 0x120),
                     rd(CAP + 0x124), rd(CAP + 0x1E4), rd(CAP + 0x140), rd(CAP + 0x144), rd(CAP + 0x148)])


TGT2 = M + 0x9000        # the attack target of an autocast candidate (refcount at +4, vtable slot 0xEC)
VT_CURSE_ABIL, ABIL_CURSE = 0x6F8A9694, 0x41637273
VT_FAERIE_ABIL, ABIL_FAERIE = 0x6F8AA36C, 0x41666165


def check_buff_apply(name, vt, code, effect, existing_slot, new_slot, creator, flavour):
    """The target effect of Curse / Faerie Fire: hero-rule duration and DataA reach the buff. Rows: dur herodur DataA extra hero
    existing | path (1 existing, 2 new) | smart arg-is-ability | duration | DataA | owner player | target."""
    def run(cases, r, rows):
        S.install()
        ret_var(0x6F1D2660, 0, 4)
        A.patch_jump(0x6F0787D0, new_code(b"\xA1" + a32(VARS + 8) + b"\xC2\x14\x00"))
        A.patch_jump(creator, A.stub_ret(0, BUFF))
        A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))
        unit_vtables()
        if flavour == "curse":      # (smart, &dur, &DataA, owner) existing / (target, &dur, &DataA, owner) new
            spec_e = [(1, True), (2, True), (3, False)]
            spec_n = [(0, False), (1, True), (2, True), (3, False)]
        else:                       # faerie: (smart, &dur, owner, &DataA) / (target, &dur, owner, &DataA)
            spec_e = [(1, True), (3, True), (2, False)]
            spec_n = [(0, False), (1, True), (3, True), (2, False)]
        s_e = recorder(0x140, spec_e, 0x10, counter=0x1E4)
        s_n = recorder(0x160, spec_n, 0x10, counter=0x1E8)
        s324 = A.stub_ret(4, 0)
        for _ in range(cases):
            dur, hdur, da = rbits(r), rbits(r), rbits(r)
            extra = rbits(r) if r.random() < 0.4 else 0
            hero = r.random() < 0.5
            existing = r.random() < 0.4
            A.make_ability(vt, code, 75)
            level(A.LVL, dur, hdur, [da])
            wr(A.OBJ + 0x9C, extra)
            wr(A.OBJ + 0x30, A.UNIT)
            wr(A.UNIT, VTU)
            S.make_target(False)
            mu.mem_write(S.FVT, b"\0" * 0x400)
            wr(S.FVT + 0x324, s324)
            wr(S.FVT + existing_slot, s_e)
            wr(S.FVT + new_slot, s_n)
            mu.mem_write(BUFF, b"\0" * 0x200)
            wr(BUFF, S.FVT)
            S.clear_cap()
            wr(VARS + 4, int(hero))
            wr(VARS + 8, BUFF if existing else 0)
            S.call(effect, ecx=A.OBJ, stack_args=(TGT,))
            base = 0x140 if existing else 0x160
            n = 3 if existing else 4
            rows.append([dur, hdur, da, extra, int(hero), int(existing), rd(CAP + 0x1E4), rd(CAP + 0x1E8)]
                        + [rd(CAP + base + 4 * i) for i in range(n)] + [0] * (4 - n))
    return run


def check_filter_pair(vt, code, filt, kind):
    def run(cases, r, rows):
        """Autocast filters of Curse (0x6F09BF80) and Faerie Fire (0x6F09C240). Rows: base has_attack target_present rel frac DataB
        attacks has_buff | result."""
        S.install()
        ret_var(0x6F030730, 8, 0)
        A.patch_jump(0x6F4786B0, new_code(b"\xA1" + a32(VARS + 12) + b"\xC3"))
        A.patch_jump(0x6F285B60, new_code(b"\xA1" + a32(VARS + 16) + b"\xC2\x04\x00"))
        A.patch_jump(0x6F0787D0, new_code(b"\xA1" + a32(VARS + 20) + b"\xC2\x14\x00"))
        A.patch_jump(0x6F020640, new_code(b"\x8B\x44\x24\x04\x8B\x15" + a32(VARS + 24) + b"\x89\x10\xC2\x04\x00"))
        ret_var(0x6F26D5D0, 0, 28)
        unit_vtables()
        for _ in range(cases):
            base = r.random() < 0.8
            has_attack = r.random() < 0.8
            tpresent = r.random() < 0.8
            rel = r.random() < 0.5
            frac = f2u(r.choice([0.05, 0.1, 0.11, 0.5, 1.0, 0.0])) if r.random() < 0.8 else rbits(r)
            db = r.choice([0, 0, f2u(1.0), 0x80000000, 0x7FC00000, rbits(r)])
            attacks = r.random() < 0.7
            hasbuff = r.random() < 0.4
            A.make_ability(vt, code, 75)
            level(A.LVL, 0, 0, [0, db])
            wr(A.OBJ + 0x30, A.UNIT)
            wr(A.UNIT, VTU)
            S.make_target(False)
            mu.mem_write(S.ATK, b"\0" * 0x400)
            wr(TGT + 0x1E8, S.ATK if has_attack else 0)
            wr(S.ATK + 0x6C, 0xFFFFFFFF if not tpresent else 5)
            wr(S.ATK + 0x70, 0xFFFFFFFF if not tpresent else 5)
            mu.mem_write(TGT2, b"\0" * 0x100)
            wr(TGT2 + 4, 100)
            wr(TGT2, VTT)
            wr(VARS + 0, int(base))
            wr(VARS + 12, TGT2)
            wr(VARS + 16, int(rel))
            wr(VARS + 20, 0x6666 if hasbuff else 0)
            wr(VARS + 24, frac)
            wr(VARS + 28, int(attacks and has_attack))
            res = S.call(filt, ecx=A.OBJ, stack_args=(TGT, 0))
            rows.append([int(base), int(has_attack), int(tpresent), int(rel), frac, db, int(attacks), int(hasbuff), res & 0xFF])
    return run


VT_UF_ABIL, ABIL_UF = 0x6F8F8984, 0x41756866
VT_REJ_ABIL, ABIL_REJ = 0x6F8CA7D4, 0x4172656A


def check_unholy_apply(cases, r, rows):
    """Unholy Frenzy effect 0x6F18C070 -> 0x6F18BF60: existing buff slot 0x37C (smart, &dur, owner, &DataB, &DataA), created
    buff slot 0x378 (target, &dur, owner, &DataB, &DataA). Rows: dur herodur DataA DataB extra hero existing | calls e, n |
    5 words."""
    S.install()
    ret_var(0x6F1D2660, 0, 4)
    A.patch_jump(0x6F0787D0, new_code(b"\xA1" + a32(VARS + 8) + b"\xC2\x14\x00"))
    A.patch_jump(0x6F1895C0, A.stub_ret(0, BUFF))
    A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))
    s_e = recorder(0x140, [(1, True), (2, False), (3, True), (4, True)], 0x14, counter=0x1E4)
    s_n = recorder(0x160, [(0, False), (1, True), (2, False), (3, True), (4, True)], 0x14, counter=0x1E8)
    s324 = A.stub_ret(4, 0)
    for _ in range(cases):
        dur, hdur, da, db = (rbits(r) for _ in range(4))
        extra = rbits(r) if r.random() < 0.4 else 0
        hero = r.random() < 0.5
        existing = r.random() < 0.4
        A.make_ability(VT_UF_ABIL, ABIL_UF, 75)
        level(A.LVL, dur, hdur, [da, db])
        wr(A.OBJ + 0x9C, extra)
        wr(A.OBJ + 0x30, A.UNIT)
        S.make_target(False)
        mu.mem_write(S.FVT, b"\0" * 0x400)
        wr(S.FVT + 0x324, s324)
        wr(S.FVT + 0x37C, s_e)
        wr(S.FVT + 0x378, s_n)
        mu.mem_write(BUFF, b"\0" * 0x200)
        wr(BUFF, S.FVT)
        S.clear_cap()
        wr(VARS + 4, int(hero))
        wr(VARS + 8, BUFF if existing else 0)
        S.call(0x6F18C070, ecx=A.OBJ, stack_args=(TGT,))
        base = 0x140 if existing else 0x160
        rows.append([dur, hdur, da, db, extra, int(hero), int(existing), rd(CAP + 0x1E4), rd(CAP + 0x1E8)]
                    + [rd(CAP + base + 4 * i) for i in range(5)])


def check_rejuv_effect(cases, r, rows):
    """Rejuvenation effect 0x6F108530 -> 0x6F1083D0: existing slot 0x358 (smart, &dur, &A, &B, f1, f2), created slot 0x354
    (target, &dur, &A, &B, f1, f2). Rows: dur herodur DataA DataB DataC extra hero existing | calls | 6 words."""
    S.install()
    ret_var(0x6F1D2660, 0, 4)
    A.patch_jump(0x6F0787D0, new_code(b"\xA1" + a32(VARS + 8) + b"\xC2\x14\x00"))
    A.patch_jump(0x6F106CD0, A.stub_ret(0, BUFF))
    A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))
    s_e = recorder(0x140, [(1, True), (2, True), (3, True), (4, False), (5, False)], 0x18, counter=0x1E4)
    s_n = recorder(0x160, [(0, False), (1, True), (2, True), (3, True), (4, False), (5, False)], 0x18, counter=0x1E8)
    s324 = A.stub_ret(4, 0)
    for _ in range(cases):
        dur, hdur, da, db = (rbits(r) for _ in range(4))
        dc = f2u(float(r.choice([0, 1, 2, 3, 4, -1, 7]))) if r.random() < 0.8 else rbits(r)
        extra = rbits(r) if r.random() < 0.4 else 0
        hero = r.random() < 0.5
        existing = r.random() < 0.4
        A.make_ability(VT_REJ_ABIL, ABIL_REJ, 75)
        level(A.LVL, dur, hdur, [da, db, dc])
        wr(A.OBJ + 0x9C, extra)
        S.make_target(False)
        mu.mem_write(S.FVT, b"\0" * 0x400)
        wr(S.FVT + 0x324, s324)
        wr(S.FVT + 0x358, s_e)
        wr(S.FVT + 0x354, s_n)
        mu.mem_write(BUFF, b"\0" * 0x200)
        wr(BUFF, S.FVT)
        S.clear_cap()
        wr(VARS + 4, int(hero))
        wr(VARS + 8, BUFF if existing else 0)
        S.call(0x6F108530, ecx=A.OBJ, stack_args=(TGT,))
        base = 0x140 if existing else 0x160
        rows.append([dur, hdur, da, db, dc, extra, int(hero), int(existing), rd(CAP + 0x1E4), rd(CAP + 0x1E8)]
                    + [rd(CAP + base + 4 * i) for i in range(6)])


def check_rejuv_buff(cases, r, rows):
    """The Rejuvenation buff apply 0x6F0F0650 (ecx = buff; target, &T, &DataA, &DataB, flag1, flag2). Rows: T DataA DataB f1 f2 |
    set-duration value | flags & 0x300 | life per tick | mana per tick."""
    S.install()
    A.patch_jump(0x6F0D4EF0, recorder(0x100, [(1, True)], 8, counter=0x1E4))
    for _ in range(cases):
        t = rbits(r) if r.random() < 0.6 else f2u(r.choice([12.0, 1.0, 0.0, -1.0, 6.5]))
        da, db = rbits(r), rbits(r)
        f1, f2 = r.randint(0, 2), r.randint(0, 2)
        mu.mem_write(BUFF, b"\0" * 0x400)
        wr(BUFF + 0x20, r.getrandbits(32))
        wr(BUFF + 0xE0, S.HVT_A)
        wr(BUFF + 0xE8, S.HVT_A)
        wr(S.HVT_A + 0x20, 0)
        buf = M + 0xD000
        wr(buf, t)
        wr(buf + 4, da)
        wr(buf + 8, db)
        S.clear_cap()
        flags0 = rd(BUFF + 0x20)
        S.call(0x6F0F0650, ecx=BUFF, stack_args=(TGT, buf, buf + 4, buf + 8, f1, f2))
        rows.append([t, da, db, f1, f2, flags0, rd(CAP + 0x100), rd(BUFF + 0x20), rd(BUFF + 0xE4), rd(BUFF + 0xEC)])


VT_TAUNT_ABIL, ABIL_TAUNT = 0x6F8F52A4, 0x41746175
RECS = M + 0xD000          # taunt candidate records (20 bytes each)
UNITS = M + 0xE000         # fake candidate units (0x200 each): +0x0C id, +0x100 hero, +0x104 owner index


def check_taunt_compare(cases, r, rows):
    """The three sort comparators of Taunt (cdecl, two record pointers): 0x6F160310, 0x6F16E390, 0x6F16E3B0. Rows: mode
    (0, 1, 2) | A: f1 f2 f3 f4 id | B: f1 f2 f3 f4 id | result."""
    S.install()
    fns = (0x6F160310, 0x6F16E390, 0x6F16E3B0)
    for _ in range(cases):
        mode = r.randrange(3)
        recs = []
        for k in range(2):
            f1 = rbits(r) if r.random() < 0.3 else f2u(float(r.randint(0, 4)) * 100.0)
            f2 = rbits(r) if r.random() < 0.3 else f2u(float(r.choice([0, 0, -1, 5.0])))
            f3, f4 = r.choice([0, 1, 1, 2, 3]), r.choice([0, 1, 1, 2])
            uid = r.randint(1, 50)
            recs.append((f1, f2, f3, f4, uid))
        if r.random() < 0.4:      # force ties on the first key
            recs[1] = (recs[0][0],) + recs[1][1:]
        if r.random() < 0.2:
            recs[1] = recs[0][:4] + (recs[1][4],)
        for k, (f1, f2, f3, f4, uid) in enumerate(recs):
            rec = RECS + 0x40 * k
            u = UNITS + 0x200 * k
            mu.mem_write(rec, b"\0" * 0x20)
            mu.mem_write(u, b"\0" * 0x20)
            wr(rec, u)
            wr(rec + 4, f1)
            wr(rec + 8, f2)
            wr(rec + 12, f3)
            wr(rec + 16, f4)
            wr(u + 12, uid)
        res = S.call(fns[mode], stack_args=(RECS, RECS + 0x40))
        rows.append([mode] + list(recs[0]) + list(recs[1]) + [res & 0xFFFFFFFF])


def _group_effect(cases, r, rows, vt, abil, effect, slot, idx):
    """Taunt effect 0x6F17B5A0 with the group collector replaced by a prepared record array: the real qsort and comparators, the
    count rule and the application loop (ability slot 0x42C recorded). Rows: n DataA DataB DataC dur herodur extra |
    10 x (f1 f2 f3 f4 id hero) | applied count | 10 x (unit index, dur)."""
    S.install()
    unit_vtables()
    wr(VTU + 0xB8, new_code(b"\x8B\x44\x24\x04\xC2\x04\x00"))
    A.patch_jump(0x6F4743A0, A.stub_ret(0, 0))
    A.patch_jump(0x6F2A3270, new_code(b"\x8B\x44\x24\x08\x8B\x0D" + a32(VARS + 0x40) + b"\x89\x48\x08\xC7\x40\x0C" + a32(RECS) + b"\xC3"))
    A.patch_jump(0x6F29EF80, A.stub_ret(8, 0))
    A.patch_jump(0x6F2A3EA0, A.stub_ret(0xC, 0))
    A.patch_jump(0x6F6EB558, A.stub_ret(0x10, 0))
    A.patch_jump(0x6F1D2660, new_code(b"\x8B\x81" + a32(0x100) + b"\xC3"))
    mu.mem_write(VTT, b"\0" * 0x200)
    wr(VTT + 0xEC, new_code(b"\x8B\x81" + a32(0x104) + b"\xC3"))
    code = (b"\x8B\x0D" + a32(CAP + 0x1F0) + b"\x8B\x44\x24\x04\x89\x04\xCD" + a32(CAP + 0x200)
            + b"\x8B\x44\x24\x0C\x8B\x00\x89\x04\xCD" + a32(CAP + 0x204) + b"\xFF\x05" + a32(CAP + 0x1F0) + b"\xC2\x0C\x00")
    s42c = new_code(code)
    patch_n = M + 0x7000
    for _ in range(cases):
        n = r.randint(0, 10)
        da = f2u(float(r.choice([0, 1, 2, 3, 5, 12, -1]))) if r.random() < 0.85 else rbits(r)
        db = r.choice([0, 0, 0, f2u(1.0), 0x80000000, rbits(r)])
        dc = r.choice([0, 0, 0, f2u(1.0), 0x80000000, rbits(r)])
        dur, hdur = rbits(r), rbits(r)
        extra = rbits(r) if r.random() < 0.3 else 0
        A.make_ability(vt, abil, 75)
        data = [0] * 9
        data[idx[0]], data[idx[1]], data[idx[2]] = da, db, dc
        level(A.LVL, dur, hdur, data)
        wr(A.OBJ + 0x9C, extra)
        wr(A.OBJ + 0x30, A.UNIT)
        wr(A.UNIT, VTU)
        copy_vtable(vt, {slot: s42c})
        wr(VARS + 0x40, n)
        recs = []
        for k in range(n):
            f1 = f2u(float(r.randint(0, 5)) * 128.0) if r.random() < 0.7 else rbits(r)
            f2 = f2u(float(r.choice([0, 0, -1]))) if r.random() < 0.8 else rbits(r)
            f3, f4 = r.choice([0, 1]), r.choice([0, 1])
            hero = r.random() < 0.3
            uid = r.randint(1, 4000)
            while any(x[4] == uid for x in recs):
                uid = r.randint(1, 4000)
            recs.append((f1, f2, f3, f4, uid, int(hero)))
            u = UNITS + 0x200 * k
            mu.mem_write(u, b"\0" * 0x200)
            wr(u, VTT)
            wr(u + 12, uid)
            wr(u + 0x100, int(hero))
            wr(u + 0x104, r.randint(0, 15))
            rec = RECS + 20 * k
            wr(rec, u)
            wr(rec + 4, f1)
            wr(rec + 8, f2)
            wr(rec + 12, f3)
            wr(rec + 16, f4)
        S.clear_cap()
        wr(CAP + 0x1F0, 0)
        S.call(effect, ecx=A.OBJ)
        k = rd(CAP + 0x1F0)
        out = []
        for i in range(10):
            if i < k:
                out += [(rd(CAP + 0x200 + 8 * i) - UNITS) // 0x200, rd(CAP + 0x204 + 8 * i)]
            else:
                out += [0xFFFF, 0]
        flat = [x for rec in recs for x in rec] + [0] * (6 * (10 - n))
        rows.append([n, da, db, dc, dur, hdur, extra] + flat + [k] + out)


VT_ROAR_ABIL, ABIL_ROAR = 0x6F8A21BC, 0x41726F61


def check_taunt_effect(cases, r, rows):
    _group_effect(cases, r, rows, VT_TAUNT_ABIL, ABIL_TAUNT, 0x6F17B5A0, 0x42C, (0, 1, 2))


def check_roar_effect(cases, r, rows):
    """Roar effect 0x6F1EED60: as Taunt, limit DataG, comparator keys DataE / DataF, per-unit slot 0x3EC. Rows as Taunt with
    (DataG, DataE, DataF) in place of (DataA, DataB, DataC)."""
    _group_effect(cases, r, rows, VT_ROAR_ABIL, ABIL_ROAR, 0x6F1EED60, 0x3EC, (6, 4, 5))


def check_roar_ready(cases, r, rows):
    """Roar readiness slot 0x3DC = 0x6F1EEBF0. Rows: base_error DataG found | result."""
    S.install()
    unit_vtables()
    wr(VTU + 0xB8, new_code(b"\x8B\x44\x24\x04\xC2\x04\x00"))
    A.patch_jump(0x6F4743A0, A.stub_ret(0, 0))
    ret_var(0x6F050880, 8, 0)
    A.patch_jump(0x6F6EB558, A.stub_ret(0x10, 0))
    A.patch_jump(0x6F2A3270, new_code(b"\x8B\x44\x24\x08\x8B\x0D" + a32(VARS + 4) + b"\x89\x48\x28\xC3"))
    for _ in range(cases):
        base = r.choice([0, 0, 0, 0x33])
        dg = f2u(float(r.choice([0, 1, 3, -1]))) if r.random() < 0.85 else rbits(r)
        found = r.random() < 0.5
        A.make_ability(VT_ROAR_ABIL, ABIL_ROAR, 75)
        level(A.LVL, 0, 0, [0, 0, 0, 0, 0, 0, dg])
        wr(A.OBJ + 0x30, A.UNIT)
        wr(A.UNIT, VTU)
        wr(VARS + 0, base)
        wr(VARS + 4, int(found))
        res = S.call(0x6F1EEBF0, ecx=A.OBJ, stack_args=(0, 0))
        rows.append([base, dg, int(found), res & 0xFFFFFFFF])


def _trace_install():
    from collections import deque
    from unicorn import UC_HOOK_CODE
    ring = deque(maxlen=40)
    mu.hook_add(UC_HOOK_CODE, lambda uc, a, s, u: ring.append(a))
    return ring


CHECKS = {
    "heal_effect": check_heal_effect,
    "heal_target": check_heal_target,
    "heal_filter": check_heal_filter,
    "dispel_args": check_dispel_args,
    "purge_args": check_purge_args,
    "abolish_args": check_abolish_args,
    "abolish_filter": check_abolish_filter,
    "curse_apply": check_buff_apply("curse", VT_CURSE_ABIL, ABIL_CURSE, 0x6F0B8450, 0x358, 0x354, 0x6F0B52E0, "curse"),
    "faerie_apply": check_buff_apply("faerie", VT_FAERIE_ABIL, ABIL_FAERIE, 0x6F0B8750, 0x360, 0x35C, 0x6F0B5480, "faerie"),
    "curse_filter": check_filter_pair(VT_CURSE_ABIL, ABIL_CURSE, 0x6F09BF80, "curse"),
    "faerie_filter": check_filter_pair(VT_FAERIE_ABIL, ABIL_FAERIE, 0x6F09C240, "faerie"),
    "taunt_compare": check_taunt_compare,
    "taunt_effect": check_taunt_effect,
    "roar_effect": check_roar_effect,
    "roar_ready": check_roar_ready,
    "unholy_apply": check_unholy_apply,
    "rejuv_effect": check_rejuv_effect,
    "rejuv_buff": check_rejuv_buff,
    "polymorph_target": check_polymorph_target,
    "polymorph_effect": check_polymorph_effect,
}


def main():
    args = sys.argv[1:]
    dump = None
    only = None
    while args and args[0] in ("--dump", "--only"):
        if args[0] == "--dump":
            dump, args = args[1], args[2:]
        else:
            only, args = args[1].split(","), args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 77
    for name, fn in CHECKS.items():
        if only is None or name not in only:
            continue
        r = random.Random(seed)
        rows = []
        ring = _trace_install() if os.environ.get("V4_TRACE") else None
        try:
            fn(cases, r, rows)
        except Exception:
            if ring:
                sys.stderr.write("last: %s\n" % " ".join("%x" % a for a in ring))
            raise
        print("%s rows %d" % (name, len(rows)))
        for row in rows[:5]:
            print(" ".join("%08x" % x for x in row))
        if dump:
            os.makedirs(dump, exist_ok=True)
            with open(os.path.join(dump, name + ".txt"), "w") as fh:
                for row in rows:
                    fh.write(" ".join("%08x" % x for x in row) + "\n")


if __name__ == "__main__":
    main()
