"""Unicorn run of Healing Wave (`AOhw`, `CAbilityHealingWave`, vtable 0x6F8B12BC) and of the shared chain candidate
callback of Game.dll 1.26a (docs/spec/abilities-orc-heroes.md section 10, task S34).

Healing Wave is a RTTI child of `CAbilityChainLightning`: the cast entry `0x6F1CB550`, the hop body `0x6F1C2F50`, the
next-hop gate `0x6F1C3200` and the candidate callback `0x6F1B0780` are the chain code; the class only overrides the slots
`0x42C` (= 1, "heals"), `0x434` (= 1, score mode), `0x438` (= 0, distance mode off), `0x1FC` (= 1) and the order id
`0x308` (= 0xD0215).

Three groups of cases are produced, all running the ORIGINAL code on fake objects (the harness of `chain_emu.py`):

  HH  the hop body `0x6F1C2F50` on the Healing Wave vtable: damage record flags / amount (the amount is negated when
      slot `0x42C` is non-zero), the bolt delay pointers (0.25 and `[0x6FAAE504]`), the new factor and the hit count.
        HH <DataA> <DataC> <factor> <count> | <flags> <amount> <t_next> <t_destroy> <new factor> <count'>
  HG  the next-hop gate `0x6F1C3200` on the Healing Wave vtable (the flag word handed to slot `0x43C`):
        HG <DataB> <count> <targs> <ability flags> <found> | <hops> <flag word> <ended>
  CB  the candidate callback `0x6F1B0780` with a fake candidate (REAL validity chain `0x6F1B06F0` and slot `0x3F0`
      `0x6F1B0650`, real soft compares; stubbed: the relation test `0x6F284EE0`, the immunity test `0x6F28D750`, the
      hit-list test `0x6F1A2110`, slot `0x348`, the Misc lookup `0x6F009F90`, the smart-pointer assign `0x6F02A560`, the
      distance routine `0x6F476860` = a value read from the candidate):
        CB <class 0=chain 1=heal> <score mode> <dist mode> <ability flags> <cand life> <cand max> <cand dist> <cand flags5c>
           <cand EC> <best dist> <best score> <relation ok> <immune> <hit> <misc> | <picked 0/1> <best dist'> <best score'>

Usage: GAME_DLL=/path/Game.dll py tools/emu/heal_emu.py OUT.txt COUNT SEED
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import chain_emu as C  # noqa: E402
import emu  # noqa: E402
import abilities4_emu as A  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

mu = emu.mu
D = emu.DATA
w32, r32, fbits, stub, ret, patch = C.w32, C.r32, C.fbits, C.stub, C.ret, C.patch

VT_HEAL = 0x6F8B12BC
F_CALLBACK = 0x6F1B0780

# callback fixtures
SVEC = D + 0x8000     # the search record S
CAND = D + 0x8400     # fake candidate unit
CAND_VT = D + 0x8800
CAND_POS = D + 0x8900  # the "position object" of the candidate (distance bits at +0)
VARS = D + 0x8A00     # per-case switches read by the stubs
CB_ABIL = D + 0x8C00  # ability for the callback tests (its own copy of the class vtable)
CB_VT = D + 0x9000
CB_DEF = D + 0x9600
CB_LVL = D + 0x9700
CB_OWNER = D + 0x9A00


def copy_vtable(src, dst):
    mu.mem_write(dst, bytes(mu.mem_read(src, 0x448)))


def setup_heal():
    rec, nxt = C.setup()
    # keep the stubs that chain_emu installed in slots 0x348 / 0x43C, then switch the class vtable to Healing Wave's
    s348 = r32(C.VT_COPY + 0x348)
    s43c = r32(C.VT_COPY + 0x43C)
    copy_vtable(VT_HEAL, C.VT_COPY)
    w32(C.VT_COPY + 0x348, s348)
    w32(C.VT_COPY + 0x43C, s43c)
    return rec


def setup_callback():
    # stubs for the callback fixture (all `ret n` stubs read their answer from VARS)
    # VARS: +0 relation ok, +4 immune, +8 hit, +0xC misc lookup, +0x10 dist value (read by the 0x6F476860 stub)
    patch(0x6F284EE0, stub(b"\xA1" + struct.pack("<I", VARS) + ret(0xC)))           # mov eax,[VARS]
    patch(0x6F28D750, stub(b"\xA1" + struct.pack("<I", VARS + 4) + ret(4)))
    patch(0x6F1A2110, stub(b"\xA1" + struct.pack("<I", VARS + 8) + ret(4)))
    patch(0x6F009F90, stub(b"\xA1" + struct.pack("<I", VARS + 0xC) + ret(4)))
    # smart pointer assign: [ecx] = [esp+4]
    patch(0x6F02A560, stub(b"\x8B\x44\x24\x04" b"\x89\x01" + ret(4)))
    # 0x6F476860 (ecx = position object, 4 stack args: out, x, y, flag) -> [out] = [ecx]; eax = out; ret 0x10
    patch(0x6F476860, stub(b"\x8B\x44\x24\x04" b"\x8B\x11" b"\x89\x10" + ret(0x10)))
    # candidate unit vtable: 0x12C -> &life, 0x130 -> &max, 0xB8 -> position object (ret 0, args stay for 0x6F476860)
    w32(CAND_VT + 0x12C, stub(b"\x8D\x81" + struct.pack("<I", 0x100) + ret(4)))   # lea eax,[ecx+0x100]
    w32(CAND_VT + 0x130, stub(b"\x8D\x81" + struct.pack("<I", 0x104) + ret(4)))   # lea eax,[ecx+0x104]
    w32(CAND_VT + 0xB8, stub(b"\xB8" + struct.pack("<I", CAND_POS) + ret(0)))
    w32(CAND, CAND_VT)
    # ability slot 0x348: constant 0x55
    mu.mem_write(CB_VT, bytes(0x448))


STUB348 = None


def cb_ability(cls_vt, score_mode, dist_mode, flags):
    copy_vtable(cls_vt, CB_VT)
    w32(CB_VT + 0x348, STUB348)
    # the real S fields 0x1c / 0x20 are what slots 0x434 / 0x438 return in the real caller; here they are set directly
    mu.mem_write(CB_ABIL, bytes(0x400))
    w32(CB_ABIL, CB_VT)
    w32(CB_ABIL + 0x30, CB_OWNER)
    w32(CB_ABIL + 0x20, flags)
    w32(CB_OWNER, C.UNIT_VT)


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    rec_addr = setup_heal()
    mu.hook_add(UC_HOOK_CODE, C.on_effect_start, begin=rec_addr, end=rec_addr)
    lines = []
    # ---- HH ----
    for _ in range(count):
        data_a = C.rnd(r, 0.0, 400.0)
        data_c = C.rnd(r, 0.0, 1.0)
        factor = C.rnd(r, 0.0, 2.0) if r.random() < 0.6 else 0x3F800000
        n = r.choice([0, 0, 1, 2, 3, 7])
        C.fresh_ability(data_a, 0, data_c, factor, n)
        C.records.clear()
        emu.call(C.F_HOP, ecx=C.ABIL, stack_args=(C.UNIT_A, C.UNIT_B, r.choice([0, 1])))
        assert len(C.records) == 1
        args, rec = C.records[0]
        flags = struct.unpack_from("<I", rec, 0x10)[0]
        amount = struct.unpack_from("<I", rec, 0x18)[0]
        t_next = r32(args[4]) if args[4] else 0
        t_destroy = r32(args[5]) if args[5] else 0
        lines.append("HH %08x %08x %08x %d | %08x %08x %08x %08x %08x %d" % (
            data_a, data_c, factor, n, flags, amount, t_next, t_destroy, r32(C.ABIL + 0x12C), r32(C.ABIL + 0x124)))
    # ---- HG ----
    for _ in range(count):
        data_b = C.fbits(float(r.randint(0, 9)) + r.choice([0.0, 0.0, 0.5, 0.99])) if r.random() < 0.8 else C.rnd(r, 0.0, 9.0)
        n = r.randint(0, 10)
        targs = r.getrandbits(32)
        flags = r.choice([0, 0x80000, 0x80000 | r.getrandbits(32), r.getrandbits(32)])
        C.fresh_ability(0x42C80000, data_b, 0x3E19999A, 0x3F800000, n, targs, flags)
        found_unit = C.UNIT_B if r.random() < 0.6 else 0
        w32(C.LOG + 0x90, found_unit)
        w32(C.LOG + 0x94, 0xDEADBEEF)
        C.records.clear()
        emu.call(C.F_GATE, ecx=C.ABIL, stack_args=(C.UNIT_A,))
        ended = r32(C.LOG + 0x80)
        lines.append("HG %08x %d %08x %08x %d | %d %08x %d" % (
            data_b, n, targs, flags, 1 if found_unit else 0, len(C.records), r32(C.LOG + 0x94), ended))
    # ---- CB ----
    setup_callback()
    global STUB348
    STUB348 = stub(bytes([0xB8, 0x55, 0, 0, 0]) + ret(4))
    ring = []
    mu.hook_add(UC_HOOK_CODE, lambda uc, a, sz, u: (ring.append(a), ring.__delitem__(0) if len(ring) > 30 else None))
    for _ in range(count * 2):
        heal = r.random() < 0.5
        cls_vt = VT_HEAL if heal else C.VT_CHAIN
        score_mode = 1 if heal else 0
        dist_mode = 0 if heal else 1
        if r.random() < 0.15:      # unusual mixes: both modes on, or none
            score_mode, dist_mode = r.choice([(1, 1), (0, 0)])
        flags = r.choice([0, 0x100000, 0x80000, 0x180000, r.getrandbits(32) & 0x1B0000])
        cur = C.rnd(r, 0.0, 800.0)
        mx = C.fbits(r.uniform(0.0, 800.0) + 801.0) if r.random() < 0.7 else (C.rnd(r, 0.0, 800.0) if r.random() < 0.5 else cur)
        if r.random() < 0.1:
            mx = cur
        dist = C.rnd(r, 0.0, 9.0e5)
        c5c = r.choice([0, 0x40000000, r.getrandbits(32)])
        cec = r.choice([0, 0, 1, 7, 0x80000000])
        best_d = C.rnd(r, 0.0, 9.0e5) if r.random() < 0.8 else 0x7F7FFFFF
        best_s = C.rnd(r, 0.0, 800.0) if r.random() < 0.6 else 0
        rel = r.choice([0] + [1] * 9)
        imm = r.choice([0] * 9 + [1])
        hit = r.choice([0] * 9 + [1])
        misc = r.choice([0, 1])
        cb_ability(cls_vt, score_mode, dist_mode, flags)
        mu.mem_write(CAND, bytes(0x200))
        w32(CAND, CAND_VT)
        w32(CAND + 0x5C, c5c)
        w32(CAND + 0xEC, cec)
        w32(CAND + 0x100, cur)
        w32(CAND + 0x104, mx)
        w32(CAND_POS, dist)
        mu.mem_write(SVEC, bytes(0x40))
        w32(SVEC, CB_ABIL)
        w32(SVEC + 0x10, 0)
        w32(SVEC + 0x14, best_d)
        w32(SVEC + 0x18, best_s)
        w32(SVEC + 0x1C, score_mode)
        w32(SVEC + 0x20, dist_mode)
        w32(VARS, rel)
        w32(VARS + 4, imm)
        w32(VARS + 8, hit)
        w32(VARS + 0xC, misc)
        try:
            emu.call(F_CALLBACK, ecx=CAND, edx=SVEC)
        except Exception:
            from unicorn.x86_const import UC_X86_REG_EIP
            print('ring', [hex(a) for a in ring[-14:]]); print('eip', hex(mu.reg_read(UC_X86_REG_EIP)), 'heal', heal, 'mode', score_mode, dist_mode)
            raise
        picked = 1 if r32(SVEC + 0x10) == CAND else 0
        lines.append("CB %d %d %d %08x %08x %08x %08x %08x %08x %08x %08x %d %d %d %d | %d %08x %08x" % (
            1 if heal else 0, score_mode, dist_mode, flags, cur, mx, dist, c5c, cec, best_d, best_s, rel, imm, hit, misc,
            picked, r32(SVEC + 0x14), r32(SVEC + 0x18)))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
