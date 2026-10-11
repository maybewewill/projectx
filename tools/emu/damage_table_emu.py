"""Unicorn check of the Misc damage-type table builder (Game.dll 1.26a, VA 0x6F26D300).

What it checks (docs/spec/attack-damage.md, section 4):
- the builder reads 7 rows x 8 columns of `DamageBonus<Name>` and 7 entries of `EtherealDamageBonus`
  through the Misc reader (VA 0x6F009EE0, called as key + index), and stores the returned f32 bits;
- the table at 0x6FAB4358 is row-major (row = attack type, 8 columns), the ethereal entries sit at
  0x6FAB42E4..0x6FAB42FC in the order given by the store addresses (not in index order);
- the builder runs once (flag at 0x6FA70E90 is cleared after the first build);
- the lookup `FUN_6f26d300(out, row, col)` returns table[row][col].

The Misc reader is replaced by a hook that answers from a synthetic table (the reader's own list and
S2R parsing are not part of this check). Synthetic values are written as f32 bits; nothing from the
user's game is embedded in the repository.

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/damage_table_emu.py
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll and provides call())
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_ECX, UC_X86_REG_EAX  # noqa: E402

BUILDER = 0x6F26D300
READER = 0x6F009EE0
LOOKUP = 0x6F26D300  # same function: the builder returns table[row][col] once built
FLAG = 0x6FA70E90
TABLE = 0x6FAB4358
ETHEREAL = {  # store address -> reader index (from the builder's store sequence)
    0x6FAB42E8: 0,
    0x6FAB42EC: 1,
    0x6FAB42F0: 2,
    0x6FAB42F4: 3,
    0x6FAB42F8: 4,
    0x6FAB42E4: 5,
    0x6FAB42FC: 6,
}
ROWS = ["DamageBonusSpells", "DamageBonusNormal", "DamageBonusPierce", "DamageBonusSiege",
        "DamageBonusMagic", "DamageBonusChaos", "DamageBonusHero"]

# synthetic Misc content: key -> list of f32 values (missing key = absent; short list = missing tail)
SYNTH = {}
for r, key in enumerate(ROWS):
    SYNTH[key] = [float(r * 8 + c) + 0.25 for c in range(8)]
SYNTH["EtherealDamageBonus"] = [100.5 + k for k in range(7)]

calls = {"reader": 0}
log = []


def f32_bits(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def reader_hook(uc, address, size, user):
    """Replaces the Misc reader: [esp]=return, [esp+4]=key, [esp+8]=index, ECX=out, EDX=section."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
    key_ptr = struct.unpack("<I", bytes(uc.mem_read(esp + 4, 4)))[0]
    index = struct.unpack("<I", bytes(uc.mem_read(esp + 8, 4)))[0]
    out = uc.reg_read(UC_X86_REG_ECX)
    key = bytes(uc.mem_read(key_ptr, 64)).split(b"\0")[0].decode()
    calls["reader"] += 1
    values = SYNTH.get(key)
    if values is None or index >= len(values):
        bits = f32_bits(0.0)  # DAT_6faae470 = 0.0 is the reader's default
    else:
        bits = f32_bits(values[index])
    log.append((key, index))
    uc.mem_write(out, struct.pack("<I", bits))
    uc.reg_write(UC_X86_REG_EAX, out)
    uc.reg_write(UC_X86_REG_ESP, esp + 12)  # callee pops two arguments (ret 8)
    uc.reg_write(UC_X86_REG_EIP, ret)


def main():
    emu.mu.hook_add(UC_HOOK_CODE, reader_hook, begin=READER, end=READER)
    # Start with the builder "not yet run" (flag set), as at process start.
    emu.mu.mem_write(FLAG, struct.pack("<I", 1))
    # Build: ECX = output (ignored for the table fill), EDX = row 0, stack = col 0.
    out_buf = emu.DATA + 0x800
    emu.call(BUILDER, ecx=out_buf, edx=0, stack_args=(0,))
    n_reads = calls["reader"]
    expected_reads = 7 * 8 + 7
    failures = 0
    checks = 0

    # 1. every DamageBonus cell
    for r, key in enumerate(ROWS):
        for c in range(8):
            got = struct.unpack("<I", bytes(emu.mu.mem_read(TABLE + r * 0x20 + c * 4, 4)))[0]
            want = f32_bits(SYNTH[key][c])
            checks += 1
            if got != want:
                failures += 1
                print("MISMATCH table", key, c, hex(got), hex(want))

    # 2. ethereal stores
    for addr, idx in ETHEREAL.items():
        got = struct.unpack("<I", bytes(emu.mu.mem_read(addr, 4)))[0]
        want = f32_bits(SYNTH["EtherealDamageBonus"][idx])
        checks += 1
        if got != want:
            failures += 1
            print("MISMATCH ethereal", hex(addr), idx, hex(got), hex(want))

    # 3. the flag is cleared and a second call does not read again
    flag = struct.unpack("<I", bytes(emu.mu.mem_read(FLAG, 4)))[0]
    checks += 1
    if flag != 0:
        failures += 1
        print("MISMATCH flag", flag)
    before = calls["reader"]
    emu.call(BUILDER, ecx=out_buf, edx=3, stack_args=(5,))
    checks += 1
    if calls["reader"] != before:
        failures += 1
        print("MISMATCH second call read the Misc data again")

    # 4. the lookup returns table[row][col] (row = attack type, column = armor type)
    for row in range(7):
        for col in range(8):
            got = emu.call(LOOKUP, ecx=out_buf, edx=row, stack_args=(col,))
            val = struct.unpack("<I", bytes(emu.mu.mem_read(got, 4)))[0]
            want = f32_bits(SYNTH[ROWS[row]][col])
            checks += 1
            if val != want:
                failures += 1
                print("MISMATCH lookup", row, col, hex(val), hex(want))

    print(f"reader calls in first build: {n_reads} (expected {expected_reads})")
    checks += 1
    if n_reads != expected_reads:
        failures += 1
    print(f"checks: {checks}, mismatches: {failures}")
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    main()
