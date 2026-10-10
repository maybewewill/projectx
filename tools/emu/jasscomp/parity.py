"""Differential test: our bytecode compiler (crates/jass) against the original compiler in Unicorn.

For every snippet in tests.TESTS and corpus_extra.json:
  - the original (jcomp.py, emulated Game.dll) compiles it to a stream of 8-byte instructions,
  - our compiler (`cargo run -p jass --example dump_bytecode -- FILE`) compiles the same text,
and the two streams are compared word by word. Compile errors must agree (both error).

Run (needs the game DLL and the unicorn library):
  GAME_DLL=/path/Game.dll PYTHONPATH=<dir with unicorn> python3 parity.py [-v] [name ...]
Exit status 0 when there are no diffs.
"""
import json
import os
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import jcomp as J  # noqa: E402
import tests as T  # noqa: E402

REPO = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
OUT = os.path.join(HERE, "out", "parity")
MNEM = {0x01: "END", 0x02: "DBGSKIP", 0x03: "HDR", 0x04: "ENDF", 0x05: "DL", 0x06: "DG", 0x07: "DCG",
        0x08: "BIND", 0x09: "TYPEP", 0x0a: "TYPEN", 0x0b: "POPA", 0x0c: "K", 0x0d: "MOV", 0x0e: "LD",
        0x0f: "FREF", 0x10: "ALD", 0x11: "ST", 0x12: "AST", 0x13: "PUSH", 0x14: "POP", 0x15: "CALLN",
        0x16: "CALL", 0x17: "I2R", 0x1a: "EQ", 0x1b: "NE", 0x1c: "LE", 0x1d: "GE", 0x1e: "LT", 0x1f: "GT",
        0x20: "ADD", 0x21: "SUB", 0x22: "MUL", 0x23: "DIV", 0x25: "NEG", 0x26: "NOT", 0x27: "RET",
        0x28: "LBL", 0x29: "JNZ", 0x2a: "JZ", 0x2b: "JMP"}


def decode(w0, w1):
    op = (w0 >> 24) & 255
    return "%s[%d,%d,%d,%d]" % (MNEM.get(op, "?%02x" % op), w0 & 255, (w0 >> 8) & 255,
                                (w0 >> 16) & 255, w1)


def original(src):
    """Stream of the original compiler as a list of (w0, w1), or ('ERROR', info)."""
    H = J.Harness()
    J.run_crt_init(H)
    buf, info = J.compile_text(H, src.encode("latin-1"))
    if info.get("error"):
        return ("ERROR", info["error"])
    if buf is None or not info.get("size"):
        return ("ERROR", {"note": "no output"})
    data = H.rdbytes(buf, info["size"])
    return [struct.unpack("<II", data[i:i + 8]) for i in range(0, len(data), 8)]


def ours(path):
    """Our stream (list of (w0, w1)) or ('ERROR', text) from the dump_bytecode example."""
    r = subprocess.run(
        ["cargo", "run", "-q", "-p", "jass", "--example", "dump_bytecode", "--", path],
        cwd=REPO, capture_output=True, text=True, env=os.environ.copy())
    lines = [l for l in r.stdout.splitlines() if l.strip()]
    if r.returncode != 0 or (lines and lines[0].startswith("ERROR")):
        return ("ERROR", lines[0] if lines else r.stderr[-400:])
    return [tuple(int(x) for x in l.split()) for l in lines]


def corpus():
    cases = list(T.TESTS.items())
    with open(os.path.join(HERE, "corpus_extra.json")) as f:
        cases += [tuple(c) for c in json.load(f)]
    return cases


def main(argv):
    verbose = "-v" in argv
    only = [a for a in argv if not a.startswith("-")]
    os.makedirs(OUT, exist_ok=True)
    cases = [c for c in corpus() if not only or c[0] in only]
    total_instr = 0
    bad = []
    t0 = time.time()
    for name, src in cases:
        path = os.path.join(OUT, name + ".j")
        with open(path, "w", encoding="latin-1") as f:
            f.write(src)
        want = original(src)
        got = ours(path)
        if isinstance(want, tuple):
            ok = isinstance(got, tuple)
            status = "both-error" if ok else "ORIG-ERROR-OURS-OK"
        else:
            total_instr += len(want)
            if isinstance(got, tuple):
                ok, status = False, "OURS-ERROR " + got[1]
            elif got == want:
                ok, status = True, "same (%d instr)" % len(want)
            else:
                ok = False
                n = min(len(got), len(want))
                first = next((i for i in range(n) if got[i] != want[i]), n)
                status = "DIFF len %d vs %d, first at %d: want %s got %s" % (
                    len(want), len(got), first,
                    decode(*want[first]) if first < len(want) else "-",
                    decode(*got[first]) if first < len(got) else "-")
                if verbose:
                    status += "\n    want: " + " ".join(decode(*w) for w in want)
                    status += "\n    got:  " + " ".join(decode(*w) for w in got)
        if not ok:
            bad.append((name, status))
        if verbose or not ok:
            print("%-28s %s" % (name, status if ok else "DIFF " + status))
    print("cases: %d  same: %d  diffs: %d  original instructions compared: %d  (%.0fs)" % (
        len(cases), len(cases) - len(bad), len(bad), total_instr, time.time() - t0))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
