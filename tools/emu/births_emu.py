"""Registration (birth-tag) counts of the movement-object creators of Game.dll 1.26a (docs/spec/lockstep.md section 17).

Real code runs in Unicorn on a synthetic world: the pathing-object registry (`0x6F4AEFF0` register, `0x6F4AEF00` unregister,
section 16) is the real code on a fake world object at [0x6FAB7788]; the movement world [0x6FAB73D8] holds the object pools
used here (CDataAllocator layout, initialised by hand). The Storm heap, memset, the cookie check and the SEH `fs:[0]` accesses
are stubbed as in vision_grid_emu.py. Counters are the registry's own Y (`world+0x50`) plus a code hook on both entry points.

Checks (each over many cases, numbers printed):
  C1  `0x6F48DF00` (create CPrCluster + attach)     : registrations per call
  C2  `0x6F474220` (create CPrClusterGen + attach)  : registrations per call
  C3  `0x6F48BFD0` (create CLrPath + attach)        : registrations per call
  C5  flush `0x6F493590` of a cluster generator with k valid members (random k, random member slots); callees that are not
      creators are stubbed (STUBS below): registrations = 2k, unregistrations = 1 (the generator releases itself)

    GAME_DLL=/path/Game.dll py tools/emu/births_emu.py [--cases N]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import pefile  # noqa: E402
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ_UNMAPPED, UC_HOOK_MEM_WRITE_UNMAPPED  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_EIP  # noqa: E402

mu = emu.mu
M = 0xFFFFFFFF
WORLD = 0x00210000          # registry world ([0x6FAB7788])
TAB0 = 0x00220000
TAB1 = 0x00230000
GW = 0x00280000             # movement world ([0x6FAB73D8]), 0x4000 bytes
HEAP = 0x00400000
HEAP_SIZE = 0x00800000
SCRATCH = 0x00E00000
STUB_ALLOC = 0x00300100
STUB_FREE = 0x00300200
STUB_REALLOC = 0x00300300
UNITS = 0x00F00000          # synthetic unit objects
mu.mem_map(WORLD, 0x1000)
mu.mem_map(TAB0, 0x10000)
mu.mem_map(TAB1, 0x10000)
mu.mem_map(GW, 0x4000)
mu.mem_map(HEAP, HEAP_SIZE)
mu.mem_map(SCRATCH, 0x1000)
mu.mem_map(UNITS, 0x40000)
mu.msr_write(0xC0000100, SCRATCH)   # fs base: fs:[0] reads the (zero) SEH chain head


def rd32(a):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack('<I', v & M))


for a in (STUB_ALLOC, STUB_FREE, STUB_REALLOC):
    mu.mem_write(a, b'\xc3')
_heap = [HEAP]
_sizes = {}


def _alloc(n):
    p = _heap[0]
    _heap[0] += (max(n, 1) + 0x1F) & ~0x1F
    if _heap[0] > HEAP + HEAP_SIZE:
        raise RuntimeError('heap exhausted')
    _sizes[p] = n
    mu.mem_write(p, bytes(n))
    return p


def _storm(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    if addr == STUB_ALLOC:
        res, nargs = _alloc(rd32(esp + 4)), 4
    elif addr == STUB_FREE:
        res, nargs = 1, 4
    else:
        old, n = rd32(esp + 4), rd32(esp + 8)
        res = _alloc(n)
        if old:
            keep = min(_sizes.get(old, 0), n)
            if keep:
                mu.mem_write(res, bytes(mu.mem_read(old, keep)))
        nargs = 5
    uc.reg_write(UC_X86_REG_EAX, res)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + 4 * nargs)
    uc.reg_write(UC_X86_REG_EIP, ret)


for a in (STUB_ALLOC, STUB_FREE, STUB_REALLOC):
    mu.hook_add(UC_HOOK_CODE, _storm, begin=a, end=a)
_pe = pefile.PE(emu.PATH, fast_load=True)
_pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY['IMAGE_DIRECTORY_ENTRY_IMPORT']])
for entry in _pe.DIRECTORY_ENTRY_IMPORT:
    if entry.dll.decode().lower() != 'storm.dll':
        continue
    for imp in entry.imports:
        if imp.ordinal == 401:
            wr32(imp.address, STUB_ALLOC)
        elif imp.ordinal == 403:
            wr32(imp.address, STUB_FREE)
        elif imp.ordinal == 405:
            wr32(imp.address, STUB_REALLOC)


def _memset(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    dst, val, n = rd32(esp + 4), rd32(esp + 8) & 0xFF, rd32(esp + 12)
    if n:
        mu.mem_write(dst, bytes([val]) * n)
    uc.reg_write(UC_X86_REG_EAX, dst)
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def _cookie(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    uc.reg_write(UC_X86_REG_EIP, rd32(esp))
    uc.reg_write(UC_X86_REG_ESP, esp + 4)


mu.hook_add(UC_HOOK_CODE, _memset, begin=0x6F7E11C2, end=0x6F7E11C2)
mu.hook_add(UC_HOOK_CODE, _cookie, begin=0x6F7E1059, end=0x6F7E1059)

for a in (0x6FAB7788, 0x6FAB778C):
    wr32(a, WORLD)
wr32(0x6FAB73D8, GW)


def reset_world():
    _heap[0] = HEAP
    mu.mem_write(WORLD, bytes(0xDC))
    mu.mem_write(TAB0, bytes(0x10000))
    mu.mem_write(TAB1, bytes(0x10000))
    for base, tab in ((WORLD, TAB0), (WORLD + 0x20, TAB1)):
        wr32(base + 0x0C, tab)
        wr32(base + 0x14, 16)
        wr32(base + 0x18, 2048)
        wr32(base + 0x1C, 0)
    wr32(WORLD + 0x40, M)
    wr32(WORLD + 0x44, M)
    mu.mem_write(GW, bytes(0x4000))
    for off in (0x638, 0x678, 0x958):          # generator, cluster and path pools: element size, elements per block
        wr32(GW + off + 0, 0x200)
        wr32(GW + off + 4, 16)


PTRS = {'reg': 0, 'unreg': 0, 'reg_cls': [], 'unreg_cls': []}


def _count_reg(uc, addr, size, user):
    PTRS['reg'] += 1
    PTRS['reg_cls'].append(rd32(uc.reg_read(UC_X86_REG_ECX)))


def _count_unreg(uc, addr, size, user):
    PTRS['unreg'] += 1
    PTRS['unreg_cls'].append(rd32(uc.reg_read(UC_X86_REG_ECX)))


mu.hook_add(UC_HOOK_CODE, _count_reg, begin=0x6F4AEFF0, end=0x6F4AEFF0)
mu.hook_add(UC_HOOK_CODE, _count_unreg, begin=0x6F4AEF00, end=0x6F4AEF00)


def ret_stub(addr, pops=0, value=0):
    """Replace the function at `addr` with `return value` (the callee pops `pops` bytes of arguments)."""
    def h(uc, a, s, u):
        esp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, value)
        uc.reg_write(UC_X86_REG_EIP, rd32(esp))
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops)
    mu.hook_add(UC_HOOK_CODE, h, begin=addr, end=addr)


TRACE = []


def _tr(uc, addr, size, user):
    TRACE.append(addr)
    if len(TRACE) > 400:
        del TRACE[:200]


def _unmapped(uc, access, address, size, value, user):
    print('trace tail', [hex(a) for a in TRACE[-60:]])
    print('UNMAPPED access', access, hex(address), 'eip', hex(uc.reg_read(UC_X86_REG_EIP)))
    return False


mu.hook_add(UC_HOOK_MEM_READ_UNMAPPED | UC_HOOK_MEM_WRITE_UNMAPPED, _unmapped)
if os.environ.get('TRACE'):
    mu.hook_add(UC_HOOK_CODE, _tr)

LOOKUP = {}


def _lookup(uc, addr, size, user):
    """0x6F03FA30 fastcall(ECX = id, EDX = tag): the object registered under that pair, else 0."""
    ecx = uc.reg_read(UC_X86_REG_ECX)
    edx = uc.reg_read(UC_X86_REG_EDX)
    esp = uc.reg_read(UC_X86_REG_ESP)
    uc.reg_write(UC_X86_REG_EAX, LOOKUP.get((ecx, edx), 0))
    uc.reg_write(UC_X86_REG_EIP, rd32(esp))
    uc.reg_write(UC_X86_REG_ESP, esp + 4)


mu.hook_add(UC_HOOK_CODE, _lookup, begin=0x6F03FA30, end=0x6F03FA30)

# STUBS: callees of the flush that neither register nor unregister anything (checked below by the counters staying exact):
#   0x6F4927A0 (current mover manager lookup), 0x6F4927F0 (member detach with path maths; recursion), 0x6F4932D0 (cluster init),
#   0x6F491410 / 0x6F48E5F0 / 0x6F48E600 / 0x6F48F650 (field copies)
STUBS = ((0x6F4927A0, 0), (0x6F4927F0, 8), (0x6F4932D0, 4), (0x6F491410, 8), (0x6F48E5F0, 4), (0x6F48E600, 4), (0x6F48F650, 4))
_unit_n = [0]


def make_unit():
    """A synthetic unit/mover object: pair (id, tag) at +0x14/+0x18, a refcount block at +0x98, a path object at +0xA8."""
    n = _unit_n[0]
    _unit_n[0] += 1
    u = UNITS + 0x800 * (n % 64)
    mu.mem_write(u, bytes(0x800))
    wr32(u + 0x14, 0x1000 + n)
    wr32(u + 0x18, 0x5000 + n)
    wr32(u + 152, u + 0x400)
    wr32(u + 168, u + 0x500)
    LOOKUP[(0x1000 + n, 0x5000 + n)] = u
    return u


def births(fn, ecx=None, edx=None, args=()):
    r0, u0, y0 = PTRS['reg'], PTRS['unreg'], rd32(WORLD + 0x50)
    emu.call(fn, ecx=ecx, edx=edx, stack_args=args)
    return PTRS['reg'] - r0, PTRS['unreg'] - u0, rd32(WORLD + 0x50) - y0


def main():
    cases = 300
    if '--cases' in sys.argv:
        cases = int(sys.argv[sys.argv.index('--cases') + 1])
    out = GW + 0x3000
    reset_world()
    got = {}
    for i in range(cases):
        got.setdefault('C1 0x6F48DF00 cluster+path', set()).add(births(0x6F48DF00, ecx=out, edx=0, args=(1,)))
        got.setdefault('C2 0x6F474220 cluster generator', set()).add(births(0x6F474220, ecx=out + 8, edx=0, args=(1,)))
        got.setdefault('C3 0x6F48BFD0 path', set()).add(births(0x6F48BFD0, ecx=out + 16, edx=0, args=(1,)))
        got.setdefault('C1b 0x6F48DF00 no attach', set()).add(births(0x6F48DF00, ecx=out + 24, edx=0, args=(0,)))
    for k, v in got.items():
        print(k, '(reg, unreg, dY):', sorted(v), f'cases={cases}')
    for addr, pops in STUBS:
        ret_stub(addr, pops)
    rng = random.Random(0x5EED)
    seen = {}
    bad = 0
    for case in range(cases):
        reset_world()
        LOOKUP.clear()
        births(0x6F474220, ecx=out + 8, edx=0, args=(1,))
        g = rd32(out + 8)
        k = rng.randint(0, 12)
        for _ in range(k):
            births(0x6F491EA0, ecx=g, args=(make_unit(), 1))
        r = births(0x6F493590, ecx=g)
        seen.setdefault(k, set()).add(r)
        if r[0] != 2 * k or r[1] != 1:
            bad += 1
    print('C5 flush 0x6F493590 per member count k -> (reg, unreg, dY):')
    for k in sorted(seen):
        print('   k =', k, sorted(seen[k]))
    print(f'C5 cases={cases} mismatches against (2k, 1): {bad}')
    # C8: the mover start 0x6F474D40 (MoveTo) on a live unit: generator + cluster + path
    # (Y + 3, one unregistration); on a dead unit (the pair no longer resolves) only the generator is born.
    for addr, stub in ((0x6F4956C0, 28), (0x6F4937D0, 4), (0x6F48EF80, 4), (0x6F48F010, 4), (0x6F471AB0, 8), (0x6F471B70, 8),
                       (0x6F4A73F0, 4), (0x6F4937B0, 4), (0x6F48EEC0, 4), (0x6F48F070, 4), (0x6F48E5D0, 4)):
        ret_stub(addr, stub)
    mu.mem_map(0x00270000, 0x1000)
    mu.mem_write(0x00270100, bytes([0xC3]))
    for i in range(64):
        wr32(0x00270000 + 4 * i, 0x00270100)
    wr32(0x6FAB7368, GW + 0x2000)       # movement bounds block read by 0x6F474D40 (zero floats are fine here)
    seen8 = {}
    for case in range(cases):
        reset_world()
        LOOKUP.clear()
        u = make_unit()
        m = GW + 0x1000
        mu.mem_write(m, bytes(0x100))
        wr32(m, 0x00270000)
        wr32(m + 8, rd32(u + 0x14))
        wr32(m + 12, rd32(u + 0x18))
        flt = GW + 0x1800
        for i in range(8):
            mu.mem_write(flt + 4 * i, struct.pack('<f', rng.uniform(-3000, 3000)))
        alive = rng.random() < 0.8
        if not alive:
            LOOKUP.clear()
        # first, ensure the unit's own path exists like creation does
        which = 'move'
        r = births(0x6F474D40, ecx=m, args=(flt, flt + 4, 852374, 852376, m, flt + 8, 1, flt + 12))
        seen8.setdefault((which, alive), set()).add(r)
    print('C8 mover start (reg, unreg, dY) by kind and unit alive:', {str(k): sorted(v) for k, v in seen8.items()})
    # C7: releasing a cluster through its vtable slot 4 (0x6F493130): the path it owns and the cluster itself unregister
    seen7 = {}
    for case in range(cases):
        reset_world()
        LOOKUP.clear()
        births(0x6F48DF00, ecx=out, edx=0, args=(1,))
        c = rd32(out)
        vt = rd32(c)
        r = births(rd32(vt + 16), ecx=c, args=(0,))
        seen7.setdefault('release', set()).add(r)
        # the freed pool entry is reused by the next creation (LIFO): create again and release again
        births(0x6F48DF00, ecx=out, edx=0, args=(1,))
        seen7.setdefault('release2', set()).add(births(rd32(rd32(rd32(out)) + 16), ecx=rd32(out), args=(0,)))
    print('C7 cluster release via vtable slot 4 (reg, unreg, dY):', {k: sorted(v) for k, v in seen7.items()})
    # C6: unit creation, 0x6F2A0E30 calls 0x6F474C10 on the unit's mover (unit+0x164) with a2 = 0: the first call
    # creates the mover's path object (one registration); later calls find it and register nothing.
    seen6 = {}
    for case in range(cases):
        reset_world()
        LOOKUP.clear()
        u = make_unit()
        wr32(u + 168, 0)   # the unit's path pointer (unit+0xA8) is empty before the first call
        m = GW + 0x1000 + 0x100 * (case % 8)
        mu.mem_write(m, bytes(0x100))
        wr32(m + 8, rd32(u + 0x14))
        wr32(m + 12, rd32(u + 0x18))
        first = births(0x6F474C10, ecx=m, args=(rng.randint(0, 1) * 0,))
        again = births(0x6F474C10, ecx=m, args=(rng.randint(0, 1),))
        seen6.setdefault('first', set()).add(first)
        seen6.setdefault('again', set()).add(again)
    print('C6 0x6F474C10 on a fresh mover (first / again) -> (reg, unreg, dY):', {k: sorted(v) for k, v in seen6.items()})


if __name__ == '__main__':
    main()
