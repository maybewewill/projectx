"""Unicorn probe of the base percent setter CAbilityBaseBuild vf191 (Game.dll 1.26a, docs/spec/construction.md 12.7).

`0x6F0239B0(this, p)` (ECX = this, one stack argument p, thiscall, ret 4) clamps p to 0..100 and writes a float
through the agile float property at this+0x7C (`0x6F477350`, ECX = this+0x7C, stack = pointer to the value).
CAbilityTrain does not override slot 191, so it runs this setter too (vtables.tsv).

The probe records the float written to this+0x7C for several this+0x78 values and percents. The callee
`0x6F477350` is replaced by a recorder (it reads the agent table, not under test).

Usage:  GAME_DLL=/path/Game.dll python3 tools/emu/base_progress_emu.py [--vectors out.tsv]
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402

mu = emu.mu
DATA = emu.DATA

SETTER = 0x6F0239B0
PROP_SET = 0x6F477350
OBJ = DATA + 0x8000
LOG = DATA + 0x8100
STUB = DATA + 0x8200
RET = DATA + 0x8300


def f2b(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def b2f(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def setup():
    # Runtime constants written by the start-up initialisers (tools/emu/runtime_consts.py).
    mu.mem_write(0x6FAAE4A0, struct.pack("<I", 0x3DCCCCCD))  # 0.1
    mu.mem_write(0x6FAAE4F4, struct.pack("<I", 0x3F800000))  # 1.0
    mu.mem_write(0x6FAAE574, struct.pack("<I", 0x42C80000))  # 100.0
    # Recorder for the property setter: log ECX and the value pointed to by [esp+4]; ret 4.
    code = b""
    code += b"\x89\x0D" + struct.pack("<I", LOG)                    # mov [LOG], ecx
    code += b"\x8B\x44\x24\x04"                                      # mov eax, [esp+4]
    code += b"\xA3" + struct.pack("<I", LOG + 4)                     # mov [LOG+4], eax
    code += b"\x8B\x00"                                              # mov eax, [eax]
    code += b"\xA3" + struct.pack("<I", LOG + 8)                     # mov [LOG+8], eax
    code += b"\x31\xC0"                                              # xor eax, eax
    code += b"\xC2\x04\x00"                                          # ret 4
    mu.mem_write(STUB, code)
    mu.mem_write(PROP_SET, b"\xE9" + struct.pack("<i", STUB - (PROP_SET + 5)))
    mu.mem_write(RET, b"\xC3")


def run(v, p):
    mu.mem_write(LOG, b"\0" * 16)
    mu.mem_write(OBJ, b"\0" * 0x100)
    mu.mem_write(OBJ + 0x78, struct.pack("<I", f2b(v)))
    emu.call(SETTER, ecx=OBJ, stack_args=[p])
    ecx, ptr, val = struct.unpack("<III", bytes(mu.mem_read(LOG, 12)))
    return {
        "v": f2b(v),
        "p": p,
        "ecx": ecx,
        "written": val,
        "ecx_is_obj_7c": ecx == OBJ + 0x7C,
    }


def main():
    setup()
    rows = []
    if "--vectors" in sys.argv:
        rnd = random.Random(7)
        vals = [1000.0, 0.0, 1.0, 37.5, -250.0, 12345.678, 60.0, 1.0e-30]
        vals += [struct.unpack("<f", struct.pack("<I", rnd.getrandbits(32) & 0x7F7FFFFF))[0] for _ in range(52)]
        for v in vals:
            for p in list(range(0, 101)) + [-1, 150]:
                rows.append(run(v, p))
    else:
        for v in (1000.0, 0.0, 1.0, 37.5, -250.0, 12345.678):
            for p in (0, 1, 25, 50, 99, 100, 150):
                rows.append(run(v, p))
    if "--vectors" in sys.argv:
        path = sys.argv[sys.argv.index("--vectors") + 1]
        with open(path, "w") as fh:
            fh.write("# v_bits\tp\twritten_bits (p is the signed argument; the setter clamps it as unsigned)\n")
            for r in rows:
                fh.write("%08x\t%d\t%08x\n" % (r["v"], r["p"], r["written"]))
        print("vectors:", len(rows))
    bad = [r for r in rows if not r["ecx_is_obj_7c"]]
    print("rows:", len(rows), "ecx mismatches:", len(bad))
    if "--vectors" not in sys.argv:
        for r in rows:
            print("v=%10g p=%3d -> %10g (ecx ok=%s)" % (b2f(r["v"]), r["p"], b2f(r["written"]), r["ecx_is_obj_7c"]))


if __name__ == "__main__":
    main()
