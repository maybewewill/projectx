"""Extract spell order getters from the ability atlas and audit literal Rust registrations.

Only state-independent leaf constants and exact tail forwarders are resolved.
Unresolved getters stay unresolved. --verify executes original entries in Unicorn.
"""
import argparse
from collections import Counter
import csv
import hashlib
import json
from pathlib import Path
import re
import struct

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from capstone.x86 import X86_OP_IMM, X86_OP_MEM, X86_OP_REG, X86_REG_EAX, X86_REG_ECX, X86_REG_EDX
import pefile

ROOT = Path(__file__).resolve().parents[2]
ORDER_SLOTS = {"primary": 0x308, "alternate": 0x318, "cast": 0x348, "special": 0x368}
FLAG_SLOTS = {"flags_30c": 0x30C, "flags_31c": 0x31C, "flags_34c": 0x34C}


def resolve(image, base, address, vtable, seen=()):
    if address in seen or len(seen) >= 8:
        return None, "forwarding_cycle", list(seen)
    offset = address - base
    if not 0 <= offset <= len(image) - 16:
        return None, "outside_image", list(seen)
    path = [*seen, address]
    data = image[offset:offset + 16]
    if data[0] == 0xB8 and data[5] == 0xC3:
        return struct.unpack_from("<I", data, 1)[0], "constant", path
    if data[:3] in (b"\x33\xc0\xc3", b"\x31\xc0\xc3"):
        return 0, "constant", path
    if data[:4] == b"\x83\xc8\xff\xc3":
        return 0xFFFFFFFF, "constant", path
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    instructions = list(md.disasm(data, address))
    if not instructions:
        return None, "not_decoded", path
    first = instructions[0]
    if first.mnemonic == "jmp" and first.operands[0].type == X86_OP_IMM:
        return resolve(image, base, first.operands[0].imm, vtable, tuple(path))
    # Exact virtual tail forwarder: mov eax,[ecx]; mov edx,[eax+offset]; jmp edx.
    if len(instructions) >= 3:
        a, b, c = instructions[:3]
        if (a.mnemonic == "mov" and len(a.operands) == 2 and
                a.operands[0].type == X86_OP_REG and a.operands[0].reg == X86_REG_EAX and
                a.operands[1].type == X86_OP_MEM and a.operands[1].mem.base == X86_REG_ECX and
                a.operands[1].mem.index == 0 and a.operands[1].mem.disp == 0 and
                b.mnemonic == "mov" and len(b.operands) == 2 and
                b.operands[0].type == X86_OP_REG and b.operands[0].reg == X86_REG_EDX and
                b.operands[1].type == X86_OP_MEM and b.operands[1].mem.base == X86_REG_EAX and
                b.operands[1].mem.index == 0 and
                c.mnemonic == "jmp" and c.operands[0].type == X86_OP_REG and c.operands[0].reg == X86_REG_EDX):
            target = vtable.get(hex(b.operands[1].mem.disp))
            if target is not None:
                return resolve(image, base, int(target, 16), vtable, tuple(path))
    return None, "requires_context_or_unhandled_body", path


def registrations(root):
    token = r"(?:0x[0-9a-fA-F_]+|[0-9][0-9_]*|[A-Z][A-Z0-9_]*)"
    patterns = [rf"\b(?:insert|set_spell_order)\s*\(\s*({token})\s*,\s*([A-Z][A-Z0-9_]*)\s*\)",
                rf"\(\s*({token})\s*,\s*([A-Z][A-Z0-9_]*)\s*\)"]
    result = []
    for source in sorted((root / "crates/sim/src").rglob("*.rs")):
        source_bytes = source.read_bytes()
        text = source_bytes.decode("utf-8")
        # Navigation audit only; this deliberately is not a Rust type checker.
        text = re.sub(r"//[^\n]*|/\*.*?\*/", lambda m: "".join("\n" if c == "\n" else " " for c in m[0]), text, flags=re.S)
        codes = dict(re.findall(r'\bconst\s+(\w+)\s*:\s*u32\s*=\s*code\(b"(.{4})"\)', text))
        integers = {name: int(value.replace("_", ""), 16 if value.startswith("0x") else 10)
                    for name, value in re.findall(rf"\bconst\s+(\w+)\s*:\s*u32\s*=\s*({token})\s*;", text)
                    if value[0].isdigit()}
        found = set()
        for pattern in patterns:
            for match in re.finditer(pattern, text):
                order, symbol = match.groups()
                if symbol not in codes:
                    continue
                if order[0].isdigit():
                    value = int(order.replace("_", ""), 16 if order.startswith("0x") else 10)
                else:
                    value = integers.get(order)
                if value is None or not 0xD0000 <= value < 0xD1000:
                    continue
                key = (value, codes[symbol], match.start())
                if key in found:
                    continue
                # The tuple regex can also match the argument pair of insert().
                if any(v == value and c == codes[symbol] and abs(at - match.start()) < 32 for v, c, at in found):
                    continue
                found.add(key)
                result.append({"file": source.relative_to(root).as_posix(),
                               "line": text.count("\n", 0, match.start()) + 1,
                               "rawcode": codes[symbol], "symbol": symbol,
                               "order_expression": order, "registered_id": value,
                               "file_sha256": hashlib.sha256(source_bytes).hexdigest()})
    return result


def verify(rows, pe, image):
    from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
    base = pe.OPTIONAL_HEADER.ImageBase
    size = (pe.OPTIONAL_HEADER.SizeOfImage + 4095) & ~4095
    mu = Uc(UC_ARCH_X86, UC_MODE_32)
    mu.mem_map(base, size)
    mu.mem_write(base, image[:size].ljust(size, b"\0"))
    mu.mem_map(0x100000, 4096)
    mu.mem_map(0x200000, 4096)
    count = 0
    for row in rows:
        if row["value"] is None:
            continue
        for initial in (0, 0xFFFFFFFF):
            mu.mem_write(0x100100, struct.pack("<I", int(row["vtable"], 16)))
            mu.mem_write(0x100800, struct.pack("<I", 0x200000))
            mu.reg_write(UC_X86_REG_ESP, 0x100800)
            mu.reg_write(UC_X86_REG_ECX, 0x100100)
            mu.reg_write(UC_X86_REG_EAX, initial)
            mu.emu_start(int(row["function"], 16), 0x200000, count=64)
            assert mu.reg_read(UC_X86_REG_EIP) == 0x200000, row
            assert mu.reg_read(UC_X86_REG_ESP) == 0x100804, row
            assert mu.reg_read(UC_X86_REG_EAX) == row["value"], row
            count += 1
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("game_dll", type=Path)
    parser.add_argument("--atlas", type=Path, default=ROOT / "docs/research/ability-atlas/atlas.json")
    parser.add_argument("--out-dir", type=Path, default=ROOT / "docs/research/ability-orders")
    parser.add_argument("--source-root", type=Path, default=ROOT,
                        help="Rust checkout to audit (defaults to this repository)")
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    atlas = json.loads(args.atlas.read_text(encoding="utf-8"))
    digest = hashlib.sha256(args.game_dll.read_bytes()).hexdigest()
    if atlas["dll_sha256"] != digest:
        raise ValueError("Atlas and DLL SHA-256 do not match")
    pe = pefile.PE(str(args.game_dll), fast_load=True)
    if pe.FILE_HEADER.Machine != 0x14C:
        raise ValueError("Expected x86 Game.dll")
    base, image = pe.OPTIONAL_HEADER.ImageBase, pe.get_memory_mapped_image()
    rows = []
    for entry in atlas["classes"]:
        if not entry["factory"] or "CAbilitySpell" not in entry["ancestors"]:
            continue
        for role, slot in {**ORDER_SLOTS, **FLAG_SLOTS}.items():
            target = entry["vtable_slots"].get(hex(slot))
            if target is None:
                continue
            value, status, path = resolve(image, base, int(target, 16), entry["vtable_slots"])
            rows.append({"rawcode": entry["rawcode"], "class": entry["class"],
                         "vtable": entry["vtable"], "role": role, "slot": hex(slot),
                         "function": target, "value": value, "status": status,
                         "forwarding_path": [hex(address) for address in path]})
    checks = verify(rows, pe, image) if args.verify else None
    by_code = {}
    for row in rows:
        if row["role"] in ORDER_SLOTS:
            by_code.setdefault(row["rawcode"], {})[row["role"]] = row
    audits = registrations(args.source_root)
    for audit in audits:
        getters = by_code.get(audit["rawcode"], {})
        values = {r["value"] for r in getters.values() if r["value"] is not None}
        audit["original_orders"] = {role: row["value"] for role, row in getters.items()}
        cast_order = getters.get("cast", {}).get("value")
        audit["recommended_cast_id"] = cast_order
        audit["status"] = ("matches_getter" if audit["registered_id"] in values else
                           "mismatch" if cast_order is not None else "unresolved")
        audit["other_original_abilities_for_id"] = sorted(
            code for code, roles in by_code.items() if code != audit["rawcode"] and
            any(row["value"] == audit["registered_id"] for row in roles.values()))
    summary = {"spell_classes": len(by_code), "getter_entries": len(rows),
               "resolved_entries": sum(row["value"] is not None for row in rows),
               "unicorn_execution_checks": checks,
               "rust_literal_registrations": len(audits),
               "audit_status": dict(Counter(a["status"] for a in audits))}
    args.out_dir.mkdir(parents=True, exist_ok=True)
    document = {"dll_sha256": digest, "summary": summary, "getters": rows, "registrations": audits}
    (args.out_dir / "orders.json").write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    for filename, records in [("getters.tsv", rows), ("registration-audit.tsv", audits)]:
        with (args.out_dir / filename).open("w", encoding="utf-8", newline="") as stream:
            if not records:
                continue
            writer = csv.DictWriter(stream, records[0].keys(), delimiter="\t")
            writer.writeheader()
            writer.writerows({key: json.dumps(value) if isinstance(value, (list, dict)) else value
                              for key, value in row.items()} for row in records)
    print(json.dumps(summary, indent=2))
    for audit in audits:
        if audit["status"] == "mismatch":
            print(f"{audit['file']}:{audit['line']} {audit['rawcode']}: "
                  f"{audit['registered_id']} -> {audit['recommended_cast_id']}")


if __name__ == "__main__":
    main()
