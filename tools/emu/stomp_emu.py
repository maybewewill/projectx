"""Unicorn run of the per-unit body of War Stomp (`AOws`, `CAbilityStomp`) of Game.dll 1.26a
(docs/spec/abilities-orc-heroes.md section 13, task S39).

The ORIGINAL per-unit callback `0x6F1607B0` (ECX = ability, stack = victim unit, pointer to the ability's hit array,
`ret 8`) runs on a fake ability (real level getters on a real-layout level record, the real class vtable with the
stun slot `0x430` replaced by a recorder) and a fake victim unit whose vtable slots `0x11C`, `0x114`, `0x120`, `0xEC` are
stubs reading per-case switches. Stubbed helpers: the spell-immunity test `0x6F28D750`, the record builder `0x6F2556E0`
(records its arguments), the handle resolver `0x6F282F60`.

Output (text, derived numbers only, one case per line, floats as hex bits):
  U <DataA> <slot 0x11C> <immune 0x6F28D750> <targType slot 0x114> <resolved 0/1> <owner idx> |
    <record built 0/1> <amount> <class> <flags> <slot 0x120 called> <hit array index> <hit stored 0/1> <stun called>

Usage: GAME_DLL=/path/Game.dll py tools/emu/stomp_emu.py OUT.txt COUNT SEED
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
from unicorn.x86_const import UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
D = emu.DATA
w32, r32, fbits, stub, ret, patch = C.w32, C.r32, C.fbits, C.stub, C.ret, C.patch

VT_STOMP = 0x6F8F67C4 if os.environ.get("THUNDER_VT") else 0x6F8F6C04  # V5: THUNDER_VT=1 runs the Thunder Clap class (same slots, slot 0x430 recorded)
F_UNIT = 0x6F1607B0

ABIL = D + 0xA000
DEF = D + 0xA400
LVL = D + 0xA500
VT_COPY = D + 0xA800
VICTIM = D + 0xB400
VICTIM_VT = D + 0xB600
VARS = D + 0xB800
LOGB = D + 0xBA00
ARRAY = D + 0xBC00
CASTER = D + 0xBE00
CASTER_VT = D + 0xBF00


def setup():
    mem = lambda a: struct.pack("<I", a)
    # victim vtable slots: 0x11C, 0x114 and 0xEC read the case switches, 0x120 counts the call
    s_11c = stub(b"\xA1" + mem(VARS + 0) + ret(0))
    s_114 = stub(b"\xA1" + mem(VARS + 4) + ret(0))
    s_ec = stub(b"\xA1" + mem(VARS + 8) + ret(0))
    s_120 = stub(b"\xFF\x05" + mem(LOGB + 0x14) + ret(0x10))  # inc [LOGB+0x14]
    w32(VICTIM_VT + 0x11C, s_11c)
    w32(VICTIM_VT + 0x114, s_114)
    w32(VICTIM_VT + 0xEC, s_ec)
    w32(VICTIM_VT + 0x120, s_120)
    w32(VICTIM, VICTIM_VT)
    # immunity test 0x6F28D750 (ECX = unit, stack = order id; ret 8 in the original: it pops two words)
    patch(0x6F28D750, stub(b"\xA1" + mem(VARS + 0xC) + ret(8)))
    # handle resolver 0x6F282F60 (ECX = unit): the resolved unit or 0
    patch(0x6F282F60, stub(b"\xA1" + mem(VARS + 0x10) + ret(0)))
    # record builder 0x6F2556E0 (ECX = record; stack = amount pointer, class, source, flags, 0); records them
    rec = stub(b"\x90" + ret(0x14))
    patch(0x6F2556E0, rec)
    # class vtable copy: the stun slot is a recorder (inc [LOGB+0x18]; ret 4)
    mu.mem_write(VT_COPY, bytes(mu.mem_read(VT_STOMP, 0x448)))
    w32(VT_COPY + 0x430, stub(b"\xFF\x05" + mem(LOGB + 0x18) + ret(4)))
    # the caster
    w32(CASTER, CASTER_VT)
    return rec


def fresh_ability(data_a):
    mu.mem_write(ABIL, bytes(0x400))
    mu.mem_write(DEF, bytes(0x100))
    mu.mem_write(LVL, bytes(0x400))
    w32(ABIL, VT_COPY)
    w32(ABIL + 0x30, CASTER)
    w32(ABIL + 0x34, 0x77)
    w32(ABIL + 0x50, 0)
    w32(ABIL + 0x54, DEF)
    w32(DEF + 0x2C, 1)
    w32(DEF + 0x50, 3)
    w32(DEF + 0x54, LVL)
    w32(LVL + 0x20, data_a)


records = []


def on_record(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    args = [r32(esp + 4 + 4 * i) for i in range(5)]
    records.append((args, r32(args[0])))


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    rec_addr = setup()
    mu.hook_add(UC_HOOK_CODE, on_record, begin=rec_addr, end=rec_addr)
    lines = []
    for _ in range(count):
        data_a = C.rnd(r, 0.0, 200.0)
        inv = r.choice([0, 0, 0, 1])
        immune = r.choice([0, 0, 0, 1])
        targ = r.choice([0, 2, 4, 8, 0xA, 0xC, 0x1A, 0x1E, 0x10])
        resolved = r.choice([0, 1, 1, 1])
        owner = r.randint(0, 15)
        fresh_ability(data_a)
        w32(VARS + 0, inv)
        w32(VARS + 4, targ)
        w32(VARS + 8, owner)
        w32(VARS + 0xC, immune)
        w32(VARS + 0x10, VICTIM if resolved else 0)
        mu.mem_write(LOGB, bytes(0x40))
        mu.mem_write(ARRAY, bytes(0x80))
        records.clear()
        emu.call(F_UNIT, ecx=ABIL, stack_args=(VICTIM, ARRAY))
        built = len(records)
        amount = cls = flags = 0
        if built:
            args, amount = records[0]
            cls, flags = args[1], args[3]
            # the stack arguments of the recorder: (amount ptr, class, source, flags, 0)
            assert args[2] == CASTER and args[4] == 0
        called120 = r32(LOGB + 0x14)
        stun = r32(LOGB + 0x18)
        stored = 0
        idx = -1
        for i in range(16):
            if r32(ARRAY + 4 + 4 * i) == VICTIM:
                stored = 1
                idx = i
        lines.append("U %08x %d %d %d %d %d | %d %08x %08x %08x %d %d %d %d" % (
            data_a, inv, immune, targ, resolved, owner, built, amount, cls, flags, called120, idx & 0xFFFFFFFF if idx >= 0 else 0xFFFFFFFF, stored, stun))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
