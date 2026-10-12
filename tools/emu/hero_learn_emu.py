"""Unicorn check of the hero skill learning (Game.dll 1.26a, docs/spec/heroes.md section 12).

Runs the REAL routines of the hero ability object (`CAbilityHero`, the 'AHer' object, ECX = hero):

  0x6F207B60  required hero level of the next level of a slot:  slot base (+0x124) + level * levelSkip
  0x6F208A00  can the slot learn its next level (the gate of the learn)
  0x6F2095D0  the learn itself (SelectHeroSkill: error cases, slot search, level set, skill point spend)
  0x6F207C80 / 0x6F207CC0  the learned level count and the slot max (the skill point cap of the kill branch)

Only the callees below the hero object answer from synthetic data (stand-ins, recorded by the hooks): the ability
lookup 0x6F0787D0, the type lookup 0x6F265ED0, the player accessor 0x6F2834E0, the ability-availability test
0x6F40B490, the dependency check 0x6F40FB70, the level counter read 0x6F473170, the cheat object 0x6F4C34D0, the Misc
integer reader 0x6F009F90, and the notifications (0x6F2077A0 error, 0x6F296F70 level set, 0x6F2076F0 add, 0x6F3329F0,
0x6F021270, 0x6F26FE50). The ability level-up (vtable slot 185, offset 0x2E4) is a stand-in that adds one to the level
index at ability+0x50 (its meaning is CONFIRMED in docs/spec/abilities.md; the stub only keeps the arithmetic going).

Output: one line per case; the Rust test crates/sim/tests/hero_learn_vectors.rs recomputes each with sim::heroes::learn.

Run:  GAME_DLL=/path/to/Game.dll py tools/emu/hero_learn_emu.py OUT_CAN.txt OUT_LEARN.txt [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX  # noqa: E402

D = emu.DATA
HERO = D + 0x2000
UNIT = D + 0x2400
PLAYER = D + 0x2500
TYPEDEF = D + 0x2600
CHEAT = D + 0x2700
CELL = D + 0x2780
CFLAGS = D + 0x2790
OUTFLAG = D + 0x2800
VTAB = D + 0x3000
STUB_LVLUP = D + 0x3800
ABIL0 = D + 0x4000     # fake ability objects, 0x100 apart
DEF0 = D + 0x6000      # fake definitions, 0x40 apart
KNOWN_FLAG_ADDR = D + 0x2900

A_LOOKUP, A_TYPE, A_PLAYER, A_AVAIL, A_DEPS, A_COUNTER, A_CHEAT, A_MISC = (
    0x6F0787D0, 0x6F265ED0, 0x6F2834E0, 0x6F40B490, 0x6F40FB70, 0x6F473170, 0x6F4C34D0, 0x6F009F90)
A_ERR, A_SETLVL, A_ADD, A_ND, A_U, A_N = 0x6F2077A0, 0x6F296F70, 0x6F2076F0, 0x6F3329F0, 0x6F021270, 0x6F26FE50
A_472890 = 0x6F472890

ROA, ROU = 0x41556661, 0x41556675  # 'AUfa', 'AUfu' (Frost Armor and its autocast form: the alias pair of the slot search)


def rd32(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def wr32(a, v):
    emu.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


st = {}
log = []


def pop_ret(uc, eax, nbytes):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    uc.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + nbytes)
    uc.reg_write(UC_X86_REG_EIP, ret)


def arg(uc, i):
    return rd32(uc.reg_read(UC_X86_REG_ESP) + 4 + 4 * i)


def hook(addr, fn):
    def cb(uc, address, size, user):
        fn(uc)
    emu.mu.hook_add(UC_HOOK_CODE, cb, begin=addr, end=addr)


def h_lookup(uc):  # ecx = unit, stack: code, 0, 0, 1, 0 (ret 0x14)
    code = arg(uc, 0)
    pop_ret(uc, st["abil_ptr"].get(code, 0), 0x14)


def h_type(uc):  # fastcall ecx = code (rawcode or type id), edx = 0
    code = uc.reg_read(UC_X86_REG_ECX)
    pop_ret(uc, st["def_ptr"].get(code, 0), 0)


def h_player(uc):
    pop_ret(uc, PLAYER, 0)


def h_avail(uc):  # thiscall ecx = player, stack: code (ret 4)
    st["avail_ecx"] = uc.reg_read(UC_X86_REG_ECX)
    pop_ret(uc, st["avail"], 4)


def h_deps(uc):  # stdcall (type id, level, &flag, a4), ret 0x10
    a1, a2, a3, a4 = arg(uc, 0), arg(uc, 1), arg(uc, 2), arg(uc, 3)
    st["deps_args"] = (a1, a2, a4)
    wr32(a3, st["deps_flag"])
    pop_ret(uc, st["deps_ok"], 0x10)


def h_counter(uc):
    st["counter_ecx"] = uc.reg_read(UC_X86_REG_ECX)
    pop_ret(uc, st["hero_counter"], 0)


def h_cheat(uc):
    pop_ret(uc, CHEAT if st["cheat_obj"] else CHEAT, 0)


def h_misc(uc):  # ecx "Misc", edx key, stack: 0 (ret 4)
    pop_ret(uc, st["misc_skip"], 4)


def h_err(uc):
    log.append(("E", arg(uc, 0)))
    pop_ret(uc, 0, 0xC)


def h_setlvl(uc):
    log.append(("S", arg(uc, 0), arg(uc, 1)))
    pop_ret(uc, 0, 8)


def h_add(uc):
    log.append(("A", arg(uc, 0)))
    pop_ret(uc, 0, 4)


def h_nd(uc):
    log.append(("D",))
    pop_ret(uc, 0, 0)


def h_u(uc):
    log.append(("U",))
    pop_ret(uc, 0, 0)


def h_n(uc):
    log.append(("N",))
    pop_ret(uc, 0, 4)


def h_lvlup(uc):  # ecx = ability object: vtable slot 185 stand-in
    ab = uc.reg_read(UC_X86_REG_ECX)
    wr32(ab + 0x50, rd32(ab + 0x50) + 1)
    log.append(("V", st["ptr_code"][ab]))
    pop_ret(uc, 0, 0)


def setup(case):
    emu.mu.mem_write(D + 0x2000, b"\0" * 0x2000)  # hero, unit, player, ... scratch
    emu.mu.mem_write(D + 0x4000, b"\0" * 0x2800)
    wr32(HERO + 0x30, UNIT)
    wr32(HERO + 0x34, 0x48706C64)  # hero type id
    wr32(HERO + 0x54, TYPEDEF)      # cached type record: the real 0x6F02E640 then answers from it
    wr32(TYPEDEF + 0x28, case["dep_ptr"])
    wr32(TYPEDEF + 0x2C, case["dep_nonzero"])
    wr32(HERO + 0x90, case["points"])
    for i in range(5):
        wr32(HERO + 0xF4 + 4 * i, case["codes"][i])
        wr32(HERO + 0x10C + 4 * i, case["maxl"][i])
        wr32(HERO + 0x124 + 4 * i, case["req"][i])
    wr32(VTAB + 0x2E4, STUB_LVLUP)
    st["abil_ptr"] = {}
    st["ptr_code"] = {}
    st["def_ptr"] = {}
    n = 0
    for code, lvl in case["learned"].items():
        p = ABIL0 + 0x100 * n
        wr32(p, VTAB)
        wr32(p + 0x50, lvl)
        st["abil_ptr"][code] = p
        st["ptr_code"][p] = code
        n += 1
    m = 0
    for code, skip in case["skip"].items():
        p = DEF0 + 0x40 * m
        wr32(p + 0x44, skip)
        st["def_ptr"][code] = p
        m += 1
    wr32(CHEAT + 0x10, CELL)
    wr32(CELL, CFLAGS)
    wr32(CFLAGS + 4, case["cheat"])
    st["avail"] = case["avail"]
    st["deps_ok"] = case["deps_ok"]
    st["deps_flag"] = case["deps_flag"]
    st["hero_counter"] = case["counter"]
    st["cheat_obj"] = True
    st["misc_skip"] = case["misc"]
    log.clear()


def rand_case(rng):
    ids = rng.sample(range(0x41300000, 0x41300040), 5)
    codes = [c if rng.random() < 0.85 else 0 for c in ids]
    if rng.random() < 0.15:
        codes[rng.randrange(5)] = ROU
    maxl = [rng.choice([0, 1, 2, 3, 3, 3, 4, 5]) for _ in range(5)]
    req = [rng.choice([1, 1, 2, 3, 5, 6, 6, 8, 10]) for _ in range(5)]
    learned = {}
    skip = {}
    for i in range(5):
        c = codes[i]
        if c == 0:
            continue
        if rng.random() < 0.6:
            learned[c] = rng.randint(0, max(maxl[i] - 1, 0)) if rng.random() < 0.8 else rng.randint(0, 5)
        if rng.random() < 0.7:
            skip[c] = rng.choice([0, 0, 1, 2, 2, 3, 6])
    return dict(
        codes=codes, maxl=maxl, req=req, learned=learned, skip=skip,
        points=rng.choice([0, 1, 1, 2, 3, 5]),
        avail=rng.choice([0, 1, 1, 1, 1, 1]),
        deps_ok=rng.choice([0, 1, 1, 1, 1]),
        deps_flag=rng.choice([0, 1]),
        counter=rng.randint(0, 14),
        cheat=rng.choice([0, 0, 0x100]),
        misc=rng.choice([0, 1, 2, 3]),
        dep_ptr=rng.choice([0, 0x1234, 0xBEEF00]),
        dep_nonzero=rng.choice([0, 1]),
    )


def fmt_case(c):
    parts = []
    parts.append(",".join("%08x" % x for x in c["codes"]))
    parts.append(",".join(str(x) for x in c["maxl"]))
    parts.append(",".join(str(x) for x in c["req"]))
    parts.append(",".join("%08x:%d" % kv for kv in sorted(c["learned"].items())) or "-")
    parts.append(",".join("%08x:%d" % kv for kv in sorted(c["skip"].items())) or "-")
    parts += [str(c["points"]), str(c["avail"]), str(c["deps_ok"]), str(c["deps_flag"]), str(c["counter"]),
              "%x" % c["cheat"], str(c["misc"])]
    return " ".join(parts)


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(2)
    out_can, out_learn = sys.argv[1], sys.argv[2]
    cases = int(sys.argv[3]) if len(sys.argv) > 3 else 400
    rng = random.Random(int(sys.argv[4]) if len(sys.argv) > 4 else 1)
    for a, f in ((A_LOOKUP, h_lookup), (A_TYPE, h_type), (A_PLAYER, h_player), (A_AVAIL, h_avail),
                 (A_DEPS, h_deps), (A_COUNTER, h_counter), (A_CHEAT, h_cheat), (A_MISC, h_misc),
                 (A_ERR, h_err), (A_SETLVL, h_setlvl), (A_ADD, h_add), (A_ND, h_nd), (A_U, h_u), (A_N, h_n),
                 (STUB_LVLUP, h_lvlup)):
        hook(a, f)
    can_lines, learn_lines = [], []
    for _ in range(cases):
        c = rand_case(rng)
        slot = rng.randrange(5)
        setup(c)
        wr32(OUTFLAG, 0xDEAD)
        r = emu.call(0x6F208A00, ecx=HERO, stack_args=(slot, OUTFLAG))
        out = rd32(OUTFLAG)
        assert st.get("counter_ecx", HERO + 0x6C) == HERO + 0x6C
        can_lines.append("%s | %d | %d %d" % (fmt_case(c), slot, r & 0xFFFFFFFF, out))
        # required level of the slot at the next level of the unit's ability (also run directly)
        setup(c)
        code = c["codes"][slot]
        lvl = rng.randint(0, 5)
        rq = emu.call(0x6F207B60, ecx=HERO, stack_args=(slot, lvl))
        rq = struct.unpack("<i", struct.pack("<I", rq & 0xFFFFFFFF))[0]
        can_lines.append("REQ %s | %d %d | %d" % (fmt_case(c), slot, lvl, rq))
        # the cap helpers: learned count and slot max for a code
        for probe in (code, rng.choice(c["codes"]), 0x41399999):
            setup(c)
            g = emu.call(0x6F207C80, ecx=HERO, stack_args=(probe,))
            setup(c)
            m = emu.call(0x6F207CC0, ecx=HERO, stack_args=(probe,))
            m = struct.unpack("<i", struct.pack("<I", m & 0xFFFFFFFF))[0]
            can_lines.append("CAP %s | %08x | %d %d" % (fmt_case(c), probe, g & 0xFFFFFFFF, m))
        # the learn
        choices = [0, 0x41399998]
        nz = [x for x in c["codes"] if x]
        if nz:
            choices += [rng.choice(nz)] * 6
        if rng.random() < 0.1 or (ROU in c["codes"] and rng.random() < 0.7):
            choices += [ROA] * 8
        a2 = rng.choice(choices)
        a3 = rng.choice([0, 0, 0, 1])
        setup(c)
        r = emu.call(0x6F2095D0, ecx=HERO, stack_args=(a2, a3))
        points = rd32(HERO + 0x90)
        learn_lines.append("%s | %08x %d | %s | %d" % (fmt_case(c), a2, a3,
                           ";".join(" ".join(str(x) for x in e) for e in log) or "-", points))
    with open(out_can, "w") as fh:
        fh.write("\n".join(can_lines) + "\n")
    with open(out_learn, "w") as fh:
        fh.write("\n".join(learn_lines) + "\n")
    print("cases: %d can-learn lines, %d learn lines" % (len(can_lines), len(learn_lines)))


if __name__ == "__main__":
    main()
