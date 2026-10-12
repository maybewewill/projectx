"""Unicorn harness for the day / night switch of Game.dll 1.26a (docs/spec/agile.md section 9.6).

The real routine 0x6F429460 (called by the world's handler of message 0x80263 and at start-up) runs on a synthetic world object
`T`: it destroys its two threshold listeners (+0x78, +0x7C), reads the time of day (game-state float 2, the Float wrapper at
T + 8 + 16 * 2), reads Dawn and Dusk, creates the listeners that will wake it (message 0x80263) at the next switch, derives the
day flag and, when the flag (the integer at T + 0x80) changed, stores it and sends message 0x80264 (day begins) or 0x80265
(night begins) to the world and to every unit.

Replaced pieces (recorders): the Misc reader 0x6F009EE0 (Dawn / Dusk values), the integer cell 0x6F473170 / 0x6F473140 (the flag),
the world's self dispatch (vtable slot 4), the unit broadcast 0x6F2A3270, operator delete. The time of day itself is the real
property with the real modifier (rate DayHours * scale / DayLength) and the wrap listener (message 0x80262, handler: value := 0).

Usage:  GAME_DLL=/path/Game.dll py -I tools/emu/daynight_emu.py gen OUT.txt COUNT SEED
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import agile_engine_emu as A  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_ECX, UC_X86_REG_EAX  # noqa: E402
from agile_engine_emu import rd32, wr32, f2b, b2f, call, mu, TMP, BASE, VT_W_FLOAT, LOG  # noqa: E402

TICK = A.TICK
FS_SCRATCH = BASE + 0x1E0000
DISP = BASE + 0x2600           # the world's self dispatch (vtable slot 4)
ST = dict(counter=0, dawn=0, dusk=0, events=[], ticks=0)


def misc_hook(uc, address, size, user):
    """0x6F009EE0: thiscall(ecx = out), stack (name, flag), ret 8. Writes Dawn / Dusk."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    name = rd32(esp + 4)
    out = uc.reg_read(UC_X86_REG_ECX)
    if name == 0x6F940BF0:
        wr32(out, ST["dawn"])
    elif name == 0x6F940BF8:
        wr32(out, ST["dusk"])
    else:
        raise RuntimeError("misc key %#x" % name)
    uc.reg_write(UC_X86_REG_EAX, out)
    uc.reg_write(UC_X86_REG_ESP, esp + 12)
    uc.reg_write(UC_X86_REG_EIP, ret)


def counter_get_hook(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    uc.reg_write(UC_X86_REG_EAX, ST["counter"])
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def counter_set_hook(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    ST["counter"] = rd32(esp + 4)
    ST["events"].append(("C", ST["ticks"], rd32(A.Q + 0x40), rd32(A.Q + 0x44), ST["counter"]))
    uc.reg_write(UC_X86_REG_ESP, esp + 8)
    uc.reg_write(UC_X86_REG_EIP, ret)


def disp_hook(uc, address, size, user):
    """thiscall(world, message object) with one stack argument: records the id (+8)."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    ST["events"].append(("D", ST["ticks"], rd32(rd32(esp + 4) + 8)))
    uc.reg_write(UC_X86_REG_ESP, esp + 8)
    uc.reg_write(UC_X86_REG_EIP, ret)


def bcast_hook(uc, address, size, user):
    """0x6F2A3270(cb, msg, 4, 6, 0x1f) cdecl: the unit broadcast; records the message id."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    ST["events"].append(("B", ST["ticks"], rd32(rd32(esp + 8) + 8)))
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def delete_hook(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + 16)
    uc.reg_write(UC_X86_REG_EIP, ret)


_hooked = [False]


def install():
    if _hooked[0]:
        return
    mu.hook_add(UC_HOOK_CODE, misc_hook, begin=0x6F009EE0, end=0x6F009EE0)
    mu.hook_add(UC_HOOK_CODE, counter_get_hook, begin=0x6F473170, end=0x6F473170)
    mu.hook_add(UC_HOOK_CODE, counter_set_hook, begin=0x6F473140, end=0x6F473140)
    mu.hook_add(UC_HOOK_CODE, bcast_hook, begin=0x6F2A3270, end=0x6F2A3270)
    mu.hook_add(UC_HOOK_CODE, delete_hook, begin=0x6F6EB558, end=0x6F6EB558)
    mu.mem_write(DISP, b"\xC3")
    mu.hook_add(UC_HOOK_CODE, disp_hook, begin=DISP, end=DISP)
    _hooked[0] = True


def listener_state(slot):
    w = rd32(T[0] + slot)
    if not w:
        return "-"
    idx = rd32(w + 8)
    ent = rd32(A.HT_ARR + idx * 8 + 4) if idx < 4000 else 0
    if not ent or rd32(A.HT_ARR + idx * 8) != 0xFFFFFFFE:
        return "x"
    return "%d:%08x" % (rd32(ent + 0x54), rd32(ent + 0x50))


T = [0]


def run_case(d):
    """One scenario. d: length, hours, scale, start (floats), dawn, dusk, ticks."""
    A.install_hooks()
    install()
    mu.msr_write(0xC0000100, FS_SCRATCH)
    sc = A.Scn(0, 0, f2b(d["start"]))
    ST.update(counter=d.get("counter", 0), dawn=f2b(d["dawn"]), dusk=f2b(d["dusk"]), events=[], ticks=0)
    A.wr32(A.REC_VT + 0x10, DISP)
    t = A.make_owner()
    T[0] = t
    # the world's float state 2 wrapper at T + 0x28 refers to the time-of-day property
    wr32(t + 0x28, VT_W_FLOAT)
    wr32(t + 0x30, rd32(sc.prop + 0x14))
    wr32(t + 0x34, rd32(sc.prop + 0x18))
    wr32(t + 0x78, 0)
    wr32(t + 0x7C, 0)
    rate = A.sdiv(A.smul(f2b(d["scale"]), f2b(d["hours"])), f2b(d["length"]))
    sc.add_mod(rate, 0)
    sc.add_lis(1, f2b(d["hours"]), 0x80262)
    wrap_obj = [sc.lis_objs[0]]
    out = ["CASE %08x %08x %08x %08x %08x %08x %d %d" % (f2b(d["length"]), f2b(d["hours"]), f2b(d["scale"]), f2b(d["start"]),
                                                      f2b(d["dawn"]), f2b(d["dusk"]), d["counter"], d["ticks"])]
    pending = []

    def drain():
        """Handle the listener messages recorded so far (the world's handlers); runs the real routines."""
        saved = A.CHAIN["handler"]
        A.CHAIN["handler"] = None
        try:
            drain_inner()
        finally:
            A.CHAIN["handler"] = saved

    def drain_inner():
        while True:
            fresh = LOG[sc.nlog:]
            sc.nlog = len(LOG)
            for e in fresh:
                if e[0] == "create" and e[1] == A.VT_LIS:
                    sc.lis_objs.append(e[2])
            news = [e for e in fresh if e[0] == "sink"]
            if not news:
                break
            for e in news:
                msg = 0x80262 if e[4] == wrap_obj[0] else 0x80263
                if msg == 0x80262:
                    wr32(TMP + 0x10, f2b(0.0))      # the wrap handler 0x6F42FC9F: set value 0.0
                    call(0x6F477350, ecx=sc.pw, stack_args=(TMP + 0x10,))
                    ST["events"].append(("W", ST["ticks"], rd32(A.Q + 0x40), rd32(A.Q + 0x44)))
                elif msg == 0x80263:
                    ST["events"].append(("R", ST["ticks"], rd32(A.Q + 0x40), rd32(A.Q + 0x44)))
                    call(0x6F429460, ecx=t)
                    ST["events"].append(("L", ST["ticks"], listener_state(0x78), listener_state(0x7C)))
                else:
                    raise RuntimeError("message %#x" % msg)

    def handler():
        drain()

    # start-up: the first evaluation
    sc.nlog = len(LOG)
    call(0x6F429460, ecx=t)
    ST["events"].append(("L", 0, listener_state(0x78), listener_state(0x7C)))
    drain()
    A.CHAIN["handler"] = handler
    vals = []
    for i in range(d["ticks"]):
        ST["ticks"] = i
        sc.tick = i
        A.advance(TICK)
        if i % 5000 == 0 or i == d["ticks"] - 1:
            vals.append((i, sc.value()))
    A.CHAIN["handler"] = None
    for e in ST["events"]:
        if e[0] == "C":
            out.append("C %d %08x %d %d" % e[1:])
        elif e[0] == "D":
            out.append("D %d %x" % e[1:])
        elif e[0] == "B":
            out.append("B %d %x" % e[1:])
        elif e[0] == "W":
            out.append("W %d %08x %d" % e[1:])
        elif e[0] == "R":
            out.append("R %d %08x %d" % e[1:])
        elif e[0] == "L":
            out.append("L %d %s %s" % e[1:])
    for (i, v) in vals:
        out.append("V %d %08x" % (i, v))
    out.append("END")
    return out


CASES = [
    # length, hours, scale, start, dawn, dusk, counter
    (240.0, 24.0, 1.0, 0.0, 6.0, 18.0, 0),
    (240.0, 24.0, 1.0, 12.0, 6.0, 18.0, 0),
    (240.0, 24.0, 1.0, 12.0, 6.0, 18.0, 1),
    (240.0, 24.0, 1.0, 5.99, 6.0, 18.0, 0),
    (240.0, 24.0, 1.0, 17.99, 6.0, 18.0, 1),
    (240.0, 24.0, 1.0, 23.9, 6.0, 18.0, 0),
    (240.0, 24.0, 1.0, 6.0, 6.0, 18.0, 0),
    (240.0, 24.0, 1.0, 18.0, 6.0, 18.0, 1),
    (120.0, 24.0, 2.0, 5.5, 6.0, 18.0, 0),
    (60.0, 12.0, 0.5, 3.0, 3.0, 9.0, 0),
    (37.0, 9.0, 2.5, 0.0, 2.0, 7.0, 0),
    (240.0, 24.0, 0.1, 8.0, 6.0, 18.0, 0),
    (240.0, 24.0, 1.0, 0.0, 0.0, 24.0, 0),
    (240.0, 24.0, 1.0, 0.0, 24.0, 18.0, 0),
    (180.0, 24.0, 1.0, 11.0, 7.5, 19.5, 1),
    (480.0, 24.0, 1.0, 20.0, 6.0, 18.0, 0),
]


def gen(path, count, seed):
    rnd = random.Random(seed)
    lines = ["# Day / night switch scenarios (docs/spec/agile.md section 9.6): tools/emu/daynight_emu.py gen on the real routine 0x6F429460.",
             "# CASE length hours scale start dawn dusk counter ticks | C tick clock row flag (flag stored) | D tick msg (world dispatch) |",
             "# B tick msg (unit broadcast) | W tick clock row (wrap) | R tick clock row (0x80263 handled) | L tick listenerA listenerB (mode:threshold) | V tick value | END"]
    cases = []
    for c in CASES:
        cases.append(dict(length=c[0], hours=c[1], scale=c[2], start=c[3], dawn=c[4], dusk=c[5], counter=c[6]))
    while len(cases) < count:
        length = rnd.choice([240.0, 120.0, 60.0, 180.0, 90.0])
        hours = rnd.choice([24.0, 24.0, 12.0, 10.0])
        dawn = rnd.choice([6.0, 5.5, 4.0, rnd.uniform(1.0, hours / 2)])
        dusk = rnd.choice([18.0, 19.5, 20.0, rnd.uniform(hours / 2, hours - 0.5)])
        if dusk >= hours:
            dusk = hours - 1.0
        start = rnd.choice([0.0, dawn - 0.003, dawn, dawn + 0.01, dusk - 0.004, dusk, dusk + 0.02, rnd.uniform(0, hours)])
        counter = 1 if dawn <= start < dusk else 0
        if rnd.random() < 0.15:
            counter = 1 - counter
        cases.append(dict(length=length, hours=hours, scale=rnd.choice([1.0, 1.0, 2.0, 0.5, 0.1, 3.0]), start=start,
                          dawn=dawn, dusk=dusk, counter=counter))
    for n, d in enumerate(cases):
        # about two full cycles
        rate = d["scale"] * d["hours"] / d["length"]
        d["ticks"] = int(min(2.2 * d["hours"] / rate * 200.0, 60000))
        lines += run_case(d)
        if n % 5 == 0:
            print("case", n, "ticks", d["ticks"], flush=True)
    with open(path, "w", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote", path, len(cases))


def probe():
    d = dict(length=240.0, hours=24.0, scale=1.0, start=0.0, dawn=6.0, dusk=18.0, counter=0, ticks=20000)
    for line in run_case(d):
        print(line)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "probe"
    if cmd == "probe":
        probe()
    elif cmd == "gen":
        gen(sys.argv[2], int(sys.argv[3]), int(sys.argv[4]) if len(sys.argv) > 4 else 1)
