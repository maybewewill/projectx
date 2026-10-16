"""V5 Unicorn run of the Water Elemental family summon loop `0x6F163390` (vtable slot 0x3C0) of Game.dll 1.26a
(docs/spec/abilities-human-heroes.md section 18).

Classes sharing the loop (slot 0x3C0 = 0x6F163390): `AHwe` CAbilityWaterElemental `0x6F89BF84`, `AEst` CAbilityScout
`0x6F91E16C`, `AEsv` CAbilitySpiritOfVengeance `0x6F92685C`. ORIGINAL code runs on a fake ability with a real level record
(real getters UnitID slot 0x3E8 -> 0x6F0B0C30, Dur 0x6F02E760, Area 0x6F02E900, DataA 0x6F02E9E0, soft add, F2I) and a fake caster
(vtable slot 0xEC = owner index). Stubbed: the summon creator wrapper `0x6F1D27A0` (records its arguments, returns a new unit),
the link `0x6F052EC0`, `0x6F1D2600`, the release `0x6F299390`, the class slot 0x3E4 (the timed-life buff: records unit and
duration). The placement (`0x6F1D27A0` -> `0x6F1D26B0`) was run for real by `feral_emu.py` (group P): the same wrapper and arguments.

Row:  W <vtable> <Dur> <+0x9C> <DataA> <Area> <UnitID> <owner> | <creator calls> <per call: type is-caster owner-arg ptr-is-Area
      count-arg flag> ... | <link calls> | <slot 0x3E4 calls> <per call: unit-ok duration>

Usage: GAME_DLL=/path/Game.dll py tools/emu/v5_summon_emu.py OUT.txt COUNT SEED
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import chain_emu as C  # noqa: E402
import emu  # noqa: E402
import feral_emu as F  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
w32, r32, fbits, stub, ret, patch = C.w32, C.r32, C.fbits, C.stub, C.ret, C.patch
mem = F.mem

VTABLES = (0x6F89BF84, 0x6F91E16C, 0x6F92685C)
F_LOOP = 0x6F163390
LINK = 0x6F052EC0

creator, slot, link = [], [], []


def on_creator(uc, address, size, user):
    esp, ecx, edx = (uc.reg_read(x) for x in (UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EDX))
    creator.append((ecx, edx, [r32(esp + 4 + 4 * i) for i in range(4)]))


def on_slot(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    unit, dur_ptr = r32(esp + 4), r32(esp + 8)
    slot.append((unit, r32(dur_ptr)))


def on_link(uc, address, size, user):
    link.append(r32(uc.reg_read(UC_X86_REG_ESP) + 4))


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    rec, _ = F.setup()
    patch(LINK, stub(ret(4)))
    s3e4 = stub(b"\x90" + ret(8))
    mu.hook_add(UC_HOOK_CODE, on_creator, begin=rec, end=rec)
    mu.hook_add(UC_HOOK_CODE, on_slot, begin=s3e4, end=s3e4)
    mu.hook_add(UC_HOOK_CODE, on_link, begin=LINK, end=LINK)
    lines = []
    for i in range(count):
        vt = VTABLES[i % len(VTABLES)]
        mu.mem_write(F.VT_COPY, bytes(mu.mem_read(vt, 0x448)))
        w32(F.VT_COPY + 0x3E4, s3e4)
        dur = r.choice([0, 0, C.rnd(r, 0.0, 120.0)])
        plus = r.choice([0, 0, 0, C.rnd(r, 0.0, 5.0)])
        data_a = r.choice([fbits(2.0), fbits(1.0), fbits(3.0), fbits(0.0), fbits(r.uniform(-1.0, 9.0))])
        area = r.choice([fbits(250.0), C.rnd(r, 0.0, 600.0)])
        unit_id = r.choice([0x6F737731, 0x6F737732, 0x6F737733])
        F.fresh_ability(dur, plus, 0, area, unit_id)
        w32(F.LVL + 0x20, data_a)
        w32(F.CASTER + 0x58, r.randint(0, 15))
        creator.clear()
        slot.clear()
        link.clear()
        emu.call(F_LOOP, ecx=F.ABIL)
        owner = r32(F.CASTER + 0x58)
        parts = ["%08x %d %d %d %d %08x" % (ecx, edx == F.CASTER, a[0] == owner, r32(a[1]) == area, a[2], a[3])
                 for ecx, edx, a in creator]
        sl = " ".join("%d %08x" % (u == F.NEWUNIT, d) for u, d in slot)
        lines.append("W %08x %08x %08x %08x %08x %08x %d | %d %s | %d | %d %s" % (
            vt, dur, plus, data_a, area, unit_id, owner, len(creator), " ; ".join(parts), len(link), len(slot), sl))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
