"""Unicorn check V2-3: Spirit Link (`Aspl`) damage share, `CBuffSpiritLink` event handler `0x6F1E9760` -> slot `0x35C` `0x6F1EFED0`.

ORIGINAL code runs; hooks stand in for the agent world: smart-pointer wrap `0x6F038450`, relation test `0x6F285B00`, the
linked-unit list builder `0x6F46D1B0` (fills `n` members), the record constructor `0x6F2556E0`, the owner fallback
`0x6F472890`, unit vtable slots (owner `0xEC`, damage `0x120`).
The buff stores the share fraction (DataA) at `+0xD0`.
Output per case: the hit record's amount after the handler (`amount * fraction`), whether the share was dealt, and the
list of (member index, amount bits, class, flags) records delivered.

File `spiritlink.txt`: `amount fraction n gate_flags | rec_after nshares [amt class flags]...`

Usage:  GAME_DLL=/path/Game.dll py tools/emu/v2_spiritlink_emu.py [--dump DIR] [CASES] [SEED]
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import undead_ne3_emu as U3  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
DATA = emu.DATA
wr, rd = A.wr, A.rd
f2u, u2f = A.f2u, A.u2f
hook_fn = U3.hook_fn
stub_slot = U3.stub_slot

VT_BUFF = 0x6F9264F4
F_HANDLER = 0x6F1E9760
F_WRAP = 0x6F038450
F_REL = 0x6F285B00
F_LIST = 0x6F46D1B0
F_RECORD = 0x6F2556E0
F_OWNER = 0x6F472890

BUFF = DATA + 0xE000
OWNER = DATA + 0xE200
REC = DATA + 0xE400
ARR = DATA + 0xE600          # member array: {unit, aux} pairs
UNITS = DATA + 0xE800        # up to 8 units x 0x40
AUX = DATA + 0xEC00          # aux objects x 0x40
OVT = DATA + 0xF000          # owner vtable
UVT = DATA + 0xF400          # unit vtable (damage slot)


def run(cases, r):
    A.setup_stubs()
    st = {}
    delivered = []

    hook_fn(F_WRAP, lambda uc, esp, ecx: wr(ecx, REC) or 0, 4)
    hook_fn(F_REL, lambda uc, esp, ecx: st["gate"], 4)
    hook_fn(F_OWNER, lambda uc, esp, ecx: OWNER, 0)

    def lister(uc, esp, ecx):
        n = st["n"]
        for i in range(n):
            wr(ARR + 8 * i, UNITS + 0x40 * i)
            wr(ARR + 8 * i + 4, AUX + 0x40 * i)
        lst = rd(esp + 4)
        wr(lst + 8, n)
        wr(lst + 0xC, ARR)
        return 0

    hook_fn(F_LIST, lister, 8)
    hook_fn(0x6F6EB558, lambda uc, esp, ecx: 0, 16)   # Storm SMemFree(ptr, name, line, flags)

    def record(uc, esp, ecx):
        amt_ptr, cls, att, flags, zero = (rd(esp + 4 + 4 * i) for i in range(5))
        st["rec"] = (rd(amt_ptr), cls, flags)
        return ecx

    hook_fn(F_RECORD, record, 20)

    s_dmg = stub_slot()

    def damage(uc, esp, ecx):
        idx = (ecx - UNITS) // 0x40
        delivered.append((idx,) + st["rec"])
        return 0

    hook_fn(s_dmg, damage, 16)
    s_own = stub_slot()
    hook_fn(s_own, lambda uc, esp, ecx: 0x5555, 0)

    mu.mem_write(UVT, b"\0" * 0x400)
    wr(UVT + 0x120, s_dmg)
    mu.mem_write(OVT, b"\0" * 0x400)
    wr(OVT + 0xEC, s_own)

    rows = []
    for _ in range(cases):
        amount = r.choice((r.uniform(1, 500), float(r.randrange(1, 300)), 100.0, 0.0))
        frac = r.choice((0.5, 0.5, r.random(), r.uniform(0, 1), 0.0, 1.0, 0.25))
        n = r.randrange(0, 7)
        gate = int(r.random() > 0.15)
        rflags = r.choice((0, 0, 0x10, 0x40))
        rclass = r.choice((0x20, 0x20, 0x1000000, 0x100))
        st.update(gate=gate, n=n, rec=None)
        del delivered[:]
        mu.mem_write(BUFF, b"\0" * 0x400)
        wr(BUFF, VT_BUFF)
        wr(BUFF + 0x30, OWNER)
        wr(BUFF + 0xD0, f2u(frac))
        mu.mem_write(OWNER, b"\0" * 0x100)
        wr(OWNER, OVT)
        for i in range(8):
            mu.mem_write(UNITS + 0x40 * i, b"\0" * 0x40)
            wr(UNITS + 0x40 * i, UVT)
            mu.mem_write(AUX + 0x40 * i, b"\0" * 0x40)
        mu.mem_write(REC, b"\0" * 0x40)
        wr(REC + 4, 9)
        wr(REC + 0xC, rflags)
        wr(REC + 0x10, f2u(amount))
        wr(REC + 0x14, rclass)
        wr(REC + 0x18, f2u(amount * 0.5))
        ring = []
        from unicorn import UC_HOOK_CODE
        hk = mu.hook_add(UC_HOOK_CODE, lambda uc, a, sz, ud: (ring.append(a), ring.__delitem__(0) if len(ring) > 30 else None) and None)
        try:
            emu.call(F_HANDLER, ecx=BUFF, stack_args=(REC,))
        except Exception as e:
            from unicorn.x86_const import UC_X86_REG_EDX, UC_X86_REG_EDI
            raise SystemExit("handler failed: %s edx=%#x edi=%#x n=%d trail=%s" % (e, mu.reg_read(UC_X86_REG_EDX), mu.reg_read(UC_X86_REG_EDI), n, [hex(x) for x in ring[-8:]]))
        finally:
            mu.hook_del(hk)
        rows.append((f2u(amount), f2u(frac), n, gate, rflags, rclass, rd(REC + 0x10), list(delivered)))
    return rows


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump = args[1]
        args = args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 3
    rows = run(cases, random.Random(seed))
    for amt, fr, n, gate, rf, rc, after, dl in rows[:12]:
        print("amt=%g frac=%g n=%d gate=%d flags=%x class=%x -> rec=%g shares=%s" % (
            u2f(amt), u2f(fr), n, gate, rf, rc, u2f(after), [(d[0], round(u2f(d[1]), 3), hex(d[2]), hex(d[3])) for d in dl]))
    if dump:
        os.makedirs(dump, exist_ok=True)
        with open(os.path.join(dump, "spiritlink.txt"), "w") as fh:
            for amt, fr, n, gate, rf, rc, after, dl in rows:
                fh.write("%08x %08x %d %d %x %x | %08x %d %s\n" % (
                    amt, fr, n, gate, rf, rc, after, len(dl),
                    " ".join("%d:%08x:%x:%x" % d for d in dl)))
    print("cases", len(rows))


if __name__ == "__main__":
    main()
