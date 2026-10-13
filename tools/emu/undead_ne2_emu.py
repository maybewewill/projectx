"""Unicorn checks for the Death Pact / Blink / Vampiric Aura classes (docs/spec/abilities-undead-ne-heroes.md, S28).

ORIGINAL code of the user's Game.dll is run for real; everything that needs the agent world is replaced by a
small stub that answers or records. Checks (each writes a vector file for the Rust test with --dump DIR):

  blink     `CAbilityBlink` point validation `0x6F1AEB10`: caster position, target point, DataA (maximum range)
            -> the point written back (projected onto the range circle when it is beyond it). The pathing
            query (unit vtable slot 0xDC) is the identity stub. File `blink.txt`:
            `cx cy tx ty dataA | ox oy ret` (hex float bits).
  deathpact `CAbilityDeathPact` effect `0x6F09FD50`: target life, DataA..DataE -> the amounts delivered to the
            caster (mana via `0x6F279140`, life via `0x6F277C20`) and the target's fate (killed / drained).
            File `deathpact.txt`: `life A B C D E | mana_in life_in killed life_after` (hex).
  vampiric  `CBuffAuraVampiric` damage handler `0x6F1AD980` (vtable slot 0x170): damage amount, DataA -> healing
            given to the attacker and the gates. File `vampiric.txt`.

Usage:  GAME_DLL=/path/Game.dll py tools/emu/undead_ne2_emu.py [--dump DIR] [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_ESI,
)

mu = emu.mu
DATA = emu.DATA
wr, rd = A.wr, A.rd
f2u, u2f = A.f2u, A.u2f

OBJ, UNIT, DEF, LVL = A.OBJ, A.UNIT, A.DEF, A.LVL
VT_BLINK = 0x6F91087C
VT_DEATHPACT = 0x6F8A3DE4
F_NORM = 0x6F4743A0
F_BLINK_POINT = 0x6F1AEB10
F_DEATHPACT = 0x6F09FD50

# scratch layout
UVT = DATA + 0x5000     # fake unit vtable (0x400 bytes)
AVT = DATA + 0x5400     # copy of the ability vtable (0x500 bytes)
POSX, POSY = DATA + 0xD100, DATA + 0xD104     # what the normaliser stub returns
OUT = DATA + 0xD200
TGT = DATA + 0x6000     # target unit (Death Pact)
TVT = DATA + 0x6400     # target unit vtable
REC = DATA + 0xD400     # call records


def code_stub(code: bytes) -> int:
    a = A._stub_next[0]
    mu.mem_write(a, code)
    A._stub_next[0] += 0x40
    return a


def install_common():
    A.setup_stubs()
    # normaliser: out pointer = [esp+4]; writes (POSX, POSY); ret 4
    n = code_stub(b"\x8B\x44\x24\x04" + b"\x8B\x0D" + struct.pack("<I", POSX) + b"\x89\x08"
                  + b"\x8B\x0D" + struct.pack("<I", POSY) + b"\x89\x48\x04" + b"\xC2\x04\x00")
    A.patch_jump(F_NORM, n)
    for fn in (0x6F3332F0, 0x6F2ABF20):
        A.patch_jump(fn, A.stub_ret(0, 0))
    A.patch_jump(0x6F194230, A.stub_ret(0, 0))
    A.patch_jump(0x6F040000, A.stub_ret(0, 0))
    A.patch_jump(0x6F469490, A.stub_ret(0, 0))     # playable-area clamp is checked elsewhere (unit_clamp_emu)


def f(v):
    return f2u(v)


def setup_ability(vt, datas):
    """Fake ability: vtable copy (so single slots can be stubbed), owner = UNIT, one level with `datas`."""
    A.make_ability(vt, 0x41456C62, 0)
    for i in range(0x500 // 4):
        wr(AVT + 4 * i, rd(vt + 4 * i))
    wr(OBJ, AVT)
    for i, v in enumerate(datas):
        wr(LVL + 0x20 + 4 * i, v)
    # unit vtable: slot 0xB8 = `mov eax, ecx; ret` (the position object is the unit itself), slot 0xDC = stub
    mu.mem_write(UNIT, b"\0" * 0x400)
    wr(UNIT, UVT)
    ident = code_stub(b"\x8B\xC1\xC3")
    wr(UVT + 0xB8, ident)


# ---------------------------------------------------------------------------------------------------------------
# Blink

def blink_case(r):
    def pos():
        return r.choice((0.0, r.uniform(-3000, 3000), r.uniform(-300, 300)))

    cx, cy, tx, ty = pos(), pos(), pos(), pos()
    rng = r.choice((r.uniform(100, 1200), r.uniform(1, 50), 1000.0, 0.0))
    return cx, cy, tx, ty, rng


def run_blink(cases, r):
    install_common()
    # unit vtable 0xDC (placement query): returns 1, v27 (= [ebp... esp+0x28 of the caller) stays 0 -> success
    ok = code_stub(b"\xB8\x01\x00\x00\x00\xC2\x2C\x00")
    rows = []
    ownerid = A.stub_ret(0, 0)
    for _ in range(cases):
        cx, cy, tx, ty, rng = blink_case(r)
        setup_ability(VT_BLINK, [f(rng)])
        wr(UVT + 0xEC, ownerid)
        wr(UVT + 0xDC, ok)
        wr(POSX, f(cx))
        wr(POSY, f(cy))
        wr(OUT, f(tx))
        wr(OUT + 4, f(ty))
        mu.reg_write(UC_X86_REG_ECX, OBJ)
        ret = emu.call(F_BLINK_POINT, ecx=OBJ, stack_args=(OUT, OUT + 4))
        rows.append((f(cx), f(cy), f(tx), f(ty), f(rng), rd(OUT), rd(OUT + 4), ret))
    return rows


# ---------------------------------------------------------------------------------------------------------------
# Hook helper: replaces a function (thiscall/stdcall) by a Python handler.

def hook_fn(addr, handler, pop):
    """At `addr` call handler(uc, esp) -> eax; then return to the caller popping `pop` argument bytes."""
    def cb(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = rd(esp)
        eax = handler(uc, esp) or 0
        uc.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
        uc.reg_write(emu_eip, ret)
    return mu.hook_add(UC_HOOK_CODE, cb, begin=addr, end=addr)


from unicorn.x86_const import UC_X86_REG_EIP as emu_eip  # noqa: E402


def stub_addr() -> int:
    a = A._stub_next[0]
    mu.mem_write(a, b"\x90" * 8)
    A._stub_next[0] += 0x20
    return a


# ---------------------------------------------------------------------------------------------------------------
# Death Pact

TGT_LIFE = [0]
EVENTS = []


def run_deathpact(cases, r):
    install_common()
    rows = []
    # target vtable: 0x12C GetLife(out), 0x124 SetLife(&v) , 0x128 AddLife(out,&delta)
    s12c, s124, s128 = stub_addr(), stub_addr(), stub_addr()

    def get_life(uc, esp):
        wr(rd(esp + 4), TGT_LIFE[0])
        return 0

    def set_life(uc, esp):
        EVENTS.append(("setlife", rd(rd(esp + 4))))
        return 0

    def add_life_target(uc, esp):
        # ecx = target, out = [esp+4], delta ptr = [esp+8]; the real result is the applied change; the stub
        # applies exactly the delta (no clamping) and reports it
        delta = rd(rd(esp + 8))
        EVENTS.append(("tgtlife", delta))
        wr(rd(esp + 4), delta)
        return 0

    for adr, fnc, pop in ((s12c, get_life, 4), (s124, set_life, 4), (s128, add_life_target, 8)):
        hook_fn(adr, fnc, pop)
    mu.mem_write(TVT, b"\0" * 0x400)
    wr(TVT + 0x12C, s12c)
    wr(TVT + 0x124, s124)
    wr(TVT + 0x128, s128)
    # helpers
    hook_fn(0x6F4773A0, lambda uc, esp: 0, 4)
    hook_fn(0x6F2829D0, lambda uc, esp: EVENTS.append(("credit", rd(esp + 4))) or 0, 4)
    hook_fn(0x6F279140, lambda uc, esp: EVENTS.append(("mana", rd(rd(esp + 8)))) or 0, 8)
    hook_fn(0x6F277C20, lambda uc, esp: EVENTS.append(("life", rd(rd(esp + 8)))) or 0, 8)

    def pick():
        return r.choice((0.0, 1.0, 0.5, r.uniform(0.01, 3), r.uniform(0.01, 3), r.uniform(1, 500)))

    for _ in range(cases):
        life = r.choice((r.uniform(1, 2000), r.uniform(0.01, 1), 1.0, 0.0, 450.0))
        d = [pick(), pick(), r.choice((0.0, 1.0, 0.0)), r.choice((0.0, 1.0, 0.0)), r.choice((0.0, 0.0, 1.0, 2.0))]
        flag = r.choice((0, 0, 0x40000000))
        setup_ability(VT_DEATHPACT, [f(x) for x in d])
        mu.mem_write(TGT, b"\0" * 0x400)
        wr(TGT, TVT)
        wr(TGT + 0x5C, flag | 0x200)
        TGT_LIFE[0] = f(life)
        EVENTS.clear()
        emu.call(F_DEATHPACT, ecx=OBJ, stack_args=(TGT,))
        tflags = rd(TGT + 0x5C)
        rows.append((f(life), [f(x) for x in d], flag, tflags, list(EVENTS)))
    return rows


def main_deathpact(cases, r, dump):
    rows = run_deathpact(cases, r)
    for row in rows[:10]:
        life, d, flag, tflags, ev = row
        print("deathpact life=%g d=%s flag=%x -> flags=%x %s" % (
            u2f(life), [round(u2f(x), 3) for x in d], flag, tflags, [(k, round(u2f(v), 3) if k != "credit" else v) for k, v in ev]))
    if dump:
        with open(os.path.join(dump, "deathpact.txt"), "w") as fh:
            for life, d, flag, tflags, ev in rows:
                fh.write("%08x %s %08x %08x | %d %s\n" % (
                    life, " ".join("%08x" % x for x in d), flag, tflags, len(ev),
                    " ".join("%s:%08x" % (k, v if k != "credit" else 1) for k, v in ev)))


# ---------------------------------------------------------------------------------------------------------------
# Vampiric Aura damage handler

F_VAMP = 0x6F1AD980
BUFF = DATA + 0x7000
HOLDER = DATA + 0x7800
RECO = DATA + 0x7900
RVT = DATA + 0x7A00
AMOUNT = DATA + 0x7B00
V = {}


def run_vampiric(cases, r):
    install_common()
    calls = []
    hook_fn(0x6F038450, lambda uc, esp: wr(uc.reg_read(UC_X86_REG_ECX), TGT) or 0, 4)
    hook_fn(0x6F285B20, lambda uc, esp: V["enemy"], 4)
    hook_fn(0x6F28D750, lambda uc, esp: V["blocked"], 8)
    hook_fn(0x6F009F90, lambda uc, esp: V["misc"], 4)
    hook_fn(0x6F277C20, lambda uc, esp: calls.append(("heal", rd(rd(esp + 8)))) or 0, 8)
    hook_fn(0x6F053040, lambda uc, esp: calls.append(("effect", 1)) or 0, 4)
    inv = stub_addr()
    hook_fn(inv, lambda uc, esp: V["invuln"], 0)
    ownerid = stub_addr()
    hook_fn(ownerid, lambda uc, esp: 3, 0)
    mu.mem_write(TVT, b"\0" * 0x400)
    wr(TVT + 0x11C, inv)
    mu.mem_write(RVT, b"\0" * 0x400)
    wr(RVT + 0xEC, ownerid)
    rows = []

    def amount():
        return r.choice((0.0, -5.0, r.uniform(0.001, 300), r.uniform(0.5, 60), 1.0, 75.0, float("nan")))

    for _ in range(cases):
        V["enemy"] = r.choice((0, 1, 1, 1))
        V["blocked"] = r.choice((0, 0, 0, 1))
        V["misc"] = r.choice((0, 1))
        V["invuln"] = r.choice((0, 0, 0, 1))
        holder_flag = r.choice((0, 0, 0, 1))
        victim_class = r.choice((0, 0, 8, 2, 0x800))
        a_flags = r.choice((0, 0, 0x40000000))
        v_flags = r.choice((0, 0, 0x40000000))
        amt = amount()
        frac = r.choice((0.15, 0.3, 0.45, 0.6, r.uniform(0.0, 1.0), 1.0, 0.0))
        mu.mem_write(UNIT, b"\0" * 0x400)
        wr(UNIT, UVT)
        wr(UNIT + 0x5C, a_flags)
        mu.mem_write(BUFF, b"\0" * 0x400)
        wr(BUFF + 0x30, UNIT)
        wr(BUFF + 0xD0, f(frac))
        mu.mem_write(TGT, b"\0" * 0x400)
        wr(TGT, TVT)
        wr(TGT + 4, 5)
        wr(TGT + 0x248, victim_class)
        wr(TGT + 0x5C, v_flags)
        mu.mem_write(HOLDER, b"\0" * 0x40)
        wr(HOLDER + 0xC, holder_flag)
        mu.mem_write(RECO, b"\0" * 0x40)
        wr(RECO, RVT)
        wr(AMOUNT, f(amt))
        calls.clear()
        emu.call(F_VAMP, ecx=BUFF, stack_args=(RECO, HOLDER, AMOUNT))
        rows.append((dict(V), holder_flag, victim_class, a_flags, v_flags, f(amt), f(frac), list(calls)))
    return rows


def main_vampiric(cases, r, dump):
    rows = run_vampiric(cases, r)
    for row in rows[:8]:
        v, hf, vc, af, vf, amt, frac, calls = row
        print("vamp", v, hf, hex(vc), hex(af), hex(vf), u2f(amt), u2f(frac), [(k, u2f(x) if k == "heal" else x) for k, x in calls])
    if dump:
        with open(os.path.join(dump, "vampiric.txt"), "w") as fh:
            for v, hf, vc, af, vf, amt, frac, calls in rows:
                fh.write("%d %d %d %d %d %x %x %x %x %08x %08x | %s\n" % (
                    v["enemy"], v["blocked"], v["misc"], v["invuln"], hf, vc, af, vf, 0, amt, frac,
                    " ".join("%s:%08x" % (k, x) for k, x in calls) or "none"))


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump = args[1]
        args = args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 5
    r = random.Random(seed)
    if dump:
        os.makedirs(dump, exist_ok=True)
    which = os.environ.get("CHECK", "all")
    if which in ("all", "deathpact"):
        main_deathpact(cases, random.Random(seed + 1), dump)
        if which == "deathpact":
            return
    if which in ("all", "vampiric"):
        main_vampiric(cases, random.Random(seed + 2), dump)
        if which == "vampiric":
            return
    rows = run_blink(cases, r)
    for row in rows[:12]:
        cx, cy, tx, ty, rng, ox, oy, ret = row
        print("blink c=(%g,%g) t=(%g,%g) R=%g -> (%g,%g) ret=%d" % (
            u2f(cx), u2f(cy), u2f(tx), u2f(ty), u2f(rng), u2f(ox), u2f(oy), ret))
    if dump:
        os.makedirs(dump, exist_ok=True)
        with open(os.path.join(dump, "blink.txt"), "w") as fh:
            for row in rows:
                fh.write(" ".join("%08x" % v for v in row) + "\n")


if __name__ == "__main__":
    main()
