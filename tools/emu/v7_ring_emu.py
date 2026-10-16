"""V7 Unicorn run of the ring collector `0x6F2C2E80` of Game.dll 1.26a (the per-candidate callback of the area damage body `0x6F2C31D0`,
used by Flak Cannon, Frag Shards, Death Damage, Pulverize, Cleave, Orb of Fire, ... ).

ORIGINAL collector on a fake context and a fake candidate unit. Stubs: the ring membership `0x6F476C40` (true when the ring radius is not
below the case's distance), the max-mode test `0x6F26F2B0`, the unit's owner object `0x6F282F60`, the Misc reader `0x6F009F90`, the two
alliance tests `0x6F3A3400` / `0x6F3A36D0` (bit tables), unit vtable slots 184 / 236 / 284 (position, owner index, invulnerable).

Row (hex words, `|` separates the case from the result):
  <count> <r0 r1 r2 r3> <a0 a1 a2 a3> <class> <row> <maxmode-flag> <max-test> <owner-filter> | dist misc resist immune_cnt ethereal_cnt invuln unit-null owner-idx ally-bits enemy-bits
  | <ret> <entries> <total> <b0> <b1> <b2> <entry-amount> <entry-bucket-word>

Usage: GAME_DLL=/path/Game.dll py tools/emu/v7_ring_emu.py [--dump OUT_FILE] [CASES] [SEED]
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
import v7_hits_emu as H  # noqa: E402

mu = emu.mu
wr, rd, f2u, u2f = A.wr, A.rd, A.f2u, A.u2f
M = S.M
CTX, ENT, UNIT2, UV2 = M + 0x7800, M + 0x7A00, M + 0x7C00, M + 0x8200
F_COLLECT = 0x6F2C2E80
st = {}


def on_member(esp, ecx, edx):
    radius = u2f(rd(rd(esp + 4)))
    return int(radius >= st["dist"])


on_member.pop = 0x10


def on_max(esp, ecx, edx):
    return st["maxtest"]


on_max.pop = 4


def on_owner(esp, ecx, edx):
    return 0 if st["unit_null"] else UNIT2


on_owner.pop = 0


def on_misc(esp, ecx, edx):
    return st["misc"]


on_misc.pop = 4


def on_ally(esp, ecx, edx):
    return (st["ally"] >> (rd(esp + 8) & 31)) & 1


on_ally.pop = 8


def on_enemy(esp, ecx, edx):
    return (st["enemy"] >> (rd(esp + 8) & 31)) & 1


on_enemy.pop = 8


def install():
    S.install()
    A.patch_seh()
    H.hook(0x6F476C40, on_member)
    H.hook(0x6F26F2B0, on_max)
    H.hook(0x6F282F60, on_owner)
    H.hook(0x6F009F90, on_misc)
    H.hook(0x6F3A3400, on_ally)
    H.hook(0x6F3A36D0, on_enemy)
    mu.mem_write(UV2, b"\0" * 0x400)
    wr(UV2 + 184, A.stub_ret(0, 0))


def case(r):
    count = r.choice([0, 1, 2, 3, 4, 4])
    radii = [f2u(r.choice([r.uniform(0, 600), 0.0, 100.0, 250.0, 300.0])) for _ in range(4)]
    amounts = [f2u(r.choice([r.uniform(0, 200), 0.0, -5.0, 50.0, 100.0])) for _ in range(4)]
    if r.random() < 0.1:
        amounts[r.randrange(4)] = 0x7FC00000
    cls = r.choice([16, 32, 0x4000000, 0x400000, 1, 2, 4, 8, 0x20000, 0x8000, r.getrandbits(32)])
    row = r.choice([1, 2, 3, 4, 5, 6, 0, 7])
    maxflag = int(r.random() < 0.3)
    maxtest = int(r.random() < 0.5)
    owner_filter = r.choice([0xFFFFFFFF, 0xFFFFFFFF, 0, 3, 7])
    dist = r.choice([0.0, r.uniform(0, 700), 99.0, 250.0, 300.0])
    misc = int(r.random() < 0.5)
    immune = r.choice([0, 0, 1, 2, -1]) & 0xFFFFFFFF
    ethereal = r.choice([0, 0, 1, 2, -1]) & 0xFFFFFFFF
    invuln = int(r.random() < 0.25)
    unit_null = int(r.random() < 0.1)
    owner_idx = r.randint(0, 15)
    ally = r.getrandbits(32)
    enemy = r.getrandbits(32)
    return (count, radii, amounts, cls, row, maxflag, maxtest, owner_filter, dist, misc, immune, ethereal, invuln, unit_null,
            owner_idx, ally, enemy)


def run(c):
    (count, radii, amounts, cls, row, maxflag, maxtest, owner_filter, dist, misc, immune, ethereal, invuln, unit_null,
     owner_idx, ally, enemy) = c
    mu.mem_write(CTX, b"\0" * 0x100)
    mu.mem_write(ENT, b"\xEE" * 0x100)
    wr(CTX + 0x00, 1000)
    wr(CTX + 0x04, 0)
    wr(CTX + 0x08, ENT)
    wr(CTX + 0x0C, 16)
    for i in range(4):
        wr(CTX + 0x28 + 4 * i, radii[i])
        wr(CTX + 0x38 + 4 * i, amounts[i])
    wr(CTX + 0x48, count)
    maxr = 0.0
    for x in radii[:min(count, 4)]:
        if u2f(x) > maxr:
            maxr = u2f(x)
    wr(CTX + 0x4C, f2u(maxr))
    wr(CTX + 0x50, cls)
    wr(CTX + 0x58, row)
    wr(CTX + 0x5C, 0)
    wr(CTX + 0x60, maxflag)
    wr(CTX + 0x64, owner_filter)
    mu.mem_write(UNIT2, b"\0" * 0x400)
    wr(UNIT2, UV2)
    wr(UNIT2 + 0x158, ethereal)
    wr(UNIT2 + 0x15C, immune)
    wr(UV2 + 236, A.stub_ret(0, owner_idx) if owner_idx not in st.setdefault("own", {}) else st["own"][owner_idx])
    st["own"][owner_idx] = rd(UV2 + 236)
    wr(UV2 + 284, st.setdefault("inv", {}).get(invuln) or st["inv"].setdefault(invuln, A.stub_ret(0, invuln)))
    st.update(dist=dist, maxtest=maxtest, misc=misc, unit_null=unit_null, ally=ally, enemy=enemy)
    ret = S.call(F_COLLECT, ecx=UNIT2, edx=CTX)
    n = rd(CTX + 0x04)
    ent_unit, ent_amount, ent_bucket = (rd(ENT), rd(ENT + 4), rd(ENT + 8)) if n else (0, 0, 0)
    return ret, n, rd(CTX + 0x10), rd(CTX + 0x14), rd(CTX + 0x18), rd(CTX + 0x1C), rd(CTX + 0x20), ent_unit == UNIT2, ent_amount, ent_bucket


def row_text(c, res):
    (count, radii, amounts, cls, row, maxflag, maxtest, owner_filter, dist, misc, immune, ethereal, invuln, unit_null,
     owner_idx, ally, enemy) = c
    return "%d %s %s %x %x %d %d %x | %08x %d %d %x %x %d %d %d %x %x | %x %d %d %d %d %d %d %d %08x %08x" % (
        count, " ".join("%08x" % x for x in radii), " ".join("%08x" % x for x in amounts), cls, row, maxflag, maxtest,
        owner_filter, f2u(dist), misc, 0, immune, ethereal, invuln, unit_null, owner_idx, ally, enemy,
        res[0], res[1], res[2], res[3], res[4], res[5], res[6], int(res[7]), res[8], res[9])


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
    for _ in range(cases):
        c = case(r)
        rows.append(row_text(c, run(c)))
    for row in rows[:8]:
        print(row)
    if dump:
        with open(dump, "w") as fh:
            fh.write("\n".join(rows) + "\n")
    print("cases", len(rows))


if __name__ == "__main__":
    main()
