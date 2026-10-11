#!/usr/bin/env python3
"""Unicorn check of the category list walk 0x6F46D0B0 (list part) against a Python model.

The real routine runs on a synthetic holder: holder+0x6C = category object, category+0x58 = head of a
singly linked list (next at node+8, removed flag at node+0x20, reference at node+0x54). A reference
holds a handle index (+0xC) and a tag (+0x10). The handle lookup 0x6F03FA30 is replaced by a table hook
(a stand-in, not original code) that returns the unit when index and tag match, else 0. The predicate is
a hook stub whose address goes in EDX, as the original does; it records each call (ECX = the reference)
and returns a scripted value.

Checked per random list (the routine's own rules, read from the disassembly):
  * visit order = list order (head first, next at +8),
  * a node with its removed flag set is skipped without a predicate call,
  * a reference that does not resolve, a looked-up object whose tag is not 'lga+' (0x2B61676C; the code
    keeps the object only when the tag EQUALS 0x2B61676C) and a removed object are skipped,
  * the walk stops after the first predicate call that returns ZERO (the loop continues while the
    predicate returns non-zero; the bit accumulated is "predicate returned zero").

Cell part (A3 = 0 here) is not run. Usage:
  GAME_DLL=/path/to/Game.dll python3 tools/emu/category_walk_emu.py [CASES] [SEED]
Nothing from the DLL is written to disk; the run prints counts only.
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import softfloat_vectors as sv  # noqa: E402  (Game: loads the DLL image into Unicorn)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_ECX,
    UC_X86_REG_EDX,
    UC_X86_REG_EIP,
    UC_X86_REG_EAX,
    UC_X86_REG_ESP,
)

WALK = 0x6F46D0B0      # category list walk: ECX = holder, EDX = predicate, 3 stack args (ret 0xC)
HANDLE = 0x6F03FA30    # handle lookup: ECX = index, EDX = tag, returns the object or 0 (no stack args)
LGA = 0x2B61676C       # tag the walk excludes
PRED = 0x00780000      # predicate stub (hook)
HEAP = 0x00400000      # synthetic objects
HEAP_SIZE = 0x00100000

CAT_HEAD = 0x58        # category+0x58 = first node
HOLDER_CAT = 0x6C      # holder+0x6C = category object
NODE_NEXT = 0x08
NODE_REMOVED = 0x20
NODE_REF = 0x54
REF_INDEX = 0x0C
REF_TAG = 0x10
UNIT_TAG = 0x0C
UNIT_REMOVED = 0x20


def ret_from(uc, value, pops):
    """Return from a hooked routine: pop the return address (and `pops` argument bytes), set EAX."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops)
    uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_EIP, ret)


class Walk:
    def __init__(self, game):
        self.g = game
        self.mu = game.mu
        self.mu.mem_map(PRED, 0x1000)
        self.mu.mem_map(HEAP, HEAP_SIZE)
        self.next = HEAP
        self.handles = {}   # index -> (unit address, tag the reference must carry)
        self.preds = {}     # reference address -> value the predicate stub returns
        self.calls = []     # reference addresses in predicate call order
        self.mu.hook_add(UC_HOOK_CODE, self._hook_handle, begin=HANDLE, end=HANDLE)
        self.mu.hook_add(UC_HOOK_CODE, self._hook_pred, begin=PRED, end=PRED)

    def alloc(self, size):
        p = self.next
        self.next = (p + size + 15) & ~15
        return p

    def wr(self, a, v):
        self.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))

    def _hook_handle(self, uc, address, size, user):
        index = uc.reg_read(UC_X86_REG_ECX)
        tag = uc.reg_read(UC_X86_REG_EDX)
        ent = self.handles.get(index)
        obj = ent[0] if ent is not None and ent[1] == tag else 0
        ret_from(uc, obj, 0)

    def _hook_pred(self, uc, address, size, user):
        ref = uc.reg_read(UC_X86_REG_ECX)
        self.calls.append(ref)
        ret_from(uc, self.preds.get(ref, 1), 0)

    def run(self, specs, ctx=0x1234):
        """Build the holder and the list in `specs` order, run the real walk, return (calls, refs)."""
        self.handles.clear()
        self.preds.clear()
        self.next = HEAP
        self.mu.mem_write(HEAP, b"\0" * 0x4000)
        cat = self.alloc(0x80)
        holder = self.alloc(0x80)
        self.wr(holder + HOLDER_CAT, cat)
        head = 0
        prev = 0
        refs = []
        for s in specs:
            node = self.alloc(0x60)
            ref = self.alloc(0x20)
            self.wr(ref + REF_INDEX, s["index"])
            self.wr(ref + REF_TAG, s["tag"])
            self.wr(node + NODE_REF, ref)
            self.wr(node + NODE_REMOVED, 1 if s["removed"] else 0)
            if s["unit"]:
                unit = self.alloc(0x100)
                self.wr(unit + UNIT_TAG, s["unit_tag"])
                self.wr(unit + UNIT_REMOVED, 1 if s["unit_removed"] else 0)
                self.handles[s["index"]] = (unit, s["tag"])
            self.preds[ref] = s["pred"]
            if prev:
                self.wr(prev + NODE_NEXT, node)
            else:
                head = node
            prev = node
            refs.append(ref)
        self.wr(cat + CAT_HEAD, head)
        self.calls = []
        # walk(ECX = holder, EDX = predicate stub, A1 = predicate context, A2 = 0, A3 = 0).
        # A3 = 0 means no cell part is entered after the list.
        self.g.call(WALK, ecx=holder, edx=PRED, stack=(ctx, 0, 0), count=2_000_000)
        return list(self.calls), refs


def python_model(specs):
    """Visit list from the routine's rules; returns the indices the predicate is called with."""
    out = []
    for s in specs:
        if s["removed"]:
            continue
        if not s["unit"]:
            continue
        if s["unit_tag"] != LGA or s["unit_removed"]:
            continue
        out.append(s["index"])
        if s["pred"] == 0:
            break
    return out


def random_spec(rng, index):
    return {
        "index": index,
        "tag": rng.choice([0x1234, 0x0BEEF, 0x0F00D]),
        "removed": rng.random() < 0.2,
        "unit": rng.random() < 0.9,
        "unit_tag": rng.choice([0x6E6F6465, LGA, 0x6F6F6F6F]),
        "unit_removed": rng.random() < 0.15,
        "pred": 0 if rng.random() < 0.15 else 1,
    }


def main(argv):
    path = os.environ.get("GAME_DLL")
    if not path:
        print("set GAME_DLL to the Game.dll path", file=sys.stderr)
        return 2
    cases = int(argv[1]) if len(argv) > 1 else 300
    seed = int(argv[2]) if len(argv) > 2 else 1
    rng = random.Random(seed)
    g = sv.Game(path)
    w = Walk(g)
    bad = 0
    visits = 0
    with_zero = 0
    for c in range(cases):
        n = rng.randint(0, 12)
        specs = [random_spec(rng, 0x1000 + i) for i in range(n)]
        calls, refs = w.run(specs)
        index_of = {ref: s["index"] for ref, s in zip(refs, specs)}
        got = [index_of.get(r, -1) for r in calls]
        want = python_model(specs)
        visits += len(got)
        if any(s["pred"] == 0 for s in specs):
            with_zero += 1
        if got != want:
            bad += 1
            if bad <= 5:
                print("MISMATCH case", c, "got", got, "want", want)
    print(f"cases {cases}, predicate calls {visits}, lists containing a zero predicate {with_zero}, "
          f"mismatches {bad}")
    return 0 if bad == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
