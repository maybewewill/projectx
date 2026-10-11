#!/usr/bin/env python3
"""Differential checks of the random unit pick helpers of Game.dll 1.26a (natives-pending.md section 3).

Runs the ORIGINAL routines in Unicorn (the user's own Game.dll, mapped at its image base) and compares them with
Python transcriptions of the rules read from the disassembly:

  race     the builder's race text -> enum routine 0x6F26E210 (table at 0x6FA70FE8, 8-byte entries {name, value};
           the table itself is read from the image and checked against RACE_MODEL first).
  creep    the creep candidate walk 0x6F29FD90 (ECX = level, DL = map tileset char, three stack flags).
  neutral  the neutral building walk 0x6F294740 (CL = map tileset char).

Environment stand-ins (each one is named; none is the original code):
  * Storm 508 at IAT 0x6F86D608 (name compare with a length limit, used by 0x6F6EB68A): MODEL = ASCII
    case-insensitive equality. NOT CHECKED: Storm.dll is not available.
  * Storm 571 at IAT 0x6F86D60C (used by 0x6F6EB690, called with ECX = text, DL = char): MODEL = strchr(text, char).
    NOT CHECKED: the name and the semantics of ordinal 571 are not known from this container.
  * 0x6F29C240 (movetp of a raw code, a hash lookup in the unit database): replaced by a table, one value per code.
  * 0x6F4C4630 (the string getter behind 0x6F4C4660, the tilesets text of record +0x29C): replaced by a table per record.
  * 0x6F09EAF0 (growable array append): the array object is kept in the layout the walks read (count at +4,
    data at +8), so the walk's own loads work.
  * 0x6F285320 (the draw): replaced by a stub that records the count (EDX) and returns index 0.
  * 0x6F6EB558 (free, stdcall, 4 arguments): no-op.

The records hold the fields read by the walks (rawcode +0x14, level +0x68, race +0x1C0, isbldg +0x28C,
special +0x290, hostilePal +0x294, nbrandom +0x298, link +4 with LINK = 0). The list head is 0x6FAB4468.

Usage:

    GAME_DLL=/path/to/Game.dll python3 tools/emu/random_pick_emu.py --cases 400 --seed 1

Prints the counts of each check; exits 1 on any mismatch.
"""

import argparse
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import softfloat_vectors as sv  # noqa: E402  (the Game loader and the call convention)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

RACE_FN = 0x6F26E210
RACE_TABLE = 0x6FA70FE8
CREEP_FN = 0x6F29FD90
NEUTRAL_FN = 0x6F294740
IAT_STORM_508 = 0x6F86D608
IAT_STORM_571 = 0x6F86D60C
MOVETP_FN = 0x6F29C240
TILESET_GET_FN = 0x6F4C4630
APPEND_FN = 0x6F09EAF0
DRAW_FN = 0x6F285320
FREE_FN = 0x6F6EB558
LIST_HEAD = 0x6FAB4468
LIST_LINK = 0x6FAB4460

STUB = 0x00600000
STRINGS = 0x00500000
NODES = 0x00510000
ARRAYS = 0x00520000
NODE_SIZE = 0x300

# The builder's race enum, as the table at RACE_TABLE gives it (the names are lower case in the table).
RACE_MODEL = {
    "human": 1,
    "orc": 2,
    "undead": 3,
    "nightelf": 4,
    "demon": 5,
    "other": 7,
    "commoner": 9,
    "critters": 10,
    "naga": 11,
    "creeps": 8,
    "unknown": 0,
}


def race_model(text):
    """0x6F26E210: NULL gives 7; the first table entry whose name matches gives its value; no match gives 0."""
    if text is None:
        return 7
    return RACE_MODEL.get(text.lower(), 0)


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", type=int, default=400)
    ap.add_argument("--seed", type=int, default=1)
    args = ap.parse_args(argv)
    rng = random.Random(args.seed)

    path = os.environ.get("GAME_DLL")
    if not path:
        print("set GAME_DLL to the path of Game.dll", file=sys.stderr)
        return 2
    g = sv.Game(path)
    mu = g.mu
    for base, size in ((STUB, 0x1000), (STRINGS, 0x10000), (NODES, 0x10000), (ARRAYS, 0x10000)):
        mu.mem_map(base, size)

    # The walks set up an exception frame through fs:[0]. Unicorn has no FS base here, so the accesses are
    # rewritten in the emulated copy only (not in the file): fs:[0] becomes an absolute slot in a scratch page.
    TEB = 0x00700000
    mu.mem_map(TEB, 0x1000)
    patched = 0
    for lo, hi in ((0x6F29FD90, 0x6F29FF40), (0x6F294740, 0x6F294890)):
        blob = bytearray(bytes(mu.mem_read(lo, hi - lo)))
        # (prefix, length, replacement opcode bytes): 64 A1/A3 disp32 are 6 bytes, the ModRM forms 7 bytes
        forms = [
            (b"\x64\xa1\x00\x00\x00\x00", 6, b"\xa1"),
            (b"\x64\xa3\x00\x00\x00\x00", 6, b"\xa3"),
            (b"\x64\x8b\x0d\x00\x00\x00\x00", 7, b"\x8b\x0d"),
            (b"\x64\x89\x0d\x00\x00\x00\x00", 7, b"\x89\x0d"),
            (b"\x64\x8b\x05\x00\x00\x00\x00", 7, b"\x8b\x05"),
        ]
        i = 0
        while i < len(blob):
            for pref, length, op in forms:
                if bytes(blob[i:i + len(pref)]) == pref:
                    blob[i:i + length] = op + struct.pack("<I", TEB) + b"\x90" * (length - len(op) - 4)
                    patched += 1
                    i += length
                    break
            else:
                i += 1
        mu.mem_write(lo, bytes(blob))
    print("fs:[0] accesses rewritten in the emulated copy:", patched)

    def w32(addr, v):
        mu.mem_write(addr, struct.pack("<I", v & 0xFFFFFFFF))

    def r32(addr):
        return struct.unpack("<I", bytes(mu.mem_read(addr, 4)))[0]

    def read_c(addr):
        out = bytearray()
        while True:
            b = bytes(mu.mem_read(addr + len(out), 1))[0]
            if b == 0:
                break
            out.append(b)
            if len(out) > 4096:
                raise RuntimeError("runaway string")
        return out.decode("latin1")

    # ---- check the table the model is written from ----
    table = {}
    for i in range(0, 0x100, 8):
        name_ptr = r32(RACE_TABLE + i)
        if name_ptr == 0:
            break
        table[read_c(name_ptr).lower()] = r32(RACE_TABLE + i + 4)
    if table != RACE_MODEL:
        print("race table in the image differs from RACE_MODEL:", table, file=sys.stderr)
        return 1

    # ---- hooks (each one returns the way the original would: pop the arguments, put the result in EAX) ----
    state = {"slots": [], "draw_n": None, "movetp": {}, "tileset": {}, "buf": ARRAYS, "tbuf": None}

    def ret_from(uc, value, pops):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops)
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_EIP, ret)

    def stack_args(uc, n):
        esp = uc.reg_read(UC_X86_REG_ESP)
        return [struct.unpack("<I", bytes(uc.mem_read(esp + 4 + 4 * k, 4)))[0] for k in range(n)]

    def storm508(uc, address, size, user):  # stdcall (name, text, max): 0 when equal
        name_p, text_p, _ = stack_args(uc, 3)
        same = read_c(name_p).lower() == read_c(text_p).lower()
        ret_from(uc, 0 if same else 1, 12)

    def storm571(uc, address, size, user):  # fastcall (ECX = text, DL = char): nonzero when found
        text = read_c(uc.reg_read(UC_X86_REG_ECX))
        ch = chr(uc.reg_read(UC_X86_REG_EDX) & 0xFF)
        ret_from(uc, 1 if ch in text else 0, 0)

    def movetp_hook(uc, address, size, user):  # fastcall (ECX = raw code)
        ret_from(uc, state["movetp"].get(uc.reg_read(UC_X86_REG_ECX), 0), 0)

    def tileset_hook(uc, address, size, user):  # thiscall (ECX = record object of +0x29C)
        ret_from(uc, state["tileset"].get(uc.reg_read(UC_X86_REG_ECX), 0), 0)

    def append_hook(uc, address, size, user):  # thiscall (ECX = array: capacity +0, count +4, data +8)
        arr = uc.reg_read(UC_X86_REG_ECX)
        count = r32(arr + 4)
        data = r32(arr + 8)
        if data == 0:
            data = state["buf"]
            state["buf"] += 0x100
            w32(arr + 8, data)
            w32(arr + 0, 64)
        slot = data + 4 * count
        w32(arr + 4, count + 1)
        state["slots"].append(slot)
        ret_from(uc, slot, 0)

    def draw_hook(uc, address, size, user):  # ECX = stream, EDX = count; index 0 is returned
        state["draw_n"] = uc.reg_read(UC_X86_REG_EDX)
        ret_from(uc, 0, 0)

    def free_hook(uc, address, size, user):  # stdcall, 4 arguments
        ret_from(uc, 0, 16)

    w32(IAT_STORM_508, STUB + 0x00)
    w32(IAT_STORM_571, STUB + 0x10)
    mu.hook_add(UC_HOOK_CODE, storm508, begin=STUB + 0x00, end=STUB + 0x00)
    mu.hook_add(UC_HOOK_CODE, storm571, begin=STUB + 0x10, end=STUB + 0x10)
    mu.hook_add(UC_HOOK_CODE, movetp_hook, begin=MOVETP_FN, end=MOVETP_FN)
    mu.hook_add(UC_HOOK_CODE, tileset_hook, begin=TILESET_GET_FN, end=TILESET_GET_FN)
    mu.hook_add(UC_HOOK_CODE, append_hook, begin=APPEND_FN, end=APPEND_FN)
    mu.hook_add(UC_HOOK_CODE, draw_hook, begin=DRAW_FN, end=DRAW_FN)
    mu.hook_add(UC_HOOK_CODE, free_hook, begin=FREE_FN, end=FREE_FN)

    strcur = [STRINGS]

    def put_str(text):
        addr = strcur[0]
        data = text.encode("latin1") + b"\0"
        mu.mem_write(addr, data)
        strcur[0] += (len(data) + 15) & ~15
        return addr

    def reset_call_state():
        state["slots"] = []
        state["draw_n"] = None
        state["buf"] = ARRAYS

    # ---- 1. race table (0x6F26E210) ----
    names = list(RACE_MODEL)
    race_n = race_bad = 0
    for _ in range(args.cases):
        strcur[0] = STRINGS
        r = rng.random()
        if r < 0.05:
            text = None
        elif r < 0.2:
            text = ""
        else:
            base = rng.choice(names) if rng.random() < 0.7 else "".join(rng.choice("abcdefgxyz ") for _ in range(rng.randint(1, 9)))
            text = "".join(c.upper() if rng.random() < 0.5 else c for c in base)
        got = g.call(RACE_FN, ecx=0 if text is None else put_str(text))
        race_n += 1
        if got != race_model(text):
            race_bad += 1
            if race_bad <= 5:
                print("race mismatch", repr(text), "got", got, "want", race_model(text))

    # ---- 2. creep walk (0x6F29FD90) and neutral walk (0x6F294740) ----
    codes = [b"hfoo", b"Hfoo", b"nwol", b"ngrd", b"nskm", b"Ncrp", b"ncks", b"opeo", b"ogru", b"hpea", b"nbrn", b"ngol"]
    creep_n = creep_bad = neutral_n = neutral_bad = 0
    gate_hist = {"creep_gate_pass": 0, "neutral_pass": 0, "empty_list": 0}
    for _ in range(args.cases):
        strcur[0] = STRINGS
        n_nodes = rng.randint(0, len(codes))
        picks = rng.sample(range(len(codes)), n_nodes)
        recs = []
        for idx, ci in enumerate(picks):
            rawcode = struct.unpack(">I", codes[ci])[0]
            rec = {
                "addr": NODES + idx * NODE_SIZE,
                "raw": rawcode,
                "race": rng.choice([7, 8, 8, 7, 8, 7, 1, 0, 9]),
                "level": rng.randint(0, 3),
                "isbldg": rng.choice([0, 0, 1]),
                "special": rng.choice([0, 0, 0, 1]),
                "hostile": rng.choice([0, 0, 1]),
                "nbrandom": rng.choice([0, 1, 1, 2]),
                "tilesets": "".join(rng.choice("ABCDEFGHIJKLMNOPQRSTUVWXYZ*") for _ in range(rng.randint(0, 4))),
                "movetp": rng.choice([0, 2, 0x10, 5]),
            }
            recs.append(rec)
            b = rec["addr"]
            mu.mem_write(b, b"\0" * NODE_SIZE)
            w32(b + 0x14, rawcode)
            w32(b + 0x68, rec["level"])
            w32(b + 0x1C0, rec["race"])
            w32(b + 0x28C, rec["isbldg"])
            w32(b + 0x290, rec["special"])
            w32(b + 0x294, rec["hostile"])
            w32(b + 0x298, rec["nbrandom"])
            state["tileset"][b + 0x29C] = put_str(rec["tilesets"])
        for idx, rec in enumerate(recs):  # the chain: next = [node + 4]
            w32(rec["addr"] + 4, recs[idx + 1]["addr"] if idx + 1 < len(recs) else 0)
        state["movetp"] = {rec["raw"]: rec["movetp"] for rec in recs}
        w32(LIST_LINK, 0)
        w32(LIST_HEAD, recs[0]["addr"] if recs else 0)

        # creep: level -1 means any level; the map tileset char 0 skips the tileset test
        level = rng.choice([-1, 0, 1, 2, 3])
        tchar = rng.choice([0, ord("A"), ord("B"), ord("L"), ord("*")])
        flags = [rng.randint(0, 1) for _ in range(3)]
        want = []
        for rec in recs:
            if 0x41 <= ((rec["raw"] >> 24) & 0xFF) <= 0x5A:
                continue
            a, b_, c = flags
            ok = (b_ != 0) if rec["movetp"] == 2 else ((c != 0) if rec["movetp"] == 0x10 else (a != 0))
            if not ok:
                continue
            if not (level == -1 or rec["level"] == level):
                continue
            if not (rec["race"] == 8 and rec["isbldg"] == 0 and rec["special"] == 0):
                continue
            if tchar and not ("*" in rec["tilesets"] or chr(tchar) in rec["tilesets"]):
                continue
            want.append(rec["raw"])
        reset_call_state()
        try:
            got_ret = g.call(CREEP_FN, ecx=level & 0xFFFFFFFF, edx=tchar, stack=flags)
        except Exception as exc:  # debugging aid: report where the emulated walk stopped
            from unicorn.x86_const import UC_X86_REG_EIP as _EIP
            print("creep call failed at eip", hex(mu.reg_read(_EIP)), exc, "nodes", [hex(r["addr"]) for r in recs])
            raise
        got = [r32(s) for s in state["slots"]]
        creep_n += 1
        if want:
            gate_hist["creep_gate_pass"] += 1
        else:
            gate_hist["empty_list"] += 1
        exp_ret = want[0] if want else 0
        exp_n = len(want) if want else None
        if got != want or got_ret != exp_ret or state["draw_n"] != exp_n:
            creep_bad += 1
            if creep_bad <= 5:
                print("creep mismatch level", level, "tchar", chr(tchar) if tchar else 0, "flags", flags,
                      "got", [hex(x) for x in got], "want", [hex(x) for x in want], "ret", hex(got_ret), "n", state["draw_n"])

        # neutral: CL = map tileset char
        want = []
        for rec in recs:
            if not (rec["race"] == 7 and rec["isbldg"] != 0 and rec["hostile"] == 0 and rec["special"] == 0 and rec["nbrandom"] != 0):
                continue
            if tchar and not ("*" in rec["tilesets"] or chr(tchar) in rec["tilesets"]):
                continue
            want.append(rec["raw"])
        reset_call_state()
        got_ret = g.call(NEUTRAL_FN, ecx=tchar)
        got = [r32(s) for s in state["slots"]]
        neutral_n += 1
        if want:
            gate_hist["neutral_pass"] += 1
        exp_ret = want[0] if want else 0
        exp_n = len(want) if want else None
        if got != want or got_ret != exp_ret or state["draw_n"] != exp_n:
            neutral_bad += 1
            if neutral_bad <= 5:
                print("neutral mismatch tchar", tchar, "got", [hex(x) for x in got], "want", [hex(x) for x in want],
                      "ret", hex(got_ret), "n", state["draw_n"])
                print("  head", hex(r32(LIST_HEAD)), "link", hex(r32(LIST_LINK)),
                      [(hex(r["addr"]), hex(r32(r["addr"] + 4)), hex(r32(r["addr"] + 0x1C0)), hex(r32(r["addr"] + 0x28C)),
                        hex(r32(r["addr"] + 0x290)), hex(r32(r["addr"] + 0x294)), hex(r32(r["addr"] + 0x298)))
                       for r in recs])

    print("race table (0x6F26E210): %d cases, %d mismatches" % (race_n, race_bad))
    print("creep walk (0x6F29FD90): %d cases, %d mismatches (non-empty candidate lists: %d)"
          % (creep_n, creep_bad, gate_hist["creep_gate_pass"]))
    print("neutral walk (0x6F294740): %d cases, %d mismatches (non-empty candidate lists: %d)"
          % (neutral_n, neutral_bad, gate_hist["neutral_pass"]))
    return 1 if (race_bad or creep_bad or neutral_bad) else 0


if __name__ == "__main__":
    sys.exit(main())
