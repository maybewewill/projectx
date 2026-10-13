"""Unicorn run of the Earthquake start of Game.dll 1.26a (`AOeq`, `CAbilityEarthquake`, vtable 0x6F8A8154, effect
`CEffectEarthquake` vtable 0x6F88F86C; docs/spec/abilities-orc-heroes.md section 20, task S39).

The ORIGINAL handler of the ability's timer message `0xD01C1` (`0x6F0C4420`, which creates the effect and calls the effect
initialiser `0x6F0C4230`) runs on a fake ability with a real-layout level record (real level getters, soft float) and a
fake effect object. The start of the handler (the effect factory lookups) is skipped by a hook that puts the fake effect into
its local; the presentation helpers are stubbed. The effect's embedded cells (`+0x24`, `+0x2C`, `+0x38`, `+0x40`, `+0x48`,
`+0x64`) have a recording setter (it logs the VALUE the pointer argument points at).

  Q <DataB> <DataC> <DataD> <Area> <Dur> <ability+0x9C> <centre x> <centre y> <level> | <cell +0x24> <+0x2C> <+0x38> <+0x40>
    <+0x48> <+0x64> | <+0x34 int> <+0x7C> <+0x60 raw> <+0x6C raw> <+0x98>

Usage: GAME_DLL=/path/Game.dll py tools/emu/quake_emu.py OUT.txt COUNT SEED
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
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EIP  # noqa: E402

mu = emu.mu
D = emu.DATA
w32, r32, fbits, stub, ret, patch = C.w32, C.r32, C.fbits, C.stub, C.ret, C.patch

F_HANDLER = 0x6F0C4420
VT_QUAKE = 0x6F8A8154
ABIL = D + 0xA000
DEF = D + 0xA400
LVL = D + 0xA500
VT_COPY = D + 0xA800
EFF = D + 0xB000
CELLVT = D + 0xB400
CASTER = D + 0xB800
EFFIDS = D + 0xB900
cell_log = []


def mem(a):
    return struct.pack("<I", a)


def setup():
    rec = stub(b"\x90" + ret(8))                 # cell setter: ECX = cell, stack = pointer to the value, flag
    w32(CELLVT, rec)
    for off in (0x24, 0x2C, 0x38, 0x40, 0x48, 0x64):
        w32(EFF + off, CELLVT)
    w32(EFF + 4, 5)                              # reference count of the effect
    w32(CASTER, 0x6F931934)
    patch(0x6F014060, stub(b"\xC7\x01" + mem(D + 0xAD00) + b"\x89\xC8" + ret(0x1C)))
    patch(0x6F32E350, stub(b"\x31\xC0" + ret(0)))
    patch(0x6F32E510, stub(b"\x31\xC0" + ret(0)))
    patch(0x6F32FA90, stub(b"\x31\xC0" + ret(0x18)))
    patch(0x6F2AAC60, stub(b"\x31\xC0" + ret(8)))
    patch(0x6F029B40, stub(b"\x31\xC0" + ret(4)))
    patch(0x6F0419C0, stub(ret(4)))
    patch(0x6F0C3780, stub(ret(0)))              # the first tick of the effect (separate section)
    patch(0x6F4778F0, stub(ret(0x14)))           # timer start (the handler's 1.0 s timer)
    patch(0x6F091C80, stub(ret(4)))              # smart pointer destructor path (the prefix is skipped anyway)
    mu.mem_write(VT_COPY, bytes(mu.mem_read(VT_QUAKE, 0x448)))
    return rec


def on_cell(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    ptr = r32(esp + 4)
    try:
        v = r32(ptr)
    except Exception:
        v = 0xDEAD0000
    cell_log.append((ecx - EFF, v))


def fresh_ability(level, dur, plus9c, data_b, data_c, data_d, area, cx, cy):
    mu.mem_write(ABIL, bytes(0x400))
    mu.mem_write(DEF, bytes(0x100))
    mu.mem_write(LVL, bytes(0x400))
    w32(ABIL, VT_COPY)
    w32(ABIL + 0x30, CASTER)
    w32(ABIL + 0x34, 0x77)
    w32(ABIL + 0x50, level)
    w32(ABIL + 0x54, DEF)
    w32(ABIL + 0x9C, plus9c)
    w32(ABIL + 0x154, cx)
    w32(ABIL + 0x15C, cy)
    w32(DEF + 0x2C, 1)
    w32(DEF + 0x50, 4)
    w32(DEF + 0x54, LVL - 0 * 0x68)
    # level records are 0x68 bytes each; the getters index `level * 0x68 + base`
    for lv in range(4):
        base = LVL + lv * 0x68
        w32(base + 0x08, dur)
        w32(base + 0x18, area)
        w32(base + 0x24, data_b)
        w32(base + 0x28, data_c)
        w32(base + 0x2C, data_d)
        w32(base + 0x5C, 1)
        w32(base + 0x60, EFFIDS)
    w32(EFFIDS, 0x584F6571)


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    rec = setup()
    mu.hook_add(UC_HOOK_CODE, on_cell, begin=rec, end=rec)

    def skip_prefix(uc, address, size, user):
        # at 0x6F0C444C the prologue is done; put the fake effect into the local [esp+0x14] and jump past the lookups
        esp = uc.reg_read(UC_X86_REG_ESP)
        w32(esp + 0x14, EFF)
        uc.reg_write(UC_X86_REG_EIP, 0x6F0C44BB)

    mu.hook_add(UC_HOOK_CODE, skip_prefix, begin=0x6F0C444C, end=0x6F0C444C)
    ring = []
    if os.environ.get('QUAKE_DEBUG'):
        mu.hook_add(UC_HOOK_CODE, lambda uc, a, sz, u: ring.append(a))
    lines = []
    for _ in range(count):
        level = r.randint(0, 3)
        dur = fbits(float(r.choice([20, 25, 5, r.randint(1, 60)])))
        plus = r.choice([0, 0, fbits(r.uniform(0.0, 3.0))])
        data_b = fbits(r.uniform(1.0, 200.0))
        data_c = fbits(r.uniform(0.1, 1.0))
        data_d = fbits(r.uniform(100.0, 500.0))
        area = fbits(r.uniform(100.0, 500.0))
        cx = fbits(r.uniform(-3000.0, 3000.0))
        cy = fbits(r.uniform(-3000.0, 3000.0))
        fresh_ability(level, dur, plus, data_b, data_c, data_d, area, cx, cy)
        mu.mem_write(EFF + 0x50, bytes(0x60))
        w32(EFF + 0x64, CELLVT)
        w32(EFF + 4, 5)                           # the handler drops one reference at its end
        cell_log.clear()
        try:
            emu.call(F_HANDLER, ecx=ABIL)
        except Exception:
            print("case", level, hex(dur), hex(plus), hex(data_b), hex(data_c), hex(data_d), hex(area), hex(cx), hex(cy))
            print('last', [hex(a) for a in ring[-6:]])
            raise
        out = {}
        for off, v in cell_log:
            out[off] = v
        cells = " ".join("%08x" % out.get(o, 0xFFFFFFFF) for o in (0x24, 0x2C, 0x38, 0x40, 0x48, 0x64))
        flds = " ".join("%08x" % r32(EFF + o) for o in (0x34, 0x7C, 0x60, 0x6C, 0x98))
        lines.append("Q %08x %08x %08x %08x %08x %08x %08x %08x %d | %s | %s" % (
            data_b, data_c, data_d, area, dur, plus, cx, cy, level, cells, flds))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
