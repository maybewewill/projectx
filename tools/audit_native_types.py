"""Flags native implementations whose argument/return value kinds disagree with common.j.

common.j declares enum-like types (gametype, mapflag, placement, playercolor, ...) as handles, so
the VM passes them as `Value::Handle`. A native that reads such an argument with `int_at` gets 0,
and one that returns `Value::Integer` for a handle return type fails with TypeError.

Usage: python tools/audit_native_types.py path/to/common.j [crates/sim/src]
The common.j path is the user's own extracted copy; nothing from it is written out except
native names and declared type names.
"""

import os
import re
import sys

VALUE_TYPES = {"integer", "real", "boolean", "string", "code", "nothing"}

SIG = re.compile(
    r"^\s*(?:constant\s+)?native\s+(\w+)\s+takes\s+(.*?)\s+returns\s+(\w+)", re.M
)
REG = re.compile(r'(?:native|register|reg\.register|\w+)\s*\(\s*(?:reg\s*,\s*\w+\s*,\s*)?"(\w+)"')
ACCESS = re.compile(r"\b(int_at|real_at|bool_at|str_at|handle_at|string_at)\s*\(\s*\w+\s*,\s*(\d+)\s*\)")
INT_RET = re.compile(r"Value::Integer\s*\(")
HANDLE_RET = re.compile(r"Value::Handle\s*\(")

KIND_OF_ACCESSOR = {
    "int_at": "integer",
    "real_at": "real",
    "bool_at": "boolean",
    "str_at": "string",
    "string_at": "string",
    "handle_at": "handle",
}


def kind(ty):
    return ty if ty in VALUE_TYPES else "handle"


def parse_common(path):
    src = open(path, encoding="latin-1").read()
    sigs = {}
    for m in SIG.finditer(src):
        name, takes, ret = m.groups()
        params = []
        if takes.strip() != "nothing":
            for p in takes.split(","):
                parts = p.split()
                if len(parts) == 2:
                    params.append(parts[0])
        sigs[name] = (params, ret)
    return sigs


def native_bodies(root):
    """Yields (file, line, name, body) for each `"Name"` registration; the body runs to the next one."""
    for dirpath, _, files in os.walk(root):
        for f in sorted(files):
            if not f.endswith(".rs"):
                continue
            path = os.path.join(dirpath, f)
            src = open(path, encoding="utf-8").read()
            hits = [(m.start(), m.group(1)) for m in REG.finditer(src)]
            for i, (pos, name) in enumerate(hits):
                end = hits[i + 1][0] if i + 1 < len(hits) else len(src)
                body = src[pos:end]
                # Cut at the end of the registering statement block (blank line + new item).
                yield path, src.count("\n", 0, pos) + 1, name, body


def main():
    common = sys.argv[1]
    root = sys.argv[2] if len(sys.argv) > 2 else "crates/sim/src"
    sigs = parse_common(common)
    problems = []
    for path, line, name, body in native_bodies(root):
        if name not in sigs:
            continue
        params, ret = sigs[name]
        for acc, idx in ACCESS.findall(body):
            idx = int(idx)
            if idx >= len(params):
                continue
            want = kind(params[idx])
            got = KIND_OF_ACCESSOR[acc]
            if want != got and not (want == "real" and got == "integer"):
                problems.append(
                    f"{path}:{line}: {name}: arg {idx} is {params[idx]} but read with {acc}"
                )
        if kind(ret) == "handle" and INT_RET.search(body) and not HANDLE_RET.search(body):
            problems.append(f"{path}:{line}: {name}: returns {ret} but builds Value::Integer")
        if kind(ret) == "integer" and HANDLE_RET.search(body) and not INT_RET.search(body):
            problems.append(f"{path}:{line}: {name}: returns integer but builds Value::Handle")
    seen = set()
    for p in problems:
        if p not in seen:
            seen.add(p)
            print(p)
    print(f"{len(seen)} problem(s)", file=sys.stderr)


if __name__ == "__main__":
    main()
