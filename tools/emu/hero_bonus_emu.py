"""Unicorn run of the hero derived-bonus readers (Game.dll 1.26a, docs/spec/heroes.md section 9).

Each reader is __thiscall(ECX = hero sub-object at unit+0x1F0) and reads one Misc real through the real reader
`0x6F009EE0` (hooked: returns the synthetic value for the key), one attribute total through the counter read
`0x6F473170` (hooked: returns the counter keyed by the reference object address). The attribute cores
`0x6F0DB300/0x6F0DB350/0x6F0DB3A0` run for real, and so do the softfloat helpers. The result is written through
stack pointers: both stack pointers point to zeroed buffers and both are logged, with the return value.

Run:  GAME_DLL=/path/Game.dll python3 tools/emu/hero_bonus_emu.py OUT.txt [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX, UC_X86_REG_ECX  # noqa: E402

COUNTER = 0x6F473170    # __thiscall(ECX = reference object), returns the counter, no stack arguments
MISC_REAL = 0x6F009EE0  # __thiscall(ECX = out real), [esp+4] = key (C string), [esp+8] = index; ret 8

READERS = {
    "HP": (0x6F208060, 2),      # StrHitPointBonus (two stack pointers)
    "REGEN": (0x6F2080F0, 1),   # StrRegenBonus
    "STRATK": (0x6F207F40, 1),  # StrAttackBonus, core chosen by hero+0xCC
    "ARMOR": (0x6F207FB0, 1),   # AgiDefenseBase + AgiDefenseBonus
    "MOVE": (0x6F208170, 1),    # AgiMoveBonus
    "ASPD": (0x6F2081F0, 1),    # AgiAttackSpeedBonus
    "MANA": (0x6F208270, 1),    # IntManaBonus
}
MISC_KEYS = ("StrHitPointBonus", "StrRegenBonus", "StrAttackBonus", "AgiDefenseBase", "AgiDefenseBonus",
             "AgiMoveBonus", "AgiAttackSpeedBonus", "IntManaBonus")

UNIT = emu.DATA + 0x1000
HERO = emu.DATA + 0x2000
OUT1 = emu.DATA + 0x3000
OUT2 = emu.DATA + 0x3010
state = {"counter": {}, "misc": {}}


def rd32(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def wr32(a, v):
    emu.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def read_cstr(a):
    out = b""
    while True:
        c = bytes(emu.mu.mem_read(a, 1))
        if c == b"\0":
            return out.decode()
        out += c
        a += 1


def on_counter(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    uc.reg_write(UC_X86_REG_EAX, state["counter"].get(ecx, 0) & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def on_misc(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    key = read_cstr(rd32(esp + 4))
    ecx = uc.reg_read(UC_X86_REG_ECX)
    wr32(ecx, state["misc"].get(key, 0))
    uc.reg_write(UC_X86_REG_EAX, ecx)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + 8)
    uc.reg_write(UC_X86_REG_EIP, ret)


def f2bits(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def setup_hero(rng):
    """Writes random hero fields and the counter and Misc stubs; returns the input record."""
    wr32(UNIT + 0x1F0, HERO)
    cc = rng.randint(0, 3)
    base_str = rng.randint(-40, 400)
    base_agi = rng.randint(-40, 400)
    base_int = rng.randint(-40, 400)
    gains = [0.0, 1.0, 2.5, 0.5, 1.75, 3.3333333, 0.1, -1.25, 2.2, 1000.0, 7.9]

    def g():
        return rng.choice(gains) if rng.random() < 0.6 else rng.uniform(-5.0, 20.0)

    gs, ga, gi = f2bits(g()), f2bits(g()), f2bits(g())
    counter = rng.randint(0, 25)
    base_counter_int = rng.randint(-5, 60)
    wr32(HERO + 0x94, base_str & 0xFFFFFFFF)
    wr32(HERO + 0xD4, gs)
    wr32(HERO + 0xA8, base_agi & 0xFFFFFFFF)
    wr32(HERO + 0xE4, ga)
    wr32(HERO + 0xDC, gi)
    wr32(HERO + 0xCC, cc)
    state["counter"] = {HERO + 0x6C: counter, HERO + 0x7C: base_counter_int}
    misc = {}
    for key in MISC_KEYS:
        if rng.random() < 0.5:
            v = rng.choice([0.0, 1.0, 0.5, 0.25, 0.3, 0.16666667, 2.0, -0.5, 1.2])
        else:
            v = rng.uniform(-3.0, 5.0)
        misc[key] = f2bits(v)
    state["misc"] = misc
    return dict(cc=cc, base_str=base_str, gs=gs, base_agi=base_agi, ga=ga, base_int=base_int, gi=gi,
                counter=counter, base_counter_int=base_counter_int, misc=misc)


FACTORS = (1.0, 0.5, 2.0, 0.0, 1.25, 0.75, -1.5, 3.0)


def run_reader(name, factor_bits):
    """Runs one reader. The HP reader multiplies by the real at its second stack pointer (the factor), which is
    written here; the others take one pointer. Returns (eax, out1, out2)."""
    fn, nargs = READERS[name]
    wr32(OUT1, 0)
    wr32(OUT2, factor_bits)
    args = (OUT1, OUT2) if nargs == 2 else (OUT1,)
    eax = emu.call(fn, ecx=HERO, stack_args=args)
    return eax, rd32(OUT1), rd32(OUT2)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    out_path = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 200
    rng = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 11)
    emu.mu.hook_add(UC_HOOK_CODE, on_counter, begin=COUNTER, end=COUNTER)
    emu.mu.hook_add(UC_HOOK_CODE, on_misc, begin=MISC_REAL, end=MISC_REAL)
    lines = []
    for _ in range(cases):
        rec = setup_hero(rng)
        factor = f2bits(rng.choice(FACTORS))
        for name in READERS:
            eax, o1, o2 = run_reader(name, factor)
            fields = [name, str(rec["cc"]), str(rec["base_str"]), "%08x" % rec["gs"], str(rec["base_agi"]),
                      "%08x" % rec["ga"], str(rec["base_int"]), "%08x" % rec["gi"], str(rec["counter"]),
                      str(rec["base_counter_int"])]
            fields += ["%08x" % rec["misc"][k] for k in MISC_KEYS]
            fields += ["%08x" % (factor if name == "HP" else 0), "%08x" % o1, "%08x" % o2,
                       "%08x" % (eax & 0xFFFFFFFF)]
            lines.append(" ".join(fields))
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("reader cases written: %d -> %s" % (len(lines), out_path))


if __name__ == "__main__":
    main()
