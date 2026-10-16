"""V7 Unicorn runs of the init slots (vtable 0x80) of Gold Mine (Agld), Harvest Lumber (Ahrl) and Harvest (Ahar) of Game.dll 1.26a.

These init slots read their level-1 numbers BY NAME from the ability's data row: `0x6F01F7D0` = "Area1", `0x6F01FA50` = "DataA1",
`0x6F01FAD0` = "DataB1", `0x6F01FB50` = "DataC1" (the format strings come from the cells 0x6FA6F278 / 294 / 298 / 29C: "Area",
"DataA", "DataB", "DataC"); the row lookup `0x6F262760` is the stub of this harness (it serves the case's values). The real getters,
the real soft-float f2i `0x6F6EEC20` and the real init bodies run; stubs: the base init `0x6F082550` / `0x6F052B10`, the property
holders (a vtable whose slot 0 stores the pointee of its first argument at holder+4), the Misc reader `0x6F009EE0` and the typed-agent
lookups of the mine's slot loop.

Row (hex words):
  GM <DataA> <DataB> <DataC> <misc> | <+0x6C> <holder0x84 value> <+0x8C> <+0xAC> <holder0xB0 value>
  HL <Area> <DataA> <DataB> <unit-has-attack> <+0x16C(364)> <+400> | <holder 396> <+472> <+408> <holder 456> <holder 464> <flags>
  HV <Area> <DataA> <DataB> <DataC> <unit-has-attack> | <holder 396> <+472> <+408> <+404> <flags>

Usage: GAME_DLL=/path/Game.dll py tools/emu/v7_harvest_emu.py [--dump OUT_FILE] [CASES] [SEED]
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
wr, rd, f2u = A.wr, A.rd, A.f2u
M = S.M
THIS, ROW, UNIT, HVT, ZMEM, UVT = M + 0x4000, M + 0x4800, M + 0x5000, M + 0x5800, M + 0x5C00, M + 0x6000
st = {}


def on_row(esp, ecx, edx):
    """thiscall 0x6F262760(ecx = row, out, name): stores the case value for the field name."""
    out, name = rd(esp + 4), rd(esp + 8)
    s = bytes(mu.mem_read(name, 12)).split(b"\0")[0].decode()
    wr(out, st["fields"].get(s, 0))
    return out


on_row.pop = 8


def on_snprintf(esp, ecx, edx):
    """cdecl _snprintf(buf, count, "%s%d", string, int): the only format the field-name builders use."""
    buf, count, fmt, s_ptr, num = (rd(esp + 4 + 4 * i) for i in range(5))
    assert bytes(mu.mem_read(fmt, 5)) == b"%s%d\0", "unexpected format"
    s = bytes(mu.mem_read(s_ptr, 32)).split(b"\0")[0].decode()
    text = ("%s%d" % (s, num - (1 << 32) if num >= 1 << 31 else num)).encode()[:count - 1] + b"\0"
    mu.mem_write(buf, text)
    return len(text) - 1


on_snprintf.pop = 0


def on_misc_int(esp, ecx, edx):
    return st["misc"]


on_misc_int.pop = 8


def install():
    S.install()
    A.patch_seh()
    H.hook(0x6F262760, on_row)
    H.hook(0x6F6EB5A6, on_snprintf)
    H.hook(0x6F009EE0, on_misc_int)
    for fn, pop in ((0x6F082550, 8), (0x6F052B10, 8), (0x6F0A5230, 4)):
        A.patch_jump(fn, A.stub_ret(pop, 0))
    A.patch_jump(0x6F0823F0, A.stub_ret(0, 0))
    for fn, pop in ((0x6F4C8520, 0), (0x6F001EC0, 8), (0x6F48AB80, 4)):
        A.patch_jump(fn, A.stub_ret(pop, ZMEM))
    A.patch_jump(0x6F472C20, A.stub_ret(4, 0))
    mu.mem_write(UVT, b"\0" * 0x40)
    wr(UVT + 8, A.stub_ret(12, 0))
    A.patch_jump(0x6F471910, A.stub_ret(0, 0))
    mu.mem_write(HVT, b"\0" * 0x40)
    wr(HVT, S.new_code(b"\x8B\x44\x24\x04\x8B\x00\x89\x41\x04\xC2\x08\x00"))


def holder(base, off):
    wr(base + off, HVT)


def run_gm(r):
    a, b = (S.rbits(r) for _ in range(2))
    # DataC is the slot count; a negative or NaN value would loop ~2^31 times in the original, so only sane values are drawn
    c = f2u(r.choice([0.0, -0.0, 1.0, 5.0, 12.5, 31.9, 32.0, 33.0, 100.0, 3.5, r.uniform(0.0, 40.0)]))
    misc = r.choice([0, 1, 5, 0xFFFFFFFF])
    st["fields"] = {"DataA1": a, "DataB1": b, "DataC1": c}
    st["misc"] = misc
    mu.mem_write(THIS, b"\0" * 0x400)
    mu.mem_write(ROW, b"\0" * 0x100)
    for o in (0x84, 0xB0):
        holder(THIS, o)
        wr(THIS + o + 4, 0xDEAD)
    wr(THIS + 0xE4, 0)
    mu.mem_write(ZMEM, bytes(0x200))
    wr(THIS + 0xEC, ZMEM)
    S.call(0x6F0ACB00, ecx=THIS, stack_args=(THIS, ROW))
    return "GM %08x %08x %08x %x | %08x %08x %x %x %08x" % (a, b, c, misc, rd(THIS + 0x6C), rd(THIS + 0x88), rd(THIS + 0x8C),
                                                          rd(THIS + 0xAC), rd(THIS + 0xB4))


def run_hl(r, harvest):
    area, a, b, c = (S.rbits(r) for _ in range(4))
    attack = r.random() < 0.6
    u364, u400 = S.rbits(r), S.rbits(r)
    st["fields"] = {"Area1": area, "DataA1": a, "DataB1": b, "DataC1": c}
    mu.mem_write(THIS, b"\0" * 0x400)
    mu.mem_write(UNIT, b"\0" * 0x600)
    mu.mem_write(ROW, b"\0" * 0x100)
    wr(THIS + 0x30, UNIT)
    wr(UNIT, UVT)
    wr(UNIT + 0x1E8, UNIT + 0x100 if attack else 0)
    wr(UNIT + 0x100 + 364, u364)
    wr(UNIT + 0x100 + 400, u400)
    for o in (396, 456, 464):
        holder(THIS, o)
        wr(THIS + o + 4, 0xDEAD)
    if harvest:
        S.call(0x6F055A20, ecx=THIS, stack_args=(THIS, ROW))
        return "HV %08x %08x %08x %08x %d | %08x %08x %08x %08x %x" % (
            area, a, b, c, attack, rd(THIS + 400), rd(THIS + 472), rd(THIS + 408), rd(THIS + 404), rd(THIS + 0x20))
    S.call(0x6F0557D0, ecx=THIS, stack_args=(THIS, ROW))
    return "HL %08x %08x %08x %d %08x %08x | %08x %08x %08x %08x %08x %x" % (
        area, a, b, attack, u364, u400, rd(THIS + 400), rd(THIS + 472), rd(THIS + 408), rd(THIS + 460), rd(THIS + 468),
        rd(THIS + 0x20))


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 300
    seed = int(args[1]) if len(args) > 1 else 7
    r = random.Random(seed)
    install()
    rows = []
    for _ in range(cases):
        rows.append(run_gm(r))
        rows.append(run_hl(r, False))
        rows.append(run_hl(r, True))
    for row in rows[:6]:
        print(row)
    if dump:
        with open(dump, "w") as fh:
            fh.write("\n".join(rows) + "\n")
    print("cases", len(rows))


if __name__ == "__main__":
    main()
