"""V7 Unicorn checks of the regeneration auras Aabr / Aoar / Aarm of Game.dll 1.26a (docs/spec/abilities-unit-a.md, V7).

ORIGINAL code run on fake objects (the harness of `v5_aura_emu.py` / `spell_buff_emu.py`):
  buff apply   Babr 0x6F043FA0 (life), Boar 0x6F043A20 (life), Barm 0x6F043CE0 (mana): flag 0x200 = applied, flag 0x100 CLEAR =
               percent (strength x the owner's max life / max mana through the soft mul 0x6F6EEE20); the rate goes to the
               regeneration-modifier creator 0x6F02DC60 / 0x6F02DD00 (recorded). The illusion gate 0x6F043F60 / 0x6F0439E0 /
               0x6F043CA0 reads Misc IllusionsCanRestoreLife / Mana (stubbed 0x6F009F90).
  ability apply  0x6F06F430 (Aabr), 0x6F06F310 (Aoar), 0x6F06F3A0 (Aarm): the buff is made, its slot 0x36C gets (owner,
               duration, flag) with flag = (0.0 == DataB) from the level record.

Rows (hex words):
  AP <kind> <flags> <strength> <stat> <illusion> <misc> | <sink calls> <sink value> <flags after>
  AB <kind> <DataB bits> | <flag passed to the buff>
Usage: GAME_DLL=/path/Game.dll py tools/emu/v7_aura_emu.py [--dump OUT_FILE] [CASES] [SEED]
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import spell_buff_emu as S  # noqa: E402

mu = emu.mu
wr, rd, f2u = A.wr, A.rd, A.f2u
BUFF, TGT, CAP = S.BUFF, S.TGT, S.CAP
UV = S.M + 0x8000
VAR = S.M + 0x8400       # +0 misc, +4 armor, +8 max life, +0xC max mana, +0x10 DataB
ABIL = S.M + 0x8800
FB = S.M + 0x8A00        # the fake buff handed out by the ability's buff creator
FBVT = S.M + 0x8C00
REC = S.M + 0x8E00       # slot 0x36C arguments: +0 owner, +4 duration, +8 flag, +0xC count

# kind: (function, value slot of the sink in CAP, owner max slot)
APPLY = {"BB": (0x6F043FA0, 0x40), "OL": (0x6F043A20, 0x40), "RM": (0x6F043CE0, 0x50)}
ABIL_FN = {"BB": (0x6F06F430, 0x6F06E630), "OL": (0x6F06F310, 0x6F06E490), "RM": (0x6F06F3A0, 0x6F06E560)}


def install():
    S.install()
    A.patch_jump(0x6F477D30, A.stub_ret(0, 0))
    A.patch_jump(0x6F021500, A.stub_ret(12, 0))
    A.patch_jump(0x6F009F90, S.new_code(b"\xA1" + S.a32(VAR) + b"\xC2\x04\x00"))
    A.patch_jump(0x6F4773E0, S.new_code(b"\x8B\x44\x24\x04\x8B\x0D" + S.a32(VAR + 0xC) + b"\x89\x08\xC2\x04\x00"))
    mu.mem_write(UV, b"\0" * 0x400)
    wr(UV + 0x130, S.new_code(b"\x8B\x44\x24\x04\x8B\x0D" + S.a32(VAR + 8) + b"\x89\x08\xC2\x04\x00"))
    # ability side
    A.patch_jump(0x6F042B20, A.stub_ret(8, 0))
    # level value lookup 0x6F02EA50(this, &tmp, level): returns a pointer to DataB
    A.patch_jump(0x6F02EA50, S.new_code(b"\xB8" + S.a32(VAR + 0x10) + b"\xC2\x08\x00"))
    # fake buff: slot 0x324 (804): ret 4; slot 0x36C (876): record owner, duration, flag
    mu.mem_write(FBVT, b"\0" * 0x400)
    wr(FBVT + 804, A.stub_ret(4, 0))
    rec = (b"\x8B\x44\x24\x04\xA3" + S.a32(REC) + b"\x8B\x44\x24\x08\xA3" + S.a32(REC + 4)
           + b"\x8B\x44\x24\x0C\x25\xFF\x00\x00\x00\xA3" + S.a32(REC + 8) + b"\xFF\x05" + S.a32(REC + 0xC)
           + b"\xC2\x0C\x00")
    wr(FBVT + 876, S.new_code(rec))
    wr(FB, FBVT)
    for fn in (0x6F06E630, 0x6F06E490, 0x6F06E560):
        A.patch_jump(fn, A.stub_ret(0, FB))


def run_apply(r, i):
    kind = list(APPLY)[i % len(APPLY)]
    fn, sink = APPLY[kind]
    strength = S.rbits(r)
    stat = S.rbits(r)
    flags = (0x200 if r.random() < 0.15 else 0) | (0x100 if r.random() < 0.5 else 0)
    illusion = r.random() < 0.3
    misc = int(r.random() < 0.4)
    mu.mem_write(TGT, b"\0" * 0x400)
    wr(TGT, UV)
    wr(TGT + 0x5C, 0x40000000 if illusion else 0)
    mu.mem_write(BUFF, b"\0" * 0x400)
    mu.mem_write(S.FVT, bytes(mu.mem_read(0x6F8811AC, 0x400)))
    wr(BUFF, S.FVT)
    wr(BUFF + 0x20, flags)
    wr(BUFF + 0x30, TGT)
    wr(BUFF + 0xD0, strength)
    wr(VAR, misc)
    wr(VAR + 8, stat)
    wr(VAR + 0xC, stat)
    S.clear_cap()
    S.call(fn, ecx=BUFF)
    return ("AP", kind, flags, strength, stat, int(illusion), misc, rd(CAP + sink + 4), rd(CAP + sink), rd(BUFF + 0x20))


def run_ability(r, i):
    kind = list(ABIL_FN)[i % len(ABIL_FN)]
    fn, _ = ABIL_FN[kind]
    k = r.random()
    datab = r.choice([0, 0x80000000, 0x7FC00000, 0x3F800000, 1, 0xBF800000]) if k < 0.5 else S.rbits(r)
    mu.mem_write(ABIL, b"\0" * 0x200)
    wr(ABIL + 0x30, S.TGT)
    wr(ABIL + 0x50, 0)
    wr(VAR + 0x10, datab)
    mu.mem_write(REC, b"\0" * 0x20)
    S.call(fn, ecx=ABIL, stack_args=(0x40800000,))
    return ("AB", kind, datab, rd(REC + 8), rd(REC + 0xC), rd(REC + 4))


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 600
    seed = int(args[1]) if len(args) > 1 else 7
    r = random.Random(seed)
    install()
    rows = []
    for i in range(cases):
        rows.append(run_apply(r, i))
    for i in range(cases // 2):
        rows.append(run_ability(r, i))
    for row in rows[:6] + rows[-6:]:
        print(row)
    if dump:
        with open(dump, "w") as fh:
            for row in rows:
                if row[0] == "AP":
                    fh.write("AP %s %x %08x %08x %d %d | %d %08x %x\n" % row[1:])
                else:
                    fh.write("AB %s %08x | %d %d %08x\n" % row[1:])
    print("cases", len(rows))


if __name__ == "__main__":
    main()
