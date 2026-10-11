"""Unicorn check of the argument -> attack record mapping of UnitDamageTarget (0x6F3C8850) and the argument checks
and branch of UnitDamagePoint (0x6F3C8770), Game.dll 1.26a.

What runs: the original native code and the real record constructor 0x6F2556E0. Stubbed: the handle resolvers
(0x6F3BDCB0 unit, 0x6F3BE5B0 widget), the target's virtual slot 0x120 (captures the record and its arguments),
and, for UnitDamagePoint, the two area-damage entry points 0x6F088050 (immediate) and 0x6F1D4440 (delayed), which
only record which one was reached and with which class / flags / attack type.

Output: one line per case, `key=value` pairs, written to the path given as argv[1] (argv[2] = number of cases per
native). Inputs are random, with edge values for the three range checks. Compared by
crates/sim/tests/unit_damage_vectors.rs against `sim::damage::native_record`.

Run: GAME_DLL=/path/to/Game.dll python tools/emu/unit_damage_target_emu.py out.txt 600
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

DAMAGE_TARGET = 0x6F3C8850
DAMAGE_POINT = 0x6F3C8770
RESOLVE_UNIT = 0x6F3BDCB0
RESOLVE_WIDGET = 0x6F3BE5B0
IMMEDIATE = 0x6F088050
DELAYED = 0x6F1D4440

DATA = emu.DATA
T = DATA + 0x1000
VT = DATA + 0x2000
STUB_TARGET = DATA + 0x2100
SRC = DATA + 0x1400
ARGS = DATA + 0x5000  # pointers to reals
UNIT_H = 0x100001
WIDGET_H = 0x100002

seen = {}


def rd32(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def wr32(a, v):
    emu.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def f32_bits(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def add_hook(addr, pop, handler):
    def cb(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = rd32(esp)
        args = [rd32(esp + 4 + 4 * i) for i in range(pop // 4)]
        value = handler(uc, args)
        if value is not None:
            uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
        uc.reg_write(UC_X86_REG_EIP, ret)

    emu.mu.hook_add(UC_HOOK_CODE, cb, begin=addr, end=addr)


def resolve_unit(uc, args):
    return SRC if uc.reg_read(UC_X86_REG_ECX) == UNIT_H else 0


def resolve_widget(uc, args):
    return T if uc.reg_read(UC_X86_REG_ECX) == WIDGET_H else 0


def target_slot(uc, args):
    rec = args[1]
    seen["rec"] = [rd32(rec + 4 * i) for i in range(9)]
    seen["slot_args"] = (args[2], args[3])
    seen["this"] = uc.reg_read(UC_X86_REG_ECX)
    return None


def area(kind):
    def h(uc, args):
        seen["area"] = kind
        seen["area_args"] = list(args)
        return None

    return h


def setup():
    wr32(T, VT)
    wr32(VT + 0x120, STUB_TARGET)
    add_hook(RESOLVE_UNIT, 0, resolve_unit)
    add_hook(RESOLVE_WIDGET, 0, resolve_widget)
    add_hook(STUB_TARGET, 16, target_slot)
    add_hook(IMMEDIATE, 0x30, area("immediate"))
    add_hook(DELAYED, 0x28, area("delayed"))


def real_ptr(slot, value):
    wr32(ARGS + 4 * slot, f32_bits(value))
    return ARGS + 4 * slot


def pick(rng, edge, plain):
    return rng.choice(edge) if rng.random() < 0.5 else rng.choice(plain)


def main():
    out_path = sys.argv[1]
    n = int(sys.argv[2])
    rng = random.Random(20261011)
    setup()
    lines = []
    for i in range(n):
        atk = pick(rng, [0, 6, 7, 8, 9, 0xFFFFFFFF], list(range(0, 8)))
        dmg = pick(rng, [0, 4, 5, 31, 32, 33, 0x7FFFFFFF, 0x80000000, 0x80000001, 0xFFFFFFFF, 0x104], list(range(0, 26)))
        wpn = pick(rng, [0, 23, 24, 25, 26, 0xFFFFFFFF], list(range(0, 24)))
        attack = rng.choice([0, 1, 1, 2, 0x100])
        ranged = rng.choice([0, 1, 1, 3])
        amount = float(rng.randrange(0, 200))
        unit = UNIT_H if rng.random() < 0.9 else 0
        widget = WIDGET_H if rng.random() < 0.9 else 0
        seen.clear()
        ret = emu.call(
            DAMAGE_TARGET,
            stack_args=[unit, widget, real_ptr(0, amount), attack, ranged, atk, dmg, wpn],
        )
        rec = seen.get("rec")
        lines.append(
            "T unit=%d widget=%d attack=%d ranged=%d atk=%d dmg=%d wpn=%d ret=%d rec=%s"
            % (
                unit != 0,
                widget != 0,
                attack,
                ranged,
                atk,
                dmg,
                wpn,
                ret & 0xFF,
                "-" if rec is None else "%d,%d,%d,%d,%d,%d,%d,%d,%d" % tuple(rec[:9]),
            )
        )
    for i in range(n):
        atk = pick(rng, [0, 7, 8, 0xFFFFFFFF], list(range(0, 8)))
        dmg = pick(rng, [0, 4, 31, 32, 33, 0x80000000, 0x80000001, 0xFFFFFFFF], list(range(0, 26)))
        wpn = pick(rng, [0, 24, 25, 0xFFFFFFFF], list(range(0, 24)))
        attack = rng.choice([0, 1, 2])
        ranged = rng.choice([0, 1, 3])
        delay = rng.choice([0.0, -1.0, 0.5, 1.0, 1e-30, -1e-30, 3.0, float("nan")])
        unit = UNIT_H if rng.random() < 0.9 else 0
        seen.clear()
        ret = emu.call(
            DAMAGE_POINT,
            stack_args=[
                unit,
                real_ptr(1, delay),
                real_ptr(2, 200.0),
                real_ptr(3, 10.0),
                real_ptr(4, 20.0),
                real_ptr(5, 55.0),
                attack,
                ranged,
                atk,
                dmg,
                wpn,
            ],
        )
        area_kind = seen.get("area", "-")
        aa = seen.get("area_args", [])
        lines.append(
            "P unit=%d delay=%08x attack=%d ranged=%d atk=%d dmg=%d wpn=%d ret=%d area=%s args=%s"
            % (
                unit != 0,
                f32_bits(delay),
                attack,
                ranged,
                atk,
                dmg,
                wpn,
                ret & 0xFF,
                area_kind,
                ",".join("%d" % ((x + (1 << 31)) % (1 << 32) - (1 << 31)) for x in aa) if aa else "-",
            )
        )
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("wrote", len(lines), "cases")


if __name__ == "__main__":
    main()
