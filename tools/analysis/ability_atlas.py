"""Map WC3 ability rawcodes to RTTI classes and virtual entry points.

Only derived metadata is emitted; no game bytes or decompiled bodies are exported.
Uses the existing rtti_vtables.py scanner. --verify executes each factory code getter
in Unicorn, independently of the static immediate decoder.
"""
import argparse
from collections import Counter, defaultdict
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import sys

import pefile

ROOT = Path(__file__).resolve().parents[2]
SLOTS = {
    "message": 0x0C, "code": 0x1C, "init": 0x80, "removal": 0x1A8,
    "cast_forward": 0x330, "cast_body": 0x338,
    "channel_380": 0x380, "channel_384": 0x384, "channel_end": 0x388,
    "effect_dispatch": 0x3C0, "effect_418": 0x418,
    "effect_point": 0x420, "effect_unit": 0x424, "effect_none": 0x428,
}


def clean_name(name):
    return name.removeprefix(".?AV").removeprefix(".?AU").removesuffix("@@")


def collect(dll, factory_table):
    pe = pefile.PE(str(dll), fast_load=True)
    if pe.FILE_HEADER.Machine != 0x14C:
        raise ValueError("Expected a 32-bit x86 Game.dll")
    base = pe.OPTIONAL_HEADER.ImageBase
    image = pe.get_memory_mapped_image()

    def u32(va):
        offset = va - base
        if not 0 <= offset <= len(image) - 4:
            raise ValueError(f"RTTI pointer outside image: {va:#x}")
        return struct.unpack_from("<I", image, offset)[0]

    def type_name(td):
        offset = td - base + 8
        if not 0 <= offset < len(image):
            raise ValueError("Type descriptor outside image")
        end = image.index(b"\0", offset)
        return clean_name(image[offset:end].decode("ascii"))

    scan = subprocess.check_output(
        [sys.executable, str(ROOT / "tools/analysis/rtti_vtables.py"), str(dll)],
        text=True,
    )
    tables = {}
    for line in scan.splitlines():
        address, mangled, subobject_offset, _, targets = line.split("\t")
        if subobject_offset != "0":
            continue
        address = int(address, 16)
        functions = [int(value, 16) for value in targets.split(",")]
        col = u32(address - 4)
        hierarchy = u32(col + 16)
        count, array = u32(hierarchy + 8), u32(hierarchy + 12)
        if not 1 <= count <= 256:
            raise ValueError("Unexpected RTTI base-class count")
        ancestors = [type_name(u32(u32(array + i * 4))) for i in range(count)]
        name = clean_name(mangled)
        if name in tables:
            raise ValueError(f"Duplicate primary vtable for {name}")
        tables[name] = {"class": name, "vtable": hex(address),
                        "ancestors": ancestors, "functions": functions}

    with factory_table.open(encoding="utf-8") as stream:
        factory = {}
        for line in stream:
            if line.startswith("#") or not line.strip():
                continue
            code, wrapper, helper, *_ = line.rstrip().split("\t")
            if code in factory:
                raise ValueError(f"Duplicate factory rawcode: {code}")
            factory[code] = {"factory_wrapper": wrapper,
                             "typed_pointer_helper": helper}

    classes = []
    for name, table in sorted(tables.items()):
        if not name.startswith(("CAbility", "CBuff", "CEffect")):
            continue
        functions = table["functions"]
        code = None
        if len(functions) > 7:
            getter = functions[7]
            data = image[getter - base:getter - base + 6]
            if data[:1] == b"\xb8" and data[5:] == b"\xc3":
                value = u32(getter + 1)
                raw = value.to_bytes(4, "big")
                if all(32 <= byte < 127 for byte in raw):
                    code = raw.decode("ascii")
        parent = next((ancestor for ancestor in table["ancestors"][1:]
                       if ancestor in tables), None)
        parent_functions = tables[parent]["functions"] if parent else []
        overrides = {hex(i * 4): hex(target) for i, target in enumerate(functions)
                     if i >= len(parent_functions) or target != parent_functions[i]}
        entry = {"class": name, "rawcode": code, "vtable": table["vtable"],
                 "ancestors": table["ancestors"], "comparison_parent": parent,
                 "slot_count": len(functions), "overrides": overrides,
                 "key_slots": {label: hex(functions[offset // 4])
                               for label, offset in SLOTS.items()
                               if offset // 4 < len(functions) and
                               (label in ("message", "code") or
                                ("CAbility" in table["ancestors"] and offset <= 0x1A8) or
                                ("CAbilitySpell" in table["ancestors"] and offset >= 0x330))},
                 "vtable_slots": {hex(i * 4): hex(target) for i, target in enumerate(functions)},
                 "factory": factory.get(code), "vtable_xrefs": []}
        classes.append(entry)
    for code in factory:
        matches = [row for row in classes if row["rawcode"] == code]
        if len(matches) != 1:
            raise ValueError(f"{code}: expected one primary class, found {len(matches)}")
    return classes, factory, pe, image


def verify_getters(classes, pe, image):
    by_code = {row["rawcode"]: row for row in classes if row["rawcode"]}
    assert by_code["AHtb"]["class"] == "CAbilityThunderBolt"
    assert by_code["AHtb"]["vtable"] == "0x6f8f56dc"
    assert by_code["AHtb"]["key_slots"]["cast_body"] == "0x6f054880"
    assert by_code["AOsh"]["comparison_parent"] == "CAbilityCarrionSwarm"
    assert "cast_body" not in by_code["AHbh"]["key_slots"]
    assert "effect_unit" not in by_code["BHbz"]["key_slots"]
    assert by_code["XHbz"]["class"] == "CEffectBlizzard"
    from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP
    mu = Uc(UC_ARCH_X86, UC_MODE_32)
    base = pe.OPTIONAL_HEADER.ImageBase
    size = (pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF
    mu.mem_map(base, size)
    mu.mem_write(base, image[:size].ljust(size, b"\0"))
    stack, stop = 0x100000, 0x200000
    mu.mem_map(stack, 0x1000)
    mu.mem_map(stop, 0x1000)
    count = 0
    for row in classes:
        if row["factory"] is None:
            continue
        mu.mem_write(stack + 0x800, struct.pack("<I", stop))
        mu.reg_write(UC_X86_REG_ESP, stack + 0x800)
        mu.reg_write(UC_X86_REG_ECX, 0)
        mu.reg_write(UC_X86_REG_EAX, 0)
        mu.emu_start(int(row["key_slots"]["code"], 16), stop, count=16)
        expected = int.from_bytes(row["rawcode"].encode("ascii"), "big")
        if mu.reg_read(UC_X86_REG_EIP) != stop or mu.reg_read(UC_X86_REG_EAX) != expected:
            raise AssertionError(f"Getter failed: {row['rawcode']} / {row['class']}")
        count += 1
    # Only message routing is checked; handler bodies are stubbed in emulator memory.
    routes = {0xD01B0: 0x6F1B6C00, 0xD01B1: 0x6F1B9DB0,
              0xD01C1: 0x6F1A1000, 0x123456: None}
    seen = []
    for target in routes.values():
        if target:
            mu.mem_write(target, b"\xc3")

    def record_route(uc, address, size, user):
        if address in routes.values():
            seen.append(address)

    mu.hook_add(UC_HOOK_CODE, record_route)
    dispatcher = int(by_code["XHbz"]["key_slots"]["message"], 16)
    for message, target in routes.items():
        seen.clear()
        mu.mem_write(stack + 0x800, struct.pack("<II", stop, stack + 0x900))
        mu.mem_write(stack + 0x908, struct.pack("<I", message))
        mu.reg_write(UC_X86_REG_ESP, stack + 0x800)
        mu.reg_write(UC_X86_REG_ECX, 0)
        mu.reg_write(UC_X86_REG_EAX, 0)
        mu.emu_start(dispatcher, stop, count=100)
        assert seen == ([] if target is None else [target]), (hex(message), seen)
        assert mu.reg_read(UC_X86_REG_EIP) == stop
        assert mu.reg_read(UC_X86_REG_EAX) == 1
        assert mu.reg_read(UC_X86_REG_ESP) == stack + 0x808
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("game_dll", type=Path)
    parser.add_argument("--factory-table", type=Path,
                        default=ROOT / "docs/spec/ability_factory_1.26a.tsv")
    parser.add_argument("--out-dir", type=Path,
                        default=ROOT / "docs/research/ability-atlas")
    parser.add_argument("--xref-json", type=Path,
                        help="Optional saved IDA xref_query result (addresses/metadata only)")
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    dll_sha256 = hashlib.sha256(args.game_dll.read_bytes()).hexdigest()
    classes, factory, pe, image = collect(args.game_dll, args.factory_table)
    if args.xref_json:
        data = json.loads(args.xref_json.read_text(encoding="utf-8"))
        if data.get("dll_sha256") != dll_sha256:
            raise ValueError("IDA xref snapshot does not match the input DLL SHA-256")
        by_vtable = {int(row["vtable"], 16): row for row in classes}
        for result in data["result"]:
            if result.get("error") or result.get("next_offset") is not None:
                raise ValueError(f"Incomplete IDA xrefs: {result.get('target')}")
            address = int(result["resolved_addr"], 16)
            if address in by_vtable:
                by_vtable[address]["vtable_xrefs"] = result.get("data", [])
    verified = verify_getters(classes, pe, image) if args.verify else None
    factory_classes = sorted((row for row in classes if row["factory"]), key=lambda row: row["rawcode"])
    # Source mentions are navigation aids, never a claim that an ability is implemented.
    mentions = defaultdict(set)
    for source in (ROOT / "crates/sim/src").rglob("*.rs"):
        for code in re.findall(r'''["']([A-Za-z0-9]{4})["']''', source.read_text(encoding="utf-8")):
            if code in factory:
                mentions[code].add(source.relative_to(ROOT).as_posix())
    for row in factory_classes:
        row["source_mentions"] = sorted(mentions[row["rawcode"]])
    summary = {"factory_rawcodes": len(factory), "mapped_factory_classes": len(factory_classes),
               "total_primary_classes": len(classes),
               "ability_and_buff_primary_classes": sum(row["class"].startswith(("CAbility", "CBuff")) for row in classes),
               "buff_primary_classes": sum(row["class"].startswith("CBuff") for row in classes),
               "effect_primary_classes": sum(row["class"].startswith("CEffect") for row in classes),
               "getter_execution_checks": verified,
               "blizzard_effect_dispatch_checks": 4 if args.verify else None,
               "spell_factory_classes": sum("CAbilitySpell" in row["ancestors"] for row in factory_classes),
               "factory_families": dict(Counter(row["comparison_parent"] for row in factory_classes).most_common())}
    output = args.out_dir
    output.mkdir(parents=True, exist_ok=True)
    document = {"dll_sha256": dll_sha256,
                "image_base": hex(pe.OPTIONAL_HEADER.ImageBase), "summary": summary,
                "slot_offsets": {name: hex(value) for name, value in SLOTS.items()}, "classes": classes}
    (output / "atlas.json").write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    columns = ["rawcode", "class", "vtable", "comparison_parent", "factory_wrapper",
               "typed_pointer_helper", "slot_count", *SLOTS, "overrides", "vtable_xrefs", "source_mentions"]
    with (output / "factory.tsv").open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, columns, delimiter="\t")
        writer.writeheader()
        for row in factory_classes:
            record = {key: row[key] for key in ["rawcode", "class", "vtable", "comparison_parent", "slot_count"]}
            record.update(row["factory"])
            record.update(row["key_slots"])
            record.update({key: json.dumps(row[key]) for key in ["overrides", "vtable_xrefs", "source_mentions"]})
            writer.writerow(record)
    users = defaultdict(set)
    for row in factory_classes:
        for offset, function in row["overrides"].items():
            users[function].add(f"{row['rawcode']}:{offset}")
    with (output / "override-functions.tsv").open("w", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream, delimiter="\t")
        writer.writerow(["function", "rawcode_slot_users"])
        writer.writerows((function, ",".join(sorted(values))) for function, values in sorted(users.items()))
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
