"""Compiles every text in tests.py with the original compiler (fresh Unicorn state per text).
Writes out/RESULTS.txt (decoded streams) and out/results.json (raw words)."""
import sys, os, json, struct, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import jcomp as J
import tests as T
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "out")
os.makedirs(OUT, exist_ok=True)
MNEM = {0x01:"END",0x03:"FUNC_HDR",0x04:"FUNC_END",0x05:"DECL_LOCAL",0x06:"DECL_GLOBAL",0x07:"DECL_CONST_GLOBAL",
        0x08:"PARAM_BIND",0x0b:"POP_ARGS",0x0c:"LOAD_CONST",0x0d:"MOVE",0x0e:"LOAD_VAR",0x0f:"LOAD_FUNCREF",
        0x10:"ARRAY_LOAD",0x11:"STORE_VAR",0x12:"ARRAY_STORE",0x13:"PUSH",0x14:"POP",0x16:"CALL_SCRIPT",
        0x1a:"EQ",0x1b:"NE",0x1c:"LE",0x1d:"GE",0x1e:"LT",0x1f:"GT",0x20:"ADD",0x21:"SUB",0x22:"MUL",0x23:"DIV",
        0x25:"NEG_INPLACE",0x26:"NOT_INPLACE",0x27:"RET",0x28:"LABEL",0x29:"JNZ",0x2a:"JZ",0x2b:"JMP"}
def run(name, src):
    H = J.Harness()
    J.run_crt_init(H)
    buf, info = J.compile_text(H, src.encode("latin-1"))
    rec = {"name": name, "src": src, "info_eax": info.get("eax")}
    if info.get("error"):
        rec["error"] = {"kind": info["error"]["kind"], "line": info["error"]["line"]}
        return rec
    if buf is None or not info.get("size"):
        rec["error"] = {"note": "no output (stopped_at=%s)" % info.get("stopped_at")}
        return rec
    data = H.rdbytes(buf, info["size"])
    rec["words"] = [list(struct.unpack("<II", data[i:i+8])) for i in range(0, len(data), 8)]
    rec["unimpl"] = sorted(H.unimpl)
    return rec
def fmt(rec):
    L = ["== %s" % rec["name"], "source:"] + ["    " + l for l in rec["src"].splitlines()]
    if "error" in rec:
        L.append("  ERROR: %s" % json.dumps(rec["error"])); return "\n".join(L)
    for i, (w0, w1) in enumerate(rec["words"]):
        op = (w0 >> 24) & 255
        L.append("  %2d  b0=%02x b1=%02x b2=%02x op=%02x imm=%08x  %s" % (
            i, w0 & 255, (w0 >> 8) & 255, (w0 >> 16) & 255, op, w1, MNEM.get(op, "?")))
    L.append("  total instructions: %d" % len(rec["words"]))
    return "\n".join(L)
if __name__ == "__main__":
    only = sys.argv[1:]
    recs = []
    t0 = time.time()
    for name, src in T.TESTS.items():
        if only and name not in only:
            continue
        recs.append(run(name, src))
    with open(os.path.join(OUT, "RESULTS.txt"), "w") as f:
        f.write("\n\n".join(fmt(r) for r in recs) + "\n")
    with open(os.path.join(OUT, "results.json"), "w") as f:
        json.dump(recs, f, indent=1)
    print("compiled %d texts in %.1fs" % (len(recs), time.time() - t0))
    for r in recs:
        if "error" in r:
            print("%-26s ERROR %s" % (r["name"], json.dumps(r["error"])))
        else:
            print("%-26s instructions=%d" % (r["name"], len(r["words"])))
