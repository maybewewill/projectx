"""Unicorn probe of the attack timing (Game.dll 1.26a): the speed multiplier, the cycle and damage-point delays, the
swing-timer scheduling, the backswing timer and the attack-speed bonus rescaling.

Real code that runs (the attack object `A` is built in emulator memory):
  0x6F0C6EB0  speed multiplier M(flag, weapon)            (reads A+0x1B0, A+0x1BC/0x1C0, A+0x218+4w)
  0x6F0C8D70  cycle delay  = cooldown-value / M(1)        (weapon cooldown property at A+0x158+8w)
  0x6F0CB9E0  schedule(delay, flag): timer 0xD01B0 (A+0x1D8) and execute timer 0xD01B1 (A+0x1EC)
  0x6F0C8DA0  backswing timer 0xD01B2 (A+0x200) from the backswing property at A+0x190+16w
  0x6F0C9090  attack speed bonus: updates the multiplier properties and re-arms the three timers in proportion
plus the soft-float library and the compare helpers 0x6F0223C0, 0x6F07C350, 0x6F089050, 0x6F01FED0.
Stubbed: the timer start 0x6F4778F0 and stop 0x6F477D30 (the calls are recorded), the property setter (vtable slot 0 of
A+0x1AC / A+0x1BC writes the value dword), the timer remaining-time getter (vtable slot 6), 0x6F0C9EC0 (stance timer).

Usage: GAME_DLL=... py tools/emu/attack_timing_emu.py [cases] [--rs out_dir]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_ECX  # noqa: E402

mu = emu.mu
EXTRA = 0x00400000
mu.mem_map(EXTRA, 0x10000)
ATK = EXTRA + 0x1000
VT_TIMER, VT_PROP, CODE = EXTRA + 0x100, EXTRA + 0x200, EXTRA + 0x300
SCRATCH = EXTRA + 0x800
START, STOP, STANCE = 0x6F4778F0, 0x6F477D30, 0x6F0C9EC0
F_MULT, F_CYCLE, F_SCHED, F_BACK, F_BONUS = 0x6F0C6EB0, 0x6F0C8D70, 0x6F0CB9E0, 0x6F0C8DA0, 0x6F0C9090
T_CD, T_EXEC, T_BSW = 0x1D8, 0x1EC, 0x200
EVENTS = []


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def fb(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def bf(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def ret_to(pop):
    esp = mu.reg_read(UC_X86_REG_ESP)
    mu.reg_write(UC_X86_REG_EIP, r32(esp))
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + pop)


def hook(uc, eip, size, user):
    if eip == START:
        esp = mu.reg_read(UC_X86_REG_ESP)
        obj = mu.reg_read(UC_X86_REG_ECX) - ATK
        EVENTS.append((obj, r32(esp + 8), r32(r32(esp + 4))))
        ret_to(0x14)
    elif eip == STOP:
        EVENTS.append((mu.reg_read(UC_X86_REG_ECX) - ATK, 0xFFFFFFFF, 0))
        ret_to(0)
    elif eip == STANCE:
        ret_to(0)


mu.hook_add(UC_HOOK_CODE, hook)
# stubs: timer getter (writes the remaining time [obj+4] to *out, returns out) and property setter (value -> obj+4)
mu.mem_write(CODE, b"\x8B\x44\x24\x04\x8B\x49\x04\x89\x08\xC2\x04\x00")  # mov eax,[esp+4]; mov ecx,[ecx+4]; mov [eax],ecx; ret 4
mu.mem_write(CODE + 0x20, b"\x8B\x44\x24\x04\x8B\x00\x89\x41\x04\xC2\x08\x00")  # mov eax,[esp+4]; mov eax,[eax]; mov [ecx+4],eax; ret 8
w32(VT_TIMER + 0x18, CODE)
w32(VT_PROP + 0x00, CODE + 0x20)


def setup(c):
    mu.mem_write(ATK, b"\0" * 0x400)
    for t in (T_CD, T_EXEC, T_BSW):
        w32(ATK + t, VT_TIMER)
    w32(ATK + 0x1AC, VT_PROP)
    w32(ATK + 0x1BC, VT_PROP)
    w32(ATK + 0x1B0, c.get("b", 0))
    w32(ATK + 0x1C0, c.get("p", 0))
    w32(ATK + 0x2B8, c.get("w", 0))
    for w in (0, 1):
        w32(ATK + 0x218 + 4 * w, c.get("mode", 0))
        w32(ATK + 0x158 + 8 * w, c.get("x1", 0))
        w32(ATK + 0x16C + 16 * w, c.get("x2", 0))
        w32(ATK + 0x190 + 16 * w, c.get("x3", 0))
    w32(ATK + T_CD + 4, c.get("r_cd", 0))
    w32(ATK + T_EXEC + 4, c.get("r_ex", 0))
    w32(ATK + T_BSW + 4, c.get("r_bs", 0))
    del EVENTS[:]


def call(fn, stack=(), ecx=ATK):
    return emu.call(fn, ecx=ecx, stack_args=stack)


def run(kind, c):
    setup(c)
    if kind == "mult":
        call(F_MULT, (SCRATCH, c["flag"], c["w"]))
        return r32(SCRATCH), []
    if kind == "cycle":
        call(F_CYCLE, (SCRATCH, c["w"]))
        return r32(SCRATCH), []
    if kind == "sched":
        w32(SCRATCH, c["delay"])
        call(F_SCHED, (SCRATCH, c["flag"]))
        return 0, list(EVENTS)
    if kind == "back":
        call(F_BACK)
        return 0, list(EVENTS)
    if kind == "bonus":
        w32(SCRATCH, c["delta"])
        call(F_BONUS, (SCRATCH, c["flag"]))
        return (r32(ATK + 0x1B0), r32(ATK + 0x1C0)), list(EVENTS)
    raise ValueError(kind)


def pick_f(rng, specials, lo, hi):
    return fb(rng.choice(specials)) if rng.random() < 0.5 else fb(rng.uniform(lo, hi))


def gen(rng, kind):
    c = {}
    c["b"] = pick_f(rng, [1.0, 0.5, 1.5, 2.0, 0.0, 0.2, 5.0, 6.0, 1.0004], -0.5, 7.0)
    c["p"] = pick_f(rng, [0.0, -0.3, -0.0005, -0.9, 0.2, -0.001, -0.0010000001], -1.5, 1.0)
    c["mode"] = rng.choice([0, 1, 2, 3, 0x40, 0x80, 0x100, 4, 0x20])
    c["w"] = rng.choice([0, 1, 0, 1, 0xFFFFFFFF] if kind == "mult" else [0, 1])
    c["flag"] = rng.choice([0, 1])
    c["x1"] = pick_f(rng, [1.0, 1.35, 0.0, 2.25, 0.01, 3.0], 0.0, 4.0)
    c["x2"] = pick_f(rng, [0.3, 0.5, 0.0, 1.0, 0.01, 0.667], 0.0, 2.0)
    c["x3"] = pick_f(rng, [0.3, 0.5, 0.0, 0.01, 0.667, 1.0], 0.0, 2.0)
    c["delay"] = pick_f(rng, [1.0, 1.35, 0.0, 0.5, 0.01, 0.009, -1.0, 2.0], -0.5, 4.0)
    c["r_cd"] = pick_f(rng, [0.0, 0.5, 1.0, 0.005, 0.1, 0.3], -0.2, 3.0)
    c["r_ex"] = pick_f(rng, [0.0, 0.3, 0.1, 0.0000001], 0.0, 1.0)
    c["r_bs"] = pick_f(rng, [0.0, 0.3, 0.1, 0.0000001], 0.0, 1.0)
    c["delta"] = pick_f(rng, [0.0, 0.25, -0.25, 0.5, 1.0, -0.3, 0.0005, -0.0005, 4.0, -0.8, 0.0000001], -2.0, 2.0)
    return c


KINDS = ("mult", "cycle", "sched", "back", "bonus")


def row(kind, c, out, ev):
    """The vector row of one case: the inputs, the output(s), then the event list padded to 3 events."""
    if kind == "mult":
        head = [c["b"], c["p"], c["mode"], c["w"], c["flag"]]
        res = [out]
    elif kind == "cycle":
        head = [c["b"], c["p"], c["mode"], c["w"], c["x1"]]
        res = [out]
    elif kind == "sched":
        head = [c["b"], c["p"], c["mode"], c["w"], c["x2"], c["delay"], c["flag"], c["r_cd"]]
        res = []
    elif kind == "back":
        head = [c["b"], c["p"], c["mode"], c["w"], c["x3"], c["r_cd"]]
        res = []
    else:
        head = [c["b"], c["p"], c["delta"], c["flag"], c["r_cd"], c["r_ex"], c["r_bs"]]
        res = list(out)
    evw = [len(ev)]
    for e in ev:
        evw += list(e)
    evw += [0] * (10 - len(evw))
    return head + res + evw


def main():
    n = 500
    out_dir = None
    args = sys.argv[1:]
    if args and args[0].isdigit():
        n = int(args.pop(0))
    if args and args[0] == "--rs":
        out_dir = args[1]
    rng = random.Random(20261012)
    total = 0
    for kind in KINDS:
        rows = []
        for i in range(n):
            c = gen(rng, kind)
            out, ev = run(kind, c)
            rows.append(row(kind, c, out, ev))
        total += len(rows)
        print(kind, len(rows), "rows; events per row max", max(r[-10] for r in rows))
        if out_dir:
            width = len(rows[0])
            lines = [
                "// Generated by tools/emu/attack_timing_emu.py from the original Game.dll; do not edit.",
                "// Row = inputs, output(s), then the timer events: count, then (timer offset, message or 0xFFFFFFFF for stop, delay bits) x3.",
                f"pub const ROWS: &[[u32; {width}]] = &[",
            ]
            lines += ["    [" + ", ".join("0x%08X" % v for v in r) + "]," for r in rows]
            lines.append("];")
            with open(os.path.join(out_dir, f"attack_timing_{kind}.rs"), "w", newline="\n") as fh:
                fh.write("\n".join(lines) + "\n")
    print("total", total)


if __name__ == "__main__":
    main()
