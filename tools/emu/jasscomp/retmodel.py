#!/usr/bin/env python3
"""Python model of the original's `return` analysis (docs/spec/jass-frontend.md, "Return analysis") and
a random-program differential test against the real compile path.

The model is the reference the Rust port in crates/jass/src/check.rs was written from: statement nodes
carry (flag, type code, type ref); the statement-list constructor 0x6F4614D0, the if/else node 0x6F4617C0
with its validator 0x6F461B80, the return / loop / exitwhen nodes (0x6F461E30 / 0x6F461C30 / 0x6F461D30)
and the function validator 0x6F4611D0 merge them. Both modes are modelled: strict (p5 = 1, what the
game uses) and lenient (p5 = 0). Run:

    GAME_DLL=/path/Game.dll python3 retmodel.py SEED COUNT [strict]

It generates COUNT random function bodies, compiles each through the original and compares with the
model. 1,400 strict and 600 lenient programs matched with 0 mismatches.
"""
import random
import sys

PRE = "type widget extends handle\ntype unit extends widget\ntype item extends widget\ntype player extends handle\n"
# type codes: 2 null, 3 code, 4 int, 5 real, 6 string, 7 handle(ref), 8 boolean
EXPRS = {  # name -> (source, code, ref)
    "int": ("1", 4, None), "real": ("1.5", 5, None), "str": ('"a"', 6, None), "bool": ("true", 8, None),
    "null": ("null", 2, None), "code": ("function f", 3, None),
    "h": ("vh", 7, "handle"), "w": ("vw", 7, "widget"), "u": ("vu", 7, "unit"), "i": ("vi", 7, "item"), "p": ("vp", 7, "player"),
}
DECL = {"nothing": None, "integer": (4, None), "real": (5, None), "string": (6, None), "boolean": (8, None), "code": (3, None),
        "handle": (7, "handle"), "widget": (7, "widget"), "unit": (7, "unit"), "item": (7, "item"), "player": (7, "player")}
PARENT = {"unit": "widget", "item": "widget", "widget": "handle", "player": "handle", "handle": None}

def subtype(a, b):
    while a:
        if a == b: return True
        a = PARENT.get(a)
    return False

class N:  # statement node
    def __init__(s, kind, flag=0, ty=0, ref=None):
        s.kind, s.flag, s.ty, s.ref, s.conflict = kind, flag, ty, ref, False

def mk_list(S, prev, strict):
    """prev: previous list node (with .stmt) or None. Returns list node."""
    L = N("list")
    L.stmt = S
    if S is None:
        return L
    P = prev.stmt if prev else None
    if not strict:
        L.ty, L.ref = S.ty, S.ref
        if prev:
            if P and P.kind != "exitwhen": L.flag = S.flag
        else:
            L.flag = S.flag
        return L
    if (not prev) or (P and P.kind != "exitwhen"):
        L.flag = S.flag
    v7 = False
    if (not P) or (P.flag == 0 and S.flag == 0):
        L.ty, L.ref = S.ty, S.ref
        return L
    v8, v9 = P.flag, S.flag
    if v9 != v8:
        if v9:
            if v8:
                L.conflict = True
            else:
                L.flag, L.ty, L.ref = v9, S.ty, S.ref
                v7 = True
        else:
            L.flag, L.ty, L.ref = v8, P.ty, P.ref
            S.flag, S.ty, S.ref = L.flag, L.ty, L.ref
            v7 = True
    if P.flag != 2 or S.flag != 2:
        if v7: return L
        L.ty, L.ref = S.ty, S.ref
        return L
    tS, tP = S.ty, P.ty
    if tS == 2:
        if tP != 2:
            L.ty, L.ref = tP, P.ref
            return L
    elif tP == 2:
        L.ty, L.ref = tS, S.ref
        return L
    else:
        if tP != tS:
            if (tS == 4 and tP == 5) or (tS == 5 and tP == 4):
                L.ty, L.ref = tS, S.ref
                return L
            L.conflict = True
            return L
    if tS == 7 and S.ref != P.ref:
        L.conflict = True
        return L
    L.ty, L.ref = S.ty, S.ref
    return L

def mk_if(a4, a5, strict):
    """a4: then list (or None), a5: else part (list/if node or None)."""
    I = mk_if0(a4, a5, strict)
    if a4 and a5 and {a4.ty, a5.ty} == {4, 5}:
        return I
    t4, f4 = (a4.ty, a4.flag) if a4 else (0, 0)
    t5, f5 = (a5.ty, a5.flag) if a5 else (0, 0)
    if not if_valid(t4, f4, t5, f5, strict):
        CONFLICTS.append(1)
    return I

def if_valid(t4, f4, t5, f5, strict):
    if strict:
        return not (t4 and f4 == 2 and t5 and f5 == 2 and t5 != t4)
    return not (t4 and t5 and t5 != t4)

def mk_if0(a4, a5, strict):
    I = N("if")
    # snapshot after null coercion happens inside; validate afterwards using the (coerced) fields
    I._chk = (a4, a5)
    if strict:
        if a4 and a5:
            if a4.ty == 2:
                if a5.ty in (7, 6, 3):
                    a4.ty, a4.ref = a5.ty, a5.ref
                    a4.flag = a5.flag
            if a5.ty == 2:
                if a4.ty in (7, 6, 3):
                    a5.ty, a5.ref = a4.ty, a4.ref
                    a5.flag = a4.flag
            f4, f5 = a4.flag, a5.flag
            if f4 == f5 or f4:
                I.flag = f4
            elif f5:
                I.flag = f5
                I.ty, I.ref = a5.ty, a5.ref
                return I
            if a4.flag == 2: I.ty, I.ref = a4.ty, a4.ref
            else: I.ty, I.ref = 0, None
            return I
        if a4:
            I.flag = a4.flag
            I.ty, I.ref = a4.ty, a4.ref
            if a4.flag == 2: I.ty, I.ref = a4.ty, a4.ref
            else: I.ty, I.ref = 0, None
            return I
        if a5:
            I.flag = a5.flag
            I.ty, I.ref = a5.ty, a5.ref
            if a5.flag != 2:
                I.ty, I.ref = 0, None
            return I
        return I
    # lenient
    if not a4:
        if a5:
            if a5.flag == 2: I.ty, I.ref = a5.ty, a5.ref
            else: a5.ty, a5.ref = 0, None
        return I
    if a5:
        if a4.ty == 2:
            if a5.ty in (7, 6, 3): a4.ty, a4.ref = a5.ty, a5.ref
        if a5.ty == 2:
            if a4.ty in (7, 6, 3): a5.ty, a5.ref = a4.ty, a4.ref
    if a4.flag == 2: I.ty, I.ref = a4.ty, a4.ref
    else: a4.ty, a4.ref = 0, None
    if a5:
        if a4.flag == a5.flag: I.flag = a4.flag
        if a5.flag == 2: I.ty, I.ref = a5.ty, a5.ref
        else: a5.ty, a5.ref = 0, None
    return I

def analyse(stmts, strict):
    """stmts: list of AST tuples. Returns the final list node (or None)."""
    prev = None
    for s in stmts:
        node = build(s, strict)
        prev = mk_list(node, prev, strict)
    return prev

def build(s, strict):
    k = s[0]
    if k == "ret":
        if s[1] is None: return N("ret", 1)
        _, code, ref = EXPRS[s[1]]
        return N("ret", 2, code, ref)
    if k == "simple": return N("simple")
    if k == "dbg": return N("simple")
    if k == "exitwhen": return N("exitwhen")
    if k == "loop":
        b = analyse(s[1] + [("exitwhen",)], strict)
        n = N("loop")
        if b: n.flag, n.ty, n.ref = b.flag, b.ty, b.ref
        return n
    if k == "if":
        # s = ("if", then, elifs/else)  where else is None | list | ("if",...) nested
        a4 = analyse(s[1], strict)
        if s[2] is None: a5 = None
        elif s[2][0] == "else": a5 = analyse(s[2][1], strict)
        else: a5 = build(s[2], strict)
        return mk_if(a4, a5, strict)
    raise ValueError(k)

def verdict(decl, body, strict):
    """decl: DECL entry or None. Returns True if accepted."""
    lst = analyse(body, strict)
    # any conflict anywhere is reported when the list node is created: track via global
    return lst

CONFLICTS = []
_orig_mk_list = mk_list
def mk_list_tracked(S, prev, strict):
    L = _orig_mk_list(S, prev, strict)
    if L.conflict: CONFLICTS.append(1)
    return L
mk_list = mk_list_tracked

def check_ifs():
    pass

def accepts(declname, body, strict):
    CONFLICTS.clear()
    decl = DECL[declname]
    lst = analyse(body, strict)
    if CONFLICTS: return False
    flag = lst.flag if lst else 0
    if decl is None:
        return not (flag & 2)
    v5, dref = decl
    if lst is None:
        return False
    v4, aref = lst.ty, lst.ref
    if v4 == 2 and v5 in (7, 6, 3):
        v4, aref = v5, dref
    ok = (v4 == 4 and v5 == 5) or (v4 == 5 and v5 == 4) or v4 == v5
    if ok and strict and v5 == 7 and v4 != 0:
        ok = subtype(aref, dref)
    if not ok: return False
    return bool(flag & 2)

# ---- source rendering
def render(stmts, ind=""):
    out = []
    for s in stmts:
        k = s[0]
        if k == "ret": out.append(ind + ("return " + EXPRS[s[1]][0] if s[1] else "return"))
        elif k == "simple": out.append(ind + "call f()")
        elif k == "dbg": out.append(ind + "debug call f()")
        elif k == "loop":
            out.append(ind + "loop"); out += render(s[1], ind + " "); out.append(ind + " exitwhen true"); out.append(ind + "endloop")
        elif k == "if":
            out.append(ind + "if true then"); out += render(s[1], ind + " ")
            e = s[2]
            while e is not None:
                if e[0] == "else":
                    out.append(ind + "else"); out += render(e[1], ind + " "); e = None
                else:
                    out.append(ind + "elseif true then"); out += render(e[1], ind + " "); e = e[2]
            out.append(ind + "endif")
    return out

def source(declname, body):
    ret = declname
    pre = "function f takes nothing returns nothing\nendfunction\n"
    vars_ = "local handle vh\nlocal widget vw\nlocal unit vu\nlocal item vi\nlocal player vp\n"
    return PRE + pre + "function g takes nothing returns %s\n%s%s\nendfunction\n" % (ret, vars_, "\n".join(render(body)))

def rnd_body(r, depth):
    n = r.choice([1, 1, 2, 2, 3, 4])
    out = []
    for _ in range(n):
        x = r.random()
        if x < 0.45 or depth >= 3:
            out.append(("ret", r.choice(list(EXPRS) + [None, None] if True else None)) if r.random() < 0.8 else ("simple",))
        elif x < 0.55:
            out.append(("simple",) if r.random() < 0.5 else ("dbg",))
        elif x < 0.85:
            e = None
            m = r.random()
            if m < 0.5: e = ("else", rnd_body(r, depth + 1))
            elif m < 0.7: e = ("if", rnd_body(r, depth + 1), ("else", rnd_body(r, depth + 1)) if r.random() < .5 else None)
            out.append(("if", rnd_body(r, depth + 1), e))
        else:
            out.append(("loop", rnd_body(r, depth + 1)))
    return out

if __name__ == "__main__":
    import json
    import os
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from realpath import Lab
    lab = Lab()
    seed = int(sys.argv[1])
    count = int(sys.argv[2])
    only_strict = len(sys.argv) > 3
    r = random.Random(seed)
    bad = 0
    for i in range(count):
        decl = r.choice(list(DECL))
        body = rnd_body(r, 0)
        src = source(decl, body)
        for p5, strict in (((1, True),) if only_strict else ((1, True), (0, False))):
            ok = lab.accepts(src, p5=p5)
            model = accepts(decl, body, strict)
            if model != ok:
                bad += 1
                print("MISMATCH p5=%d decl=%s original=%s model=%s\n%s" % (p5, decl, ok, model, "\n".join(render(body))))
    print("done", count * (1 if only_strict else 2), "programs,", bad, "mismatches")
