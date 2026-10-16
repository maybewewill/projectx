"""Unicorn check V2-2: Mana Burn (`AEmb`) impact `0x6F119650` (CMissileManaBurn slot 0x84) + mana taker `0x6F283DB0`.

ORIGINAL code is run; hooks stand in for the agent world: handle refs `0x6F4786B0`, the agile mana property read/write
`0x6F4773A0` / `0x6F477350`, the damage record constructor `0x6F2556E0`, the unit vtable (invulnerable `0x11C`, damage
`0x120`), the immunity scan `0x6F077110`. The missile stores the burn amount (DataA) at `+0xBC`.
Output per case: mana written back (or none), damage delivered (amount bits, record class, flags) or none.

File `manaburn.txt`: `mana amount invuln hasability immune | mana_after damage_amount flags`  (hex bits / 0|1; `-` = none)

Usage:  GAME_DLL=/path/Game.dll py tools/emu/v2_manaburn_emu.py [--dump DIR] [CASES] [SEED]
"""
import os
import random
import struct
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

F_IMPACT = 0x6F119650
F_REF = 0x6F4786B0
F_PROP_READ = 0x6F4773A0
F_PROP_WRITE = 0x6F477350
F_RECORD = 0x6F2556E0
F_IMMUNE = 0x6F077110

MISSILE = DATA + 0xE000
CASTER = DATA + 0xE600
TARGET = DATA + 0xE800
TVT = DATA + 0xEC00
AMT = DATA + 0xEF00


def run(cases, r):
    A.setup_stubs()
    st = {}
    log = {"mana": None, "dmg": None}

    def ref(uc, esp, ecx):
        return {MISSILE + 0x24: CASTER, MISSILE + 0x30: TARGET}.get(ecx, 0)

    hook_fn(F_REF, ref, 0)

    def pread(uc, esp, ecx):
        out = rd(esp + 4)
        wr(out, st["mana"])
        return out

    hook_fn(F_PROP_READ, pread, 4)

    def pwrite(uc, esp, ecx):
        log["mana"] = rd(rd(esp + 4))
        return 0

    hook_fn(F_PROP_WRITE, pwrite, 4)

    def record(uc, esp, ecx):
        amt_ptr, cls, att, flags, zero = (rd(esp + 4 + 4 * i) for i in range(5))
        st["rec"] = (rd(amt_ptr), cls, flags)
        return ecx

    hook_fn(F_RECORD, record, 20)
    hook_fn(F_IMMUNE, lambda uc, esp, ecx: st["immune"], 12)

    # unit vtable
    mu.mem_write(TVT, b"\0" * 0x400)
    s_inv = stub_slot()
    s_dmg = stub_slot()
    s_text = stub_slot()
    hook_fn(s_inv, lambda uc, esp, ecx: st["invuln"], 0)

    def damage(uc, esp, ecx):
        log["dmg"] = st.get("rec")
        return 0

    hook_fn(s_dmg, damage, 16)
    hook_fn(s_text, lambda uc, esp, ecx: 0, 8)
    wr(TVT + 0x11C, s_inv)
    wr(TVT + 0x120, s_dmg)
    wr(TVT + 0x100, s_text)

    rows = []
    for _ in range(cases):
        mana = r.choice((0.0, r.uniform(0, 600), float(r.randrange(0, 400)), r.uniform(0, 5), 1e9))
        amount = r.choice((0.0, 50.0, 100.0, 150.0, r.uniform(0, 400), float(r.randrange(1, 300))))
        invuln = int(r.random() < 0.15)
        hasab = int(r.random() > 0.1)
        immune = int(r.random() < 0.15)
        st.update(mana=f2u(mana), invuln=invuln, immune=immune, rec=None)
        log["mana"] = None
        log["dmg"] = None
        mu.mem_write(MISSILE, b"\0" * 0x100)
        mu.mem_write(TARGET, b"\0" * 0x400)
        mu.mem_write(CASTER, b"\0" * 0x400)
        wr(TARGET, TVT)
        wr(CASTER, TVT)
        wr(TARGET + 0x1DC, 0xFFFFFFFF)
        wr(TARGET + 0x1E0, 0xFFFFFFFF)
        wr(MISSILE + 0x24, 1)
        wr(MISSILE + 0x28, 2)
        wr(MISSILE + 0x30, 3)
        wr(MISSILE + 0x34, 4)
        wr(MISSILE + 0xAC, 0xFFFFFFFF)
        wr(MISSILE + 0xB0, 0xFFFFFFFF)
        wr(MISSILE + 0x80, 1 if hasab else 0)
        wr(MISSILE + 0xBC, f2u(amount))
        from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ESI
        mu.reg_write(UC_X86_REG_EBX, 0x41414141)
        mu.reg_write(UC_X86_REG_ESI, 0x42424242)
        ring = []
        from unicorn import UC_HOOK_CODE
        hk = mu.hook_add(UC_HOOK_CODE, lambda uc, a, sz, ud: (ring.append(a), ring.__delitem__(0) if len(ring) > 24 else None) and None)
        try:
            emu.call(F_IMPACT, ecx=MISSILE)
        except Exception as e:
            raise SystemExit("impact failed: %s trail=%s" % (e, [hex(x) for x in ring]))
        finally:
            mu.hook_del(hk)
        rows.append((f2u(mana), f2u(amount), invuln, hasab, immune, log["mana"], log["dmg"]))
    return rows


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump = args[1]
        args = args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 5
    rows = run(cases, random.Random(seed))
    for m, a, inv, ha, im, mana, dmg in rows[:12]:
        print("mana=%g A=%g inv=%d ab=%d immune=%d -> mana_after=%s dmg=%s" % (
            u2f(m), u2f(a), inv, ha, im, None if mana is None else u2f(mana),
            None if dmg is None else (u2f(dmg[0]), hex(dmg[1]), hex(dmg[2]))))
    if dump:
        os.makedirs(dump, exist_ok=True)
        with open(os.path.join(dump, "manaburn.txt"), "w") as fh:
            for m, a, inv, ha, im, mana, dmg in rows:
                fh.write("%08x %08x %d %d %d | %s %s\n" % (
                    m, a, inv, ha, im, "-" if mana is None else "%08x" % mana,
                    "-" if dmg is None else "%08x %x %x" % dmg))
    print("cases", len(rows))


if __name__ == "__main__":
    main()
