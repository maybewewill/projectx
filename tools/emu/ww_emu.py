"""Unicorn run of the Wind Walk buff `BOwk` (`CBuffWindWalk`, vtable 0x6F8ED58C) first-strike code of Game.dll 1.26a
(docs/spec/abilities-orc-heroes.md section 11, task S34).

The ORIGINAL code runs on a fake buff object (the real class vtable):
  * the damage handler, slot `0x134` = `0x6F1637B0` (category `0x130` = 0x0B, same family as Critical Strike): when the
    buff flag `0x200` is set it posts the owner event 0xE, adds the buff's bonus (`buff+0xE8` = DataC) to the damage
    record through the REAL `0x6F255720` (real soft add / sub), ORs the record flag `0x400` when the buff flag `0x400`
    is set, and calls the buff slot `0x34C` with (0x6FAAE480 = 0.01 s, 0) = "expire in 0.01 s" (recorded here);
  * the arming slot `0x124` = `0x6F171870` (order broadcast `0xD000F` at the swing start): clears `0x200`; with a target
    and buff flag `0x100` clear it asks the target filter `0x6F26D590(owner, target, buff+0xB8)` (stubbed answer): a zero
    answer sets `0x200` and posts event 0xE; a non-zero answer removes the buff (`0x6F079CC0`, recorded).

Rows (floats as hex bits):
  W <buff flags> <A> <B> <V> <rec flags> | <A'> <B'> <rec+0x1C> <rec flags'> <buff flags'> <event 0/1> <slot34C called 0/1>
  X <has target 0/1> <buff flags> <filter answer> | <buff flags'> <removed 0/1> <event 0/1>

Usage: GAME_DLL=/path/Game.dll py tools/emu/ww_emu.py OUT.txt COUNT SEED
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import chain_emu as C  # noqa: E402
import emu  # noqa: E402
import abilities4_emu as A  # noqa: E402

mu = emu.mu
D = emu.DATA
w32, r32, stub, ret, patch = C.w32, C.r32, C.stub, C.ret, C.patch

VT_BOWK = 0x6F8ED58C
F_HANDLER = 0x6F1637B0
F_ARM = 0x6F171870

BUFF = D + 0x8000
VT = D + 0x8400
OWNER = D + 0x8C00
OWNER_VT = D + 0x8D00
REC = D + 0x9000
LOGV = D + 0x9400   # +0 event count, +4 slot34C count, +8 removed count, +0xC filter answer


def setup():
    mu.mem_write(VT, bytes(mu.mem_read(VT_BOWK, 0x448)))
    # event poster 0x6F27B4F0 / 0x6F27B460 (ecx = unit, push event id): count
    cnt_event = b"\xFF\x05" + struct.pack("<I", LOGV)
    patch(0x6F27B4F0, stub(cnt_event + ret(4)))
    patch(0x6F27B460, stub(cnt_event + ret(4)))
    # slot 0x34C (ecx = buff, push ptr, push 0): count
    w32(VT + 0x34C, stub(b"\xFF\x05" + struct.pack("<I", LOGV + 4) + ret(8)))
    # buff removal 0x6F079CC0 (ecx = unit, push buff): count
    patch(0x6F079CC0, stub(b"\xFF\x05" + struct.pack("<I", LOGV + 8) + ret(4)))
    # target filter 0x6F26D590 (ecx = owner, push target, push flags): answer from LOGV+0xC
    patch(0x6F26D590, stub(b"\xA1" + struct.pack("<I", LOGV + 0xC) + ret(8)))
    w32(OWNER, OWNER_VT)


def fresh(flags, bonus):
    mu.mem_write(BUFF, bytes(0x400))
    w32(BUFF, VT)
    w32(BUFF + 0x20, flags)
    w32(BUFF + 0x30, OWNER)
    w32(BUFF + 0xE8, bonus)
    w32(BUFF + 0xB8, 0x1234)
    mu.mem_write(LOGV, bytes(0x20))


SPECIAL = [0, 0x80000000, 0x3F800000, 0xBF800000, 0x7F800000, 0x7FC00000, 0x42C80000, 0x00000001, 0xC2C80000]


def rnd(r, lo, hi):
    k = r.random()
    if k < 0.15:
        return r.choice(SPECIAL)
    if k < 0.9:
        return C.fbits(r.uniform(lo, hi))
    return r.getrandbits(32)


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    setup()
    lines = []
    for _ in range(count):
        flags = r.choice([0x200, 0x200, 0x600, 0x000, 0x400, 0x200 | r.getrandbits(32) & 0xFF0000])
        bonus = rnd(r, 0.0, 120.0)
        a = rnd(r, 0.0, 400.0)
        b = rnd(r, -5.0, 60.0) if r.random() < 0.7 else r.choice([0, 0x80000000, 0x7FC00000])
        recflags = r.getrandbits(32) & 0xFFFF
        fresh(flags, bonus)
        mu.mem_write(REC, bytes(0x40))
        w32(REC + 0xC, recflags)
        w32(REC + 0x10, a)
        w32(REC + 0x18, b)
        emu.call(F_HANDLER, ecx=BUFF, stack_args=(0xAAAA, REC))
        lines.append("W %08x %08x %08x %08x %08x | %08x %08x %08x %08x %08x %d %d" % (
            flags, a, b, bonus, recflags, r32(REC + 0x10), r32(REC + 0x18), r32(REC + 0x1C), r32(REC + 0xC),
            r32(BUFF + 0x20), 1 if r32(LOGV) else 0, 1 if r32(LOGV + 4) else 0))
    for _ in range(count):
        has_target = r.choice([0, 1, 1, 1])
        flags = r.choice([0, 0x100, 0x200, 0x300, r.getrandbits(32)])
        ans = r.choice([0, 0, 1, 0xA2])
        fresh(flags, 0)
        w32(LOGV + 0xC, ans)
        emu.call(F_ARM, ecx=BUFF, stack_args=(0x5555 if has_target else 0, 0, 0))
        lines.append("X %d %08x %d | %08x %d %d" % (
            has_target, flags, ans, r32(BUFF + 0x20), 1 if r32(LOGV + 8) else 0, 1 if r32(LOGV) else 0))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
