"""Differential test of the pathing-object registry (docs/spec/lockstep.md section 16).

Runs the real code of Game.dll 1.26a in Unicorn and compares it with the Python model that crates/sim/src/sync.rs
(`PrRegistry`) follows:

  * 0x6F4AEFF0  CPrRgEntry registration (takes a table slot, assigns the tag, bumps the birth counter)
  * 0x6F4AEFC0  tag assignment (counter world+0x50, or world+0x54 for slots with bit 31)
  * 0x6F4AEF00  unregistration (frees the slot, clears slot/tag, calls virtual slot 1)
  * 0x6F4BC310 / 0x6F4BC200 and the slot allocators/free-list pushes 0x6F4BC240 / 0x6F4BC2B0 / 0x6F4BC1D0

The world object is a fake 0xDC-byte block at [0x6FAB7788]; the two entry tables (8-byte (key, object) entries) are
big enough that the growth routine is never reached.

    GAME_DLL=/path/Game.dll py tools/emu/pr_registry_emu.py [--vectors N] [--embed-rs]

`--embed-rs` prints a few sequences with the original's final state as Rust arrays (no game bytes), used by
crates/sim/tests/pr_registry_vectors.rs.
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402
from emu import mu, call, u32  # noqa: E402

WORLD_PTR_SLOTS = (0x6FAB7788, 0x6FAB778C)
WORLD = 0x00210000
TAB0 = 0x00220000
TAB1 = 0x00230000
OBJS = 0x00240000
DESCS = 0x00250000
VT = 0x00260000
CAP = 2048
M = 0xFFFFFFFF
FREE_END = 0xFFFFFFFF

mu.mem_map(WORLD, 0x1000)
mu.mem_map(TAB0, 0x10000)
mu.mem_map(TAB1, 0x10000)
mu.mem_map(OBJS, 0x10000)
mu.mem_map(DESCS, 0x1000)
mu.mem_map(VT, 0x1000)
mu.mem_write(VT + 0x100, b'\xc3')  # ret
mu.mem_write(VT, struct.pack('<II', VT + 0x100, VT + 0x100))
for a in WORLD_PTR_SLOTS:
    mu.mem_write(a, struct.pack('<I', WORLD))

OBJ_SIZE = 0x40


def w32(a, v):
    mu.mem_write(a, struct.pack('<I', v & M))


def r32(a):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def reset_world():
    mu.mem_write(WORLD, b'\0' * 0xDC)
    mu.mem_write(TAB0, b'\0' * 0x10000)
    mu.mem_write(TAB1, b'\0' * 0x10000)
    # table 0 at world+0x00 and table 1 at world+0x20: data +0xC, step +0x14, capacity +0x18, count +0x1C
    for base, tab in ((WORLD, TAB0), (WORLD + 0x20, TAB1)):
        w32(base + 0x0C, tab)
        w32(base + 0x14, 16)
        w32(base + 0x18, CAP)
        w32(base + 0x1C, 0)
    w32(WORLD + 0x40, M)
    w32(WORLD + 0x44, M)


class Model:
    """The Python reference (sim::sync::PrRegistry)."""

    def __init__(self):
        self.entries = [[], []]  # (key, obj)
        self.head = [M, M]
        self.live = [0, 0]
        self.y = 0   # world+0x50
        self.y2 = 0  # world+0x54
        self.slot = {}
        self.tag = {}

    def alloc(self, t, obj):
        if self.head[t] == M:
            idx = len(self.entries[t])
            self.entries[t].append([0xFFFFFFFE, obj])
        else:
            idx = self.head[t]
            self.head[t] = self.entries[t][idx][0]
            self.entries[t][idx] = [0xFFFFFFFE, obj]
        self.live[t] += 1
        return idx

    def alloc_at(self, t, idx, obj):
        while len(self.entries[t]) <= idx:
            self.entries[t].append([M, 0])
        self.entries[t][idx] = [0xFFFFFFFE, obj]
        self.live[t] += 1
        return idx

    def free(self, t, idx):
        self.entries[t][idx] = [self.head[t], 0]
        self.head[t] = idx
        self.live[t] -= 1

    def next_tag(self, slot):
        if slot & 0x80000000:
            v = self.y2
            self.y2 += 1
        else:
            v = self.y
            self.y += 1
        return v

    def register(self, obj, mode, fixed=None, tag=None):
        """mode: 'auto' (no descriptor / +0x24 == -1), 'flag' (+0x24 == -2), 'fixed' (+0x24 = index, +0x28 = tag)."""
        if mode == 'auto':
            idx = self.alloc(0, obj)
            self.slot[obj] = idx
            self.tag[obj] = self.y
            self.y += 1
        elif mode == 'flag':
            idx = self.alloc(1, obj) | 0x80000000
            self.slot[obj] = idx
            self.tag[obj] = self.next_tag(idx)
        else:
            self.alloc_at(0, fixed, obj)
            self.slot[obj] = fixed
            self.tag[obj] = self.next_tag(fixed) if tag == M else tag

    def unregister(self, obj):
        s = self.slot.get(obj, M)
        if s != M:
            if s & 0x80000000:
                self.free(1, s & 0x7FFFFFFF)
            else:
                self.free(0, s)
        self.slot[obj] = M
        self.tag[obj] = M

    def x(self):
        return self.head[0] if self.head[0] != M else len(self.entries[0])


def snapshot_hw():
    st = {
        'y': r32(WORLD + 0x50), 'y2': r32(WORLD + 0x54),
        'head0': r32(WORLD + 0x40), 'head1': r32(WORLD + 0x44),
        'live0': r32(WORLD + 0x48), 'live1': r32(WORLD + 0x4C),
        'n0': r32(WORLD + 0x1C), 'n1': r32(WORLD + 0x3C),
    }
    st['entries0'] = [[r32(TAB0 + 8 * i), r32(TAB0 + 8 * i + 4)] for i in range(st['n0'])]
    st['entries1'] = [[r32(TAB1 + 8 * i), r32(TAB1 + 8 * i + 4)] for i in range(st['n1'])]
    return st


def compare(model, hw, objs):
    errs = []
    chk = [('y', model.y, hw['y']), ('y2', model.y2, hw['y2']), ('head0', model.head[0], hw['head0']),
           ('head1', model.head[1], hw['head1']), ('live0', model.live[0], hw['live0']),
           ('live1', model.live[1], hw['live1']), ('n0', len(model.entries[0]), hw['n0']),
           ('n1', len(model.entries[1]), hw['n1'])]
    for name, a, b in chk:
        if a != b:
            errs.append(f'{name}: model {a:#x} hw {b:#x}')
    if model.entries[0] != hw['entries0']:
        errs.append('entries0')
    if model.entries[1] != hw['entries1']:
        errs.append('entries1')
    for o, addr in objs.items():
        if model.slot.get(o, M) != r32(addr + 0x14) or model.tag.get(o, M) != r32(addr + 0x18):
            errs.append(f'obj {o} slot/tag')
    return errs


def run_sequence(rng, nops, embed=None):
    reset_world()
    m = Model()
    objs = {}      # model id -> address
    live = []
    nxt = 0
    log = []
    for step in range(nops):
        roll = rng.random()
        if (roll < 0.55 or not live) and nxt < 60:
            kind = rng.choice(['auto', 'auto', 'auto', 'none', 'm1', 'flag', 'fixed_auto', 'fixed_tag'])
            addr = OBJS + OBJ_SIZE * nxt
            mu.mem_write(addr, b'\0' * OBJ_SIZE)
            w32(addr, VT)
            w32(addr + 0x14, M)
            w32(addr + 0x18, M)
            oid = addr
            nxt += 1
            objs[oid] = addr
            d = DESCS + 0x40 * (oid % 8)
            if kind == 'none':
                call(0x6F4AEFF0, ecx=addr, stack_args=(0,))
                m.register(oid, 'auto')
                log.append(('reg_none', oid))
            elif kind == 'auto' or kind == 'm1':
                w32(d + 0x24, M)
                w32(d + 0x28, 0x1234)
                call(0x6F4AEFF0, ecx=addr, stack_args=(d,))
                m.register(oid, 'auto')
                log.append(('reg_auto', oid))
            elif kind == 'flag':
                w32(d + 0x24, 0xFFFFFFFE)
                w32(d + 0x28, M)
                call(0x6F4AEFF0, ecx=addr, stack_args=(d,))
                m.register(oid, 'flag')
                log.append(('reg_flag', oid))
            else:
                # explicit slot: the next free index at the end of the table (never an index in use)
                fixed = len(m.entries[0]) + rng.randint(0, 2)
                tag = M if kind == 'fixed_auto' else rng.randint(0, 1000)
                w32(d + 0x24, fixed)
                w32(d + 0x28, tag)
                call(0x6F4AEFF0, ecx=addr, stack_args=(d,))
                m.register(oid, 'fixed', fixed=fixed, tag=tag)
                log.append(('reg_fixed', oid, fixed, tag))
            live.append(oid)
        else:
            oid = live.pop(rng.randrange(len(live)))
            call(0x6F4AEF00, ecx=objs[oid], stack_args=(0,))
            m.unregister(oid)
            log.append(('unreg', oid))
        errs = compare(m, snapshot_hw(), objs)
        if errs:
            return errs, step, log
    # X as the dump prints it
    hx = r32(WORLD + 0x40)
    hx = r32(WORLD + 0x1C) if hx == M else hx
    if hx != m.x():
        return [f'x: model {m.x()} hw {hx}'], nops, log
    return [], nops, log


def main():
    nvec = 400
    if '--vectors' in sys.argv:
        nvec = int(sys.argv[sys.argv.index('--vectors') + 1])
    rng = random.Random(0x9E3779B9)
    bad = 0
    ops = 0
    for v in range(nvec):
        n = rng.randint(5, 120)
        errs, step, log = run_sequence(rng, n)
        ops += n
        if errs:
            bad += 1
            print('MISMATCH seq', v, 'step', step, errs[:3], log[-4:])
            if bad > 5:
                break
    print(f'{nvec} sequences, {ops} operations, mismatches: {bad}')
    if '--embed-rs' in sys.argv:
        embed(rng)
    return 1 if bad else 0


def embed(rng):
    """Prints ten short sequences (auto registrations and frees) with the original's counters as Rust data."""
    print('// ops: (0 = register auto, 1 = register flagged, 2 = unregister live index i); final (y, y2, x, live0, live1)')
    for _ in range(10):
        reset_world()
        ops = []
        live = []
        nxt = 0
        for _ in range(rng.randint(8, 40)):
            if (rng.random() < 0.6 or not live) and nxt < 60:
                addr = OBJS + OBJ_SIZE * nxt
                mu.mem_write(addr, b'\0' * OBJ_SIZE)
                w32(addr, VT)
                w32(addr + 0x14, M)
                w32(addr + 0x18, M)
                d = DESCS
                if rng.random() < 0.2:
                    w32(d + 0x24, 0xFFFFFFFE)
                    w32(d + 0x28, M)
                    ops.append((1, 0))
                else:
                    w32(d + 0x24, M)
                    ops.append((0, 0))
                call(0x6F4AEFF0, ecx=addr, stack_args=(d,))
                live.append(nxt)
                nxt += 1
            else:
                i = rng.randrange(len(live))
                oid = live.pop(i)
                call(0x6F4AEF00, ecx=OBJS + OBJ_SIZE * oid, stack_args=(0,))
                ops.append((2, oid))
        hx = r32(WORLD + 0x40)
        hx = r32(WORLD + 0x1C) if hx == M else hx
        print('    (&%s, [%d, %d, %d, %d, %d]),' % (
            '[' + ', '.join('(%d, %d)' % o for o in ops) + ']',
            r32(WORLD + 0x50), r32(WORLD + 0x54), hx, r32(WORLD + 0x48), r32(WORLD + 0x4C)))


if __name__ == '__main__':
    sys.exit(main())
