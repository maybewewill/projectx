#!/usr/bin/env python3
"""Generates handle-table test vectors from the original allocator (Game.dll 1.26a, Unicorn).

Random sequences of create / add_ref / release are run through the real allocator code of
handle_alloc_emu.py. After every operation the script records the returned id, the reference count
of the id the operation touched, the table length and the free-stack length. The result is written
as a Rust constant between the markers in crates/sim/tests/handles.rs (or printed with --stdout).

Object death (event 0x80267) is not part of this sequence: the harness does not run that path.

Usage:
  GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn and pefile> \
      python3 tools/emu/handle_vectors.py [--seed 1] [--ops 400] [--rs crates/sim/tests/handles.rs]
"""
import argparse
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
# Importing the harness runs its own scenarios first (their PASS/FAIL lines are printed).
import handle_alloc_emu as h  # noqa: E402

BASE = 0x100000
BEGIN = '// BEGIN GENERATED (tools/emu/handle_vectors.py)'
END = '// END GENERATED'

KIND = {'C': 'Create', 'A': 'AddRef', 'R': 'Release'}


def pick(rnd, issued, live_bias):
    """Mostly a live id (count > 0) when live_bias is set; otherwise any issued id, freed ones too."""
    live = [i for i in issued if h.refcnt(i) > 0]
    if live_bias and live and rnd.random() < 0.9:
        return rnd.choice(live)
    return rnd.choice(issued)


def generate(seed, nops, live_bias):
    rnd = random.Random(seed)
    h.fresh_mgr()
    # The game-state reset (0x6f42d2e0) leaves the dummy entry in slot 0: set-size 1.
    h.call(h.F['tbl_set'], ecx=h.TBL, args=(1,))
    assert h.table_size() == 1
    agents = {}          # object key (small index) -> agent pointer
    dup_steps = 0        # operations after which the free stack holds an id twice
    issued = []          # every id ever returned by create, in order
    ops = []
    for _ in range(nops):
        x = rnd.random()
        if x < 0.40 or not issued:
            k = rnd.randrange(24) if agents and rnd.random() < 0.7 else len(agents)
            if k not in agents:
                agents[k] = h.new_agent(0x5000 + 0x40 * k)
            ret = h.create(agents[k])
            issued.append(ret)
            ops.append(('C', k, ret, h.refcnt(ret) if h.table_size() else 0))
        elif x < 0.60:
            i = pick(rnd, issued, live_bias)
            h.addref(i)
            ops.append(('A', i, 0, h.refcnt(i)))
        elif x < 0.95:
            i = pick(rnd, issued, live_bias)
            h.release(i)
            ops.append(('R', i, 0, h.refcnt(i)))
        else:
            # A null handle (below the base) is ignored by the original.
            i = rnd.randrange(0, BASE)
            if rnd.random() < 0.5:
                h.addref(i)
                ops.append(('A', i, 0, 0))
            else:
                h.release(i)
                ops.append(('R', i, 0, 0))
        # Table length and free-stack length after the operation.
        ops[-1] = ops[-1] + (h.table_size(), len(h.free_stack()))
        if len(set(h.free_stack())) != len(h.free_stack()):
            dup_steps += 1
    print('operations with a duplicate id on the free stack: %d' % dup_steps, file=sys.stderr)
    return ops


def rust_array(name, ops, seed, nops, live_bias):
    lines = ['// Seed %d, %d operations, live-id bias %s.' % (seed, nops, 'on' if live_bias else 'off'),
             'const %s: &[(Op, u32, u32, u32, usize, usize)] = &[' % name]
    for kind, arg, ret, rc, tl, fl in ops:
        lines.append('    (Op::%s, %d, 0x%x, %d, %d, %d),' % (KIND[kind], arg, ret, rc, tl, fl))
    lines.append('];')
    return lines


def rust_block(sets):
    lines = [BEGIN,
             '// Recorded from the original allocator (Game.dll 1.26a) by tools/emu/handle_vectors.py.',
             '// Fields: (operation, argument, returned id, refcount of the id the operation touched',
             '// afterwards (0 for null ids), table length, free-stack length).']
    for name, ops, seed, nops, bias in sets:
        lines += rust_array(name, ops, seed, nops, bias)
    lines.append(END)
    return '\n'.join(lines) + '\n'


def splice(path, block):
    with open(path) as f:
        src = f.read()
    a = src.index(BEGIN)
    b = src.index(END) + len(END) + 1
    with open(path, 'w') as f:
        f.write(src[:a] + block + src[b:])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--ops', type=int, default=400)
    ap.add_argument('--rs', help='Rust test file whose generated block is replaced')
    ap.add_argument('--stdout', action='store_true')
    a = ap.parse_args()
    sets = [
        ('EMU_OPS', generate(a.seed, a.ops, True), a.seed, a.ops, True),
        ('EMU_EDGE_OPS', generate(a.seed + 1, a.ops, False), a.seed + 1, a.ops, False),
    ]
    block = rust_block(sets)
    if a.rs:
        splice(a.rs, block)
        print('wrote %d + %d operations to %s' % (len(sets[0][1]), len(sets[1][1]), a.rs))
    if a.stdout or not a.rs:
        sys.stdout.write(block)


if __name__ == '__main__':
    main()
