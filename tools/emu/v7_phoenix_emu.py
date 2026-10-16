"""V7 Unicorn run of the Summon Phoenix effect `0x6F11BA10` (AHpx, vtable `0x6F8DB09C` slot 0x3C0) of Game.dll 1.26a.

The effect registers a callback (`0x6F1132A0` + `0x6F46D1B0`, stubs here) and jumps into the Water Elemental family summon loop
`0x6F163390` that `v5_summon_emu.py` checks for AHwe / AEst / AEsv. This harness enters through the Phoenix slot itself, with the
Phoenix vtable copy and the same stubs and row format as `v5_summon_emu.py` (see its header).

Usage: GAME_DLL=/path/Game.dll py tools/emu/v7_phoenix_emu.py OUT.txt COUNT SEED
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import chain_emu as C  # noqa: E402
import emu  # noqa: E402
import feral_emu as F  # noqa: E402
import v5_summon_emu as V  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

mu = emu.mu
w32, r32, fbits, stub, ret, patch = C.w32, C.r32, C.fbits, C.stub, C.ret, C.patch

VT_PHOENIX = 0x6F8DB09C
F_PHOENIX_EFFECT = 0x6F11BA10


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    rec, _ = F.setup()
    patch(V.LINK, stub(ret(4)))
    patch(0x6F46D1B0, stub(ret(8)))
    s3e4 = stub(b"\x90" + ret(8))
    mu.hook_add(UC_HOOK_CODE, V.on_creator, begin=rec, end=rec)
    mu.hook_add(UC_HOOK_CODE, V.on_slot, begin=s3e4, end=s3e4)
    mu.hook_add(UC_HOOK_CODE, V.on_link, begin=V.LINK, end=V.LINK)
    lines = []
    for _ in range(count):
        mu.mem_write(F.VT_COPY, bytes(mu.mem_read(VT_PHOENIX, 0x448)))
        w32(F.VT_COPY + 0x3E4, s3e4)
        dur = r.choice([0, 0, C.rnd(r, 0.0, 120.0)])
        plus = r.choice([0, 0, 0, C.rnd(r, 0.0, 5.0)])
        data_a = r.choice([fbits(2.0), fbits(1.0), fbits(3.0), fbits(0.0), fbits(r.uniform(-1.0, 9.0))])
        area = r.choice([fbits(250.0), C.rnd(r, 0.0, 600.0)])
        unit_id = r.choice([0x6F737731, 0x6F737732, 0x6F737733])
        F.fresh_ability(dur, plus, 0, area, unit_id)
        w32(F.LVL + 0x20, data_a)
        w32(F.CASTER + 0x58, r.randint(0, 15))
        V.creator.clear()
        V.slot.clear()
        V.link.clear()
        emu.call(F_PHOENIX_EFFECT, ecx=F.ABIL)
        owner = r32(F.CASTER + 0x58)
        parts = ["%08x %d %d %d %d %08x" % (ecx, edx == F.CASTER, a[0] == owner, r32(a[1]) == area, a[2], a[3])
                 for ecx, edx, a in V.creator]
        sl = " ".join("%d %08x" % (u == F.NEWUNIT, d) for u, d in V.slot)
        lines.append("W %08x %08x %08x %08x %08x %08x %d | %d %s | %d | %d %s" % (
            VT_PHOENIX, dur, plus, data_a, area, unit_id, owner, len(V.creator), " ; ".join(parts), len(V.link),
            len(V.slot), sl))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
