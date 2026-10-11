"""Unicorn vectors for the kill-XP position normaliser and range test (docs/spec/heroes.md, section 8.3 pass 2).

Two kinds of vector are written to OUT (text, one case per line, all numbers as hex bit patterns):

  N  normaliser: the real code of `0x6F4743A0` (-> `0x6F4742F0` -> `0x6F497460` -> `0x6F4885A0` -> `0x6F483270`,
     `0x6F483E70`, `0x6F38B0B0`, `0x6F6EEE20/EEF0/EF000/EF350`) run on a synthetic unit object, the world origin
     object and the two grid sub-objects of the world's grid manager. Fields:
       w6c w70 | o14 o70 o74 o78 o7c o80 o84 | g1_40 g1_44 g1_48 | g2_40 g2_44 g2_48 | out_x out_y
  R  candidate test: the real routine `0x6F28B990` run on a synthetic killer record and a synthetic candidate whose
     position comes from the normaliser above. Fields:
       rec_x rec_y V cand_x cand_y D listed
     D is the distance value the routine reads (`fld [eax]` after `0x6F020000`), listed is 1 when the candidate was
     appended to the in-range list. Gate: MaxLevelHeroesDrainExp on (so only the shared/flag part decides), flags 0.

Only the lookup `0x6F03FA30`, the gate callees and the list append are answered by the harness (hero_candidate_emu).
Everything else is the binary. Run:

  GAME_DLL=/path/to/Game.dll python3 tools/emu/hero_normaliser_emu.py OUT.txt [N_CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import hero_candidate_emu as HC  # noqa: E402

mu = emu.mu
DATA = emu.DATA
NORM = 0x6F4743A0
MGR_PTR = 0x6FAB73D8        # the grid manager (`[0x6fab73d8]`)
MGR = DATA + 0xE000         # manager object: grid A at +0x14, grid B at +0x68
OUTN = DATA + 0xE400        # normaliser output (3 dwords)
OBJ = HC.OBJ                # the synthetic unit (also what the lookup hook returns)
POSREF = HC.POSREF
WORLD = HC.WORLD


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def b2f(v):
    return struct.unpack("<f", struct.pack("<I", v & 0xFFFFFFFF))[0]


def wrf(a, v):
    wr32(a, f2b(v))


def rdf(a):
    return b2f(rd32(a))


def hx(v):
    return "%08x" % (v & 0xFFFFFFFF)


def rand_float(rng, lo, hi, zero_p=0.0):
    if rng.random() < zero_p:
        return 0.0
    return rng.uniform(lo, hi)


def set_case(rng):
    """Random synthetic inputs; some special cases (zero i, j equal to the grid row, tiny differences)."""
    w6c = rand_float(rng, -4096, 4096, 0.05)
    w70 = rand_float(rng, -4096, 4096, 0.05)
    wrf(WORLD + 0x6C, w6c)
    wrf(WORLD + 0x70, w70)
    grids = []
    for base in (MGR + 0x14, MGR + 0x68):
        g40 = rand_float(rng, -100.0, 100.0, 0.05)
        g44 = rng.randrange(-5, 40)
        g48 = rng.uniform(0.25, 4.0)
        wrf(base + 0x40, g40)
        wr32(base + 0x44, g44 & 0xFFFFFFFF)
        wrf(base + 0x48, g48)
        grids.append((g40, g44, g48))
    o14 = rng.getrandbits(32)
    o70 = rand_float(rng, -50.0, 50.0, 0.1)
    if rng.random() < 0.08:
        o70 = grids[0][0] + rng.uniform(-2e-4, 2e-4)  # snap region
    o74 = rng.randrange(-5, 40)
    if rng.random() < 0.15:
        o74 = grids[0 if not (o14 & 0x80000000) else 1][1]  # equal rows: no second term
    o78 = rand_float(rng, -10.0, 10.0, 0.05)
    o7c = rand_float(rng, -10.0, 10.0, 0.05)
    o80 = rand_float(rng, -10.0, 10.0, 0.1)
    o84 = rand_float(rng, -10.0, 10.0, 0.1)
    wr32(OBJ + 0x14, o14)
    wrf(OBJ + 0x70, o70)
    wr32(OBJ + 0x74, o74 & 0xFFFFFFFF)
    wrf(OBJ + 0x78, o78)
    wrf(OBJ + 0x7C, o7c)
    wrf(OBJ + 0x80, o80)
    wrf(OBJ + 0x84, o84)
    return dict(w6c=w6c, w70=w70, o14=o14, o70=o70, o74=o74, o78=o78, o7c=o7c, o80=o80, o84=o84,
                g1=grids[0], g2=grids[1])


def normalise():
    for i in range(3):
        wr32(OUTN + 4 * i, 0)
    emu.call(NORM, ecx=POSREF, stack_args=(OUTN,))
    return rd32(OUTN), rd32(OUTN + 4)


def normaliser_line(rng):
    c = set_case(rng)
    ox, oy = normalise()
    g1, g2 = c["g1"], c["g2"]
    parts = [
        hx(f2b(c["w6c"])), hx(f2b(c["w70"])),
        hx(c["o14"]), hx(f2b(c["o70"])), hx(c["o74"] & 0xFFFFFFFF),
        hx(f2b(c["o78"])), hx(f2b(c["o7c"])), hx(f2b(c["o80"])), hx(f2b(c["o84"])),
        hx(f2b(g1[0])), hx(g1[1] & 0xFFFFFFFF), hx(f2b(g1[2])),
        hx(f2b(g2[0])), hx(g2[1] & 0xFFFFFFFF), hx(f2b(g2[2])),
        hx(ox), hx(oy),
    ]
    return "N " + " ".join(parts)


def range_line(rng, stats):
    # a random normalised candidate position (through the real normaliser) and a random record
    set_case(rng)
    cx, cy = normalise()
    HC.C.drain = 1
    HC.C.at_max = 0
    HC.C.shared = True
    HC.C.global_on = rng.randrange(2)
    HC.C.cand_flags = 0
    radius = rng.choice([8.0, 32.0, 100.0, 250.0, 600.0])
    # the killer lies near the candidate (within about 1.5 radii), so both outcomes occur
    rx = b2f(cx) + rng.uniform(-1.5, 1.5) * radius
    ry = b2f(cy) + rng.uniform(-1.5, 1.5) * radius
    v_bits = f2b(radius * radius)
    if rng.random() < 0.05:
        v_bits = 0x7FC00000          # NaN V: unordered, listed
    wrf(HC.RECORD + 0x0, rx)
    wrf(HC.RECORD + 0x4, ry)
    wr32(HC.RECORD + 0x8, rng.randrange(12))
    wr32(HC.RECORD + 0xC, v_bits)
    wr32(HC.DRAIN_FLAG, 1)
    wr32(HC.GLOBAL_FLAG, HC.C.global_on)
    wr32(HC.CAND + 0x00, HC.VT)
    wr32(HC.CAND + HC.U_FLAGS, 0)
    wr32(HC.CAND + HC.U_OWNER, rng.randrange(12))
    wr32(HC.POSREF + 8, rng.getrandbits(31))
    wr32(HC.POSREF + 0xC, rng.getrandbits(32))
    HC.C.list_of = {}
    HC.C.slot_next = 0
    HC.C.vcall_ecx = []
    HC.C.d_read = None
    emu.call(HC.CANDIDATE_ROUTINE, ecx=HC.RECORD, stack_args=(HC.CAND,))
    d_bits = f2b(HC.C.d_read) if HC.C.d_read is not None else 0
    listed = 1 if len(HC.C.list_of.get(HC.RECORD + 0x10, [])) == 1 else 0
    if rng.random() < 0.2 and HC.C.d_read is not None:
        # tie: V = D (the original lists it); reuse the same candidate and record
        wr32(HC.RECORD + 0xC, d_bits)
        HC.C.list_of = {}
        HC.C.slot_next = 0
        HC.C.d_read = None
        emu.call(HC.CANDIDATE_ROUTINE, ecx=HC.RECORD, stack_args=(HC.CAND,))
        d2 = f2b(HC.C.d_read) if HC.C.d_read is not None else 0
        tie_listed = 1 if len(HC.C.list_of.get(HC.RECORD + 0x10, [])) == 1 else 0
        stats["ties"] = stats.get("ties", 0) + 1
        stats["ties_listed"] = stats.get("ties_listed", 0) + tie_listed
        if d2 != d_bits:
            raise SystemExit("tie rerun changed D")
        return "R %s %s %s %s %s %s %d" % (hx(f2b(rx)), hx(f2b(ry)), hx(d_bits), hx(cx), hx(cy), hx(d_bits),
                                           tie_listed)
    stats["listed" if listed else "out"] = stats.get("listed" if listed else "out", 0) + 1
    return "R %s %s %s %s %s %s %d" % (hx(f2b(rx)), hx(f2b(ry)), hx(v_bits), hx(cx), hx(cy), hx(d_bits), listed)


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "hero_normaliser_vectors.txt"
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 1000
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    rng = random.Random(seed)
    HC.install()
    wr32(MGR_PTR, MGR)
    # the zero-snap constant is written by a start-up stub (0x6F84DC90: mov [0x6faae478], 1e-4); the file image has 0
    wr32(0x6FAAE478, 0x38D1B717)
    lines = []
    stats = {}
    for _ in range(cases):
        lines.append(normaliser_line(rng))
    for _ in range(cases):
        lines.append(range_line(rng, stats))
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("lines: %d -> %s" % (len(lines), out_path))
    print("range stats: " + " ".join("%s=%d" % kv for kv in sorted(stats.items())))


if __name__ == "__main__":
    main()
