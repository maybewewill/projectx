"""V7 Unicorn run of the Locust Swarm effect (AUls, vtable 0x6F8DDA84) of Game.dll 1.26a.

ORIGINAL `0x6F134EA0` (effect slot 0x428) and the spawn step `0x6F12CE70` run on a fake ability with a real level record (real getters:
Dur +8, Area +0x18, DataA..E +0x20..0x30, UnitID +0x44 through `0x6F02EDD0`). Stubs (recorded): the clean-up of an earlier swarm
`0x6F12F1B0`, the unit creator `0x6F29F990` (arguments and the registers ECX / EDX are recorded), the lookup `0x6F0787D0` (answers with a
fake locust-ability object or nothing), the locust-ability set-up `0x6F124EF0` (five arguments recorded), the timer start `0x6F4778F0`
(value, message, owner recorded).

Row (hex words):
  <DataA..E> <Dur> <+0x9C> <Area> <UnitID> <lookup-found> | <count at +0x308> <timer1: value msg> <create1: ecx edx a1.. a6 count-after>
  <timer2 after the 2nd spawn: value msg or ->  <setup args: owner? area datac-int datad datae>
Usage: GAME_DLL=/path/Game.dll py tools/emu/v7_locust_emu.py [--dump OUT_FILE] [CASES] [SEED]
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
ABIL, UNIT, NEWU, LOC = H.ABIL, H.UNIT, M + 0x6200, M + 0x6300
cap = {}
STUB = {}


def on_create(esp, ecx, edx):
    cap.setdefault("create", []).append((ecx, edx, [rd(esp + 4 + 4 * i) for i in range(6)], [rd(rd(esp + 4)), rd(rd(esp + 8)), rd(rd(esp + 12))]))
    return NEWU


on_create.pop = 0x18


def on_lookup(esp, ecx, edx):
    return LOC if cap["found"] else 0


on_lookup.pop = 0x14


def on_setup(esp, ecx, edx):
    a = [rd(esp + 4 + 4 * i) for i in range(5)]
    cap["setup"] = (a[0] == UNIT, rd(a[1]), a[2], rd(a[3]), rd(a[4]))
    return 0


on_setup.pop = 0x14


def on_timer(esp, ecx, edx):
    cap.setdefault("timers", []).append((rd(rd(esp + 4)), rd(esp + 8)))
    return 0


on_timer.pop = 0x14


def on_pos(esp, ecx, edx):
    p = rd(esp + 4)
    wr(p, cap["pos"][0])
    wr(p + 4, cap["pos"][1])
    wr(p + 8, cap["pos"][2])
    return p


on_pos.pop = 4


def install():
    S.install()
    A.patch_seh()
    H.STUBS.update({0: A.stub_ret(4, 0), 1: A.stub_ret(4, 1), "g": A.stub_ret(0, 7), "s": A.stub_ret(8, 0x1234), "f": A.stub_ret(0, 0)})
    A.patch_jump(0x6F472890, A.stub_ret(0, UNIT))
    A.patch_jump(0x6F12F1B0, A.stub_ret(4, 0))
    A.patch_jump(0x6F118750, A.stub_ret(0, 0))
    A.patch_jump(0x6F0419C0, A.stub_ret(4, 0))
    A.patch_jump(0x6F2AB3E0, A.stub_ret(8, 0))
    A.patch_jump(0x6F2AB2D0, A.stub_ret(8, 0))
    H.hook(0x6F29F990, on_create)
    H.hook(0x6F0787D0, on_lookup)
    H.hook(0x6F124EF0, on_setup)
    H.hook(0x6F4778F0, on_timer)
    H.hook(0x6F4743A0, on_pos)
    mu.mem_write(H.UVT, b"\0" * 0x400)
    wr(H.UVT + 184, A.stub_ret(0, 0))
    wr(H.UVT + 236, A.stub_ret(0, 5))


def run(r):
    data = [S.rbits(r) for _ in range(5)]
    # DataA is a count: mostly sane values, some edge ones
    data[0] = f2u(r.choice([0.0, -3.0, 0.5, 1.0, 2.0, 7.0, 20.0, 39.9, 40.0, 41.0, 100.0, r.uniform(0, 60)]))
    data[1] = f2u(r.choice([0.2, 0.0, 1.0, r.uniform(0, 3)]))
    dur = S.rbits(r)
    plus = r.choice([0, 0, f2u(r.uniform(0, 5))])
    area = S.rbits(r)
    unit_id = r.choice([0x756C6F63, 0x75303031])
    cap.clear()
    cap["found"] = r.random() < 0.7
    cap["pos"] = (S.rbits(r), S.rbits(r), S.rbits(r))
    H.fresh(0x6F8DDA84, 0, 0, data, dur, area)
    wr(H.ABIL + 0x9C, plus)
    wr(H.LVL + 0x44, unit_id)
    mu.mem_write(NEWU, b"\0" * 0x40)
    mu.mem_write(LOC, b"\0" * 0x40)
    mu.mem_write(H.ABIL + 0x300, b"\0" * 0x20)
    S.call(0x6F134EA0, ecx=H.ABIL)
    n = rd(H.ABIL + 0x308)
    created1 = len(cap.get("create", []))
    t1 = list(cap.get("timers", []))
    cap["timers"] = []
    S.call(0x6F12CE70, ecx=H.ABIL)
    t2 = list(cap.get("timers", []))
    c = cap["create"][0]
    setup = cap.get("setup")
    # run the remaining steps and count the reschedules
    resched = 0
    for _ in range(60):
        cap["timers"] = []
        before = rd(H.ABIL + 0x30C)
        S.call(0x6F12CE70, ecx=H.ABIL)
        if cap["timers"]:
            resched += 1
        if rd(H.ABIL + 0x30C) == before:
            break
    head = "LS %s %08x %08x %08x %08x %d" % (" ".join("%08x" % d for d in data), dur, plus, area, unit_id, cap["found"])
    ts = " ".join("%08x:%x" % t for t in t1) or "-"
    t2s = " ".join("%08x:%x" % t for t in t2) or "-"
    cs = "%08x %08x %s" % (c[0], c[1], " ".join("%08x" % v for v in c[2]))
    ss = "-" if setup is None else " ".join("%08x" % int(v) for v in setup)
    return "%s | %08x %d | %s | %s | %s | %s | %d" % (head, n, created1, ts, cs, t2s, ss, resched)


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 300
    seed = int(args[1]) if len(args) > 1 else 7
    r = random.Random(seed)
    install()
    rows = [run(r) for _ in range(cases)]
    for row in rows[:6]:
        print(row)
    if dump:
        with open(dump, "w") as fh:
            fh.write("\n".join(rows) + "\n")
    print("cases", len(rows))


if __name__ == "__main__":
    main()
