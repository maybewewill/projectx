"""V7 Unicorn check of the Slow aura buff of Game.dll 1.26a (Aasl / Basl, docs/spec/abilities-unit-a.md, V7).

ORIGINAL `CBuffAuraSlow` slot 0x370 (`0x6F1AD2F0`, the update with a new movement and a new attack-speed strength) runs on a fake
buff: the stored strengths are the holder values at +0xE4 (movement) and +0xEC (attack speed); the undo / apply routines
`0x6F1AD290` / `0x6F1AD240` (movement) and `0x6F1AD1A0` / `0x6F1AD150` (attack speed) are the real ones; their sinks `0x6F202FF0`
(movement multiplier) and `0x6F0C9090` (attack speed) append to a log; the base slots 0x324 / 0x34C are stubs that count.

Row (hex words):
  <flags> <stored move> <stored attack> <new move> <new attack> | <slot 0x324 calls> <slot 0x34C calls> <final move> <final attack> <flags after> <sinks as tag:value ...>
  flags: 0x200 = movement applied, 0x100 = attack applied. tag m = movement sink, a = attack sink.
Usage: GAME_DLL=/path/Game.dll py tools/emu/v7_slow_emu.py [--dump OUT_FILE] [CASES] [SEED]
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

mu = emu.mu
wr, rd = A.wr, A.rd
BUFF, TGT = S.BUFF, S.TGT
LOG = S.M + 0x9000        # +0 count, entries at +8: (tag, value)
CNT = S.M + 0x9800        # +0 slot 0x324 calls, +4 slot 0x34C calls
NEW = S.M + 0x9900        # +0 new movement, +4 new attack


def log_sink(addr, tag, nbytes):
    code = (b"\x8B\x4C\x24\x04\x8B\x09" + b"\xA1" + S.a32(LOG)
            + b"\xC7\x04\xC5" + S.a32(LOG + 8) + S.a32(tag)
            + b"\x89\x0C\xC5" + S.a32(LOG + 12)
            + b"\xFF\x05" + S.a32(LOG) + b"\xC2" + struct.pack("<H", nbytes))
    A.patch_jump(addr, S.new_code(code))


def counter(slot_off):
    return S.new_code(b"\xFF\x05" + S.a32(CNT + slot_off) + b"\xC2\x04\x00" if slot_off == 0
                      else b"\xFF\x05" + S.a32(CNT + slot_off) + b"\xC2\x08\x00")


def install():
    S.install()
    A.patch_jump(0x6F477D30, A.stub_ret(0, 0))
    log_sink(0x6F202FF0, 0x6D, 4)
    log_sink(0x6F0C9090, 0x61, 8)
    global STUB324, STUB34C
    STUB324 = S.new_code(bytes([0xFF, 0x05]) + S.a32(CNT) + bytes([0xC2, 0x04, 0x00]))
    STUB34C = S.new_code(bytes([0xFF, 0x05]) + S.a32(CNT + 4) + bytes([0xC2, 0x08, 0x00]))


def run(cases, r):
    install()
    rows = []
    for _ in range(cases):
        flags = (0x200 if r.random() < 0.5 else 0) | (0x100 if r.random() < 0.5 else 0)
        s0, s1, n0, n1 = (S.rbits(r) for _ in range(4))
        if r.random() < 0.4:
            # slow-like values: negative fractions, often equal or close
            base = [-0.1, -0.25, -0.5, -0.4, 0.0, -0.2]
            s0, s1, n0, n1 = (A.f2u(r.choice(base) + r.choice([0, 0, 0.0004, -0.002])) for _ in range(4))
        mu.mem_write(TGT, b"\0" * 0x400)
        wr(TGT + 0x1E8, S.ATK)
        wr(TGT + 0x1EC, S.ATK)
        mu.mem_write(BUFF, b"\0" * 0x400)
        mu.mem_write(S.FVT, bytes(mu.mem_read(0x6F900D4C, 0x400)))
        wr(S.FVT + 0x324, STUB324)
        wr(S.FVT + 0x34C, STUB34C)
        wr(BUFF, S.FVT)
        wr(BUFF + 0x20, flags)
        wr(BUFF + 0x30, TGT)
        wr(BUFF + 0xE0, S.HVT_A)
        wr(BUFF + 0xE4, s0)
        wr(BUFF + 0xE8, S.HVT_A)
        wr(BUFF + 0xEC, s1)
        mu.mem_write(LOG, b"\0" * 0x400)
        mu.mem_write(CNT, b"\0" * 8)
        wr(NEW, n0)
        wr(NEW + 4, n1)
        S.call(0x6F1AD2F0, ecx=BUFF, stack_args=(TGT, NEW, NEW + 4))
        n = rd(LOG)
        sinks = []
        for i in range(n):
            tag = rd(LOG + 8 + 8 * i)
            sinks.append("%s:%08x" % (chr(tag + 0) if tag in (0x61, 0x6D) else "?", rd(LOG + 12 + 8 * i)))
        rows.append((flags, s0, s1, n0, n1, rd(CNT), rd(CNT + 4), rd(BUFF + 0xE4), rd(BUFF + 0xEC), rd(BUFF + 0x20),
                     " ".join(sinks)))
    return rows


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 600
    seed = int(args[1]) if len(args) > 1 else 7
    rows = run(cases, random.Random(seed))
    for row in rows[:8]:
        print(row)
    if dump:
        with open(dump, "w") as fh:
            for row in rows:
                fh.write("%x %08x %08x %08x %08x | %d %d %08x %08x %x %s\n" % row)
    print("cases", len(rows))


if __name__ == "__main__":
    main()
