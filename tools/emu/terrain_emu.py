"""Unicorn check of the terrain queries of Game.dll 1.26a (S13): the cliff level 0x6F0128C0 (+ ramp-object list
0x6F0118C0, corner level 0x6F742820, tile mapping 0x6F73AFA0) and the ground height behind GetLocationZ
(0x6F3C0E50 -> 0x6F24F3E0 -> 0x6F0126F0 -> 0x6F763F00/0x6F763D50 with corner heights 0x6F7523C0, ramp helpers
0x6F750AB0/0x6F750860/0x6F74BAE0/0x6F74B920/0x6F747110/0x6F743C60, interpolation 0x6F741250; water 0x6F7527F0).

Real code that runs: all of the functions above. Stubbed: the two singleton getters 0x6F01F5A0 (terrain object) and
0x6F01F760 (ramp-object list), which return synthetic objects we build in memory. The terrain object holds the
grid of 28-byte corner records built exactly like the loader 0x6F756CA0 builds them from war3map.w3e (see
docs/spec/terrain.md).

Usage: GAME_DLL=... py tools/emu/terrain_emu.py [--w3e path] [--synthetic N] [--json out.json] [--rs out.rs]
"""
import json
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)

mu = emu.mu
EXTRA = 0x00400000
mu.mem_map(EXTRA, 0x400000)
T = EXTRA                 # terrain object
RAMPOBJ = EXTRA + 0x1000  # ramp-object list (16 bytes)
WTAB = EXTRA + 0x2000     # water table (0x17c per entry)
RAMPS = EXTRA + 0x3000    # 0x2C-byte ramp records
RAMPDATA = EXTRA + 0x8000  # int arrays of the ramp records
STUB = EXTRA + 0xF000
OUT = EXTRA + 0xF800
REC = EXTRA + 0x10000     # corner records

GET_TERRAIN, GET_RAMPS = 0x6F01F5A0, 0x6F01F760
LEVEL, GROUND, LOCZ = 0x6F0128C0, 0x6F763F00, 0x6F24F3E0


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def wf(a, v):
    mu.mem_write(a, struct.pack("<f", v))


BRANCHES = {0x6F750AF6: "S2", 0x6F750B10: "S1", 0x6F750B2A: "S0", 0x6F750B44: "R", 0x6F750B5E: "H"}
HITS = {}


def hook(uc, eip, size, user):
    if eip in BRANCHES:
        HITS[BRANCHES[eip]] = HITS.get(BRANCHES[eip], 0) + 1
    if eip == GET_TERRAIN:
        mu.reg_write(UC_X86_REG_EAX, T)
        esp = mu.reg_read(UC_X86_REG_ESP)
        mu.reg_write(UC_X86_REG_EIP, r32(esp))
        mu.reg_write(UC_X86_REG_ESP, esp + 4)
    elif eip == GET_RAMPS:
        mu.reg_write(UC_X86_REG_EAX, RAMPOBJ)
        esp = mu.reg_read(UC_X86_REG_ESP)
        mu.reg_write(UC_X86_REG_EIP, r32(esp))
        mu.reg_write(UC_X86_REG_ESP, esp + 4)


mu.hook_add(UC_HOOK_CODE, hook)

# stub: float_result = call GROUND(ptr, mode) with ecx = T; then fstp dword [OUT]
#   mov eax,[esp+4]; push dword [esp+8]; push eax; mov edx,GROUND; call edx; fstp dword [OUT]; ret
CODE_GROUND = bytes([0x8B, 0x44, 0x24, 0x04, 0xFF, 0x74, 0x24, 0x08, 0x50, 0xBA]) + struct.pack("<I", GROUND) + \
    bytes([0xFF, 0xD2, 0xD9, 0x1D]) + struct.pack("<I", OUT) + bytes([0xC3])
# GetLocationZ helper: 0x6F24F3E0(x, y, flag) with ecx = -1, edx = 0: args at [esp+4], [esp+8], [esp+0xC]
#   push [esp+0xC]; push [esp+0xC]; push [esp+0xC]; or ecx,-1; xor edx,edx; mov eax,LOCZ; call eax; fstp [OUT]; ret
CODE_LOCZ = bytes([0xFF, 0x74, 0x24, 0x0C, 0xFF, 0x74, 0x24, 0x0C, 0xFF, 0x74, 0x24, 0x0C,
                   0x83, 0xC9, 0xFF, 0x31, 0xD2, 0xB8]) + struct.pack("<I", LOCZ) + \
    bytes([0xFF, 0xD0, 0xD9, 0x1D]) + struct.pack("<I", OUT) + bytes([0xC3])
STUB_GROUND, STUB_LOCZ = STUB, STUB + 0x100
mu.mem_write(STUB_GROUND, CODE_GROUND)
mu.mem_write(STUB_LOCZ, CODE_LOCZ)


def set_fpu():
    # _control87(0x9001F): 53-bit precision, round to nearest, all exceptions masked (AGENTS.md 4.3)
    mu.reg_write(UC_X86_REG_FPCW, 0x027F)


# ---- w3e -----------------------------------------------------------------------------------------------------

class Corner:
    __slots__ = ("height", "water_word", "flags_tex", "variation", "misc")

    def __init__(self, height, water_word, flags_tex, variation, misc):
        self.height, self.water_word, self.flags_tex = height, water_word, flags_tex
        self.variation, self.misc = variation, misc


def parse_w3e(data):
    assert data[:4] == b"W3E!"
    ver, tileset, custom, nt = struct.unpack_from("<IBII", data, 4)
    p = 17 + 4 * nt
    (nc,) = struct.unpack_from("<I", data, p)
    p += 4 + 4 * nc
    w, h = struct.unpack_from("<II", data, p)
    ox, oy = struct.unpack_from("<ff", data, p + 8)
    p += 16
    pts = []
    for i in range(w * h):
        hh, ww, ft, var, misc = struct.unpack_from("<HHBBB", data, p + 7 * i)
        pts.append(Corner(hh, ww, ft, var, misc))
    return dict(w=w, h=h, ox=ox, oy=oy, pts=pts)


def build_records(g, water_offset=0.0, water_present=True, init_level=14):
    w, h = g["w"], g["h"]
    mem = bytearray()
    for i, c in enumerate(g["pts"]):
        x, y = i % w, i // w
        xi = int((x * 128.0 + g["ox"] + 32768.0) * 0.0078125)
        yi = int((y * 128.0 + g["oy"] + 32768.0) * 0.0078125)
        r0 = (c.height & 0x3FFF) | ((yi & 0x1FF) << 14) | ((xi & 0x1FF) << 23)
        r1 = (i & 0x3FFFF) | (((c.water_word << 18) & 0xFFFFFFFF))
        flags = 0
        if c.water_word & 0x4000:
            flags |= 0x80
        ft = c.flags_tex
        if ft & 0x10:
            flags |= 0x02
        if ft & 0x20:
            flags |= 0x04
        if ft & 0x40:
            flags |= 0x10
        if ft & 0x80:
            flags |= 0x40
        r2 = (0xE0 | (ft & 0xF)) | (c.variation << 8) | (c.misc << 16)
        mem += struct.pack("<IIIHHfff", r0, r1, r2, flags, 0, 0.0, 0.0, 1.0)
    return bytes(mem)


def load_terrain(g, water_offset=0.0, water_present=True, init_level=14, ramps=()):
    w, h = g["w"], g["h"]
    mu.mem_write(T, b"\0" * 0x1000)
    recs = build_records(g)
    mu.mem_write(REC, recs)
    w32(T + 0x00, 1)
    w32(T + 0xB4, w - 1)
    w32(T + 0xB8, h - 1)
    w32(T + 0xBC, 4 * (w - 1))
    w32(T + 0xC0, 4 * (h - 1))
    w32(T + 0xE0, w * h)
    w32(T + 0xE4, REC)
    w32(T + 0xF0, 16)
    w32(T + 0x790, 0)
    w32(T + 0x988, init_level)
    # origin / extent floats from the first and the last record (0x6F73F430)
    first = struct.unpack_from("<I", recs, 0)[0]
    last = struct.unpack_from("<I", recs, 28 * (w * h - 1))[0]

    def fa(v):  # field A (bits 14..22): float(0x47800000 | v & 0x7FC000) - 98304
        return struct.unpack("<f", struct.pack("<I", (v & 0x7FC000) | 0x47800000))[0] - 98304.0

    def fb(v):
        return struct.unpack("<f", struct.pack("<I", ((v >> 9) & 0x7FC000) | 0x47800000))[0] - 98304.0
    wf(T + 0xC4, fa(first))
    wf(T + 0xC8, fb(first))
    wf(T + 0xCC, fa(last))
    wf(T + 0xD0, fb(last))
    if water_present:
        w32(T + 0x30C, 1)
        w32(T + 0x310, WTAB)
        mu.mem_write(WTAB, b"\0" * 0x400)
        wf(WTAB + 0x108, water_offset)
    # ramp-object list
    mu.mem_write(RAMPOBJ, b"\0" * 16)
    mu.mem_write(RAMPS, b"\0" * 0x1000)
    w32(RAMPOBJ + 4, len(ramps))
    w32(RAMPOBJ + 8, RAMPS)
    off = 0
    for i, (ymin, xmin, ymax, xmax, vals) in enumerate(ramps):
        b = RAMPS + 0x2C * i
        w32(b + 0xC, ymin)
        w32(b + 0x10, xmin)
        w32(b + 0x14, ymax)
        w32(b + 0x18, xmax)
        w32(b + 0x24, RAMPDATA + off)
        for j, v in enumerate(vals):
            w32(RAMPDATA + off + 4 * j, v)
        off += 4 * len(vals)
    return dict(g=g, water_offset=water_offset, water_present=water_present, init_level=init_level, ramps=list(ramps))


def s32(v):
    return v - (1 << 32) if v & 0x80000000 else v


def q_level(cx, cy):
    set_fpu()
    flag = EXTRA + 0xF900
    w32(flag, 0xDEAD)
    ret = emu.call(LEVEL, ecx=cx & 0xFFFFFFFF, edx=cy & 0xFFFFFFFF, stack_args=(flag,))
    return s32(ret), r32(flag)


def q_ground(x, y):
    set_fpu()
    pt = EXTRA + 0xF880
    wf(pt, x)
    wf(pt + 4, y)
    wf(pt + 8, 0.0)
    emu.call(STUB_GROUND, ecx=T, stack_args=(pt, 0xFFFFFFFF))
    return r32(OUT)


def q_locz(x, y):
    set_fpu()
    emu.call(STUB_LOCZ, stack_args=(struct.unpack("<I", struct.pack("<f", x))[0],
                                    struct.unpack("<I", struct.pack("<f", y))[0], 1))
    return r32(OUT)


def f32(v):
    return struct.unpack("<f", struct.pack("<f", v))[0]


def bits(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def synthetic_grid(rng, w, h, ramp_prob=0.15, water_prob=0.2, smooth=False):
    pts = []
    for i in range(w * h):
        lay = rng.randrange(0, 4) if rng.random() < 0.6 else rng.randrange(0, 16)
        if smooth:
            # two adjacent levels with ramp patches: ramp corners whose neighbours differ by at most one
            lay = 3 + (((i % w) // 3 + (i // w) // 4) % 2 if smooth is True else rng.randrange(0, 2))
            ramp_prob = 0.7 if smooth is True else 0.55
        ft = rng.randrange(0, 16)
        if rng.random() < ramp_prob:
            ft |= 0x10
        if rng.random() < water_prob:
            ft |= 0x40
        if rng.random() < 0.05:
            ft |= 0x20
        ww = rng.randrange(0x1F00, 0x2300) | (0x4000 if rng.random() < 0.05 else 0)
        pts.append(Corner(rng.randrange(0x1E00, 0x2400), ww, ft, rng.randrange(0, 4),
                          (rng.randrange(0, 16) << 4) | lay))
    return dict(w=w, h=h, ox=-6144.0, oy=-6144.0, pts=pts)


def make_cases(rng, g, n):
    w, h = g["w"], g["h"]
    ox, oy = g["ox"], g["oy"]
    cases = []
    for _ in range(n):
        # cell coordinates: mostly inside, some outside
        cx = rng.randrange(-6, 4 * (w - 1) + 6)
        cy = rng.randrange(-6, 4 * (h - 1) + 6)
        x = rng.uniform(ox - 200, ox + 128 * (w - 1) + 200)
        y = rng.uniform(oy - 200, oy + 128 * (h - 1) + 200)
        if rng.random() < 0.1:  # exact tile / cell borders
            x = ox + 32 * rng.randrange(0, 4 * (w - 1) + 1)
            y = oy + 32 * rng.randrange(0, 4 * (h - 1) + 1)
        cases.append((cx, cy, f32(x), f32(y)))
    return cases


def run(g, cases, **kw):
    load_terrain(g, **kw)
    out = []
    # level queries with the ramp-object list loaded (0x6F0118C0 only reads the rects and arrays)
    for cx, cy, x, y in cases:
        try:
            lv, fl = q_level(cx, cy)
        except Exception as e:  # report where the original code faulted
            raise RuntimeError("level case %r: %s at eip %#x" % ((cx, cy), e, mu.reg_read(UC_X86_REG_EIP)))
        out.append(dict(cx=cx, cy=cy, x=bits(x), y=bits(y), level=lv, ramp_hit=fl))
    # height queries run with an empty list: with entries 0x6F011780 calls the objects' own methods (a ray test
    # against bridge-like models), which this harness does not build
    w32(RAMPOBJ + 4, 0)
    for r, (cx, cy, x, y) in zip(out, cases):
        try:
            r["ground"] = q_ground(x, y)
            r["locz"] = q_locz(x, y)
        except Exception as e:
            raise RuntimeError("height case %r: %s at eip %#x" % ((x, y), e, mu.reg_read(UC_X86_REG_EIP)))
    return out


SGRIDS = []
DEFAULT_SGRIDS = os.path.join(HERE, "terrain_s_grids.txt")


def load_sgrids(path):
    for line in open(path):
        tag, rest = line.split(":", 1)
        raw = eval(rest)  # a python list of (flags|texture, misc) tuples written by the Rust search
        SGRIDS.append((tag, raw))


def main():
    argv = sys.argv[1:]
    w3e = None
    nsyn = 6 + 18
    jpath = None
    rspath = None
    i = 0
    while i < len(argv):
        if argv[i] == "--w3e":
            w3e = argv[i + 1]; i += 2
        elif argv[i] == "--synthetic":
            nsyn = int(argv[i + 1]); i += 2
        elif argv[i] == "--json":
            jpath = argv[i + 1]; i += 2
        elif argv[i] == "--sgrids":
            load_sgrids(argv[i + 1]); i += 2
        elif argv[i] == "--rs":
            rspath = argv[i + 1]; i += 2
        else:
            i += 1
    if not SGRIDS and os.path.exists(DEFAULT_SGRIDS):
        load_sgrids(DEFAULT_SGRIDS)
    consts = {
        "C1_6FAA1760": struct.unpack("<f", struct.pack("<I", r32(0x6FAA1760)))[0],
        "C2_6FADB3E4": struct.unpack("<f", struct.pack("<I", r32(0x6FADB3E4)))[0],
        "d_6F981988": struct.unpack("<d", bytes(mu.mem_read(0x6F981988, 8)))[0],
        "d_6F8763A8": struct.unpack("<d", bytes(mu.mem_read(0x6F8763A8, 8)))[0],
        "d_6F875120": struct.unpack("<d", bytes(mu.mem_read(0x6F875120, 8)))[0],
        "d_6F953CC8": struct.unpack("<d", bytes(mu.mem_read(0x6F953CC8, 8)))[0],
        "d_6F9364E8": struct.unpack("<d", bytes(mu.mem_read(0x6F9364E8, 8)))[0],
        "tbl_6F9815E0": [s32(r32(0x6F9815E0 + 4 * k)) for k in range(4)],
        "tbl_6F9815F0": [s32(r32(0x6F9815F0 + 4 * k)) for k in range(4)],
        "flag_6FADB0A0": r32(0x6FADB0A0),
    }
    print("constants", consts)
    result = {"consts": consts, "sets": []}
    rng = random.Random(0x5EED13)
    if w3e:
        g = parse_w3e(open(w3e, "rb").read())
        cases = make_cases(rng, g, 700)
        res = run(g, cases, water_offset=0.0, water_present=True)
        result["sets"].append(dict(name="real", w=g["w"], h=g["h"], water_offset=0.0, ramps=[], cases=res))
        print("real map:", g["w"], g["h"], len(res), "cases; levels seen", sorted({r["level"] for r in res}))
    for k in range(nsyn):
        w, h = rng.randrange(8, 14), rng.randrange(8, 14)
        if k >= 6:
            # grids that reach the ramp variants S0/S1/S2 of 0x6F750AB0 (found by a search with the Rust model, then
            # confirmed here by the branch counters): flags|texture and misc bytes per corner, 6x6
            if not SGRIDS:
                continue
            w, h = 6, 6
            tag, raw = SGRIDS[(k - 6) % len(SGRIDS)]
            pts = [Corner(rng.randrange(0x1F80, 0x2080), rng.randrange(0x1F00, 0x2300), ft, 0, misc) for ft, misc in raw]
            g = dict(w=w, h=h, ox=-6144.0, oy=-6144.0, pts=pts)
            cases = make_cases(rng, g, 10)
            for ty in range(h - 1):
                for tx in range(w - 1):
                    cases.append((4 * tx + 2, 4 * ty + 2, f32(-6144.0 + 128.0 * tx + 64.0),
                                  f32(-6144.0 + 128.0 * ty + 64.0)))
        else:
            g = synthetic_grid(rng, w, h, smooth=(True if k >= 2 else False))
            cases = make_cases(rng, g, 260 if k < 2 else 600)
        ramps = []
        if k % 2 == 1:
            for _ in range(5):
                x0 = rng.randrange(-3, 4 * (w - 1) - 4)
                y0 = rng.randrange(-3, 4 * (h - 1) - 4)
                x1, y1 = x0 + rng.randrange(0, 14), y0 + rng.randrange(0, 14)
                vals = [rng.randrange(0, 8) for _ in range((x1 - x0 + 1) * (y1 - y0 + 1))]
                ramps.append((y0, x0, y1, x1, vals))
        wo = [0.0, 1.5, -7.25, 64.0][k % 4]
        res = run(g, cases, water_offset=wo, ramps=ramps)
        syn = dict(name="syn%d" % k, w=w, h=h, ox=g["ox"], oy=g["oy"], water_offset=wo, ramps=ramps,
                   points=[[c.height, c.water_word, c.flags_tex, c.variation, c.misc] for c in g["pts"]], cases=res)
        result["sets"].append(syn)
        hits = sum(r["ramp_hit"] for r in res)
        print("synthetic", k, dict(HITS), w, h, "ramp hits", hits, "levels", sorted({r["level"] for r in res}))
    if jpath:
        json.dump(result, open(jpath, "w"))
    if rspath:
        write_rs(result, rspath)


def write_rs(result, path):
    out = ["#![allow(dead_code, clippy::type_complexity)]", "// Generated by tools/emu/terrain_emu.py from the original Game.dll; do not edit.",
           "// Synthetic corner grids (height, water word, flags|texture, variation, misc) and the results of the real",
           "// level (0x6F0128C0) and ground height / GetLocationZ (0x6F24F3E0 with ecx=-1) code.",
           "pub struct Set {",
           "    pub w: u32,", "    pub h: u32,", "    pub water_offset: f32,",
           "    pub ramps: &'static [(i32, i32, i32, i32, &'static [i32])],",
           "    pub points: &'static [[u16; 5]],",
           "    /// (cell x, cell y, x bits, y bits, level, ramp hit, ground bits, location z bits)",
           "    pub cases: &'static [(i32, i32, u32, u32, i32, u32, u32, u32)],",
           "}", "", "pub const SETS: &[Set] = &["]
    for s in result["sets"]:
        if s["name"] == "real":
            continue
        out.append("    Set {")
        out.append("        w: %d, h: %d, water_offset: f32::from_bits(0x%08X)," % (s["w"], s["h"], bits(s["water_offset"])))
        rs = ", ".join("(%d, %d, %d, %d, &[%s])" % (r[0], r[1], r[2], r[3], ", ".join(str(v) for v in r[4]))
                       for r in s["ramps"])
        out.append("        ramps: &[%s]," % rs)
        out.append("        points: &[%s]," % ", ".join("[%d, %d, %d, %d, %d]" % tuple(p) for p in s["points"]))
        out.append("        cases: &[")
        for c in s["cases"]:
            out.append("            (%d, %d, 0x%08X, 0x%08X, %d, %d, 0x%08X, 0x%08X)," % (
                c["cx"], c["cy"], c["x"], c["y"], c["level"], c["ramp_hit"], c["ground"], c["locz"]))
        out.append("        ],")
        out.append("    },")
    out.append("];")
    for s in result["sets"]:
        if s["name"] != "real":
            continue
        out.append("")
        out.append("/// Cases of the real map used for the run (needs its `war3map.w3e`, env `TERRAIN_W3E`; the file is not committed):")
        out.append("/// (cell x, cell y, x bits, y bits, level, ground bits, location z bits); water offset 0.0.")
        out.append("pub const REAL: &[(i32, i32, u32, u32, i32, u32, u32)] = &[")
        for c in s["cases"]:
            out.append("    (%d, %d, 0x%08X, 0x%08X, %d, 0x%08X, 0x%08X)," % (
                c["cx"], c["cy"], c["x"], c["y"], c["level"], c["ground"], c["locz"]))
        out.append("];")
    open(path, "w").write("\n".join(out) + "\n")


if __name__ == "__main__":
    main()
