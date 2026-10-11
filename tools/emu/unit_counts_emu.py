"""Unicorn check of the player unit counters of Game.dll 1.26a (docs/spec/natives-pending.md section 1).

Runs the original functions that change the per-player counters and the typed count walk:
  0x6F403770 add completed        (wrapper 0x6F40FD30)
  0x6F4037B0 remove in progress   (wrapper 0x6F40FD70)
  0x6F403810 finish               (wrapper 0x6F40FDB0)
  0x6F403860 remove completed     (wrapper 0x6F40FDF0)
  0x6F403890 begin in progress    (wrapper 0x6F40FE30)
  0x6F4038D0 typed count walk     (used by GetPlayerTypedUnitCount)
against a Python model of the same rules, after random operation sequences.

Stubbed (not part of the counter logic, read separately): the type record lookup 0x6F4036B0 (hash table
and allocator), the type child lists 0x6F32D440/DBA0/D7E0/DB40, the stack helper 0x6F4C3F80 (writes a local
buffer), and the security cookie check 0x6F7E1059. The stubs answer from the Python model.

Usage: GAME_DLL=/path/Game.dll python3 -I tools/emu/unit_counts_emu.py [--vectors OUT.tsv] [--ops N] [--seed S]
Writes a vector file (one case per line) for the Rust test crates/sim/tests/unit_counts.rs when --vectors is given.
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402  (maps Game.dll and the stack; see emu.py)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
u32 = emu.u32

F_LOOKUP = 0x6F4036B0
F_ADD_COMPLETED = 0x6F403770
F_REMOVE_INPROG = 0x6F4037B0
F_FINISH = 0x6F403810
F_REMOVE_COMPLETED = 0x6F403860
F_BEGIN_INPROG = 0x6F403890
F_WALK = 0x6F4038D0
F_LIST1_COUNT = 0x6F32D440
F_LIST1_GET = 0x6F32DBA0
F_LIST2_COUNT = 0x6F32D7E0
F_LIST2_GET = 0x6F32DB40
F_STACK_HELPER = 0x6F4C3F80
F_COOKIE = 0x6F7E1059

# Model state, read by the stubs. Types are small indices here (the lookup key).
MODEL = {"records": {}, "children": {}, "upgrades": {}, "rec_mem": {}}
REC_BASE = emu.DATA + 0x2000
REC_SIZE = 0x40
COUNTER = emu.DATA + 0x1000  # the player counter object (player+0x2D0 target)


def rd(addr):
    return struct.unpack("<i", bytes(mu.mem_read(addr, 4)))[0]


def wr(addr, v):
    mu.mem_write(addr, struct.pack("<I", u32(v)))


def ret_plain(uc, ret_val, pops_args=0):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
    uc.reg_write(UC_X86_REG_EAX, u32(ret_val))
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops_args)
    uc.reg_write(UC_X86_REG_EIP, ret)


def stack_arg(uc, n):
    esp = uc.reg_read(UC_X86_REG_ESP)
    return struct.unpack("<I", bytes(uc.mem_read(esp + 4 + 4 * n, 4)))[0]


def record_for(t, create):
    if t not in MODEL["rec_mem"]:
        if not create:
            return 0
        addr = REC_BASE + len(MODEL["rec_mem"]) * REC_SIZE
        MODEL["rec_mem"][t] = addr
        for off in range(0, REC_SIZE, 4):
            wr(addr + off, 0)
    return MODEL["rec_mem"][t]


def hook(uc, address, size, user):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    edx = uc.reg_read(UC_X86_REG_EDX)
    if address == F_LOOKUP:
        t = stack_arg(uc, 0)
        create = stack_arg(uc, 1)
        rec = record_for(t, create)
        uc.reg_write(UC_X86_REG_EAX, rec)
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + 8)  # ret 8
        uc.reg_write(UC_X86_REG_EIP, ret)
    elif address == F_LIST1_COUNT:
        ret_plain(uc, len(MODEL["children"].get(ecx, [])))
    elif address == F_LIST1_GET:
        lst = MODEL["children"].get(ecx, [])
        ret_plain(uc, lst[min(edx, len(lst) - 1)] if lst else 0)
    elif address == F_LIST2_COUNT:
        ret_plain(uc, len(MODEL["upgrades"].get(ecx, [])))
    elif address == F_LIST2_GET:
        lst = MODEL["upgrades"].get(ecx, [])
        idx = min(edx, len(lst) - 1) if lst else 0
        ret_plain(uc, lst[idx] if lst else 0, pops_args=4)
    elif address == F_STACK_HELPER:
        ret_plain(uc, 0, pops_args=4)
    elif address == F_COOKIE:
        # Plain return; EAX must keep the walk's result, so do not write it.
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
        uc.reg_write(UC_X86_REG_EIP, ret)


# --- Python model of the same rules -------------------------------------------------------------

class Rec:
    def __init__(self, cls):
        self.cls = cls  # 2, 4 or 8 (record +0x30)
        self.c1c = 0    # +0x1C completed with p3 = 0
        self.p20 = 0    # +0x20 in progress with p3 = 0
        self.p24 = 0    # +0x24 in progress with p3 != 0
        self.p28 = 0    # +0x28 total (completed and finished)


class Model:
    def __init__(self):
        self.recs = {}
        self.s54 = 0
        self.s58 = 0
        self.s5c = 0
        self.s60 = 0

    def rec(self, t, cls):
        if t not in self.recs:
            self.recs[t] = Rec(cls)
        return self.recs[t]

    def add_completed(self, t, p3):
        r = self.rec(t, self.cls_of(t))
        r.p28 += 1
        if not p3:
            r.c1c += 1
        if r.cls & 4:
            self.s58 += 1
        elif r.cls & 8:
            self.s60 += 1

    def remove_inprog(self, t, p3):
        r = self.rec(t, self.cls_of(t))
        if not p3:
            r.p20 -= 1
        else:
            r.p24 -= 1
        if r.cls & 4:
            self.s54 -= 1
        elif r.cls & 8:
            self.s5c -= 1

    def finish(self, t, p3):
        r = self.rec(t, self.cls_of(t))
        r.p28 += 1
        if not p3:
            r.p20 -= 1
            r.c1c += 1
        else:
            r.p24 -= 1
        if r.cls & 4:
            self.s54 -= 1
            self.s58 += 1
        elif r.cls & 8:
            self.s5c -= 1
            self.s60 += 1

    def remove_completed(self, t):
        r = self.rec(t, self.cls_of(t))
        r.p28 -= 1
        if r.cls & 4:
            self.s58 -= 1
        elif r.cls & 8:
            self.s60 -= 1

    def begin_inprog(self, t, p3):
        r = self.rec(t, self.cls_of(t))
        if not p3:
            r.p20 += 1
        else:
            r.p24 += 1
        if r.cls & 4:
            self.s54 += 1
        elif r.cls & 8:
            self.s5c += 1

    def walk(self, t, flags):
        total = 0
        r = self.recs.get(t)
        if r is not None:
            if flags & 1:
                total = r.c1c
            elif flags & 0x10:
                total = r.p28
            if flags & 2:
                total += r.p20
            if flags & 4:
                total += r.p24
        if not flags & 0x20:
            for c in MODEL["children"].get(t, []):
                total += self.walk(c, flags)
        if flags & 8:
            for c in MODEL["upgrades"].get(t, []):
                total += self.walk(c, flags | 2)
        return u32(total)

    def cls_of(self, t):
        return MODEL["class"].get(t, 8)


# --- Unicorn side --------------------------------------------------------------------------------

def sync_counter_from_model(m):
    wr(COUNTER + 0x54, m.s54)
    wr(COUNTER + 0x58, m.s58)
    wr(COUNTER + 0x5C, m.s5c)
    wr(COUNTER + 0x60, m.s60)


def read_counter():
    return (rd(COUNTER + 0x54), rd(COUNTER + 0x58), rd(COUNTER + 0x5C), rd(COUNTER + 0x60))


def run_op(op, t, p3):
    if op == "add":
        emu.call(F_ADD_COMPLETED, ecx=COUNTER, stack_args=(t, p3))
    elif op == "remove_inprog":
        emu.call(F_REMOVE_INPROG, ecx=COUNTER, stack_args=(t, p3))
    elif op == "finish":
        emu.call(F_FINISH, ecx=COUNTER, stack_args=(t, p3))
    elif op == "remove_completed":
        emu.call(F_REMOVE_COMPLETED, ecx=COUNTER, stack_args=(t,))
    elif op == "begin":
        emu.call(F_BEGIN_INPROG, ecx=COUNTER, stack_args=(t, p3))


def run_walk(t, flags):
    return emu.call(F_WALK, ecx=COUNTER, stack_args=(t, flags))


def main():
    args = sys.argv[1:]
    n_ops = int(args[args.index("--ops") + 1]) if "--ops" in args else 400
    seed = int(args[args.index("--seed") + 1]) if "--seed" in args else 1
    vec_path = args[args.index("--vectors") + 1] if "--vectors" in args else None
    rnd = random.Random(seed)

    # Types 1..6 with classes: 1,2 units (8), 3 structure (4), 4 class 2, 5 unit, 6 structure.
    MODEL["class"] = {1: 8, 2: 8, 3: 4, 4: 2, 5: 8, 6: 4}
    # Acyclic on purpose: the walk recurses through both lists without a visited set, as the original does.
    MODEL["children"] = {1: [2, 3], 2: [5], 3: [], 5: [], 6: []}
    MODEL["upgrades"] = {1: [5], 3: [6], 6: [2], 4: [1]}
    for addr in (F_LOOKUP, F_LIST1_COUNT, F_LIST1_GET, F_LIST2_COUNT, F_LIST2_GET, F_STACK_HELPER, F_COOKIE):
        mu.hook_add(UC_HOOK_CODE, hook, None, addr, addr)

    m = Model()
    lines = []
    mism = 0
    walks = 0
    ops_done = 0
    # Preset record classes through the lookup path: the original sets +0x30 when a record is created.
    for t in MODEL["class"]:
        rec = record_for(t, True)
        wr(rec + 0x30, MODEL["class"][t])
        m.rec(t, MODEL["class"][t])
    # Start from a zero counter.
    sync_counter_from_model(m)
    ops_names = ["add", "remove_inprog", "finish", "remove_completed", "begin"]
    for _ in range(n_ops):
        op = rnd.choice(ops_names)
        t = rnd.choice(list(MODEL["class"].keys()))
        p3 = rnd.choice([0, 1])
        if op == "remove_completed":
            p3 = 0
        # Model first, then the original; both must agree on every counter afterwards.
        getattr(m, {"add": "add_completed", "remove_inprog": "remove_inprog", "finish": "finish",
                    "remove_completed": "remove_completed", "begin": "begin_inprog"}[op])(
            *([t] if op == "remove_completed" else [t, p3]))
        run_op(op, t, p3)
        ops_done += 1
        got = read_counter()
        exp = (m.s54, m.s58, m.s5c, m.s60)
        if got != exp:
            mism += 1
            print("COUNTER MISMATCH after", op, t, p3, "got", got, "expected", exp)
        for tt, r in m.recs.items():
            rec = MODEL["rec_mem"][tt]
            g = (rd(rec + 0x1C), rd(rec + 0x20), rd(rec + 0x24), rd(rec + 0x28))
            e = (r.c1c, r.p20, r.p24, r.p28)
            if g != e:
                mism += 1
                print("RECORD MISMATCH type", tt, "got", g, "expected", e)
        # The operation line goes before the walks of this step: the walks read the state after it.
        lines.append("C %s %d %d %d %d %d %d" % (op, t, p3, m.s54, m.s58, m.s5c, m.s60))
        # Typed walk on a random query after each op.
        for _ in range(2):
            qt = rnd.choice(list(MODEL["class"].keys()))
            flags = rnd.choice([0x10, 0x16, 0x18, 0x1E, 0x10 | 0x20, 0x16 | 0x20, 0x1A])
            g = run_walk(qt, flags)
            e = m.walk(qt, flags)
            walks += 1
            lines.append("W %d %d %d" % (qt, flags, e))
            if g != e:
                mism += 1
                print("WALK MISMATCH type", qt, "flags", hex(flags), "got", g, "expected", e)

    print("ops", ops_done, "walks", walks, "mismatches", mism)
    if vec_path:
        with open(vec_path, "w") as f:
            f.write("# Generated by tools/emu/unit_counts_emu.py (Game.dll 1.26a, Unicorn). Do not edit.\n")
            f.write("# Model: types 1..6 with class/children/upgrades as in the script.\n")
            f.write("# C op type flag totals(0x54 0x58 0x5C 0x60); W type walk-flags expected (after the C line above).\n")
            f.write("\n".join(lines) + "\n")
    sys.exit(1 if mism else 0)


if __name__ == "__main__":
    main()
