"""V7 Unicorn check of the Ethereal ability counter of Game.dll 1.26a (Aetl, docs/spec/abilities-unit-a.md, V7).

ORIGINAL unit methods run on a fake unit: `0x6F07A340` (ability init: ++unit+0x158, on the first count the side effects) and
`0x6F0796A0` (removal: --unit+0x158, on reaching zero the undo). The refresh `0x6F284830`, the attack-ability notices `0x6F0754E0` /
`0x6F075500`, `0x6F2AB310` / `0x6F2AC3A0` and `0x6F26F830` are counting stubs; `0x6F079FE0` and `0x6F0767F0` run for real on an
empty ability list.

Row: <start counter> <ops I/R string> | <counter after> <unit+0x1C0 after> <refresh calls> <enter side-effect calls> <leave side-effect calls>
Usage: GAME_DLL=/path/Game.dll py tools/emu/v7_ethereal_emu.py [--dump OUT_FILE] [CASES] [SEED]
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import spell_buff_emu as S  # noqa: E402

mu = emu.mu
wr, rd = A.wr, A.rd
TGT = S.TGT
CNT = S.M + 0x9800     # +0 refresh, +4 enter (0x6F2AB310), +8 leave (0x6F2AC3A0)


def count_stub(slot, nbytes):
    code = bytes([0xFF, 0x05]) + S.a32(CNT + slot) + (bytes([0xC2, nbytes, 0x00]) if nbytes else bytes([0xC3]))
    return S.new_code(code)


def install():
    S.install()
    A.patch_jump(0x6F284830, count_stub(0, 0))
    A.patch_jump(0x6F2AB310, count_stub(4, 8))
    A.patch_jump(0x6F2AC3A0, count_stub(8, 0))
    A.patch_jump(0x6F0754E0, A.stub_ret(4, 0))
    A.patch_jump(0x6F075500, A.stub_ret(4, 0))
    A.patch_jump(0x6F26F830, A.stub_ret(4, 0))


def run(cases, r):
    install()
    rows = []
    for _ in range(cases):
        start = r.choice([0, 0, 0, 1, 2, 5, 0xFFFFFFFF])
        ops = "".join(r.choice("IR") for _ in range(r.randint(1, 12)))
        mu.mem_write(TGT, b"\0" * 0x400)
        wr(TGT + 0x158, start)
        wr(TGT + 0x1DC, 0xFFFFFFFF)
        wr(TGT + 0x1E0, 0xFFFFFFFF)
        wr(TGT + 0x1E8, S.ATK if r.random() < 0.5 else 0)
        mu.mem_write(CNT, b"\0" * 12)
        for op in ops:
            S.call(0x6F07A340 if op == "I" else 0x6F0796A0, ecx=TGT)
        rows.append((start, ops, rd(TGT + 0x158), rd(TGT + 0x1C0), rd(CNT), rd(CNT + 4), rd(CNT + 8)))
    return rows


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 7
    rows = run(cases, random.Random(seed))
    for row in rows[:6]:
        print(row)
    if dump:
        with open(dump, "w") as fh:
            for row in rows:
                fh.write("%x %s | %x %x %d %d %d\n" % row)
    print("cases", len(rows))


if __name__ == "__main__":
    main()
