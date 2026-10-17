"""Unicorn checks of the V10 repair family (Arep / Aren / Arst, docs/spec/abilities-v10.md).

The unit-target effect slot 0x424 = 0x6F070AB0 and the tick / payment handler 0x6F070FE0 of CAbilityRepair run on a fake ability and
fake units; every world-dependent callee is a logging stub (see v10_emu.logstub). Rows are dumped as hex words.

Usage: GAME_DLL=/path/Game.dll py tools/emu/v10_repair_emu.py [--dump DIR] --only NAME [CASES] [SEED]
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import v10_emu as V  # noqa: E402

S, A, mu = V.S, V.A, V.mu
wr, rd, f2u = V.wr, V.rd, V.f2u
a32, new_code, level, rbits = V.a32, V.new_code, V.level, V.rbits
logstub, reset_log, read_log, VARS2, M2, UNITS, VTUNIT = V.logstub, V.reset_log, V.read_log, V.VARS2, V.M2, V.UNITS, V.VTUNIT

VT_AREP = 0x6F88CFCC
ABNP, ABLD = 0x41426E50, 0x41426C64
PLAYER = M2 + 0x6000
TGT2 = UNITS                  # the repaired unit
EB2 = M2 + 0x7700             # an existing 'ABnP' ability of the target (builder reference pair at +0xC4)
ABLD_OBJ = M2 + 0x7780

# VARS2 slots of this module
V_ABNP, V_ABLD, V_REF, V_GOLD_OK, V_LUMBER_OK = 0x20, 0x24, 0x28, 0x2C, 0x30
V_GOLDREP, V_LUMBERREP, V_REPTM, V_BLDTM = 0x38, 0x3C, 0x40, 0x44


def install_repair():
    S.install()
    reset_log()
    mu.mem_write(VARS2, b"\0" * 0x100)
    # type record getters (goldRep, lumberRep, reptm, bldtm), thiscall without stack arguments
    for fn, off in ((0x6F29B860, V_GOLDREP), (0x6F29B890, V_LUMBERREP), (0x6F29B7D0, V_REPTM), (0x6F29B7A0, V_BLDTM)):
        A.patch_jump(fn, new_code(b"\xA1" + a32(VARS2 + off) + b"\xC3"))
    # ability lookup: 'ABnP' -> V_ABNP, 'ABld' -> V_ABLD, else 0
    tail = (b"\x8B\x44\x24\x04\x3D" + a32(ABNP) + b"\x75\x08\xA1" + a32(VARS2 + V_ABNP) + b"\xC2\x14\x00"
            + b"\x3D" + a32(ABLD) + b"\x75\x08\xA1" + a32(VARS2 + V_ABLD) + b"\xC2\x14\x00" + b"\x31\xC0\xC2\x14\x00")
    A.patch_jump(0x6F0787D0, logstub(1, [(0, False), (1, False), (2, False)], tail=tail))
    A.patch_jump(0x6F4786B0, new_code(b"\xA1" + a32(VARS2 + V_REF) + b"\xC3"))      # reference resolver
    A.patch_jump(0x6F4778F0, logstub(2, [(0, True), (1, False), (2, False)], retn=0x14))
    A.patch_jump(0x6F02D730, logstub(3, [(0, False)], tail=b"\xA1" + a32(VARS2 + V_GOLD_OK) + b"\xC2\x04\x00"))
    A.patch_jump(0x6F02D760, logstub(4, [(0, False)], tail=b"\xA1" + a32(VARS2 + V_LUMBER_OK) + b"\xC2\x04\x00"))
    A.patch_jump(0x6F416A00, logstub(5, [(0, False)], retn=0x10))
    A.patch_jump(0x6F416A40, logstub(6, [(0, False)], retn=0x10))
    A.patch_jump(0x6F2834E0, new_code(b"\xB8" + a32(PLAYER) + b"\xC3"))
    A.patch_jump(0x6F021500, logstub(7, [(0, False)], retn=0xC))
    A.patch_jump(0x6F26CB90, logstub(8, [(0, False), (1, False)], retn=8))
    A.patch_jump(0x6F2AB3E0, logstub(9, [(0, False), (1, False)], retn=8))
    A.patch_jump(0x6F2AB1E0, logstub(10, [(0, False), (1, False)], retn=8))
    A.patch_jump(0x6F0452C0, logstub(11, [(0, False), (1, True), (2, True), (3, True), (4, True)], retn=0x14))
    A.patch_jump(0x6F0453C0, logstub(12, [(0, False)], retn=4))
    A.patch_jump(0x6F077630, logstub(13, [], retn=0))
    A.patch_jump(0x6F06BBB0, logstub(14, [(0, False)], retn=4))
    A.patch_jump(0x6F0419C0, logstub(15, [(0, False)], retn=4))
    A.patch_jump(0x6F02DC60, logstub(16, [(1, True), (2, True)], retn=0xC))
    A.patch_jump(0x6F029580, logstub(17, [(0, False)], retn=4))
    # target unit: slot 0x130 = maximum life at unit+0x304
    mu.mem_write(VTUNIT, b"\0" * 0x200)
    wr(VTUNIT + 0x130, new_code(b"\x8B\x44\x24\x04\x8B\x89" + a32(0x304) + b"\x89\x08\xC2\x04\x00"))


def setup_case(r, state):
    A.make_ability(VT_AREP, 0x41726570, 0)
    O = A.OBJ
    da, db, dc, dd = (rbits(r) if r.random() < 0.2 else f2u(r.choice([0.35, 1.5, 0.5, 2.0, 0.25, 1.0, 0.0, 3.0])) for _ in range(4))
    level(A.LVL, 0, 0, [da, db, dc, dd, 0, 0, 0, 0, 0])
    for off in (0x150, 0x158, 0x160, 0x168):
        wr(O + off, S.HVT_A)
    gold, lumber = r.choice([0, 0, 150, 500, 1200, 75]), r.choice([0, 0, 50, 300, 100])
    reptm, bldtm = r.choice([0, 30, 60, 120, 150, 3]), r.choice([0, 30, 60, 120, 150])
    maxlife = f2u(float(r.choice([100, 500, 1000, 1300, 2000])))
    wr(VARS2 + V_GOLDREP, gold)
    wr(VARS2 + V_LUMBERREP, lumber)
    wr(VARS2 + V_REPTM, reptm)
    wr(VARS2 + V_BLDTM, bldtm)
    mu.mem_write(TGT2, b"\0" * 0x400)
    wr(TGT2, VTUNIT)
    wr(TGT2 + 0x30, 0x68757432)
    wr(TGT2 + 0x304, maxlife)
    mu.mem_write(EB2, b"\0" * 0x100)
    wr(EB2 + 0xC4, 1)
    wr(EB2 + 0xC8, 2)
    wr(VARS2 + V_ABNP, 0 if state == 0 else EB2)
    wr(VARS2 + V_ABLD, ABLD_OBJ)
    wr(VARS2 + V_REF, 0x1234 if state == 2 else 0)
    wr(VARS2 + V_GOLD_OK, r.choice([1, 1, 0]))
    wr(VARS2 + V_LUMBER_OK, r.choice([1, 1, 0]))
    return [da, db, dc, dd, gold, lumber, reptm, bldtm, maxlife, state, rd(VARS2 + V_GOLD_OK), rd(VARS2 + V_LUMBER_OK)]


def check_rep_probe(cases, r, rows):
    """Probe of the repair start 0x6F070AB0 (not a verification): rows hold the inputs and the raw log."""
    install_repair()
    for _ in range(cases):
        head = setup_case(r, r.choice([0, 1, 2]))
        reset_log()
        S.call(0x6F070AB0, ecx=A.OBJ, stack_args=(TGT2,))
        cnt, log = read_log()
        rows.append(head + [cnt] + log)


def check_rep_start(cases, r, rows):
    """Repair start 0x6F070AB0 for a finished building (no 'ABnP' ability on the target). Rows: DataA DataB gold lumber reptm maxlife
    gold_ok lumber_ok | log count | log (5 dwords per call)."""
    install_repair()
    for _ in range(cases):
        head = setup_case(r, 0)
        reset_log()
        S.call(0x6F070AB0, ecx=A.OBJ, stack_args=(TGT2,))
        cnt, log = read_log()
        da, db, gold, lumber, reptm, maxlife = head[0], head[1], head[4], head[5], head[6], head[8]
        rows.append([da, db, gold, lumber, reptm, maxlife, head[10], head[11], cnt] + log)


def check_rep_gate(cases, r, rows):
    """The class target gate of Repair, slot 0x3F0 = 0x6F04BF70 (ecx = ability, argument = target), for a target without 'ABnP'.
    Rows: flags life maxlife buns goldRep lumberRep gold_ok lumber_ok | code."""
    install_repair()
    BUNS = 0x42756E73
    # lookups: 'ABnP' -> 0 (never present here), 'Buns' -> V_ABLD slot
    tail = (b"\x8B\x44\x24\x04\x3D" + a32(BUNS) + b"\x75\x08\xA1" + a32(VARS2 + V_ABLD) + b"\xC2\x14\x00" + b"\x31\xC0\xC2\x14\x00")
    A.patch_jump(0x6F0787D0, logstub(1, [(0, False)], tail=tail))
    wr(VARS2 + V_ABLD, 0)
    for _ in range(cases):
        A.make_ability(VT_AREP, 0x41726570, 0)
        flags = r.choice([0, 0, 0, 0x100, 0x40000000, 0x40000100, 0x4])
        k = r.random()
        if k < 0.6:
            mx = f2u(float(r.choice([100, 500, 1300])))
            life = f2u(float(r.choice([0, 50, 99, 100, 499, 500, 1299, 1300])) - r.choice([0.0, 0.5, 0.25]))
        else:
            mx, life = rbits(r), rbits(r)
        buns = r.choice([0, 0, 0, 1])
        gold, lumber = r.choice([0, 0, 150, 500]), r.choice([0, 0, 50, 300])
        gok, lok = r.choice([0, 1, 1]), r.choice([0, 1, 1])
        mu.mem_write(TGT2, b"\0" * 0x400)
        wr(TGT2, VTUNIT)
        wr(TGT2 + 0x30, 0x68757432)
        wr(TGT2 + 0x5C, flags)
        wr(TGT2 + 0x300, life)
        wr(TGT2 + 0x304, mx)
        wr(VARS2 + V_ABLD, 0x7777 if buns else 0)
        wr(VARS2 + V_GOLDREP, gold)
        wr(VARS2 + V_LUMBERREP, lumber)
        wr(VARS2 + V_GOLD_OK, gok)
        wr(VARS2 + V_LUMBER_OK, lok)
        wr(VTUNIT + 0x12C, new_code(b"\x8B\x44\x24\x04\x8B\x89" + a32(0x300) + b"\x89\x08\xC2\x04\x00"))
        reset_log()
        code = S.call(0x6F04BF70, ecx=A.OBJ, stack_args=(TGT2,))
        rows.append([flags, life, mx, buns, gold, lumber, gok, lok, code])


CHECKS = {
    "rep_probe": check_rep_probe,
    "rep_start": check_rep_start,
    "rep_gate": check_rep_gate,
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
    seed = int(args[1]) if len(args) > 1 else 111
    for name, fn in CHECKS.items():
        if only is None or name not in only:
            continue
        r = random.Random(seed)
        rows = []
        fn(cases, r, rows)
        print("%s rows %d" % (name, len(rows)))
        for row in rows[:int(os.environ.get("SHOW", "3"))]:
            print(" ".join(("%08x" % x) if isinstance(x, int) else x for x in row))
        if dump:
            os.makedirs(dump, exist_ok=True)
            with open(os.path.join(dump, name + ".txt"), "w") as fh:
                for row in rows:
                    fh.write(" ".join(("%08x" % x) if isinstance(x, int) else x for x in row) + "\n")


if __name__ == "__main__":
    main()
