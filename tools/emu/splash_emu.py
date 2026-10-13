"""Unicorn probe of the splash attack (msplash, Game.dll 1.26a): the weapon splash parameters, the per-candidate
filter and the ring selection of CMissileSplash.

Real code that runs:
  0x6F0C5510  copies the weapon's splash object (A+0x238+4w) into the launch ring spec
  0x6F2BF020  splash candidate filter (immunity gate, then 0x6F25D6F0 -> 0x6F25D280 / 0x6F25D390 / 0x6F25D560)
  0x6F2BF160  CMissileSplash slot 51: the ring test and the damage factor of one candidate
Stubbed (recorded or table driven): the alliance row tests 0x6F3A3400 / 0x6F3A36D0, the Misc flag reader 0x6F009F90,
the missile source list walker 0x6F4786B0, the handle holder 0x6F038450, the ring membership test 0x6F476C40, the damage
record builder 0x6F2BE8B0 and the owner lookup 0x6F282F60.

Usage: GAME_DLL=... py tools/emu/splash_emu.py [cases] [--rs out_dir]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_ECX  # noqa: E402

mu = emu.mu
EXTRA = 0x00400000
mu.mem_map(EXTRA, 0x10000)
M = EXTRA + 0x4000          # the missile
U = EXTRA + 0x2000          # the candidate unit
SRC = EXTRA + 0x3000        # the source unit object
AUX, OUTB, FOUT = EXTRA + 0x6000, EXTRA + 0x6400, EXTRA + 0x6800
VT, VTM, CODE = EXTRA + 0x100, EXTRA + 0x200, EXTRA + 0x300
TOK = EXTRA + 0x7000        # position tokens, 0x10 apart
ALLY, SHARED, SRC_PTR = 0x6F3A3400, 0x6F3A36D0, 0x6F4786B0
MISC, HOLD, MEMBER, RECORD, OWNER = 0x6F009F90, 0x6F038450, 0x6F476C40, 0x6F2BE8B0, 0x6F282F60
F_SPEC, F_FILTER, F_SLOT51 = 0x6F0C5510, 0x6F2BF020, 0x6F2BF160
ST = {"pass": 0, "shared": 0, "misc": 0, "src": 0, "a": 0, "b": 0, "calls": [], "amount_nonzero": 1, "ma": 0, "mb": 0}


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def ret_to(pop, eax=None):
    esp = mu.reg_read(UC_X86_REG_ESP)
    if eax is not None:
        mu.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
    mu.reg_write(UC_X86_REG_EIP, r32(esp))
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + pop)


def hook(uc, eip, size, user):
    if eip == ALLY or eip == SHARED:
        esp = mu.reg_read(UC_X86_REG_ESP)
        a, b = r32(esp + 4), r32(esp + 8)
        mask = ST["pass"] if eip == ALLY else ST["shared"]
        # a deterministic stand-in for the alliance rows: bit (a*7 + b*3) mod 16 of the case mask
        bit = (mask >> ((a * 7 + b * 3) & 15)) & 1
        ret_to(8, bit)
    elif eip == MISC:
        ret_to(4, ST["misc"])
    elif eip == SRC_PTR:
        mu.reg_write(UC_X86_REG_EAX, ST["src"])
        ret_to(0)
    elif eip == HOLD:
        esp = mu.reg_read(UC_X86_REG_ESP)
        ecx = mu.reg_read(UC_X86_REG_ECX)
        w32(ecx, r32(esp + 4))
        ret_to(4, ecx)
    elif eip == MEMBER:
        esp = mu.reg_read(UC_X86_REG_ESP)
        bits = r32(r32(esp + 4))
        ST.setdefault("tests", []).append(bits)
        if bits == ST["a"]:
            res = ST["ma"]
        elif bits == ST["b"]:
            res = ST["mb"]
        else:
            res = 0xBAD
        ret_to(0x10, res)
    elif eip == RECORD:
        esp = mu.reg_read(UC_X86_REG_ESP)
        unit, fptr = r32(esp + 8), r32(esp + 0xC)
        ST["calls"].append((unit, r32(fptr)))
        w32(FOUT, 0x3F800000 if ST["amount_nonzero"] else 0)
        ret_to(0xC, FOUT)
    elif eip == OWNER:
        ret_to(0, 0)


def patch_seh():
    """The image has no FS base: fs:[0] accesses become absolute accesses to one cell (as in abilities4_emu.py)."""
    tib = EXTRA + 0xF000
    for pat, rep in (
        (bytes.fromhex("64a100000000"), bytes.fromhex("8b05") + struct.pack("<I", tib)),
        (bytes.fromhex("64a300000000"), bytes.fromhex("8905") + struct.pack("<I", tib)),
        (bytes.fromhex("64890d00000000"), bytes.fromhex("890d") + struct.pack("<I", tib) + bytes.fromhex("90")),
    ):
        i = 0
        while True:
            i = emu.img.find(pat, i)
            if i < 0:
                break
            mu.mem_write(emu.BASE + i, rep)
            i += len(pat)


patch_seh()
mu.hook_add(UC_HOOK_CODE, hook)
mu.mem_write(CODE, b"\xB8\x75\x33\x77\x2B\xC3")                  # tag stub
mu.mem_write(CODE + 0x10, b"\x8B\x81\xF4\x03\x00\x00\xC3")        # mov eax,[ecx+0x3f4]; ret   (owner)
mu.mem_write(CODE + 0x20, b"\x8B\x81\xF0\x03\x00\x00\xC3")        # mov eax,[ecx+0x3f0]; ret   (invulnerable)
mu.mem_write(CODE + 0x30, b"\xC2\x04\x00")                        # ret 4 (position getter of the missile)
mu.mem_write(CODE + 0x40, b"\xB8\x00\x70\x40\x00\xC3")            # mov eax, TOK; ret (unit position object)
w32(VT + 0x1C, CODE)
w32(VT + 0xEC, CODE + 0x10)
w32(VT + 0x11C, CODE + 0x20)
w32(VTM + 0xA0, CODE + 0x30)
w32(0x6FA4F6E8, 1)


def setup_unit(c):
    mu.mem_write(U, b"\0" * 0x400)
    w32(U, VT)
    w32(U + 4, 50)
    w32(U + 0x5C, c["u5c"])
    w32(U + 0x20, c["u20"])
    w32(U + 0x24C, c["u24c"])
    mu.mem_write(U + 0x33, bytes([c["u33"]]))
    w32(U + 0x248, c["u248"])
    w32(U + 0x158, c["eth"])
    w32(U + 0x15C, c["mim"])
    w32(U + 0x3F0, c["inv"])
    w32(U + 0x3F4, c["towner"])
    w32(U + 0x1DC, 0xFFFFFFFF)
    w32(U + 0x1E0, 0xFFFFFFFF)
    mu.mem_write(SRC, b"\0" * 0x400)
    w32(SRC, VT)
    w32(SRC + 0x3F4, c["sowner"])


def run(kind, c):
    if kind == "spec":
        mu.mem_write(AUX, b"\0" * 0x80)
        for i, off in enumerate((0x20, 0x28, 0x30, 0x38, 0x40, 0x48)):
            w32(AUX + off, c["aux"][i])
        mu.mem_write(OUTB, b"\xEE" * 0x40)
        emu.call(F_SPEC, ecx=0x1234, stack_args=(OUTB, AUX if c["has_aux"] else 0))
        return [r32(OUTB + o) for o in (0x00, 0x04, 0x08, 0x0C, 0x10, 0x14)]
    if kind == "filter":
        setup_unit(c)
        mu.mem_write(M, b"\0" * 0x400)
        w32(M + 0x5C, c["atk"])
        w32(M + 0xB0, c["w"])
        w32(M + 0xAC, c["sowner"])
        pair = 0 if c["src"] == 0 else 1
        w32(M + 0x2C, 0xFFFFFFFF if pair == 0 else 1)
        w32(M + 0x30, 0xFFFFFFFF if pair == 0 else 1)
        ST.update(pass_=0, misc=c["misc"], src={0: 0, 1: SRC, 2: U}[c["src"]])
        ST["pass"] = c["pass"]
        ST["shared"] = c["shared"]
        return [emu.call(F_FILTER, ecx=M, stack_args=(U,))]
    if kind == "ring":
        mu.mem_write(M, b"\0" * 0x400)
        w32(M, VTM)
        for i in range(2):
            w32(TOK + 0x10 * i, 0)
        w32(M + 0xC8, c["a"])
        w32(M + 0xCC, 0)
        w32(M + 0xD0, c["b"])
        w32(M + 0xD4, 0)
        w32(M + 0xD8, c["c"])
        w32(M + 0xB8, c["hf"])
        w32(M + 0xC0, c["qf"])
        setup_unit(c)
        ST.update(a=c["a"], b=c["b"], ma=c["ma"], mb=c["mb"], calls=[], tests=[], amount_nonzero=c["nz"])
        # the candidate's vtable slot 0xb8 returns the position token
        w32(VT + 0xB8, CODE + 0x40)
        emu.call(F_SLOT51, ecx=M, stack_args=(U, OUTB))
        calls = ST["calls"]
        return [len(calls), calls[0][1] if calls else 0xFFFFFFFF, len(ST["tests"])]
    raise ValueError(kind)


def fb(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def rf(rng):
    return rng.choice([0, 0x80000000, fb(1.0), fb(0.5), fb(0.25), fb(50.0), fb(100.0), fb(150.0), fb(-1.0), fb(0.0005),
                       fb(300.0), fb(32.0), 0x7FC00000]) if rng.random() < 0.6 else fb(rng.uniform(0.0, 400.0))


def gen(rng, kind):
    c = {}
    c["aux"] = [rng.getrandbits(32)] + [rf(rng) for _ in range(5)]
    c["has_aux"] = rng.random() < 0.85
    c["u5c"] = rng.choice([0, 0x10, 0x100, 0x200])
    c["u20"] = rng.choice([0, 8, 0x10, 0x18])
    c["u24c"] = rng.choice([0x2, 0x4, 0x8, 0x10, 0x6, 0xA])
    c["u33"] = rng.choice([0x41, 0x5A, 0x61, 0x68, 0x30])
    c["u248"] = rng.getrandbits(12)
    c["eth"] = rng.choice([0, 0, 1, 2, 0xFFFFFFFF])
    c["mim"] = rng.choice([0, 0, 1, 3, 0xFFFFFFFF])
    c["inv"] = rng.choice([0, 0, 1])
    c["towner"] = rng.randrange(0, 16)
    c["sowner"] = rng.choice([rng.randrange(0, 16), rng.randrange(0, 16), 0xFFFFFFFF])
    c["atk"] = rng.randrange(0, 8)
    w = 0
    for _ in range(rng.choice([0, 1, 1, 2, 3])):
        w |= rng.choice([0x2, 0x4, 0x8, 0x10, 0x1000, 0x2000, 0x4000, 0x8000, 0x10000, 0x1E000, 0x400000, 0x800000,
                         0x4000000, 0x8000000, 0x40000000, 0x80000000, 0x100000, 0x200000, 0x800, 0x1000000])
    c["w"] = w
    c["misc"] = rng.choice([0, 1])
    c["src"] = rng.choice([0, 1, 1, 2])
    c["pass"] = rng.getrandbits(16)
    c["shared"] = rng.getrandbits(16)
    c["a"] = rf(rng)
    c["b"] = rf(rng)
    while c["b"] == c["a"]:
        c["b"] = rf(rng)
    c["c"] = rf(rng)
    c["hf"] = rf(rng)
    c["qf"] = rf(rng)
    c["ma"] = rng.choice([0, 1])
    c["mb"] = rng.choice([0, 1])
    c["nz"] = rng.choice([0, 1])
    return c


def row(kind, c, out):
    if kind == "spec":
        head = [int(c["has_aux"])] + c["aux"]
    elif kind == "filter":
        head = [c["u5c"], c["u20"], c["u24c"], c["u33"], c["u248"], c["eth"], c["mim"], c["inv"], c["towner"],
                c["sowner"], c["atk"], c["w"], c["misc"], c["src"], c["pass"], c["shared"]]
    else:
        head = [c["a"], c["b"], c["c"], c["hf"], c["qf"], c["ma"], c["mb"], c["nz"]]
    return head + out


def main():
    n = 500
    out_dir = None
    args = sys.argv[1:]
    if args and args[0].isdigit():
        n = int(args.pop(0))
    if args and args[0] == "--rs":
        out_dir = args[1]
    rng = random.Random(20261015)
    total = 0
    for kind in ("spec", "filter", "ring"):
        rows = []
        for _ in range(n):
            c = gen(rng, kind)
            rows.append(row(kind, c, run(kind, c)))
        total += len(rows)
        print(kind, len(rows), "rows")
        if out_dir:
            width = len(rows[0])
            lines = [
                "// Generated by tools/emu/splash_emu.py from the original Game.dll; do not edit.",
                f"pub const ROWS: &[[u32; {width}]] = &[",
            ]
            lines += ["    [" + ", ".join("0x%08X" % v for v in r) + "]," for r in rows]
            lines.append("];")
            with open(os.path.join(out_dir, f"splash_{kind}.rs"), "w", newline="\n") as fh:
                fh.write("\n".join(lines) + "\n")
    print("total", total)


if __name__ == "__main__":
    main()
