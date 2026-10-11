# Abilities in Warcraft III 1.26a (Game.dll): level records, the definition builder, cooldown and mana (pass 3)

Scope: the four questions of the pass-3 brief. (1) The definition and level-record builder: which data columns bind to
which record offsets. (2) Cooldown storage, countdown and the ready check. (3) Mana cost check and payment, and the
Dur/HeroDur choice. (4) The timed-life death route (not re-read in this pass). It corrects `abilities.md` and
`abilities-2.md` where they say so.

Labels: CONFIRMED = read in the disassembly/decompile and/or checked in Unicorn against the original code.
LIKELY = strong evidence, not run end to end. UNSURE = a guess or open point. NOT FOUND = searched, no answer yet.

Sources: disassembly (capstone, `tools/analysis`), decompile chunks in the scratchpad (not committed), Unicorn runs
of the original (`tools/emu/abilitytype_binder_emu.py`, 94 checks, 0 mismatches). No function body is copied into the
repo; addresses and behaviour only.

---

## 0. Summary

| Question | Answer | Confidence |
|---|---|---|
| 1. Definition fields | Builder `0x6F2657A0` (fastcall, ECX = raw code) fills the definition node and the level array. Layout in section 2. | CONFIRMED (Unicorn, 94 checks) |
| 1. Level record | 0x68 bytes, 26 dwords. Offsets in section 3. | CONFIRMED (Unicorn) |
| 1. Column binding | `Cast`, `Dur`, `HeroDur`, `Cost`, `Cool`, `Area`, `Rng`, `DataA..I`, `UnitID`, `BuffID`, `EfctID`, `targs` per level; definition-level `levels`, `version`, `checkDep`, `reqLevel`, `levelSkip`, `priority`. | CONFIRMED (field names and offsets); LIKELY (the level suffix rule) |
| 1. Ability `+0x4C` | **Not a record pointer.** It is the level's target-flags word (record `+0x00`). Corrects abilities.md 1.1/1.3 and abilities-2.md 1.3. | CONFIRMED (disassembly of `0x6F02E6A0`, Unicorn) |
| 1. Getter family | One getter per record offset (section 4). | CONFIRMED (disassembly; Unicorn 51 checks) |
| 1. Cooldown clamp at load | `Cool` stored as `Cool` when `0.0 <= Cool`, else `0.0` (NaN gives `0.0`). | CONFIRMED (Unicorn) |
| 2. Cooldown storage | The level value is `+0x14`. The per-ability running countdown is **NOT FOUND**. | CONFIRMED (value); NOT FOUND (countdown) |
| 2. Ready check | **NOT FOUND**. | NOT FOUND |
| 3. Mana cost | Cost is `+0x10` (integer), read through getter `0x6F02E840`. Check and payment: **NOT FOUND** (one predicate reads Cost, section 5.2). | CONFIRMED (read); NOT FOUND (payment) |
| 3. Dur vs HeroDur | Both getters exist (`0x6F02E760` Dur, `0x6F02E7D0` HeroDur). The choice site is **NOT FOUND**. | CONFIRMED (getters); NOT FOUND (choice) |
| 4. Timed-life death route | Not re-read in this pass (see abilities-2 section 3.3, still open). | UNSURE |

---

## 1. Corrections to earlier passes

1. `FUN_6F02E6A0` (`abilities.md` 1.1, `abilities-2.md` 1.3) is **not** "the level record at index times 0x68". The
   disassembly is: `imul edi, edi, 0x68`, then `mov eax, [edi + base]`. It returns the **first dword** of the level
   record (the target flags). It returns 0 when the index is negative or not below the count, or when the definition
   is missing or invalid (CONFIRMED, Unicorn: 8 index cases, invalid definition, lazy definition fetch).
2. Therefore ability `+0x4C` holds the current level's **target-flags word**, not a pointer. Several callers test or set bits of `+0x4C`
   (for example `(byte)+0x4C & 4` in `chunk_002` and `| 0x20` in `chunk_001`), which fits a flag word. The record
   pointer that `abilities.md` describes does not exist as a field. (CONFIRMED by the accessor; the callers were not
   all read.)
3. The definition's level count is at `+0x50` and the level array at `+0x54` (CONFIRMED, the container write in the
   builder's `levelset` call and the accessor). The builder's own `levels` value (the SLK integer) is stored at
   `+0x3C`, which is the maximum level (agrees with `abilities.md`).

---

## 2. The definition builder `0x6F2657A0` (CONFIRMED structure)

Called with ECX = raw code of the ability. Entry point used by the definition lookup `0x6F265ED0` when no definition
exists (`0x6F265ED0` calls `0x6F4C8520` for the hash, `0x6F264D50` for the lookup, and this builder on a miss).

Definition node (allocated by `0x6F2656D0` in the definition table at `0x6FAB3E48`):

| Offset | Field | Source |
|---|---|---|
| `+0x00` | hash of the raw code (`0x6F4C8520` over the four bytes) | builder |
| `+0x14` | raw code | builder |
| `+0x18..+0x24` | reader state of the row; the validity test uses `+0x1C`, `+0x20`, `+0x24` | `0x6F25F5C0` |
| `+0x28` | global object from `0x6F32C8E0` | builder |
| `+0x2C` | valid flag: 1 when `(+0x1C != 0 && +0x20 != 0) || +0x24 != 0`, else 0. Invalid stops the build | builder |
| `+0x30` | `0x6F262A50` (source not read) | builder |
| `+0x34` | owner handle `0x6F262A70` (source not read) | builder |
| `+0x38` | `checkDep` as a flag (non-zero means 1) | SLK `checkDep` |
| `+0x3C` | `levels`: maximum level | SLK `levels` |
| `+0x40` | `reqLevel` | SLK |
| `+0x44` | `levelSkip` | SLK |
| `+0x48` | `priority` | SLK |
| `+0x50` | level count (set by the container `0x6F264C50` with the same count) | builder |
| `+0x54` | level array base (0x68 per level) | `0x6F264C50` |
| `+0x1F8` | `version` | SLK |

The level array is allocated with `levels` records. Each level `k` (1-based) is read with the cell reader at row `k`
(CONFIRMED: the loop variable is passed as the row argument of every cell read).

---

## 3. The level record (0x68 bytes) (CONFIRMED layout)

| Offset | Field | Reader | Stored as |
|---|---|---|---|
| `+0x00` | `targs` (target flags) | text, then the target-flag parser `0x6F255250` | u32 from its result (see 3.2) |
| `+0x04` | `Cast` | real | f32 |
| `+0x08` | `Dur` | real | f32 |
| `+0x0C` | `HeroDur` | real | f32 |
| `+0x10` | `Cost` | integer | u32 (strtol-style, see 3.3) |
| `+0x14` | `Cool` | real, then clamp (3.4) | f32 |
| `+0x18` | `Area` | real | f32 |
| `+0x1C` | `Rng` | real | f32 |
| `+0x20`..`+0x40` | `DataA`..`DataI` (nine reals, step 4) | real | f32 |
| `+0x44` | `UnitID` | text, then the raw code parser `0x6F4C3F50` (0 if empty) | u32 |
| `+0x48` | BuffID list: capacity | | u32 |
| `+0x4C` | BuffID list: count | | u32 |
| `+0x50` | BuffID list: slot pointer | | pointer (slots are u32 raw codes) |
| `+0x54` | (not written by the builder) | | |
| `+0x58` | EfctID list: capacity | | u32 |
| `+0x5C` | EfctID list: count | | u32 |
| `+0x60` | EfctID list: slot pointer | | pointer |
| `+0x64..+0x67` | unused by the builder | | |

The list container at `+0x48` is a (capacity, count, pointer) triple: the growth routine `0x6F25A760` (thiscall, ECX =
container) sets capacity and the pointer, and the builder sets the count (CONFIRMED).

### 3.1 Column suffix (LIKELY)
The builder passes the field name (for example `Cast`) and the level number to the Storm cell reader (ordinal 578). The
column name seen in the old notes is the field with the level appended (`Cast1`, `Cast2`). The model in
`crates/gamedata/src/abilitytype.rs` uses that suffix. The exact mapping inside Storm is not read (Storm.dll is not
in the container), so this is LIKELY.

### 3.2 Target flags (UNSURE)
`0x6F255250` (fastcall, ECX = text) walks the words of the text and checks each against a table of 0x33 names
(`PTR_DAT_6fa6f478`); an unknown word reports "Unknown Target Flag" through `0x6F4CC1E0`. The function is declared
void, and the builder stores its EAX as the `targs` word, so the stored value is whatever EAX holds on return. The
flag bits are therefore NOT decoded. The Rust model keeps the text (`AbilityLevel::targs`).

### 3.3 Integer cells (CONFIRMED)
`Cost` and the definition integers use the integer reader `0x6F2625F0` (cell in, value in EAX). The conversion is the
same `strtol(text, 0)` family as the unit-type integer cell (`unittype_int_emu.py`, 25 cases, 0 mismatches), with the
apostrophe raw-code form. The Rust model reuses `unittype::cell_int_bits`.

### 3.4 The cooldown clamp (CONFIRMED, Unicorn)
After reading `Cool`, the builder stores `Cool` when `0.0 <= Cool` (x87 compare, so `-0.0` is kept) and `0.0`
otherwise (negative values and NaN). Checked on `-3` (gives 0), `-0` (gives `0x80000000`), `2.5` (kept).

### 3.5 Lists (CONFIRMED, Unicorn)
- Count = `max(1, strlen >> 2)` (the text length is taken before the split, and an empty or missing text gives 1).
- Tokens are split on space, `,`, `-`, `_` and tab (the splitter `0x6F6EB606` is an import; its exact handling of empty
  and over-long tokens is UNSURE, the tests avoid tokens of more than four characters).
- Each non-empty token is a raw code (first four bytes, big-endian). Empty tokens take no slot.
- Slots beyond the stored tokens stay zero, and the count keeps the estimate (for example `"a,b,c,d"` has one slot).

---

## 4. The per-level getter family (CONFIRMED, disassembly; Unicorn 51 checks)

Each getter takes the ability (ECX) and a zero-based level index (the ability keeps the current one in `+0x50`; the
builder's rows are 1-based and the getters are called with `level - 1`). Out-style getters take
`(out pointer, index)` and write the dword, or the float 0.0 for an invalid index. Return-style getters take `(index)`
and return the dword, or 0.

| Record offset | Getter | Style | Meaning |
|---|---|---|---|
| `+0x00` | `0x6F02E6A0` | return | target flags (the ability's `+0x4C`) |
| `+0x04` | `0x6F02E6F0` | out | Cast |
| `+0x08` | `0x6F02E760` | out | Dur |
| `+0x0C` | `0x6F02E7D0` | out | HeroDur |
| `+0x10` | `0x6F02E840` | return | Cost |
| `+0x14` | `0x6F02E890` | out | Cool |
| `+0x18` | `0x6F02E900` | out | Area |
| `+0x1C` | `0x6F02E970` | out | Rng |
| `+0x20`..`+0x40` | `0x6F02E9E0`, `0x6F02EA50`, `0x6F02EAC0`, `0x6F02EB30`, `0x6F02EBA0`, `0x6F02EC10`, `0x6F02EC80`, `0x6F02ECF0`, `0x6F02ED60` | out | DataA..DataI |
| `+0x44` | `0x6F02EDD0` | return | UnitID |
| (list) | `0x6F02EE20`, `0x6F02EE80` | (list accessors, not read) | BuffID / EfctID slot access (LIKELY) |

Callers: the Dur getter `0x6F02E760` has by far the most callers in the decompile (hundreds of lines); Cool
`0x6F02E890` has one (section 5.1). Cost `0x6F02E840` has at least two (section 5.2).

---

## 5. Cooldown, mana, and the Dur/HeroDur choice

### 5.1 Cooldown
- The level value is `Cool` (`+0x14`), read by getter `0x6F02E890` (CONFIRMED).
- Its one caller is `0x6F1F4A70` (a small method of an ability class). The decompile calls the getter into a local
  and then does not use it (UNSURE; the decompile may have lost a result). No other reader of `+0x14` was found in the
  decompile by pattern search.
- **Countdown and ready check: NOT FOUND.** The order start path `0x6F29DFF0` was followed: it calls the ability's
  start hook through the event dispatch (`0x6F279FE0` → ability dispatch slot 12, which is the generic `0x6F471C20`).
  The class start hook (for example `CAbilityAttack` slot 3 `0x6F0D16E0`) handles the weapon's own cooldown, which is
  the attack-cycle timer (see `attack-cycle.md`), not a general ability cooldown.
- `UnitResetCooldown` changes no ability field (abilities-2 section 1.2, unchanged).

Model (in `crates/sim/src/abilities.rs`): `Cooldowns` keeps one one-shot `TimerQueue` timer per ability base code. A
cast starts the timer for `Cool` seconds, the ready check is "no timer running", and `advance_to` returns the abilities
that became ready. Restarting cancels the old timer; zero or negative values start nothing. This is a model (UNSURE).

### 5.2 Mana
- Cost (`+0x10`, integer) is read by getter `0x6F02E840`.
- Predicate `0x6F052990` (`__fastcall`, ability) returns 0 when the ability is hidden by its flags (`+0x08` bit `0x20`,
  or virtual slots `0x1EC`/`0x1E8` non-zero), or when the ability has no type flag (slot `0x1D8` zero), the global
  level count is below 2 (`0x6F02E570`) and Cost is zero. Otherwise it returns 1. Its caller was not read.
- **Payment and the check against the unit's mana: NOT FOUND.**

Model: `cast_check` tests the cooldown first and then `mana >= Cost` (the order is UNSURE); `cast` pays `Cost` and
starts the cooldown. Mana is plain f32 arithmetic (UNSURE: the original may use its software float for mana).

### 5.3 Dur versus HeroDur
- Dur `+0x08` (getter `0x6F02E760`) and HeroDur `+0x0C` (getter `0x6F02E7D0`) are read in the same functions, for
  example the pair at `chunk_003` around the attack-related code and the buff code (both one getter call each).
- **The choice site for a hero unit: NOT FOUND.** The timed-life apply path takes one duration argument
  (abilities-2 2.5), so the choice, if any, happens before it.

### 5.4 Other fields
- `Area` (`+0x18`), `Rng` (`+0x1C`), `DataA..I` are read by many callers; the spell-specific meaning is not part of
  this pass.

---

## 6. Timed-life death route (not re-read)
abilities-2 section 3.3 is unchanged: the removal notify of the timed-life class does not call a kill directly, and
the death route (the observer list of the definition, or the owner's slot 33) is still open. The sim keeps `KillUnit`
at expiry, marked UNSURE.

---

## 7. Verification

| Check | Count | Result |
|---|---|---|
| Unicorn: raw code parser (`0x6F4C3F50`, original code) | 8 | 0 mismatches |
| Unicorn: level accessor (`0x6F02E6A0`) incl. invalid definition and lazy fetch; level up (`0x6F041700`) | 13 | 0 mismatches |
| Unicorn: per-level getter family (15 out-style, 2 return-style, 3 indices each) | 51 | 0 mismatches |
| Unicorn: definition builder on a synthetic 3-level ability (fields, offsets, clamp, lists, count rules) | 22 | 0 mismatches |
| Total | 94 | 0 failures (`tools/emu/abilitytype_binder_emu.py`, `GAME_DLL=...`) |

Stubs used in the builder run (not checked here): the Storm cell fetch (`0x6F6EB5A6`), the cell readers
(`0x6F262760`, `0x6F2628E0`, `0x6F2625F0`: the real-number conversion is replaced by `float()`), the target-flag parser
(result not modelled), the token splitter (`0x6F6EB606`), the list and node allocators, the definition container, and
the CRT `memset`. The record offsets, the clamp, the list rules and the count rules come from the original code.

Rust:
- `crates/gamedata/tests/abilitytype.rs`: 9 tests (raw code, list slot rule, cooldown clamp, one level with all fields,
  missing cells, integer cells, definition with levels, zero levels and missing rows, `load_all` order).
- `crates/sim/src/abilities.rs` `cooldown_tests`: 7 tests (run-down and ready once, zero cooldown, restart cancels,
  reset, independence, cast pays and refuses, exact mana).

---

## 8. Open items (priority order)
1. Cooldown countdown and ready check: find the reader of the level `Cool` value that is not the `0x6F1F4A70` stub.
   Run the candidate with a Unicorn harness on an ability object with a recorded timer list.
2. Mana payment: find the cast path that compares unit mana with `Cost` and subtracts it.
3. Dur versus HeroDur for a hero: find the caller that chooses `0x6F02E7D0` over `0x6F02E760`.
4. The target-flag parser `0x6F255250` return value and the flag table (`PTR_DAT_6fa6f478`).
5. Storm ordinals 578 (cell fetch: row and column rule) and 504 (token split): check with `Storm.dll` when available.
6. Timed-life death route (abilities-2 open item 1).

## 9. Files
- `tools/emu/abilitytype_binder_emu.py`: the Unicorn checks of section 7.
- `crates/gamedata/src/abilitytype.rs`, `crates/gamedata/tests/abilitytype.rs`: the level record reader.
- `crates/sim/src/abilities.rs` (`Cooldowns`, `cast_check`, `cast`, `cooldown_tests`).
