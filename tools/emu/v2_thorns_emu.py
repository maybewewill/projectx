"""Unicorn check V2-4: Thorns Aura / Spiked Carapace reflect, `CBuffAuraThorns` damage-event handler `0x6F1AD4E0` (vtable slot 63).

ORIGINAL code runs; hooks stand in for the agent world: smart-pointer wrap `0x6F038450` (puts the attacker into the argument
slot), allied test `0x6F285AC0`, blocked test `0x6F28D750`, the effect play `0x6F053040`, the record constructor `0x6F2556E0`,
the attacker's vtable (invulnerable `0x11C`, damage `0x120`, immune `0x11C` of the record object).
The buff holds the strength at `+0xD0` and a mode flag at `+0x20 & 0x100` (percent).

Output per case: the record delivered to the attacker (amount bits, class, record flags) or `-`, and whether the effect ran.

File `thorns.txt`: `strength amount reduced percent rflags rclass allied invuln blocked illusion | delivered amount class effect`

Usage:  GAME_DLL=/path/Game.dll py tools/emu/v2_thorns_emu.py [--dump DIR] [CASES] [SEED]
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import undead_ne3_emu as U3  # noqa: E402

mu = emu.mu
DATA = emu.DATA
wr, rd = A.wr, A.rd
f2u, u2f = A.f2u, A.u2f
hook_fn = U3.hook_fn
stub_slot = U3.stub_slot

F_HANDLER = 0x6F1AD4E0
F_WRAP = 0x6F038450
F_ALLIED = 0x6F285AC0
F_BLOCKED = 0x6F28D750
F_EFFECT = 0x6F053040
F_RECORD = 0x6F2556E0
F_OWNER = 0x6F472890

BUFF = DATA + 0xE000
VICTIM = DATA + 0xE300
ATT = DATA + 0xE500
REC = DATA + 0xE700
UVT = DATA + 0xE900


def run(cases, r):
    A.setup_stubs()
    st = {}
    out = {"rec": None, "dmg": None, "effect": 0}

    hook_fn(F_WRAP, lambda uc, esp, ecx: wr(ecx, ATT) or 0, 4)
    hook_fn(F_ALLIED, lambda uc, esp, ecx: st["allied"], 4)
    hook_fn(F_BLOCKED, lambda uc, esp, ecx: st["blocked"], 8)
    hook_fn(F_OWNER, lambda uc, esp, ecx: VICTIM, 0)

    def effect(uc, esp, ecx):
        out["effect"] = 1
        return 0

    hook_fn(F_EFFECT, effect, 4)

    def record(uc, esp, ecx):
        amt_ptr, cls, att, flags, zero = (rd(esp + 4 + 4 * i) for i in range(5))
        st["rec"] = (rd(amt_ptr), cls, flags)
        return ecx

    hook_fn(F_RECORD, record, 20)
    s_inv = stub_slot()
    s_dmg = stub_slot()
    hook_fn(s_inv, lambda uc, esp, ecx: st["invuln"], 0)

    def damage(uc, esp, ecx):
        out["dmg"] = st["rec"]
        return 0

    hook_fn(s_dmg, damage, 16)
    mu.mem_write(UVT, b"\0" * 0x400)
    wr(UVT + 0x11C, s_inv)
    wr(UVT + 0x120, s_dmg)

    rows = []
    for _ in range(cases):
        strength = r.choice((0.1, 0.2, 0.3, 0.15, r.random(), r.uniform(0, 3), 0.0, 25.0))
        amount = r.choice((r.uniform(1, 200), float(r.randrange(1, 60)), 0.0, r.uniform(0, 3), 1.0))
        reduced = r.choice((0.0, 0.0, r.uniform(0, 20), float(r.randrange(0, 10))))
        percent = int(r.random() < 0.7)
        rflags = r.choice((0, 0, 0, 1, 0x10))
        rclass = r.choice((0x10, 0x10, 0x10, 0x20))
        allied = int(r.random() < 0.2)
        invuln = int(r.random() < 0.15)
        blocked = int(r.random() < 0.15)
        illusion = int(r.random() < 0.15)
        st.update(allied=allied, invuln=invuln, blocked=blocked, rec=None)
        out["dmg"] = None
        out["effect"] = 0
        mu.mem_write(BUFF, b"\0" * 0x200)
        wr(BUFF + 0x30, VICTIM)
        wr(BUFF + 0x20, 0x100 if percent else 0)
        wr(BUFF + 0xD0, f2u(strength))
        mu.mem_write(VICTIM, b"\0" * 0x200)
        wr(VICTIM + 0x5C, 0x40000000 if illusion else 0)
        mu.mem_write(ATT, b"\0" * 0x200)
        wr(ATT, UVT)
        mu.mem_write(REC, b"\0" * 0x40)
        wr(REC + 4, 9)
        wr(REC + 0xC, rflags)
        wr(REC + 0x10, f2u(amount))
        wr(REC + 0x14, rclass)
        wr(REC + 0x18, f2u(reduced))
        ring = []
        from unicorn import UC_HOOK_CODE
        hk = mu.hook_add(UC_HOOK_CODE, lambda uc, a, sz, ud: (ring.append(a), ring.__delitem__(0) if len(ring) > 20 else None) and None)
        try:
            emu.call(F_HANDLER, ecx=BUFF, stack_args=(REC,))
        except Exception as e:
            raise SystemExit("handler failed: %s trail=%s" % (e, [hex(x) for x in ring]))
        finally:
            mu.hook_del(hk)
        rows.append((f2u(strength), f2u(amount), f2u(reduced), percent, rflags, rclass, allied, invuln, blocked, illusion,
                     out["dmg"], out["effect"]))
    return rows


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump = args[1]
        args = args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 9
    rows = run(cases, random.Random(seed))
    for row in rows[:10]:
        s, a, rd_, pc, fl, cl, al, inv, bl, il, dmg, ef = row
        print("str=%g amt=%g red=%g pct=%d fl=%x cl=%x al=%d inv=%d bl=%d ill=%d -> %s effect=%d" % (
            u2f(s), u2f(a), u2f(rd_), pc, fl, cl, al, inv, bl, il,
            None if dmg is None else (u2f(dmg[0]), hex(dmg[1]), hex(dmg[2])), ef))
    if dump:
        os.makedirs(dump, exist_ok=True)
        with open(os.path.join(dump, "thorns.txt"), "w") as fh:
            for s, a, rd_, pc, fl, cl, al, inv, bl, il, dmg, ef in rows:
                fh.write("%08x %08x %08x %d %x %x %d %d %d %d | %s %d\n" % (
                    s, a, rd_, pc, fl, cl, al, inv, bl, il,
                    "-" if dmg is None else "%08x %x" % (dmg[0], dmg[1]), ef))
    print("cases", len(rows))


if __name__ == "__main__":
    main()
