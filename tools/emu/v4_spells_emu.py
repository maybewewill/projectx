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
