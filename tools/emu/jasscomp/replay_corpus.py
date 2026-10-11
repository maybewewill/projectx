#!/usr/bin/env python3
"""Re-runs the probe corpus of crates/jass/tests/data/original_probes.txt through the real compile path
(realpath.py) and reports every program whose outcome differs from the recorded one.

    GAME_DLL=/path/Game.dll python3 replay_corpus.py [--limit N]

The Rust test `crates/jass/tests/original_probes.rs` checks our checker against the same file; this
script checks the file against the original, so the chain original -> corpus -> our code can be re-run.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from realpath import Lab  # noqa: E402

CORPUS = os.path.join(HERE, "..", "..", "..", "crates", "jass", "tests", "data", "original_probes.txt")


def unescape(s):
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            n = s[i + 1]
            out.append({"n": "\n", "r": "\r", "\\": "\\"}.get(n, "\\" + n))
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def load(path):
    probes = []
    for line in open(path, newline="\n").read().split("\n"):
        if not line or line.startswith("#"):
            continue
        if line.startswith("@"):
            name, mode, ok = line[1:].split("\t")
            probes.append({"name": name, "lenient": mode == "lenient", "ok": ok == "1", "segs": []})
        elif line.startswith("seg\t"):
            probes[-1]["segs"].append(unescape(line[4:]))
    return probes


def main():
    limit = None
    if "--limit" in sys.argv:
        limit = int(sys.argv[sys.argv.index("--limit") + 1])
    probes = load(CORPUS)[:limit]
    lab = Lab()
    bad = 0
    for p in probes:
        errs, ok, _ = lab.compile([("s%d.j" % i, t) for i, t in enumerate(p["segs"])], p5=0 if p["lenient"] else 1)
        if ok != p["ok"]:
            bad += 1
            print("DIFF", p["name"], "recorded", p["ok"], "now", ok)
    print("%d probes, %d differ" % (len(probes), bad))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
