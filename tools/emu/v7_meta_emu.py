"""V7 Unicorn run of the Metamorphosis morph-in hook and revert hook (AEme, vtable 0x6F8D9A1C) of Game.dll 1.26a.

ORIGINAL slot 0x40C `0x6F13D5C0` (usercall: ECX = ability, EDI = anything) and slot 0x414 `0x6F111D20` run on a fake ability with a real
level record (real getters HeroDur +0xC via `0x6F02E7D0`, DataE +0x30 via `0x6F02EBA0`). Stubs (recorded): the buff constructor
`0x6F13A330` (a fake buff whose slot 0x324 is counted and whose slot 0x358 records the unit and the duration pointee), the lookup of an
earlier buff `0x6F0787D0`, the removal `0x6F079CC0`, the life sink `0x6F2AC450` (value pointee and mode), the Misc reader
`0x6F009F90` (MorphAlternateDisable = 0), the property holder at ability+0x1A4 (a vtable whose slot 0 stores the pointee).

Row (hex words):
  ME <illusion> <HeroDur> <DataE> <existing> | <buff made> <duration bits> <unit-is-owner> <existing removed> <sink calls> <sink value> <sink mode> <holder value>
  MR <DataE-in-holder> <existing> | <existing removed> <sink calls> <sink value> <sink mode> <holder value after>
Usage: GAME_DLL=/path/Game.dll py tools/emu/v7_meta_emu.py [--dump OUT_FILE] [CASES] [SEED]
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import spell_buff_emu as S  # noqa: E402
import v7_hits_emu as H  # noqa: E402

mu = emu.mu
wr, rd, f2u = A.wr, A.rd, A.f2u
M = S.M
ABIL, UNIT, LVL = H.ABIL, H.UNIT, H.LVL
FB, FBVT, EXIST = M + 0x7000, M + 0x7200, M + 0x7700
HVT2 = M + 0x7800
cap = {}


def on_make(esp, ecx, edx):
    cap["made"] = cap.get("made", 0) + 1
    return FB


on_make.pop = 4


def on_lookup(esp, ecx, edx):
    return EXIST if cap["existing"] else 0


on_lookup.pop = 0x14


def on_remove(esp, ecx, edx):
    cap["removed"] = cap.get("removed", 0) + 1
    return 0


on_remove.pop = 4


def on_apply(esp, ecx, edx):
    cap["unit_ok"] = rd(esp + 4) == UNIT
    cap["dur"] = rd(rd(esp + 8))
    return 0


on_apply.pop = 8


def on_sink(esp, ecx, edx):
    cap.setdefault("sinks", []).append((rd(rd(esp + 4)), rd(esp + 8)))
    return 0


on_sink.pop = 8


def on_misc(esp, ecx, edx):
    return 0


on_misc.pop = 4


def install():
    S.install()
    A.patch_seh()
    H.STUBS.update({0: A.stub_ret(4, 0), 1: A.stub_ret(4, 1)})
    A.patch_jump(0x6F472890, A.stub_ret(0, UNIT))
    A.patch_jump(0x6F042B20, A.stub_ret(8, 0))
    A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))
    H.hook(0x6F13A330, on_make)
    H.hook(0x6F0787D0, on_lookup)
    H.hook(0x6F079CC0, on_remove)
    H.hook(0x6F2AC450, on_sink)
    H.hook(0x6F009F90, on_misc)
    mu.mem_write(FBVT, b"\0" * 0x400)
    wr(FBVT + 0x324, A.stub_ret(4, 0))
    stub358 = A.stub_ret(8, 0)
    wr(FBVT + 0x358, stub358)
    H.hook(stub358, on_apply)
    mu.mem_write(FB, b"\0" * 0x40)
    wr(FB, FBVT)
    mu.mem_write(HVT2, b"\0" * 0x40)
    wr(HVT2, S.new_code(bytes([0x8B, 0x44, 0x24, 0x04, 0x8B, 0x00, 0x89, 0x41, 0x04, 0xC2, 0x08, 0x00])))


def prepare(hero_dur, data_e, illusion):
    H.fresh(0x6F8D9A1C, 0, 0, [0, 0, 0, 0, data_e], 0, 0)
    wr(LVL + 0x0C, hero_dur)
    wr(UNIT + 0x5C, 0x40000000 if illusion else 0)
    wr(ABIL + 0x1A4, HVT2)
    wr(ABIL + 0x1A4 + 4, 0xDEAD)
    cap.clear()


def run_me(r):
    illusion = r.random() < 0.25
    hero_dur = f2u(r.choice([0.0, -0.0, 60.0, 1.5, r.uniform(0, 120), -5.0]))
    if r.random() < 0.1:
        hero_dur = r.choice([0x7FC00000, 0x7F800000, 0x80000001])
    data_e = S.rbits(r)
    cap["existing"] = r.random() < 0.3
    prepare(hero_dur, data_e, illusion)
    cap["existing"] = r.random() < 0.3
    ex = cap["existing"]
    S.call(0x6F13D5C0, ecx=ABIL, edx=0)
    sinks = cap.get("sinks", [])
    s = sinks[0] if sinks else (0, 0)
    return "ME %d %08x %08x %d | %d %08x %d %d %d %08x %d %08x" % (
        illusion, hero_dur, data_e, ex, cap.get("made", 0), cap.get("dur", 0), int(cap.get("unit_ok", False)),
        cap.get("removed", 0), len(sinks), s[0], s[1], rd(ABIL + 0x1A8))


def run_mr(r):
    data_e = S.rbits(r)
    prepare(0, 0, False)
    cap["existing"] = r.random() < 0.5
    ex = cap["existing"]
    wr(ABIL + 0x1A8, data_e)
    S.call(0x6F111D20, ecx=ABIL)
    sinks = cap.get("sinks", [])
    s = sinks[0] if sinks else (0, 0)
    return "MR %08x %d | %d %d %08x %d %08x" % (data_e, ex, cap.get("removed", 0), len(sinks), s[0], s[1], rd(ABIL + 0x1A8))


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 300
    seed = int(args[1]) if len(args) > 1 else 7
    r = random.Random(seed)
    install()
    rows = []
    for _ in range(cases):
        rows.append(run_me(r))
        rows.append(run_mr(r))
    for row in rows[:6]:
        print(row)
    if dump:
        with open(dump, "w") as fh:
            fh.write("\n".join(rows) + "\n")
    print("cases", len(rows))


if __name__ == "__main__":
    main()
