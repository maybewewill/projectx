# Verified named-order patch for Claude

Prepared 2026-10-13. Apply `docs/research/named-orders/named-orders.patch`.
This patch expands string order lookup from 12 names to the original **348 unique names**,
preserves canonical reverse names, and matches ASCII names without case.
It fixes `IssueTargetOrder(caster, "holybolt", target)` and enables existing implemented spells
to be reached through their original string names. It does not implement the other spells themselves.

## Verified evidence

1. Executed the original initializer `0x6F229A00` in Unicorn. At registrar `0x6F3B0730`,
   captured the actual stack arguments `(order id, name pointer)`.
   Only the registrar was replaced in emulator memory with a return that cleans its two arguments.
   The original initializer returned cleanly; there was no instruction-budget timeout.
2. Executed the original smart-registration slice `0x6F3B07A4..0x6F3B07BC` as well.
   Total: **349 calls, 348 unique names and ids**. `recharge` is registered twice identically.
   Conflicting names or reverse-name aliases are rejected by the extraction script.
3. Independently decoded all **348 main-initializer call sites with Capstone**:
   every pushed id and string pointer matched the Unicorn capture, and every pointed-to
   string matched the captured name. Smart's separate slice is covered by step 2.
4. Original Game.dll name lookup `0x6F3A84A0` calls Storm ordinal 590 for hashing and
   ordinal 509 for comparison. Storm ordinal 509 calls the imported MSVCR80 `_strnicmp`.
   Executed original Storm ordinal 590 for every registered name, its lowercase spelling,
   and its uppercase spelling: **1,044 calls, every case-variant hash matched**.
   Whitespace variants of `holybolt` have different hashes; the patch does not trim spaces.
5. Added the JASS regression `holy_light_casts_through_the_string_order_native`.
   It **failed before the fix** because the order was not accepted. It passes afterward,
   asserting acceptance, healing to 500 HP, mana payment to 135, and cooldown activation.
6. Ran tests with `GAME_DLL` set: **32 passed, 0 failed, 0 ignored**:
   `spell_cast` 14, `order_natives` 14, `group_order_natives` 4.
   Existing name/id round-trip assertions executed for all 348 table entries.
   Added checks for spell names, mixed-case names, unknown/whitespace names,
   and `OrderId2String` canonical names.
7. Four Python extraction tests passed: duplicate deduplication, case-name conflict rejection,
   reverse alias rejection, and preservation of registered spelling in generated Rust.
8. `git diff --check` passed. The complete patch includes the new `orders/names.rs` module.
   `git apply --check` passed against the primary checkout at
   `1f2f9106fbb6f3d271e9176c06d404106ac1d92a`.

The original DLL SHA-256 is
`6d21bd9a0f9fbc8446f455c9e89ac994fed68174426fe608fcb9baefd4dec53c`.
The extractor rejects a different binary. No original binary bytes or decompiled bodies are exported.

## Integration

The tested isolated checkout is `D:\rewar3\named-order-fixes`, branch `codex/named-orders`,
based on `6a6c5b3966e74ed21b98c2af952c887351f338a6`.
Primary Claude gameplay source files were not modified by this work.
The previous numeric-order corrections are already in the primary history (`49c1e3b`);
this patch builds on those corrections and should not revert them.

From `C:\Users\slash\projectx`, after finishing any active merge:

```powershell
git apply --check docs/research/named-orders/named-orders.patch
git apply docs/research/named-orders/named-orders.patch
$env:GAME_DLL = 'C:\Users\slash\re\game.dll'
cargo test -p sim --test spell_cast --test order_natives --test group_order_natives
```

Files changed by the patch:

- `crates/sim/src/orders/names.rs`: generated table of 348 canonical name/id pairs.
- `crates/sim/src/orders.rs`: re-export the table; use `eq_ignore_ascii_case` for name lookup.
- `crates/sim/tests/order_natives.rs`: native name/id, case and unknown-name regression checks.
- `crates/sim/tests/spell_cast.rs`: actual spell cast through the string order native.

If newer changes alter the test context, integrate the hunks rather than copying older source files.
String order natives share `natives/order.rs::string_id`, so no per-native copy of the table is needed.

## Reproduce extraction

```powershell
py tools/analysis/test_named_orders.py
py tools/analysis/named_orders.py C:\Users\slash\re\game.dll
```

To regenerate a Rust module in an isolated checkout:

```powershell
py tools/analysis/named_orders.py C:\Users\slash\re\game.dll --rust-output D:\rewar3\named-order-fixes\crates\sim\src\orders\names.rs
```

`orders.json` contains registration history and call-site provenance; `orders.tsv` is the compact table.

## Verification boundary

The registration table, original argument capture, independent static cross-check, and the listed
tests are verified. ASCII case matching is supported by the original hash executions and comparator
import; a complete live original-world lookup was not run. Full replay synchronization and arbitrary
non-ASCII/locale-dependent string behaviour were not tested. This is a verified lookup/routing patch,
not a claim of 100% equivalence of the complete game engine.
