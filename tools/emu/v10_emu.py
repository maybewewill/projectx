"""Unicorn checks of the V10 verification wave (docs/spec/abilities-v10.md): melee-critical abilities.

The ORIGINAL routines of Game.dll run on fake objects (the pattern of v9_emu.py); only world-dependent callees are stubs that
read per-case switches or append to a call log. Rows are dumped as hex words and re-computed by crates/sim/tests/v10_*.rs.

Usage: GAME_DLL=/path/Game.dll py tools/emu/v10_emu.py [--dump DIR] --only NAME [CASES] [SEED]
(each check patches functions of the image for good, so run one check per process.)

Call log: every logging stub appends 5 dwords (kind, ecx, v1, v2, v3) at LOG; the log pointer lives at LOGP.
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import spell_buff_emu as S  # noqa: E402
import v4_spells_emu as V4  # noqa: E402
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402

mu = emu.mu
wr, rd, f2u = A.wr, A.rd, A.f2u
a32, new_code, level, rbits, u32 = S.a32, S.new_code, S.level, S.rbits, S.u32
CAP = S.CAP

M2 = 0x00500000
mu.mem_map(M2, 0x20000)
LOGP = M2 + 0x0FF0
LOG = M2 + 0x10000          # log entries (5 dwords each)
LOG_MAX = 400
VARS2 = M2 + 0x0F00         # per-case switches (misc flags, ...)
ARGS = M2 + 0x0E00          # copied arguments of the group builder
RINIT = M2 + 0x0E80         # record at entry of the group builder
RSNAP = M2 + 0x0EC0         # record after the callbacks
UNITLIST = M2 + 0x0D00      # zero-terminated pointers to the fake units
UNITS = M2 + 0x4000         # fake units 0x400 each
VTUNIT = M2 + 0x9000
VTOWNER = M2 + 0x9200
VTABIL = M2 + 0x8000        # copy of the ability vtable
VTBUFF = M2 + 0x7000
VTCOOL = M2 + 0x7400
BUFFS = M2 + 0x7500         # new buff objects (0x20 each)
EBUFF = M2 + 0x7600         # the pre-existing buff of a unit


def reset_log():
    wr(LOGP, LOG)
    mu.mem_write(LOG, b"\0" * (LOG_MAX * 20))


def read_log():
    end = rd(LOGP)
    n = (end - LOG) // 20
    assert n <= LOG_MAX
    out = []
    for i in range(n):
        out += [rd(LOG + 20 * i + 4 * k) for k in range(5)]
    return n, out


def logstub(kind, args=(), retn=0, ret_ecx_off=None, ret_val=0, tail=None):
    """A stub appending (kind, ecx, v1, v2, v3) to the log. args: list of (argidx, deref) for v1..v3. The return value is
    [ecx + ret_ecx_off] when given, else ret_val. The callee pops retn bytes."""
    code = b"\xA1" + a32(LOGP)                                  # mov eax,[LOGP]
    code += b"\xC7\x00" + a32(kind)                             # mov dword [eax], kind
    code += b"\x89\x48\x04"                                     # mov [eax+4], ecx
    for i, (idx, deref) in enumerate(args):
        code += b"\x8B\x54\x24" + bytes([4 + 4 * idx])          # mov edx,[esp+4+4*idx]
        if deref:
            code += b"\x8B\x12"                                 # mov edx,[edx]
        code += b"\x89\x50" + bytes([8 + 4 * i])                # mov [eax+8+4i], edx
    code += b"\x83\x05" + a32(LOGP) + b"\x14"                   # add dword [LOGP], 0x14
    if tail is not None:
        return new_code(code + tail)                            # the caller supplies the return code
    if ret_ecx_off is not None:
        code += b"\x8B\x81" + a32(ret_ecx_off)                  # mov eax,[ecx+off]
    else:
        code += b"\xB8" + a32(ret_val)
    code += (b"\xC2" + struct.pack("<H", retn)) if retn else b"\xC3"
    return new_code(code)


def group_stub():
    """The group builder 0x6F2A3270 (cdecl, 11 arguments): copies the arguments, snapshots the record before and after, and
    calls the callback (ecx = unit, edx = record) for every unit of UNITLIST until one returns 0."""
    code = b"\x53\x56\x57"                                          # push ebx, esi, edi
    code += b"\xB9" + a32(11) + b"\x8D\x74\x24\x10" + b"\xBF" + a32(ARGS) + b"\xF3\xA5"       # copy 11 args
    code += b"\x8B\x5C\x24\x10"                                     # mov ebx,[esp+0x10]   callback
    code += b"\x8B\x74\x24\x14" + b"\xBF" + a32(RINIT) + b"\xB9" + a32(12) + b"\xF3\xA5"      # snapshot the record
    code += b"\x8B\x7C\x24\x14"                                     # mov edi,[esp+0x14]   record
    code += b"\xBE" + a32(UNITLIST)                                 # mov esi, UNITLIST
    code += (b"\x8B\x0E\x85\xC9\x74\x0D\x89\xFA\xFF\xD3\x85\xC0\x74\x05\x83\xC6\x04\xEB\xED")
    code += b"\x89\xFE" + b"\xBF" + a32(RSNAP) + b"\xB9" + a32(12) + b"\xF3\xA5"             # snapshot after
    code += b"\x5F\x5E\x5B\xC3"
    return new_code(code)


VT_ARPB, VT_ARPL, VT_ARPM = 0x6F91AA14, 0x6F91AE9C, 0x6F91B324
CODE_ARP = {0: 0x41727062, 1: 0x4172706C, 2: 0x4172706D}          # 'Arpb', 'Arpl', 'Arpm'
VT_OF = {0: VT_ARPB, 1: VT_ARPL, 2: VT_ARPM}


def install_replenish():
    S.install()
    reset_log()
    A.patch_jump(0x6F4743A0, A.stub_ret(0, 0))                      # position normaliser: identity
    # agile mana property reads (ecx = unit + 0xB8): current at unit+0x308, maximum at unit+0x30C
    A.patch_jump(0x6F4773A0, new_code(b"\x8B\x44\x24\x04\x8B\x91" + a32(0x250) + b"\x89\x10\xC2\x04\x00"))
    A.patch_jump(0x6F4773E0, new_code(b"\x8B\x44\x24\x04\x8B\x91" + a32(0x254) + b"\x89\x10\xC2\x04\x00"))
    # hero test
    A.patch_jump(0x6F1D2660, new_code(b"\x8B\x81" + a32(0x100) + b"\xC3"))
    # buff lookup: log (id), return the unit's existing buff
    A.patch_jump(0x6F0787D0, logstub(1, [(0, False), (1, False), (2, False)], retn=0x14, ret_ecx_off=0x310))
    # buff creators
    for k, fn in enumerate((0x6F1F9580, 0x6F1F9650, 0x6F1F9720)):
        A.patch_jump(fn, logstub(2 + k, [], retn=0, ret_val=BUFFS + 0x20 * k))
    A.patch_jump(0x6F079CA0, logstub(5, [(0, False)], retn=4))        # register the buff on the unit
    A.patch_jump(0x6F052EC0, logstub(6, [(0, False)], retn=4))        # target effect
    A.patch_jump(0x6F053040, logstub(7, [(0, False)], retn=4))        # caster effect
    A.patch_jump(0x6F283DB0, logstub(8, [(1, True), (2, False)], retn=0xC))     # mana payment: cost value, flag
    A.patch_jump(0x6F277C20, logstub(9, [(1, True)], retn=8))         # life adder
    A.patch_jump(0x6F279140, logstub(10, [(1, True)], retn=8))        # mana adder
    # Misc readers
    mu.mem_write(VARS2, b"\0" * 0x100)
    code = (b"\x81\xFA" + a32(0x6F876B30) + b"\x74\x08\xA1" + a32(VARS2 + 0) + b"\xC2\x04\x00"
            + b"\xA1" + a32(VARS2 + 4) + b"\xC2\x04\x00")
    A.patch_jump(0x6F009F90, new_code(code))
    A.patch_jump(0x6F009EE0, new_code(b"\xA1" + a32(VARS2 + 8) + b"\x89\x01\x89\xC8\xC2\x08\x00"))
    # owner unit: slot 0xB8 writes the position (x at VARS2+0x10, y at +0x14) to the out pointer, returns it
    mu.mem_write(VTOWNER, b"\0" * 0x200)
    wr(VTOWNER + 0xB8, new_code(b"\x8B\x44\x24\x04\x8B\x0D" + a32(VARS2 + 0x10) + b"\x89\x08\x8B\x0D" + a32(VARS2 + 0x14)
                                + b"\x89\x48\x04\xC2\x04\x00"))
    # fake units
    mu.mem_write(VTUNIT, b"\0" * 0x200)
    wr(VTUNIT + 0x12C, new_code(b"\x8B\x44\x24\x04\x8B\x89" + a32(0x300) + b"\x89\x08\xC2\x04\x00"))
    wr(VTUNIT + 0x130, new_code(b"\x8B\x44\x24\x04\x8B\x89" + a32(0x304) + b"\x89\x08\xC2\x04\x00"))
    # buff objects: slot 0x324 (ability ref), 0x350 (refresh), 0x354 (apply), all logging
    mu.mem_write(VTBUFF, b"\0" * 0x400)
    wr(VTBUFF + 0x324, logstub(11, [(0, True)], retn=4))
    wr(VTBUFF + 0x350, logstub(12, [(0, True), (1, True)], retn=8))
    wr(VTBUFF + 0x354, logstub(13, [(0, False), (1, True), (2, False)], retn=0xC))
    for k in range(3):
        mu.mem_write(BUFFS + 0x20 * k, b"\0" * 0x20)
        wr(BUFFS + 0x20 * k, VTBUFF)
    mu.mem_write(EBUFF, b"\0" * 0x20)
    wr(EBUFF, VTBUFF)
    # the embedded cooldown object at ability + 0x90: slot 0 = start (value, flag)
    mu.mem_write(VTCOOL, b"\0" * 0x20)
    wr(VTCOOL, logstub(14, [(0, True), (1, False)], retn=8))


def make_replenish(variant, cast, dur, hdur, extra, cost, cool, area, da, db, de, extra_cool, targs):
    A.make_ability(VT_OF[variant], CODE_ARP[variant], cost)
    O = A.OBJ
    wr(A.LVL + 4, cast)
    wr(A.LVL + 0x14, cool)
    wr(A.LVL + 0x18, area)
    level(A.LVL, dur, hdur, [da, db, 0, 0, de, 0, 0, 0, 0])
    wr(O + 0x9C, extra)
    wr(O + 0x13C, extra_cool)
    wr(O + 0x4C, targs)
    wr(O + 0x30, A.UNIT)
    wr(A.UNIT, VTOWNER)
    # ability vtable copy with slot 0x3EC (target gate) = unit+0x314
    mu.mem_write(VTABIL, bytes(mu.mem_read(VT_OF[variant], 0x600)))
    wr(VTABIL + 0x3EC, new_code(b"\x8B\x44\x24\x04\x8B\x80" + a32(0x314) + b"\xC2\x04\x00"))
    wr(O, VTABIL)
    wr(O + 0x90, VTCOOL)


def rnd_amount(r):
    k = r.random()
    if k < 0.15:
        return 0
    if k < 0.2:
        return r.choice([0x80000000, 0x7FC00000, 0x7F800000, 1, 0xFF800000])
    if k < 0.85:
        return f2u(float(r.randint(1, 120)))
    return rbits(r)


def check_rep_effect(cases, r, rows):
    """Moon Well pulse: the effect slot 0x428 = 0x6F1FADC0 with its callback 0x6F1FAAA0 and the group builder replaced by a loop over
    prepared units (the real callback runs for each). Rows: variant cast dur hdur extra cost cool area DataA DataB DataE extra_cool
    targs misc_life misc_mana eth_bonus cx cy n | n x unit(hero life max mana maxmana illusion eth existing gate) |
    R init (12) | R final (12) | args (11) | log count | log entries."""
    install_replenish()
    gs = group_stub()
    A.patch_jump(0x6F2A3270, gs)
    for _ in range(cases):
        variant = r.choice([0, 1, 2])
        cast = r.choice([0, 0, 0, f2u(1.0), f2u(2.0), f2u(3.0), f2u(7.0), 0x80000000, rbits(r)])
        dur = rbits(r) if r.random() < 0.3 else f2u(float(r.choice([1, 1, 2, 5])))
        hdur = rbits(r) if r.random() < 0.3 else f2u(float(r.choice([1, 1, 3, 0])))
        extra = rbits(r) if r.random() < 0.3 else 0
        cost = r.choice([0, 2, 6, 6, 10, -3])
        cool = rbits(r) if r.random() < 0.2 else f2u(1.0)
        area = f2u(float(r.choice([700, 700, 100, 450])))
        da, db = rnd_amount(r), rnd_amount(r)
        de = r.choice([0, f2u(5.0), f2u(5.0), f2u(2.0), f2u(1.0), f2u(40.0), f2u(-2.0), rbits(r)])
        extra_cool = rbits(r) if r.random() < 0.3 else 0
        targs = r.getrandbits(32)
        make_replenish(variant, cast, dur, hdur, extra, cost, cool, area, da, db, de, extra_cool, targs)
        misc_life, misc_mana = int(r.random() < 0.3), int(r.random() < 0.3)
        eth_bonus = rbits(r) if r.random() < 0.3 else f2u(r.choice([0.0, 0.5, 1.0, 1.5, 2.0]))
        cx, cy = f2u(r.uniform(-3000, 3000)), f2u(r.uniform(-3000, 3000))
        wr(VARS2 + 0, misc_life)
        wr(VARS2 + 4, misc_mana)
        wr(VARS2 + 8, eth_bonus)
        wr(VARS2 + 0x10, cx)
        wr(VARS2 + 0x14, cy)
        n = r.randint(0, 6)
        unit_rows = []
        for k in range(n):
            u = UNITS + 0x400 * k
            mu.mem_write(u, b"\0" * 0x400)
            wr(u, VTUNIT)
            hero = int(r.random() < 0.3)
            if r.random() < 0.5:
                mx = f2u(float(r.randint(50, 900)))
                life = f2u(float(r.randint(0, 900)))
            else:
                mx, life = rbits(r), rbits(r)
            if r.random() < 0.5:
                mmx = f2u(float(r.randint(0, 600)))
                mana = f2u(float(r.randint(0, 600)))
            else:
                mmx, mana = rbits(r), rbits(r)
            if r.random() < 0.1:
                life = mx
            if r.random() < 0.1:
                mana = mmx
            illusion = int(r.random() < 0.25)
            eth = r.choice([0, 0, 0, 1, 2, 0xFFFFFFFF])
            existing = int(r.random() < 0.3)
            gate = r.choice([0, 0, 0, 0, 0x1C, 0x42])
            wr(u + 0x5C, 0x40000000 if illusion else r.choice([0, 0, 0x100]))
            wr(u + 0x100, hero)
            wr(u + 0x158, eth)
            wr(u + 0x300, life)
            wr(u + 0x304, mx)
            wr(u + 0x308, mana)
            wr(u + 0x30C, mmx)
            wr(u + 0x310, EBUFF if existing else 0)
            wr(u + 0x314, gate)
            wr(UNITLIST + 4 * k, u)
            unit_rows.append([hero, life, mx, mana, mmx, illusion, eth, existing, gate])
        wr(UNITLIST + 4 * n, 0)
        mu.mem_write(RINIT, b"\xEE" * 0x40)
        mu.mem_write(RSNAP, b"\xEE" * 0x40)
        reset_log()
        S.clear_cap()
        S.call(0x6F1FADC0, ecx=A.OBJ)
        cnt, log = read_log()
        flat = [x for ur in unit_rows for x in ur]

        def idx(v):
            # unit pointers in the log become their index so that rows do not depend on the layout
            if UNITS <= v < UNITS + 0x400 * 8:
                return (v - UNITS) // 0x400
            return v

        log2 = []
        for i in range(cnt):
            e = log[5 * i:5 * i + 5]
            log2 += [e[0], idx(e[1]), idx(e[2]), e[3], e[4]]
        rows.append([variant, cast, dur, hdur, extra, cost & 0xFFFFFFFF, cool, area, da, db, de, extra_cool, targs, misc_life, misc_mana,
                     eth_bonus, cx, cy, n] + flat + [rd(RINIT + 4 * i) for i in range(12)] + [rd(RSNAP + 4 * i) for i in range(12)]
                    + [rd(ARGS + 4 * i) for i in range(11)] + [cnt] + log2)


def check_rep_precheck(cases, r, rows):
    """The precheck value of the Moon Well, slot 0x374 = 0x6F1DBCC0 (ecx = ability, argument = out pointer). Rows: variant DataE cost |
    value."""
    install_replenish()
    for _ in range(cases):
        variant = r.choice([0, 1, 2])
        de = r.choice([0, f2u(5.0), f2u(5.0), f2u(2.0), f2u(40.0), f2u(-2.0), f2u(1e10), rbits(r)])
        cost = r.choice([0, 2, 6, 6, 10, -3, 1 << 30])
        make_replenish(variant, 0, 0, 0, 0, cost, 0, 0, 0, 0, de, 0, 0)
        out = M2 + 0x0F80
        wr(out, 0xDEADBEEF)
        res = S.call(0x6F1DBCC0, ecx=A.OBJ, stack_args=(out,))
        rows.append([variant, de, cost & 0xFFFFFFFF, rd(out), res])


CHECKS = {
    "rep_effect": check_rep_effect,
    "rep_precheck": check_rep_precheck,
}


def main():
    args = sys.argv[1:]
    dump = None
    only = None
    while args and args[0] in ("--dump", "--only"):
        if args[0] == "--dump":
            dump, args = args[1], args[2:]
        else:
            only, args = args[1].split(","), args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 101
    for name, fn in CHECKS.items():
        if only is None or name not in only:
            continue
        r = random.Random(seed)
        rows = []
        fn(cases, r, rows)
        print("%s rows %d" % (name, len(rows)))
        for row in rows[:3]:
            print(" ".join(("%08x" % x) if isinstance(x, int) else x for x in row))
        if dump:
            os.makedirs(dump, exist_ok=True)
            with open(os.path.join(dump, name + ".txt"), "w") as fh:
                for row in rows:
                    fh.write(" ".join(("%08x" % x) if isinstance(x, int) else x for x in row) + "\n")


if __name__ == "__main__":
    main()
