"""Unicorn check of the ability definition builder and its level record layout (Game.dll 1.26a).

Runs the ORIGINAL code of:
- FUN_6F2657A0 (definition builder: reads the ability data rows and fills the definition and the
  0x68-byte level records),
- FUN_6F02E6A0 (level record accessor), FUN_6F041700 (level up), FUN_6F4C3F50 (raw code text parser).

The builder's external calls are replaced by Python stubs: the Storm SLK ordinals (cell fetch, token
split, strlen), the cell readers (real / int / text), the list and node allocators and the
definition-level container. The stubs only supply the synthetic cell texts and allocate memory.
They do not choose any record offset, clamp or list rule, so those are the original's.

The real-number conversion itself (FUN_6F262760) is replaced by a float() of the synthetic text. It is
therefore NOT checked here; only the store offsets and the rules around it are.

Inputs are synthetic (no game data). Run:
  GAME_DLL=/path/to/Game.dll python3 tools/emu/abilitytype_binder_emu.py
"""
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll, provides call() and mu)
import unittype_int_emu as ui  # noqa: E402  (strtol model, checked in unittype_int_emu.py)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX, UC_X86_REG_ECX  # noqa: E402

mu = emu.mu
u32 = emu.u32

BUILD = 0x6F2657A0       # definition builder (fastcall, ECX = raw code)
ACC = 0x6F02E6A0         # level record accessor (thiscall, ECX = ability, [esp+4] = index)
LEVELUP = 0x6F041700     # level up (fastcall, ECX = ability)
RAWCODE = 0x6F4C3F50     # raw code text parser (fastcall, ECX = text)

# callees replaced by stubs: target address -> stub name
STUB_TARGETS = {
    0x6F6EB5A6: "ord578",     # Storm ordinal 578 thunk (cdecl, 5 args): fetch a cell
    0x6F6EB606: "ord504",     # Storm ordinal 504 thunk (stdcall, 5 args): token split
    0x6F6EB666: "ord506",     # Storm ordinal 506 thunk (stdcall, 1 arg): strlen
    0x6F262760: "realcell",   # cell -> real (thiscall: ECX reader, out, cell)
    0x6F2628E0: "textcell",   # cell -> text pointer (thiscall: ECX reader, cell)
    0x6F2625F0: "intcell",    # cell or name -> int (thiscall: ECX reader, arg)
    0x6F255250: "targs",      # target flag text parser (fastcall: ECX text, result not modelled)
    0x6F264C50: "levelset",   # definition level container (thiscall: ECX container, count)
    0x6F25A760: "listalloc",  # list storage (thiscall: ECX container, capacity)
    0x6F255D00: "granule",    # allocation granularity (stdcall, 1 arg)
    0x6F2656D0: "nodealloc",  # definition node allocator (3 stack args)
    0x6F25F5C0: "reader",     # reader prepare (thiscall: ECX reader, 1 arg)
    0x6F32C8E0: "zero0",      # global lookup (returns 0 here)
    0x6F262A50: "k30",        # definition field +0x30 source (returns 0x11)
    0x6F262A70: "k34",        # definition field +0x34 source (returns 0x22)
    0x6F265ED0: "defget",     # definition lookup (returns the test definition)
    0x6F7E11C2: "memset",     # IAT jump to MSVCR80 memset (cdecl: ptr, value, size)
}
ARGBYTES = {  # bytes of stack arguments the callee pops (ret N)
    "ord578": 0, "ord504": 0x14, "ord506": 4, "realcell": 8, "textcell": 4, "intcell": 4,
    "targs": 0, "levelset": 4, "listalloc": 4, "granule": 4, "nodealloc": 0xC, "reader": 4,
    "zero0": 0, "k30": 0, "k34": 0, "defget": 0, "memset": 0,
}
STUB_BASE = 0x00300100   # inside the emu SENT page
STUB_STEP = 0x10

HEAP = 0x00400000
HEAP_SIZE = 0x00400000
mu.mem_map(HEAP, HEAP_SIZE)
heap_next = [HEAP]
CELL_LO, CELL_HI = 0x00100000, 0x00110000   # the emu stack, where the builder keeps cell buffers
CELL_MAGIC = 0x0CE11CE1

CELLS = {}        # (field name, row) -> synthetic cell text
DEF_INTS = {}     # definition-level integer fields by name
NODE = [0]
DEF_FOR_GET = [0]
text_cache = {}
TRACE = []
log = {"ord504": [], "targs": [], "listalloc": [], "levelset": []}


def alloc(n):
    p = heap_next[0]
    heap_next[0] = (p + n + 15) & ~15
    assert heap_next[0] < HEAP + HEAP_SIZE
    mu.mem_write(p, b"\0" * n)
    return p


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", u32(v)))


def cstr(a):
    out = bytearray()
    while True:
        b = bytes(mu.mem_read(a + len(out), 1))[0]
        if b == 0:
            break
        out.append(b)
    return out.decode("latin-1")


def put_cstr(s):
    p = alloc(len(s) + 1)
    mu.mem_write(p, s.encode("latin-1") + b"\0")
    return p


def text_ptr(s):
    if s == "":
        return 0
    if s not in text_cache:
        text_cache[s] = put_cstr(s)
    return text_cache[s]


def stack_args(esp, n):
    return [rd32(esp + 4 + 4 * i) for i in range(n)]


def handler(name, esp):
    """Runs before the stub's RET N. Returns the EAX value, or None to leave EAX alone."""
    if name == "ord578":                       # cdecl: (out, size, table, field, row)
        out, _size, _table, field, row = stack_args(esp, 5)
        wr32(out, field)
        wr32(out + 4, row)
        wr32(out + 8, CELL_MAGIC)
        wr32(out + 12, 0)
        wr32(out + 16, 0)
        return None
    if name == "realcell":                     # thiscall: (out, cell)
        out, cell = stack_args(esp, 2)
        text = CELLS[(cstr(rd32(cell)), rd32(cell + 4))]
        wr32(out, struct.unpack("<I", struct.pack("<f", float(text)))[0] if text else 0)
        return None
    if name == "textcell":                     # thiscall: (cell)
        (cell,) = stack_args(esp, 1)
        return text_ptr(CELLS.get((cstr(rd32(cell)), rd32(cell + 4)), ""))
    if name == "intcell":                      # thiscall: (cell or name pointer)
        (arg,) = stack_args(esp, 1)
        if CELL_LO <= arg < CELL_HI:
            return ui.model_cell_int(CELLS.get((cstr(rd32(arg)), rd32(arg + 4)), ""))
        return ui.model_cell_int(DEF_INTS[cstr(arg)])
    if name == "targs":
        log["targs"].append(mu.reg_read(UC_X86_REG_ECX))
        return 0
    if name == "levelset":                     # thiscall: ECX = container at def+0x4C, (count)
        ecx = mu.reg_read(UC_X86_REG_ECX)
        (count,) = stack_args(esp, 1)
        wr32(ecx + 4, count)
        wr32(ecx + 8, alloc(max(count, 1) * 0x68))
        log["levelset"].append(count)
        return None
    if name == "listalloc":                    # thiscall: ECX = list container, (capacity)
        ecx = mu.reg_read(UC_X86_REG_ECX)
        (cap,) = stack_args(esp, 1)
        wr32(ecx, cap)
        wr32(ecx + 8, alloc(max(cap, 1) * 4))
        log["listalloc"].append(cap)
        return None
    if name == "memset":
        ptr, val, size = stack_args(esp, 3)
        if size:
            mu.mem_write(ptr, bytes([val & 0xFF]) * size)
        return None
    if name == "granule":
        return 1
    if name == "nodealloc":
        NODE[0] = alloc(0x80)
        return NODE[0]
    if name == "reader":                       # thiscall: ECX = reader at def+0x18
        ecx = mu.reg_read(UC_X86_REG_ECX)
        wr32(ecx + 4, 1)
        wr32(ecx + 8, 1)
        wr32(ecx + 12, 0)
        return None
    if name == "zero0":
        return 0
    if name == "k30":
        return 0x11
    if name == "k34":
        return 0x22
    if name == "defget":
        return DEF_FOR_GET[0]
    if name == "ord506":                       # stdcall: (text) -> strlen
        (p,) = stack_args(esp, 1)
        return len(cstr(p))
    if name == "ord504":                       # stdcall: (&text, buf, size, delims, flag)
        pp, buf, size, delims, _flag = stack_args(esp, 5)
        p = rd32(pp)
        dset = cstr(delims)
        tok = bytearray()
        while True:
            c = bytes(mu.mem_read(p, 1))[0]
            if c == 0 or chr(c) in dset:
                break
            if len(tok) < size - 1:
                tok.append(c)
            p += 1
        if bytes(mu.mem_read(p, 1))[0] != 0:
            p += 1                              # consume one delimiter
        wr32(pp, p)
        mu.mem_write(buf, bytes(tok) + b"\0")
        log["ord504"].append(bytes(tok).decode("latin-1"))
        return None
    raise KeyError(name)


def install_stubs():
    names_by_stub = {}
    addr = STUB_BASE
    for target, name in STUB_TARGETS.items():
        n = ARGBYTES[name]
        code = b"\xc3" if n == 0 else b"\xc2" + struct.pack("<H", n)
        mu.mem_write(addr, code)
        names_by_stub[addr] = name
        mu.mem_write(target, b"\xe9" + struct.pack("<i", addr - (target + 5)))   # jmp stub
        addr += STUB_STEP
    end = addr

    def cb(uc, address, size, user):
        name = names_by_stub.get(address)
        if name is None:
            return
        TRACE.append(name)
        ret = handler(name, uc.reg_read(UC_X86_REG_ESP))
        if ret is not None:
            uc.reg_write(UC_X86_REG_EAX, u32(ret))

    mu.hook_add(UC_HOOK_CODE, cb, begin=STUB_BASE, end=end)


# ---- independent models (what a correct builder must produce) ----

def model_raw(s):
    """FUN_6F4C3F50: up to four bytes, packed big-endian, stops at NUL."""
    v = 0
    for i in range(4):
        if i >= len(s) or s[i] == "\0":
            return v
        v = (v << 8) | (ord(s[i]) & 0xFF)
    return v


def f32bits(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def model_cool(v):
    """Builder rule: stored = (0.0 <= cool) ? cool : 0.0 (NaN gives 0.0)."""
    return v if 0.0 <= v else 0.0


def model_list(text):
    """Builder list rule: count = max(1, strlen >> 2); the slots hold the first raw codes (non-empty
    tokens, separators ' ,-_' and tab), in order, and the rest stay zero."""
    n = len(text) >> 2 if text else 0
    count = 1 if n < 2 else n
    toks = [model_raw(t) for t in re.split(r"[ ,\-_\t]", text) if t] if text else []
    slots = [0] * count
    for i, t in enumerate(toks[:count]):
        slots[i] = t
    return count, slots


LEVEL_FIELDS = [  # (offset in the 0x68 record, field name, reader kind)
    (0x04, "Cast", "real"), (0x08, "Dur", "real"), (0x0C, "HeroDur", "real"),
    (0x10, "Cost", "int"), (0x14, "Cool", "cool"), (0x18, "Area", "real"), (0x1C, "Rng", "real"),
    (0x20, "DataA", "real"), (0x24, "DataB", "real"), (0x28, "DataC", "real"),
    (0x2C, "DataD", "real"), (0x30, "DataE", "real"), (0x34, "DataF", "real"),
    (0x38, "DataG", "real"), (0x3C, "DataH", "real"), (0x40, "DataI", "real"),
]
POINTER_WORDS = {0x50, 0x54, 0x60}   # list and array pointers: checked by content, not by value


def expected_level(row):
    rec = bytearray(0x68)
    for off, field, kind in LEVEL_FIELDS:
        text = CELLS[(field, row)]
        if kind == "real":
            v = float(text) if text else 0.0
            rec[off:off + 4] = struct.pack("<I", f32bits(v))
        elif kind == "int":
            rec[off:off + 4] = struct.pack("<I", ui.model_cell_int(text))
        else:
            v = float(text) if text else 0.0
            rec[off:off + 4] = struct.pack("<I", f32bits(model_cool(v)))
    uid = CELLS[("UnitID", row)]
    rec[0x44:0x48] = struct.pack("<I", model_raw(uid) if uid else 0)
    bcount, bslots = model_list(CELLS[("BuffID", row)])
    ecount, eslots = model_list(CELLS[("EfctID", row)])
    rec[0x48:0x4C] = struct.pack("<I", bcount)    # capacity (no granule rounding in the stub)
    rec[0x4C:0x50] = struct.pack("<I", bcount)    # count
    rec[0x58:0x5C] = struct.pack("<I", ecount)
    rec[0x5C:0x60] = struct.pack("<I", ecount)
    return bytes(rec), bslots, eslots


def main():
    install_stubs()
    failures = 0
    checks = 0

    def check(label, ok):
        nonlocal failures, checks
        checks += 1
        if not ok:
            failures += 1
            print("MISMATCH", label)

    # ---- raw code parser (original code, no stub) ----
    raw_cases = ["", "a", "abcd", "abcde", "'hfo", "\xff\x80\x01", "A\0B", "HF\x00\x00"]
    for s in raw_cases:
        got = emu.call(RAWCODE, ecx=put_cstr(s)) & 0xFFFFFFFF
        check("rawcode %r got=%08x want=%08x" % (s, got, model_raw(s)), got == model_raw(s))
    print("raw code parser cases", len(raw_cases))

    # ---- level accessor and level up (original code) ----
    def_test = alloc(0x60)
    level_base = alloc(3 * 0x68)
    wr32(def_test + 0x2C, 1)
    wr32(def_test + 0x50, 3)
    wr32(def_test + 0x54, level_base)
    ab = alloc(0x80)
    wr32(ab + 0x54, def_test)
    # FUN_6F02E6A0 returns the first dword of the level record (the target flags), not its address:
    # the ability's +0x4C is that word. Record i starts with the marker 0x100 + i.
    for i in range(3):
        wr32(level_base + i * 0x68, 0x100 + i)
    acc_cases = [0, 1, 2, 3, 4, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF]
    for idx in acc_cases:
        got = emu.call(ACC, ecx=ab, stack_args=[idx]) & 0xFFFFFFFF
        want = 0x100 + idx if idx < 3 else 0
        check("accessor idx=%x got=%x" % (idx, got), got == want)
    wr32(def_test + 0x2C, 0)
    check("accessor invalid definition", emu.call(ACC, ecx=ab, stack_args=[0]) & 0xFFFFFFFF == 0)
    wr32(def_test + 0x2C, 1)
    wr32(ab + 0x54, 0)
    DEF_FOR_GET[0] = def_test
    got = emu.call(ACC, ecx=ab, stack_args=[1]) & 0xFFFFFFFF
    check("accessor fetches the definition", got == 0x101 and rd32(ab + 0x54) == def_test)
    wr32(ab + 0x50, 0)
    wr32(ab + 0x54, def_test)
    for idx in [1, 2, 3]:
        emu.call(LEVELUP, ecx=ab)
        want = 0x100 + idx if idx < 3 else 0
        check("level up to %d (+0x4C=%x)" % (idx, rd32(ab + 0x4C)),
              rd32(ab + 0x50) == idx and rd32(ab + 0x4C) == want)
    print("accessor and level up cases", len(acc_cases) + 4)

    # ---- per-level getter family: each reads one record offset (original code) ----
    # out-style: (ECX=ability, [esp+4]=out pointer, [esp+8]=index) -> writes the dword, invalid -> 0.0
    # ret-style: (ECX=ability, [esp+4]=index) -> returns the dword, invalid -> 0
    getters_out = {0x04: 0x6F02E6F0, 0x08: 0x6F02E760, 0x0C: 0x6F02E7D0, 0x14: 0x6F02E890,
                   0x18: 0x6F02E900, 0x1C: 0x6F02E970, 0x20: 0x6F02E9E0, 0x24: 0x6F02EA50,
                   0x28: 0x6F02EAC0, 0x2C: 0x6F02EB30, 0x30: 0x6F02EBA0, 0x34: 0x6F02EC10,
                   0x38: 0x6F02EC80, 0x3C: 0x6F02ECF0, 0x40: 0x6F02ED60}
    getters_ret = {0x10: 0x6F02E840, 0x44: 0x6F02EDD0}
    for i in range(3):
        for off in range(0x00, 0x68, 4):
            wr32(level_base + i * 0x68 + off, 0x500000 + i * 0x100 + off)
    out_buf = alloc(8)
    getter_checks = 0
    for off, fn in getters_out.items():
        for idx in [0, 2, 3]:
            wr32(out_buf, 0xDEADBEEF)
            emu.call(fn, ecx=ab, stack_args=[out_buf, idx])
            want = 0x500000 + idx * 0x100 + off if idx < 3 else 0
            check("getter +%#x idx=%d" % (off, idx), rd32(out_buf) == want)
            getter_checks += 1
    for off, fn in getters_ret.items():
        for idx in [0, 2, 3]:
            got = emu.call(fn, ecx=ab, stack_args=[idx]) & 0xFFFFFFFF
            want = 0x500000 + idx * 0x100 + off if idx < 3 else 0
            check("getter ret +%#x idx=%d" % (off, idx), got == want)
            getter_checks += 1
    print("per-level getter checks", getter_checks)

    # ---- the definition builder (original code) ----
    rows = {
        1: {"targs": "", "Area": "250", "Cast": "0.25", "Dur": "10", "HeroDur": "12.5", "Cost": "75",
            "Cool": "-3", "Rng": "600", "DataA": "1", "DataB": "2.5", "DataC": "3", "DataD": "4",
            "DataE": "5", "DataF": "6", "DataG": "7", "DataH": "8", "DataI": "9",
            "UnitID": "hfoo", "BuffID": "BHwe,BUan", "EfctID": "Hfoo"},
        2: {"targs": "", "Area": "0", "Cast": "0", "Dur": "-1", "HeroDur": "0.5", "Cost": "0x10",
            "Cool": "-0", "Rng": "1e3", "DataA": "0.1", "DataB": "0", "DataC": "0", "DataD": "0",
            "DataE": "0", "DataF": "0", "DataG": "0", "DataH": "0", "DataI": "0",
            "UnitID": "", "BuffID": "", "EfctID": "AHbz"},
        3: {"targs": "", "Area": "1.5", "Cast": "2", "Dur": "3", "HeroDur": "4", "Cost": "-5",
            "Cool": "2.5", "Rng": "9", "DataA": "1", "DataB": "1", "DataC": "1", "DataD": "1",
            "DataE": "1", "DataF": "1", "DataG": "1", "DataH": "1", "DataI": "1",
            "UnitID": "AHbz", "BuffID": "AHbz", "EfctID": "a,b,c,d"},
    }
    for row, d in rows.items():
        for field, text in d.items():
            CELLS[(field, row)] = text
    DEF_INTS.update({"version": "1", "checkDep": "1", "levels": "3", "reqLevel": "6",
                     "levelSkip": "0", "priority": "0"})
    raw = 0x68666F6F
    NODE[0] = 0
    try:
        emu.call(BUILD, ecx=raw)
    except Exception as exc:  # report where the original code stopped
        from unicorn.x86_const import UC_X86_REG_EIP
        print("builder stopped:", exc, "eip=%08x" % mu.reg_read(UC_X86_REG_EIP))
        print("last stubs:", TRACE[-12:])
        raise
    node = NODE[0]
    check("node raw code +0x14", rd32(node + 0x14) == raw)
    check("definition valid +0x2C", rd32(node + 0x2C) == 1)
    check("definition +0x30/+0x34 sources", rd32(node + 0x30) == 0x11 and rd32(node + 0x34) == 0x22)
    check("definition checkDep +0x38", rd32(node + 0x38) == 1)
    check("definition levels (max level) +0x3C", rd32(node + 0x3C) == 3)
    check("definition reqLevel +0x40", rd32(node + 0x40) == 6)
    check("definition priority/levelSkip +0x44/+0x48", rd32(node + 0x44) == 0 and rd32(node + 0x48) == 0)
    check("definition version +0x1F8", rd32(node + 0x1F8) == 1)
    check("definition level count +0x50", rd32(node + 0x50) == 3)
    base = rd32(node + 0x54)
    for k in range(3):
        row = k + 1
        want, bslots, eslots = expected_level(row)
        got = bytes(mu.mem_read(base + k * 0x68, 0x68))
        bad = [hex(off) for off in range(0, 0x64, 4)
               if off not in POINTER_WORDS and got[off:off + 4] != want[off:off + 4]]
        check("level %d record words differ at %s" % (row, bad), not bad)
        bptr = struct.unpack("<I", got[0x50:0x54])[0]
        eptr = struct.unpack("<I", got[0x60:0x64])[0]
        blist = [rd32(bptr + 4 * i) for i in range(len(bslots))]
        elist = [rd32(eptr + 4 * i) for i in range(len(eslots))]
        check("level %d buff slots want %s got %s" % (row, bslots, blist), blist == bslots)
        check("level %d efct slots want %s got %s" % (row, eslots, elist), elist == eslots)
        check("level %d record bytes 0x64..0x67 untouched" % row, got[0x64:0x68] == b"\0" * 4)
    check("targs cell read once per level", len(log["targs"]) == 3)
    print("definition builder: 3 levels, stubs: levelset", log["levelset"], "list allocs", log["listalloc"])
    print("checks", checks, "failures", failures)
    return failures


if __name__ == "__main__":
    sys.exit(1 if main() else 0)
