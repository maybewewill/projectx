"""V7 Unicorn runs of the attack-hook handlers of Flak Cannon (Aflk), Flaming Attack (Afak) and Death Damage (Adda) of Game.dll 1.26a.

ORIGINAL handlers run on a fake ability with a real level record (real level getters, see `v7_getters.py`: Cast +4, Dur +8, HeroDur
+0xC, Cool +0x14, Area +0x18, DataA +0x20 ... DataE +0x30):
  FL  Aflk slot 0x154 `0x6F177D00` (this, target, record): the area body `0x6F2C31D0` gets three {radius, amount} rings
      (Area, DataC), (DataA, DataD), (DataB, DataE), the class / row / flags copied from the attack record. Stub: the area body (records).
  AF  Afak slot 0x14C `0x6F09DE00` (this, mana-ish arg, record): writes the splash fields of the launch record: +0x80 Hfact <- DataB,
      +0x84 Qfact <- DataC, +0x88 Farea <- DataD, +0x8C Harea <- DataE, +0x90 Qarea <- Area (record layout of weapon-splash.md section 1).
  AD  Adda slot 0x284 `0x6F1E7200`: the delayed damage object `0x6F2C4D70` gets the delay Dur, the unit's cell position and the rings
      (DataA, DataB), (DataC, DataD), count 2, damage class 16 and the targs word.

Rows (hex words, `|` separates input and output):
  FL <pause> <enabled> <rec3> <rec5> <rec8> <targs> <A..E> <area> | <called> <x> <y> r0 a0 r1 a1 r2 a2 <count> <a5> <a6> <a7> <a8-low16> <src> <a10> <cb> <ctx0-is-this> <ctx1-is-target> <a13> <a14>
  AF <flags> <A..E> <area> | <ret> <flags after> +0x80 +0x84 +0x88 +0x8C +0x90
  AD <enabled> <flags> <unitflags> <unitbyte> <A..D> <dur> <targs> | <called> <delay> <x> <y> r0 a0 r1 a1 <count> <class> <a7> <a8> <targs> <src-is-unit> <a10> <a11>

Usage: GAME_DLL=/path/Game.dll py tools/emu/v7_hits_emu.py [--dump OUT_FILE] [CASES] [SEED]
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
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_EAX  # noqa: E402

mu = emu.mu
wr, rd = A.wr, A.rd
M = S.M
ABIL, DEF, LVL, UNIT, TGTU, REC = M + 0x4000, M + 0x4400, M + 0x4500, M + 0x5000, M + 0x5400, M + 0x5800
AVT = M + 0x6000
UVT = M + 0x6800
CAP = M + 0x7000

VT_FLAK, VT_FLAME, VT_DEATH = 0x6F8F3CFC, 0x6F8AE814, 0x6F917E04
F_FLAK, F_FLAME, F_DEATH = 0x6F177D00, 0x6F09DE00, 0x6F1E7200
F_AREA, F_DELAYED, F_NORM = 0x6F2C31D0, 0x6F2C4D70, 0x6F4743A0

cap = {}
STUBS = {}


def hook(addr, fn):
    """At `addr`: fn(esp, ecx, edx) -> eax is run, the call returns to its caller popping fn.pop bytes."""
    def cb(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = rd(esp)
        eax = fn(esp, uc.reg_read(UC_X86_REG_ECX), uc.reg_read(UC_X86_REG_EDX)) or 0
        uc.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + fn.pop)
        uc.reg_write(UC_X86_REG_EIP, ret)
    mu.hook_add(UC_HOOK_CODE, cb, begin=addr, end=addr)


def on_area(esp, ecx, edx):
    a = [rd(esp + 4 + 4 * i) for i in range(12)]    # a3..a14
    cap["area"] = [rd(ecx), rd(edx)] + [rd(a[0] + 4 * i) for i in range(6)]
    cap["area_args"] = a
    cap["ctx"] = (rd(a[9]), rd(a[9] + 4))
    return 0


on_area.pop = 0x30


def on_delayed(esp, ecx, edx):
    p = rd(esp + 4)
    cap["delayed"] = (rd(ecx), rd(p), rd(p + 4), [rd(rd(esp + 8) + 4 * i) for i in range(4)],
                      [rd(esp + 12 + 4 * i) for i in range(8)])
    return 0


on_delayed.pop = 0x28


def on_norm(esp, ecx, edx):
    p = rd(esp + 4)
    wr(p, cap["pos"][0])
    wr(p + 4, cap["pos"][1])
    return p


on_norm.pop = 4


def install():
    S.install()
    A.patch_seh()
    STUBS.update({0: A.stub_ret(4, 0), 1: A.stub_ret(4, 1), "g": A.stub_ret(0, 7), "s": A.stub_ret(8, 0x1234), "f": A.stub_ret(0, 0)})
    H_install_frag()
    hook(F_AREA, on_area)
    hook(F_DELAYED, on_delayed)
    hook(F_NORM, on_norm)
    A.patch_jump(0x6F472890, A.stub_ret(0, UNIT))
    mu.mem_write(UVT, b"\0" * 0x400)
    wr(UVT + 184, A.stub_ret(0, 0))               # position object (the normaliser stub supplies the coordinates)
    wr(UVT + 0x11C, A.stub_ret(0, 0))


def fresh(vt, flags, targs, data, dur, area, pause=0, enabled=1):
    mu.mem_write(ABIL, b"\0" * 0x200)
    mu.mem_write(AVT, bytes(mu.mem_read(vt, 0x500)))
    wr(AVT + 616, STUBS[enabled])
    wr(ABIL, AVT)
    wr(ABIL + 0x20, flags)
    wr(ABIL + 0x30, UNIT)
    wr(ABIL + 0x3C, pause)
    wr(ABIL + 0x4C, targs)
    wr(ABIL + 0x50, 0)
    wr(ABIL + 0x54, DEF)
    mu.mem_write(DEF, b"\0" * 0x100)
    wr(DEF + 0x2C, 1)
    wr(DEF + 0x50, 1)
    wr(DEF + 0x54, LVL)
    mu.mem_write(LVL, b"\0" * 0x68)
    wr(LVL + 0x08, dur)
    wr(LVL + 0x18, area)
    for i, v in enumerate(data):
        wr(LVL + 0x20 + 4 * i, v)
    mu.mem_write(UNIT, b"\0" * 0x400)
    wr(UNIT, UVT)
    mu.mem_write(TGTU, b"\0" * 0x400)
    wr(TGTU, UVT)


def run_fl(r):
    data = [S.rbits(r) for _ in range(5)]
    area = S.rbits(r)
    pause = r.choice([0, 0, 0, 1, 3, 0xFFFFFFFF])
    enabled = int(r.random() < 0.85)
    rec = [r.getrandbits(32) for _ in range(3)]
    targs = r.getrandbits(32)
    pos = (S.rbits(r), S.rbits(r))
    fresh(VT_FLAK, 0, targs, data, 0, area, pause, enabled)
    mu.mem_write(REC, b"\0" * 0x100)
    wr(REC + 12, rec[0])
    wr(REC + 20, rec[1])
    wr(REC + 32, rec[2])
    cap.clear()
    cap["pos"] = pos
    S.call(F_FLAK, ecx=ABIL, stack_args=(TGTU, REC))
    head = "FL %x %d %08x %08x %08x %08x %s %08x" % (pause, enabled, rec[0], rec[1], rec[2], targs,
                                                    " ".join("%08x" % d for d in data), area)
    if "area" not in cap:
        return head + " | 0"
    a = cap["area"]
    aa = cap["area_args"]
    tail = " ".join("%08x" % v for v in (a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7],
                                         aa[1], aa[2], aa[3], aa[4], aa[5], aa[6], aa[7], aa[8]))
    return head + " | 1 " + tail + " %d %d %08x %08x" % (cap["ctx"][0] == ABIL, cap["ctx"][1] == TGTU, aa[10], aa[11])


def run_af(r):
    data = [S.rbits(r) for _ in range(5)]
    area = S.rbits(r)
    flags = r.choice([0x400000, 0x400000, 0, 0x400001, 0x1400000])
    fresh(VT_FLAME, flags, 0, data, 0, area)
    wr(AVT + 1144, STUBS["g"])
    wr(AVT + 1156, STUBS["s"])
    mu.mem_write(REC, b"\xCC" * 0x100)
    ret = S.call(F_FLAME, ecx=ABIL, stack_args=(0, REC))
    return "AF %x %s %08x | %x %x %s" % (flags, " ".join("%08x" % d for d in data), area, ret, rd(ABIL + 0x20),
                                         " ".join("%08x" % rd(REC + o) for o in (0x80, 0x84, 0x88, 0x8C, 0x90)))


def run_ad(r):
    data = [S.rbits(r) for _ in range(4)]
    dur = S.rbits(r)
    targs = r.getrandbits(32)
    enabled = int(r.random() < 0.85)
    flags = r.choice([0, 0, 0x80000000, 0x80])
    uflags = r.choice([0, 0, 0x40000000])
    ubyte = r.choice([0, 0, 1])
    pos = (S.rbits(r), S.rbits(r))
    fresh(VT_DEATH, flags, targs, data, dur, 0, 0, enabled)
    wr(UNIT + 92, uflags)
    mu.mem_write(UNIT + 32, bytes([ubyte]))
    cap.clear()
    cap["pos"] = pos
    S.call(F_DEATH, ecx=ABIL)
    head = "AD %d %x %x %d %s %08x %08x" % (enabled, flags, uflags, ubyte, " ".join("%08x" % d for d in data), dur, targs)
    if "delayed" not in cap:
        return head + " | 0"
    d = cap["delayed"]
    a = d[4]
    return head + " | 1 %08x %08x %08x %s %08x %08x %08x %08x %08x %d %08x %08x" % (
        d[0], d[1], d[2], " ".join("%08x" % v for v in (d[3][0], d[3][1], d[3][2], d[3][3])), a[0], a[1], a[2], a[3], a[4],
        a[5] == UNIT, a[6], a[7] if len(a) > 7 else 0)


VT_FRAG, F_FRAG = 0x6F8F404C, 0x6F183B40
FOBJ, FVT2 = M + 0x6C00, M + 0x6E00


def H_install_frag():
    hook(0x6F16C690, on_make)
    hook(0x6F17D5E0, on_init)
    hook(STUBS["f"], on_slot212)


def on_make(esp, ecx, edx):
    """0x6F16C690 (ecx = &local, one stack arg): stores the fake object pointer."""
    wr(ecx, FOBJ)
    return ecx


on_make.pop = 4


def on_init(esp, ecx, edx):
    return ecx


on_init.pop = 12


def on_slot212(esp, ecx, edx):
    cap["frag"] = [rd(esp + 4 + 4 * i) for i in range(8)]
    cap["frag_ptrs"] = [rd(cap["frag"][i]) for i in range(2, 8)]
    return 0


on_slot212.pop = 0x20


def run_fs(r):
    """Frag Shards slot 0x140: the temporary object of 0x6F16C690 / 0x6F17D5E0 gets the area ring pointers through its slot 212."""
    data = [S.rbits(r) for _ in range(5)]
    area = S.rbits(r)
    pause = r.choice([0, 0, 0, 1, 3, 0xFFFFFFFF])
    enabled = int(r.random() < 0.85)
    targs = r.getrandbits(32)
    rec = r.getrandbits(32)
    fresh(VT_FRAG, 0, targs, data, 0, area, pause, enabled)
    mu.mem_write(FOBJ, b"\0" * 0x40)
    mu.mem_write(FVT2, b"\0" * 0x400)
    wr(FOBJ, FVT2)
    wr(FVT2 + 212, STUBS["f"])
    cap.clear()
    S.call(F_FRAG, ecx=ABIL, stack_args=(TGTU, rec))
    head = "FS %x %d %08x %08x %s %08x" % (pause, enabled, rec, targs, " ".join("%08x" % d for d in data), area)
    if "frag" not in cap:
        return head + " | 0"
    f = cap["frag"]
    return head + " | 1 %d %08x %s" % (f[0] == rec, f[1], " ".join("%08x" % v for v in cap["frag_ptrs"]))


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 7
    r = random.Random(seed)
    install()
    rows = []
    for i in range(cases):
        rows.append(run_fl(r))
        rows.append(run_af(r))
        rows.append(run_ad(r))
        rows.append(run_fs(r))
    for row in rows[:6]:
        print(row)
    if dump:
        with open(dump, "w") as fh:
            fh.write("\n".join(rows) + "\n")
    print("cases", len(rows))


if __name__ == "__main__":
    main()
