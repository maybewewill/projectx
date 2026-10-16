"""V5 Unicorn checks of the stat effects of the melee auras of Game.dll 1.26a (docs/spec/buffs.md section 5, V5).

ORIGINAL buff routines run on a fake buff and a fake owner (the harness of `spell_buff_emu.py`): the Devotion armor apply
`0x6F1ACFD0`, the Endurance attack speed / movement applies `0x6F1AD150` / `0x6F1AD240`, the Unholy movement / life
regeneration applies `0x6F1AD700` / `0x6F1AD810`, the Brilliance mana regeneration apply `0x6F1AC990`. Stubbed: the agile
object refresh `0x6F477D30`, the Misc integer reader `0x6F009F90` (one switch per case: the `IllusionsGet*Bonus` /
`IllusionsCanRestoreLife/Mana` keys), the type record armor `0x6F29BF60` (writes the case armor), the agile max-mana getter
`0x6F4773E0`, the owner's slot `0x130` (max life); the sinks record their arguments (`spell_buff_emu.install`).

Row (hex words):
  <kind> <flags> <strength> <stat> <illusion> <misc> | <sink calls> <sink value> <flags after>
  kind: DV Devotion armor, EA Endurance attack speed, EM Endurance movement, UM Unholy movement, UL Unholy life
  regeneration, BM Brilliance mana regeneration. `stat` is the type armor (DV), max life (UL) or max mana (BM).

Usage: GAME_DLL=/path/Game.dll py tools/emu/v5_aura_emu.py [--dump OUT_FILE] [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import spell_buff_emu as S  # noqa: E402

mu = emu.mu
wr, rd, f2u = A.wr, A.rd, A.f2u
BUFF, TGT, CAP = S.BUFF, S.TGT, S.CAP
UV = S.M + 0x8000        # the owner's vtable
VAR = S.M + 0x8400       # case switches: +0 misc value, +4 armor, +8 max life, +0xC max mana

KINDS = {
    # kind: (function, flags bit of "applied", flags bit of "percent" or 0, value location, sink slot, sink calls slot)
    "DV": (0x6F1ACFD0, 0x200, 0x100, 0xD0, 0x30),
    "EA": (0x6F1AD150, 0x100, 0, 0xEC, 0x20),
    "EM": (0x6F1AD240, 0x200, 0, 0xE4, 0x10),
    "UM": (0x6F1AD700, 0x200, 0, 0xE4, 0x10),
    "UL": (0x6F1AD810, 0x400, 0x100, 0xEC, 0x40),
    "BM": (0x6F1AC990, 0x100, 0x200, 0xD0, 0x50),
}


def install():
    S.install()
    A.patch_jump(0x6F477D30, A.stub_ret(0, 0))
    A.patch_jump(0x6F009F90, S.new_code(b"\xA1" + S.a32(VAR) + b"\xC2\x04\x00"))
    # type armor: ECX = out pointer; writes [ecx] = armor, returns ecx
    A.patch_jump(0x6F29BF60, S.new_code(b"\xA1" + S.a32(VAR + 4) + b"\x89\x01\x89\xC8\xC3"))
    # max mana: thiscall with the out pointer on the stack (ret 4)
    A.patch_jump(0x6F4773E0, S.new_code(b"\x8B\x44\x24\x04\x8B\x0D" + S.a32(VAR + 0xC) + b"\x89\x08\xC2\x04\x00"))
    mu.mem_write(UV, b"\0" * 0x400)
    # owner slot 0x130 (max life): thiscall, the out pointer on the stack; writes the value there, returns the pointer
    wr(UV + 0x130, S.new_code(b"\x8B\x44\x24\x04\x8B\x0D" + S.a32(VAR + 8) + b"\x89\x08\xC2\x04\x00"))


def run(cases, r):
    install()
    rows = []
    for i in range(cases):
        kind = list(KINDS)[i % len(KINDS)]
        fn, applied_bit, pct_bit, val_off, sink_slot = KINDS[kind]
        strength = S.rbits(r)
        stat = S.rbits(r)
        already = r.random() < 0.15
        percent = r.random() < 0.5 and pct_bit != 0
        illusion = r.random() < 0.3
        misc = int(r.random() < 0.4)
        flags = (applied_bit if already else 0) | (pct_bit if percent else 0)
        if kind == "BM":
            # 0x100 = applied, 0x200 = percent
            pass
        mu.mem_write(TGT, b"\0" * 0x400)
        wr(TGT, UV)
        wr(TGT + 0x1E8, S.ATK)       # attack ability present
        wr(TGT + 0x1EC, S.ATK)       # movement ability present
        wr(TGT + 0x5C, 0x40000000 if illusion else 0)
        mu.mem_write(BUFF, b"\0" * 0x400)
        mu.mem_write(S.FVT, bytes(mu.mem_read(0x6F8FFFAC, 0x400)))
        wr(BUFF, S.FVT)
        wr(BUFF + 0x20, flags)
        wr(BUFF + 0x30, TGT)
        wr(BUFF + val_off, strength)
        for h in (0xE0, 0xE8):
            wr(BUFF + h, S.HVT_A)
        wr(VAR, misc)
        wr(VAR + 4, stat)
        wr(VAR + 8, stat)
        wr(VAR + 0xC, stat)
        S.clear_cap()
        S.call(fn, ecx=BUFF)
        rows.append((kind, flags, strength, stat, int(illusion), misc, rd(CAP + sink_slot + 4), rd(CAP + sink_slot),
                     rd(BUFF + 0x20)))
    return rows


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 600
    seed = int(args[1]) if len(args) > 1 else 5
    rows = run(cases, random.Random(seed))
    for row in rows[:8]:
        print(row[0], " ".join("%x" % x for x in row[1:]))
    if dump:
        with open(dump, "w") as fh:
            for row in rows:
                fh.write("%s %x %08x %08x %d %d | %d %08x %x\n" % row)
    print("cases", len(rows))


if __name__ == "__main__":
    main()
