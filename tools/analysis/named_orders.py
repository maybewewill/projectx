"""Capture WC3 1.26a order registrations by executing the original initializer.

The registrar is replaced only in emulator memory with ret 8; a hook records
its actual arguments. No game code or bytes are exported.
"""
import argparse
from collections import Counter
import csv
import hashlib
import json
from pathlib import Path
import struct

import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EIP, UC_X86_REG_ESP

ROOT = Path(__file__).resolve().parents[2]
DLL_SHA256 = "6d21bd9a0f9fbc8446f455c9e89ac994fed68174426fe608fcb9baefd4dec53c"
REGISTRAR = 0x6F3B0730


def capture(dll):
    data = dll.read_bytes()
    if hashlib.sha256(data).hexdigest() != DLL_SHA256:
        raise ValueError("Expected the recorded WC3 1.26a Game.dll SHA-256")
    pe = pefile.PE(data=data, fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    size = (pe.OPTIONAL_HEADER.SizeOfImage + 4095) & ~4095
    image = pe.get_memory_mapped_image()
    mu = Uc(UC_ARCH_X86, UC_MODE_32)
    mu.mem_map(base, size)
    mu.mem_write(base, image[:size].ljust(size, b"\0"))
    mu.mem_map(0x100000, 4096)
    mu.mem_map(0x200000, 4096)
    history = []

    def hook(uc, address, instruction_size, user):
        if address != REGISTRAR:
            return
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret, order, pointer = struct.unpack("<III", uc.mem_read(esp, 12))
        if not base <= pointer < base + size - 128:
            raise ValueError("Order name outside the image")
        raw = bytes(uc.mem_read(pointer, 128))
        end = raw.find(b"\0")
        if not 0 < end < 128 or not all(32 <= byte < 127 for byte in raw[:end]):
            raise ValueError("Invalid order name")
        if bytes(uc.mem_read(ret - 5, 1)) != b"\xe8":
            raise ValueError("Unexpected registrar call shape")
        history.append({"name": raw[:end].decode("ascii"), "id": order,
                        "call_site": hex(ret - 5), "string_address": hex(pointer)})

    mu.hook_add(UC_HOOK_CODE, hook)
    mu.mem_write(REGISTRAR, b"\xc2\x08\x00")
    mu.mem_write(0x100800, struct.pack("<I", 0x200000))
    mu.reg_write(UC_X86_REG_ESP, 0x100800)
    mu.emu_start(0x6F229A00, 0x200000, count=100000)
    if mu.reg_read(UC_X86_REG_EIP) != 0x200000 or mu.reg_read(UC_X86_REG_ESP) != 0x100804:
        raise AssertionError("Order initializer did not return cleanly")
    # Smart is registered by this short original caller slice, immediately before the main initializer.
    mu.reg_write(UC_X86_REG_ESP, 0x100800)
    mu.emu_start(0x6F3B07A4, 0x6F3B07BC, count=100)
    if mu.reg_read(UC_X86_REG_EIP) != 0x6F3B07BC or mu.reg_read(UC_X86_REG_ESP) != 0x100800:
        raise AssertionError("Smart registration slice did not finish cleanly")
    return history


def unique_orders(history):
    names, ids = {}, {}
    for row in history:
        folded = row["name"].lower()
        if folded in names and names[folded]["id"] != row["id"]:
            raise ValueError("One order name has conflicting ids")
        if row["id"] in ids and ids[row["id"]]["name"] != row["name"]:
            raise ValueError("Id has aliases; reverse-name precedence needs research")
        names[folded] = row
        ids[row["id"]] = row
    return sorted(ids.values(), key=lambda row: row["id"])


def rust_table(rows):
    header = "//! Derived order name/id registrations from the user's Game.dll 1.26a.\n"
    header += "//! Regenerate with tools/analysis/named_orders.py; original initializer executed in Unicorn.\n\n"
    body = "pub const NAMED_ORDERS: &[(&str, u32)] = &[\n"
    body += "".join(f"    ({json.dumps(row['name'])}, {row['id']:_}),\n" for row in rows)
    return header + body + "];\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("game_dll", type=Path)
    parser.add_argument("--out-dir", type=Path, default=ROOT / "docs/research/named-orders")
    parser.add_argument("--rust-output", type=Path,
                        help="Optional generated Rust names module; metadata outputs are always written")
    args = parser.parse_args()
    history = capture(args.game_dll)
    rows = unique_orders(history)
    values = {row["name"]: row["id"] for row in rows}
    for name, value in {"smart": 851971, "stop": 851972, "holybolt": 852092,
                        "thunderbolt": 852095, "blizzard": 852089,
                        "slow": 852075, "undivineshield": 852091}.items():
        assert values[name] == value, (name, values.get(name))
    summary = {"captured_registrations": len(history), "unique_names_and_ids": len(rows),
               "identical_duplicate_registrations": len(history) - len(rows)}
    args.out_dir.mkdir(parents=True, exist_ok=True)
    (args.out_dir / "orders.json").write_text(json.dumps({"dll_sha256": DLL_SHA256,
        "summary": summary, "orders": rows, "registration_history": history}, indent=2) + "\n", encoding="utf-8")
    with (args.out_dir / "orders.tsv").open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, rows[0].keys(), delimiter="\t")
        writer.writeheader()
        writer.writerows(rows)
    if args.rust_output:
        args.rust_output.parent.mkdir(parents=True, exist_ok=True)
        args.rust_output.write_text(rust_table(rows), encoding="utf-8")
    print(json.dumps(summary, indent=2))
    print("Repeated names:", dict((name, count) for name, count in Counter(r["name"] for r in history).items() if count > 1))


if __name__ == "__main__":
    main()
