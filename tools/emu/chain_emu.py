"""Unicorn run of the Chain Lightning hop of Game.dll 1.26a (`AOcl`, `CAbilityChainLightning`, docs/spec/abilities-orc-heroes.md
section 4).

The ORIGINAL hop body `0x6F1C2F50` (ECX = ability, stack = source unit, target unit, flag; `ret 0xC`) and the next-hop gate
`0x6F1C3200` (ECX = ability, stack = the unit just hit; `ret 4`) run on a fake ability object (the real level getters on a
real-layout level record, the real vtable of the class with the slot `0x348` replaced), with fake units. What is stubbed:
the handle helpers (`0x6F0419C0`, `0x6F038450`), the damage record builder `0x6F021650`, the effect constructors
(`0x6F1A87B0`, `0x6F1BB270`, which hand out a fake effect object), the owner-id helper `0x6F32E450`, the position helper
`0x6F4743A0`, the end of the chain `0x6F040020`, and the agile cell setter of the damage factor (the cell's virtual
slot 0). The effect's start slot (`vtable + 0xA4`) is a recorder: the 8 stack arguments and the 0x30 bytes of the damage
record are logged.

Output (text, derived numbers only, one case per line, floats as hex bits):
  H <DataA> <DataC> <factor> <count> | <record flags> <record amount> <arg2 flag> <arg3 ability?> <new factor> <count'> <t_next> <t_destroy>
  G <DataB> <count> <targs> <ability flags> <next found 0/1> | <hops started> <flag word given to slot 0x43C> <chain ended 0/1>

Usage: GAME_DLL=/path/Game.dll py tools/emu/chain_emu.py OUT.txt COUNT SEED
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import abilities4_emu as A  # noqa: E402  (patch_seh: the image has no FS base)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EAX  # noqa: E402

mu = emu.mu
D = emu.DATA
VT_CHAIN = 0x6F8B0E74
F_HOP = 0x6F1C2F50
F_GATE = 0x6F1C3200

ABIL = D + 0x2000      # fake ability
DEF = D + 0x3000
LVL = D + 0x3200
VT_COPY = D + 0x4000   # copy of the class vtable (0x448 bytes)
CELL_VT = D + 0x4A00   # vtable of the factor cell: slot 0 = setter
UNIT_OWNER = D + 0x5000
UNIT_A = D + 0x5100
UNIT_B = D + 0x5200
UNIT_VT = D + 0x5300
EFFECT = D + 0x5400    # fake effect object
EFFECT_VT = D + 0x5500
STUBS = D + 0x6000
LOG = D + 0x7000
POS_DUMMY = D + 0x7800

_next = [STUBS]


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def fbits(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def stub(code):
    a = _next[0]
    mu.mem_write(a, bytes(code))
    _next[0] += (len(code) + 0x1F) & ~0x1F
    return a


def ret(n):
    return b"\xC2" + struct.pack("<H", n) if n else b"\xC3"


def patch(fn, target):
    mu.mem_write(fn, b"\xE9" + struct.pack("<i", target - (fn + 5)))


def setup():
    # helpers
    patch(0x6F0419C0, stub(ret(4)))                                   # handle save
    patch(0x6F038450, stub(b"\xC7\x01\x00\x00\x00\x00" b"\x89\xC8" + ret(4)))  # mov [ecx],0 ; mov eax,ecx
    patch(0x6F021650, stub(ret(12)))                                  # damage record builder
    patch(0x6F32E450, stub(b"\xB8\x34\x12\x00\x00" + ret(0)))         # mov eax, 0x1234
    patch(0x6F1A87B0, stub(b"\xC7\x01\x00\x00\x00\x00" + ret(4)))     # mov [ecx],0
    patch(0x6F1BB270, stub(b"\xC7\x01" + struct.pack("<I", EFFECT) + ret(12)))  # mov [ecx], EFFECT
    patch(0x6F4743A0, stub(ret(4)))                                   # position helper (fills nothing)
    # end of chain: record that it ran: mov dword [LOG+0x80], 1 ; ret
    patch(0x6F040020, stub(b"\xC7\x05" + struct.pack("<I", LOG + 0x80) + struct.pack("<I", 1) + ret(0)))
    # damage cell setter: [ecx+4] = [[esp+4]] ; ret 8
    setter = stub(b"\x8B\x44\x24\x04" b"\x8B\x10" b"\x89\x51\x04" + ret(8))
    w32(CELL_VT, setter)
    # recorder of the effect start slot (vtable + 0xA4): a Python hook sits on this address
    rec = stub(b"\x90" + ret(0x20))
    w32(EFFECT_VT + 0xA4, rec)
    w32(EFFECT, EFFECT_VT)
    w32(EFFECT + 4, 2)
    # unit vtable: slot 0xB8 returns POS_DUMMY ; ret
    getpos = stub(b"\xB8" + struct.pack("<I", POS_DUMMY) + ret(0))
    w32(UNIT_VT + 0xB8, getpos)
    for u in (UNIT_OWNER, UNIT_A, UNIT_B):
        w32(u, UNIT_VT)
    # class vtable copy with slot 0x348 returning 0 and the next-target slot 0x43C as a recorder
    mu.mem_write(VT_COPY, bytes(mu.mem_read(VT_CHAIN, 0x448)))
    w32(VT_COPY + 0x348, stub(b"\x31\xC0" + ret(0)))
    # next-target slot: records the flag word ([esp+8]) at LOG+0x94, returns [LOG+0x90]; ret 8
    nxt = stub(b"\x8B\x4C\x24\x08" b"\x89\x0D" + struct.pack("<I", LOG + 0x94)
               + b"\x8B\x05" + struct.pack("<I", LOG + 0x90) + ret(8))
    w32(VT_COPY + 0x43C, nxt)
    return rec, nxt


def fresh_ability(data_a, data_b, data_c, factor, count, targs=0, flags=0):
    mu.mem_write(ABIL, bytes(0x400))
    mu.mem_write(DEF, bytes(0x100))
    mu.mem_write(LVL, bytes(0x400))
    w32(ABIL, VT_COPY)
    w32(ABIL + 0x30, UNIT_OWNER)
    w32(ABIL + 0x34, 0x77)
    w32(ABIL + 0x4C, targs)
    w32(ABIL + 0x20, flags)
    w32(ABIL + 0x50, 0)
    w32(ABIL + 0x54, DEF)
    w32(DEF + 0x2C, 1)
    w32(DEF + 0x50, 3)
    w32(DEF + 0x54, LVL)
    w32(LVL + 0x20, data_a)
    w32(LVL + 0x24, data_b)
    w32(LVL + 0x28, data_c)
    w32(ABIL + 0x124, count)
    w32(ABIL + 0x128, CELL_VT)
    w32(ABIL + 0x12C, factor)
    w32(LOG + 0x80, 0)
    w32(EFFECT + 4, 2)


records = []


def on_effect_start(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    args = [r32(esp + 4 + 4 * i) for i in range(8)]
    rec = bytes(uc.mem_read(args[0], 0x30))
    records.append((args, rec))


SPECIAL = [0x00000000, 0x80000000, 0x3F800000, 0xBF800000, 0x7F800000, 0xFF800000, 0x7FC00000, 0x3E19999A, 0x3E99999A,
           0x42C80000, 0x43340000, 0x41200000, 0x00000001, 0x3F7FFFFF]


def rnd(r, lo=0.0, hi=300.0):
    k = r.random()
    if k < 0.12:
        return r.choice(SPECIAL)
    if k < 0.85:
        return fbits(r.uniform(lo, hi))
    return r.getrandbits(32)


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    rec_addr, _ = setup()
    mu.hook_add(UC_HOOK_CODE, on_effect_start, begin=rec_addr, end=rec_addr)
    lines = []
    ring = []
    if os.environ.get("CHAIN_DEBUG"):
        mu.hook_add(UC_HOOK_CODE, lambda uc, a, sz, u: (ring.append(a), ring.__delitem__(0) if len(ring) > 40 else None))
    for _ in range(count):
        data_a = rnd(r, 0.0, 400.0)
        data_c = rnd(r, 0.0, 1.0)
        factor = rnd(r, 0.0, 2.0) if r.random() < 0.6 else 0x3F800000
        n = r.choice([0, 0, 1, 2, 3, 7])
        fresh_ability(data_a, 0, data_c, factor, n)
        records.clear()
        try:
            emu.call(F_HOP, ecx=ABIL, stack_args=(UNIT_A, UNIT_B, r.choice([0, 1])))
        except Exception:
            print("last addresses", [hex(a) for a in ring[-14:]])
            raise
        assert len(records) == 1, "the hop starts exactly one effect"
        args, rec = records[0]
        flags = struct.unpack_from("<I", rec, 0x10)[0]
        amount = struct.unpack_from("<I", rec, 0x18)[0]
        t_next = r32(args[4]) if args[4] else 0
        t_destroy = r32(args[5]) if args[5] else 0
        lines.append("H %08x %08x %08x %d | %08x %08x %08x %08x %08x %d %08x %08x" % (
            data_a, data_c, factor, n, flags, amount, args[1], args[2] == ABIL, r32(ABIL + 0x12C), r32(ABIL + 0x124),
            t_next, t_destroy))
    # the gate
    for _ in range(count):
        data_b = fbits(float(r.randint(0, 9)) + r.choice([0.0, 0.0, 0.5, 0.99])) if r.random() < 0.8 else rnd(r, 0.0, 9.0)
        n = r.randint(0, 10)
        targs = r.getrandbits(32)
        flags = r.choice([0, 0x80000, 0x80000 | r.getrandbits(32), r.getrandbits(32)])
        found = r.choice([0, 0])  # next-target result (0 = none); the hop path is exercised below
        fresh_ability(0x42C80000, data_b, 0x3E19999A, 0x3F800000, n, targs, flags)
        found_unit = UNIT_B if r.random() < 0.6 else 0
        w32(LOG + 0x90, found_unit)
        w32(LOG + 0x94, 0xDEADBEEF)
        records.clear()
        emu.call(F_GATE, ecx=ABIL, stack_args=(UNIT_A,))
        ended = r32(LOG + 0x80)
        lines.append("G %08x %d %08x %08x %d | %d %08x %d" % (
            data_b, n, targs, flags, 1 if found_unit else 0, len(records), r32(LOG + 0x94), ended))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
