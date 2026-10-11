# Heroes: experience, levels, skill points, attributes (Game.dll 1.26a)

Scope: the hero experience tables, experience gain and level changes, skill points, and the strength, agility and
intelligence getters and setters (intelligence in section 6). The kill-experience split and the derived attribute
bonuses are only partly decoded (section 7, numbers not resolved).

Labels: **CONFIRMED** = read in the disassembly and/or checked in Unicorn against the original; **LIKELY** = strong
reading of the decompile or disassembly, not run; **UNSURE** = guessed or open. Addresses are 1.26a `Game.dll` VAs.
Harnesses are in `tools/emu/`; the vector files are written to a scratch path and are not committed.

## 1. Hero object and level counter

- The hero sub-object is at unit `+0x1F0` (CONFIRMED). Its fields used here: experience `+0x8C` (int), skill points
  `+0x90` (int), experience suspended flag `+0xEC`, base strength `+0x94`, strength gain `+0xD4` (real), base agility
  `+0xA8`, agility gain `+0xE4` (real), and the five hero ability slots from `+0xF4` (codes; not modelled).
- The level counter is `c = level - 1`. It is read by `0x6F473170` and written by `0x6F473140` through the unit
  object (CONFIRMED). `GetHeroLevel` (`0x6F2A0210`) returns `c + 1` for a hero (CONFIRMED). The model keeps the level
  (1-based) in `HeroObj::level`.
- A new hero starts at level 1 with no experience (LIKELY, unchanged from the model).

## 2. Experience tables (builder `0x6F00ABD0`)

The builder runs once. It reads the Misc keys through three readers: the list reader `0x6F009F90` (key in `EDX`,
index on the stack, integer result), the count `0x6F009D10` (key in `EDI`), and the real reader `0x6F009EE0` (out
pointer in `ECX`, key and index on the stack, default `0.0`) (CONFIRMED).

Keys: `MaxHeroLevel` (N), `NeedHeroXP` and `NeedHeroXPFormulaA/B/C`, `GrantHeroXP` and `GrantHeroXPFormulaA/B/C`,
`HeroFactorXP`, `MaxUnitLevel` (N2), `GrantNormalXP` and `GrantNormalXPFormulaA/B/C` (CONFIRMED key list).

Arithmetic (CONFIRMED by Unicorn trace and the disassembly of the loops):

- Each series takes its first `min(N, count)` entries from its list. The remaining entries follow a recurrence over
  the float value `v`, starting from `prev = float(last list entry)`:
  `v = (A * prev + B * x) + C`, with `x = i + 2` for NeedHeroXP and `x = i + 1` for GrantHeroXP and GrantNormalXP,
  where `i` is the index. The stored int is `f2i(v)` (truncation); the next `prev` is the float `v`.
  The three constants `A, B, C` are the formula keys of the series (for GrantHeroXP the grant formula keys).
- Entry N-1 of NeedHeroXP is then overwritten with entry N-2 (CONFIRMED, the last-index rule). It is not applied to
  the grant or unit series.
- Factor column (`+8`, f32): for `i < min(N, count)`, `factor[i] = I2R(HeroFactorXP[i]) * recip(100.0)`. The builder
  multiplies by the reciprocal of 100.0 (the start-up constant `0x6FAAE574`); it does not divide. Past the list the
  last factor repeats (CONFIRMED).
- Unit grant column: one `u32` per unit level index `0..N2`, built like the grant series with `x = i + 1`, no
  trailing rule (CONFIRMED).

Verification: `tools/emu/hero_xp_emu.py` runs the builder in Unicorn over random synthetic Misc content (300 cases,
seed 1; lists of at least one element, see the limits below) and reads back the three columns and the unit column.
`crates/sim/tests/hero_xp_vectors.rs` compares `sim::heroes::build` with them: **300 cases, 11,336 values, 0
mismatches**, including the need lookup for levels from -2 to N+3 (section 3).

Limits (UNSURE): when a list is empty but the table is not, the builder reads the word in front of its table (the
previous record or the allocator header). This is not modelled: the Rust builder starts such a series from `0`, and
the harness never generates an empty list.

## 3. Level lookup and maximum level

- The need lookup `0x6F207760(level)` returns the entry `clamp(level, 1, N) - 1` of the hero table, field `+0`
  (CONFIRMED by disassembly and 300 Unicorn cases over levels -2..N+3). It returns 0 when the table is empty.
- The threshold for passing from level `c + 1` to `c + 2` is `need(c + 1)`. So NeedHeroXP[0] is the experience to
  reach level 2 (LIKELY from the level-up loop; consistent with the usual 200 XP for level 2).
- A hero is at maximum when `N <= c + 1`, that is level `>= N` (CONFIRMED, `0x6F207F10`).

## 4. Experience, levels and skill points

Natives (all: hero gate of section 8, no effect on a failing gate):

- **AddHeroXP(unit, amount, show)** → `0x6F3C6CF0`. Converts the amount to f32 and calls the experience engine
  `0x6F20B080` with the plain branch (no handicap scaling, no kill distribution, no range) (CONFIRMED by the
  argument pattern; LIKELY for the zero arguments).
- The engine (`0x6F20B080`, plain branch), in order:
  1. Nothing happens when the hero is suspended (`+0xEC` non-zero) or the amount is zero (CONFIRMED).
  2. Gain = `f2i(floor(value + 0.5))`, which rounds half up before truncation (CONFIRMED: constant `0x6FAAE4D0` = 0.5,
     `floor` `0x6F6EF170`). For integer amounts the gain is the amount.
  3. XP `+0x8C` += gain (wrapping int add).
  4. Level loop: while `need(c + 1) <= XP` and the hero is not at maximum: level up (`c += 1`), grant one skill point
     if the new `c < N`, and repeat (LIKELY from the loop shape; CONFIRMED the condition order).
  5. At maximum: XP is set to `need(c + 1)` (the threshold of the current level), whether or not a level was gained
     (CONFIRMED).
  6. After a level-up, skill points are capped at the learnable ability levels of the five hero ability slots
     (`0x6F20B080` tail, when a hero ability exists). Not modelled: the cap is applied only when the caller supplies
     it (the natives pass none, so the cap is off). UNSURE for real heroes.
- **SetHeroXP(unit, target, show)** → `0x6F3C6A70`. Only a positive difference is applied: when
  `target - XP > 0` it calls AddHeroXP with that difference (CONFIRMED). A lower target does nothing (CONFIRMED by the
  decompile, UNSURE as game behaviour).
- **SetHeroLevel(unit, level, show)** → `0x6F3C6D70`. Applies only when `level > 0` and the current level is not above
  `level` (CONFIRMED). The target is `need` of the argument `level - 1` through the clamp (CONFIRMED argument flow):
  so `SetHeroLevel(u, 1)` at level 1 targets `need(1)` (the threshold of level 2) and would raise the hero. LIKELY as
  written; UNSURE whether the original special-cases level 1.
- **UnitStripHeroLevel(unit, count)** → `0x6F3C6930` → `0x6F20B6E0`. Returns false at level 1 with no skill points
  (CONFIRMED). The count is treated as unsigned and limited to the current level. It removes levels one at a time
  (`0x6F20B4D0`) and stops at the first failure. Returns true when at least one level was removed (CONFIRMED).
  Removing a level: at level 1 the XP becomes 0 and the call fails; otherwise the level drops, one skill point is
  spent when there is one, and XP becomes `need(new c)` (0 at level 1) (CONFIRMED for the XP and the counter).
  Not modelled: the ability-level reduction the original does when no skill point is left and when a hero ability
  needs a lower level (UNSURE for the exact rule, `0x6F207B60` not read).
- **UnitModifySkillPoints(unit, delta)** → `0x6F3C69A0` → `0x6F208B80` (CONFIRMED): the change cannot take the
  points below zero (`delta` is clamped to `-points`); a zero change returns false; the change is capped at the
  learnable ability levels when known (not applied, as above); returns true when the change is non-zero.
- **GetHeroSkillPoints** → `0x6F3C6780` = `+0x90` (CONFIRMED).
- **GetHeroXP** → `0x6F3C6A10` = `+0x8C` (CONFIRMED).
- **GetHeroLevel** → `0x6F3C6ED0` = `c + 1` (CONFIRMED).
- **SuspendHeroXP / IsSuspendedXP**: flag `+0xEC` (CONFIRMED offset, unchanged from the earlier pass).

Verification: the arithmetic of the engine is the softfloat operations of the crate (bit-exact, see
`docs/spec/softfloat.md`). The engine itself, the level loop and the strip rules are **not run** in the emulator
(needs a hero object with the level counter and the ability records); they are covered by unit tests in
`crates/sim/src/heroes.rs` written from this section.

## 5. Strength and agility

- **GetHeroStr(unit, includeBonuses)** → `0x6F3C67E0` → `0x6F2777B0`: `base + trunc(gain * c)` where `base = +0x94`,
  `gain = +0xD4` (real), `c` the level counter (CONFIRMED by disassembly). With `includeBonuses` false the original
  subtracts the item bonus (`0x6F077FA0`). Items are not modelled, so the bonus is 0 and both values are equal.
- **GetHeroAgi** → `0x6F3C6850` → `0x6F2778B0`: the same with `base = +0xA8`, `gain = +0xE4` (CONFIRMED).
- **SetHeroStr(unit, value, show)** → `0x6F3C6630` → `0x6F277950`: the base moves by `value - current` where `current`
  is the getter without items; the base is `+0x94` (CONFIRMED that the setter adds to `+0x94` through `0x6F20A160`).
- **SetHeroAgi** → `0x6F3C66A0` → `0x6F2779B0` → `0x6F20A1E0`: the same on `+0xA8` (CONFIRMED).

Verification: `tools/emu/hero_stat_emu.py` runs the two getters in Unicorn with the counter stubbed (800 cases, random
bases, gains including negative and large values, counters 0..25). `crates/sim/tests/hero_stat_vectors.rs` compares
`sim::heroes::attribute`: **800 cases, 0 mismatches**. The truncation is `f2i` (toward zero): `-47.6 -> -47`.

Setter side effects (ability re-application, see section 7) are not modelled.

## 6. Intelligence (CONFIRMED, Unicorn 300 cases, 0 mismatches)

- **Core** `0x6F0DB3A0` (`__fastcall`, ECX = hero sub-object at unit `+0x1F0`): result = `c1 + trunc(gain * c2)`.
  `gain` is the real at hero `+0xDC`. `c2` is the counter read through the reference object at hero `+0x6C` (the
  level counter, the same one as section 1). `c1` is the counter read through the reference object at hero `+0x7C`,
  which is the **intelligence base**. The two references are different objects, so the earlier open question
  (base = level counter?) is closed: the base is its own counter (CONFIRMED by the `lea` operands of the core).
- **Counter access.** `0x6F473170` (ECX = reference object) reads a handle id from the reference (`+8`), resolves it
  through the handle table lookup `0x6F03FA30` and returns the `+0x78` field of the resolved object. `0x6F473140`
  writes the same field (CONFIRMED by disassembly).
- **GetHeroInt** → `0x6F3C68C0` → `0x6F277850(includeBonus)` → core. With `includeBonus` false the original also
  subtracts the item part (`0x6F077FA0`). Items are not modelled, so both values are equal.
- **SetHeroInt** → `0x6F277980`: `delta = value - core(without items)`, then `0x6F20A260(hero, delta, show)`, which
  adds `delta` to the counter behind hero `+0x7C` (CONFIRMED). The rest of `0x6F20A260` refreshes the unit: the
  shared refresh `0x6F332810`, the attack ability refresh `0x6F2087B0` when the unit has an ability at `+0x1E8`, and
  `0x6F209210` / `0x6F2092A0` (LIKELY the same refresh as the strength setter). Those side effects are not modelled
  (UNSURE what they change beyond the counter).
- **Creation** (`0x6F20B9F0`, body after the zero-amount call to `0x6F20B080`): the type record fills the hero:
  `+0x10` → str base (`+0x94`); `+0x14` (real) → strength gain object at hero `+0xD0` (value `+0xD4`); `+0x18` → agi
  base (`+0xA8`); `+0x24` (real) → agility gain object at `+0xE0` (value `+0xE4`); **`+0x28` → the intelligence base
  counter (reference `+0x7C`, written with `0x6F473140`)**; **`+0x2C` (real) → intelligence gain object at `+0xD8`
  (value `+0xDC`)**. The vtable-carrying objects at `+0xD0`, `+0xD8`, `+0xE0` are embedded in the hero (their vtable
  pointer is the first dword at the object address; CONFIRMED by the `lea` operands). `+0x1C` (real) and `+0x20`
  (real) go to the embedded objects at hero `+0xB4` and `+0xBC`; their meaning is not read (UNSURE; they are not
  attributes). `+0x08` is written to skill points `+0x90` and `+0x0C` to the suspend flag `+0xE8` (LIKELY, not checked
  against a hero).
- **Model.** `sim::heroes::intelligence` = `attribute` (same form, base + trunc(gain * counter)); `rebase` for the
  setter. `HeroAttributes` and `HeroObj` carry `int_base` / `int_plus`; the engine fills them from the unit record
  (`INT`, `INTplus`). Verification: `tools/emu/hero_int_emu.py` (300 cases, seed 7) against
  `crates/sim/tests/hero_int_vectors.rs` (`HERO_INT_VECTORS`), plus unit and JASS-level tests.

## 7. Kill experience and attribute bonuses (structure decoded, numbers open)

The numeric rules of the kill split are **not** resolved. Nothing below is implemented; the kill path is not hooked.
The structure is recorded so the next pass can start from it.

### 7.1 Kill path (CONFIRMED addresses)

- **Death handler** `0x6F2A4880` (`__fastcall`, ECX = dying unit). It returns early when the death has no killer
  (`0x6F4786B0` returns the killer), or when the filter `0x6F285B00` rejects the unit. Otherwise it calls
  `0x6F40B250` on the unit, updates a player counter at player `+0x250` for a hero-type victim, and, unless the unit has
  the illusion flag (unit `+0x5C` bit 30), calls `0x6F2A00E0(killer)`, the kill record builder `0x6F2A46F0`
  (dying, killer), and `0x6F282880`. Caller: `0x6F2A7F3C` (the death entry; not read further).
- **Kill record builder** `0x6F2A46F0` (`this` = record, args dying and killer). It requires a killer and that
  `0x6F285AC0(dying) == 0`. It also checks a global flag at `0x6FAB4268` and `0x6F26DE50(0)` (not identified). It
  stores the victim position (through the victim's `+0xB8` slot, UNSURE which), the victim's level (its `+0xEC` slot),
  reads the Misc real **`HeroExpRange`** (`0x6F009EE0`, index 0), and builds the candidate list. The candidates come from
  the range enumerator `0x6F46D1B0` with the callback `0x6F290770` (the spatial walk order is the one of
  `natives/region.rs` / groups-events-2; not re-read here). The record then goes to `0x6F2A3190`.
- **Per-candidate loop** `0x6F2A3190` (`this` = record, arg = victim). The list is `record+0x14` (count) with array
  `+0x18`; when that count is 0 the alternate `+0xE0` count with array `+0xE4` is used (CONFIRMED). If the value is
  not positive it returns. For each candidate hero it calls the wrapper `0x6F26EB70` with the value pointer, show 1,
  0, handicap flag 1, a flag that is 1 when the victim's level is at least 12, and range flag 1. The wrapper calls
  `0x6F20B080` (the XP engine, section 4) for the candidate's hero sub-object when it has one (CONFIRMED).
- **XP engine branch** (`0x6F20B080`, arguments: value pointer, show, a `0`/non-zero flag, handicap float, the
  level-walk flag, range float). With the level-walk flag set, the gain is taken from a walk over the hero levels
  using the need table (`0x6F207760`) and the Misc `GrantHeroXP` / `GrantNormalXP` lookups; the walk stops when the
  remaining value no longer passes the next need, and the result is truncated (CONFIRMED the control flow; the
  arithmetic not Unicorn-checked). The owner's handicap (`player +0x2A4`, read through the global at `0x6F2834E0`)
  scales the value when the handicap flag is set.

Open (kill XP): the formula for the value `F` in `0x6F2A3190` (the first `i2r` of the count, then `0x6F2A0230`
which looks at the victim's ability/level and returns an int, then a division by the candidate count is LIKELY: the
split), the `HeroFactorXP` and `SummonedKillFactor` uses, `SetPlayerHandicapXP` storage, and the rounding per
recipient. Iteration order does not change the numbers unless a shared accumulator is updated per recipient, which
has not been seen (LIKELY). These need a Unicorn run of `0x6F2A3190` with stubs for the candidate list and the
per-hero call, then a second run of the level walk in `0x6F20B080`.

### 7.2 Attribute bonuses (partly read)

- **Strength setter core** `0x6F20A160` (`this` = hero, arg = delta): base `+0x94` += delta; the shared refresh
  `0x6F332810`; the attack ability refresh `0x6F2087B0` when unit `+0x1E8` is set; then a call with the global object
  (`0x6F2834E0()`, field `+0x29C`) and `0x6F209090`. The derived bonus formula (StrHitPointBonus etc.) is in the
  callees `0x6F209000` / `0x6F209090` and the agility path `0x6F20A1E0`, which were not read to the end.
- **Intelligence setter core** `0x6F20A260`: section 6 (counter, then the same refresh pattern).
- The Misc keys (`StrAttackBonus`, `StrHitPointBonus`, `StrRegenBonus`, `AgiAttackBonus`, `AgiDefenseBonus`,
  `AgiAttackSpeedBonus`, `IntManaBonus`) are read into the model now (section 9; `IntRegenBonus` is not read by the
  game at all). The unit fields they set are section 9.4; the rounding and the life rule are open.

## 8. Hero gate (CONFIRMED)

Every hero native first finds the unit, requires its type id to start with an uppercase letter, the unit not to be
removed, and the unit not to be an illusion (`+0x5C` bit 30). A failing call returns its default. The model applies
the same gate (`natives/hero.rs`).

## 9. Derived attribute bonuses

Labels as above. Addresses are 1.26a VAs. Harness: `tools/emu/hero_bonus_emu.py` (the real readers run in Unicorn;
the counter read `0x6F473170`, the Misc reader `0x6F009EE0` and the three attribute cores `0x6F0DB300` (strength),
`0x6F0DB350` (agility), `0x6F0DB3A0` (intelligence) are the real code or hooks as stated there).

### 9.1 The Misc keys that are read (CONFIRMED)

The derived bonuses read exactly eight keys, through the real reader `0x6F009EE0` (key on the stack, index 0, default
`0.0` when absent):

| Key | Reader | Value |
|---|---|---|
| `StrHitPointBonus` | `0x6F208060` | `I2R(str) * StrHitPointBonus * *factor` (factor is a real at the reader's second stack pointer) |
| `StrRegenBonus` | `0x6F2080F0` | `I2R(str) * StrRegenBonus` |
| `StrAttackBonus` | `0x6F207F40` | `I2R(primary) * StrAttackBonus`, primary = core chosen by `+0xCC` |
| `AgiDefenseBase` + `AgiDefenseBonus` | `0x6F207FB0` | `AgiDefenseBase + I2R(agi) * AgiDefenseBonus` |
| `AgiMoveBonus` | `0x6F208170` | `I2R(agi) * AgiMoveBonus` |
| `AgiAttackSpeedBonus` | `0x6F2081F0` | `I2R(agi) * AgiAttackSpeedBonus` |
| `IntManaBonus` | `0x6F208270` | `I2R(int) * IntManaBonus` |

The same keys are read a second time in `0x6F0E58A0` (strength attack, hit points, mana, agility defense/move/speed
reads) and `0x6F0E5F20` (agility defense, move, speed, mana) and `0x6F0D4CB0` (strength attack, hit points). Those
callers were not decoded further; they read the same keys with the same meaning (LIKELY).

**Keys that are NOT read**: `AgiAttackBonus`, `IntRegenBonus`, `IntAttackBonus`, `StrDefense*` do not occur in the
1.26a code (string search of the whole decompile). The primary attribute carries the attack bonus instead.

### 9.2 Attribute totals and the primary attribute (CONFIRMED)

- Strength total `str = base + trunc(gain * counter)` (section 5), agility the same (`+0xA8`, `+0xE4`), intelligence
  `base_counter + trunc(gain * counter)` (section 6), where `counter = level - 1`.
- The primary attribute is `+0xCC`: `1` selects the strength core `0x6F0DB300`, `2` the intelligence core `0x6F0DB3A0`,
  any other value the agility core `0x6F0DB350`. Checked in Unicorn: the three cores return 451, 200 and 101 for the
  same hero (STR 339 + 112, AGI 143 + 57, INT counter 1 + 100). **The source of `+0xCC` in the unit record is not found
  (UNSURE).** The Rust default is agility (0).
- Hence the attack bonus of a strength hero uses strength, an intelligence hero uses intelligence, and an agility hero
  (and any other value) agility. There is no per-attribute attack key.

### 9.3 Arithmetic (CONFIRMED, 14,000 Unicorn cases, 0 mismatches)

All readers are softfloat `mul` / `add` of `I2R` results (`docs/spec/softfloat.md`). The harness draws random heroes
(base and gain of each attribute, counter 0..25, primary 0..3), random Misc reals, and random factors, and compares
`sim::heroes::derived_bonuses` with the original result for every reader: `HP`, `REGEN`, `STRATK`, `ARMOR`, `MOVE`,
`ASPD`, `MANA`, 2,000 cases each. Test: `crates/sim/tests/hero_bonus_vectors.rs` (`HERO_BONUS_VECTORS`).

Operand detail confirmed by the probes: the hit-point reader writes its result to its **first** pointer and multiplies
by the real at its **second** pointer (a float the caller passes; the setter `0x6F20A160` passes a pointer built from
`*(global + 0x29C)`, see 9.5, value UNSURE, the Rust side uses `1.0`, LIKELY).

### 9.4 Where the values go (setter side)

- **Strength setter** `0x6F20A160`: base `+0x94` += delta; the shared refresh `0x6F332810`; the attack ability refresh
  `0x6F2087B0` when unit `+0x1E8` is set (it runs `0x6F207F40` for the attack bonus, then the damage sink
  `0x6F0C6BD0` and the object at unit `+0x98`); then the hit-point path `0x6F209000` and the regeneration path
  `0x6F209090` (which calls `0x6F2080F0` for the regeneration value and a sink at `this+0x13C`, LIKELY).
- **Hit-point path** `0x6F209000` (CONFIRMED structure): new bonus = `0x6F208060(...)`; old = the real at hero `+0xA4`;
  delta = new - old (`0x6F6EEEF0`, sub); then the unit life sink `0x6F2AC450(unit, delta, mode)` with
  `mode = (show != 0) + 1`, where `show` is the setter's boolean (`permanent` in JASS); then the stored value object at
  hero `+0xA0` receives the add (`vt[0](value, 1)`). Whether `+0xA4` is ever written was not found (UNSURE).
- **Life sink** `0x6F2AC450` (CONFIRMED that it dispatches on mode 1 / 2, LIKELY the meaning): mode 1 path reads the
  maximum (CUnit vt `+0x130`), sets it (vt `+0x144`), rescales the current life with the ratio, and clamps the result
  to `[min, ...]` where the minimum is 1.0 for a living unit (vt `+0x13C` tests alive) and 0.0 otherwise; mode 2 with a
  positive delta adds the delta to the maximum (vt `+0x128` / `+0x130`) and to the life; mode 2 with a non-positive
  delta clamps the life to the new maximum. **The exact meaning of each mode is LIKELY from the decompile, not run.**
  `CUnit` slot names used: vt `+0x124` = `CUnit::vf73` (set life, `0x6F28B1F0`: clamps negatives to 0, then sets, then
  refreshes `0x6F2AB460`), vt `+0x128` = `vf74` (add: `get + value`, set, refresh, returns the difference), vt `+0x130`
  = `vf76` (get, `0x6F28B080`), vt `+0x13C` = `vf79`, vt `+0x144` = `vf81`.
- **Regeneration** `0x6F209190`: delta from the stored real at hero `+0xB0`, then the sink object at `+0xAC`.
  **Mana** `0x6F209210`: delta from the stored real at hero `+0xC8`, sink `+0xC4`, with the same mode flag. **Armor**
  path `0x6F202F50` (object `+0x6C`, then `0x6F201D50`). **Move** `0x6F208820` (`0x6F208170`, object `+0xB4`, then
  `0x6F202F50`). **Attack speed** `0x6F208880` (`0x6F2081F0`, object `+0xBC`, then `0x6F0C9090`). **Intelligence
  setter** `0x6F20A260`: the counter is written (`0x6F473140`), then the attack ability refresh, `0x6F209210` (mana) and
  `0x6F2092A0` (a refresh with the unit's `+0x140` state and the `+0x5C` bit 2 event `vt+0xEC`).
- Global object `0x6F2834E0()` (returns 0 when `DAT_6FAB65F4` is 0, else a runtime object); its field `+0x29C` is the
  factor passed to the hit-point path. Value UNSURE.

### 9.5 What the model applies (LIKELY, not run)

- The sim has life and mana maxima but no armor, regeneration, attack speed, move bonus or attack bonus fields on the
  unit. Those five derived values are computed (`derived_bonuses`) but **not stored or applied**; the unit model has no
  place for them yet (listed as open).
- **Maximum life and mana** change by the difference of the derived value before and after the setter
  (`max_life_delta`, `max_mana_delta`). The current life and mana move by the same amount and are clamped to
  `[0, max]` (`natives/hero.rs`, `with_attribute_change`). **UNSURE**: this is the placeholder for the life sink's
  mode rule (9.4). The original's exact rule (proportional or absolute, and which mode `permanent` selects) is open.
  Not verified.
- Hit-point factor `1.0` is used by the natives (LIKELY, see 9.3).
- Rates come from `[Misc]` through `gamedata::misc::AttrBonusMisc` (eight reals, default `0.0`), installed with
  `Game::set_hero_rates`. A zero rate changes nothing.

Verification of the applied path: `crates/sim/tests/hero_bonus_natives.rs` (JASS setter, hand-computed: StrHitPointBonus
0.5, strength 20 to 30 gives max life 600 to 605; without rates nothing changes). Unit tests of the arithmetic are the
vector test above.

### 9.6 Open items (derived bonuses)

1. The source of the primary attribute `+0xCC` in the unit record.
2. The life sink `0x6F2AC450`: run mode 1 and mode 2 in Unicorn with a unit object (the stat cell helpers `0x6F4773A0`,
   `0x6F477350`, `0x6F477440`, `0x6F4773E0` are the get and set of the stat block); decide the proportional or absolute
   rule and which mode `permanent` selects.
3. The stored old values at hero `+0xA4`, `+0xAC`/`+0xB0`, `+0xC4`/`+0xC8` (who writes them), and the global `+0x29C`.
4. Model fields for armor, regeneration, attack speed and move bonus, and the attack bonus in the damage roll
   (`combat.rs` read-only hook; not done in this pass).
5. The `0x6F0E58A0` / `0x6F0E5F20` / `0x6F0D4CB0` readers (same keys) were not checked against the vectors.

### 9.7 Model changes (earlier pass)

- `sim::heroes`: the builder (`build`), the lookup (`XpTables::need_xp`), `add_xp`, `set_xp`, `set_level`,
  `strip_levels`, `modify_skill_points`, `attribute`, `intelligence`, `rebase`.
- `HeroObj` has `str_base`, `str_plus`, `agi_base`, `agi_plus`, `int_base`, `int_plus`. A new hero copies the attributes
  of its type (`Game::new_hero`). The engine fills them from the unit records (`STR`, `AGI`, `INT` and their gains)
  and the experience tables from the `[Misc]` layers when the reciprocal table is present (`engine::data::install`).
- `gamedata::misc::HeroMisc` (`MiscData::hero`): the hero keys, read as integers (decimal, malformed = 0) and reals
  (S2R, default 0.0). LIKELY for the integer reader.
- `GetHeroInt` and `SetHeroInt` are registered (`Support::Likely`).

## 10. Life sink, stored bonuses and their application (CONFIRMED where marked)

Harnesses: `tools/emu/life_sink_emu.py` (life sink, 800 cases, seed 5), vectors generated by `tools/emu/life_sink_rs.py`
into `crates/sim/tests/life_sink_vectors.rs`; `crates/sim/tests/hero_derived_apply.rs` (application, hand-computed).
Calling convention used by the helpers (CONFIRMED by probes): a softfloat helper is `(ECX = out, EDX = pointer to the
first operand, [stack] = pointer to the second operand)`, it returns `out`. So `0x6F6EF000` (add) and `0x6F6EEEF0` (sub)
compute `first op second` with the EDX operand first. Zero signs follow this order (`sub(+0,+0) = -0`,
`add(-0,+0) = -0`, `add(+0,-0) = +0`, probed in Unicorn).

### 10.1 The setter path (CONFIRMED structure, asm)

- Hit points `0x6F209000(arg1 = show, arg2 = factor pointer)`: sink object = hero `+0x30` (the unit; `0x6F472890` when
  null). New bonus = reader `0x6F208060` (result read through the returned pointer). Old = hero `+0xA4`.
  `delta = sub(EDX = new, stack = old)` (so `new - old`). Then `0x6F2AC450(unit, &delta, mode)` with
  **`mode = (arg1 != 0) + 1`** (CONFIRMED: `cmp [esp+0x18],0; setne; add 1`). Then the stored value object at
  hero `+0xA0` receives `vt0(add(EDX = hero+0xA4, stack = delta))`, i.e. the stored old value becomes old + delta.
- The strength setter `0x6F20A160` passes the factor as a pointer to `*(FUN_6F2834E0() + 0x29C)` (see 10.4).
- Mana `0x6F209210`: the same shape, but its sink is **`0x6F278F50`**, not `0x6F2AC450` (UNVERIFIED, not run).
- Regeneration `0x6F209190` (stored `+0xB0`, sink object `+0xAC`), armor `0x6F202F50` (object `+0x6C`), move
  `0x6F208820` (object `+0xB4`), attack speed `0x6F208880` (object `+0xBC`, then `0x6F0C9090`): same stored-delta shape
  (LIKELY from the asm; not run).

### 10.2 The unit life sink `0x6F2AC450` (CONFIRMED by Unicorn, 800 valid-state cases)

`__thiscall(ECX = unit, [esp+4] = pointer to the delta, [esp+8] = mode)`, `ret 8`. Run for real together with the CUnit
slots it calls. Cells of the unit object (through the stat block helpers, the handle lookup `0x6F03FA30` and the
pair setter `0x6F4A9390`):

| Cell | Meaning | Read / written by |
|---|---|---|
| `+0x78` | current life | set by `0x6F4A92B0`; read by `vt+0x12C` (`0x6F28B2B0`, `GetLife`) |
| `+0x80` | lower clamp of the pair (floor) | written only by the pair setter (kept by SetMax) |
| `+0x84` | maximum life | read by `vt+0x130` (`0x6F28B080`, `GetMax`), written by `vt+0x144` (`0x6F28B0D0`, SetMax) |
| `+0x5C` bit 8 | the minimum flag | `vt+0x13C` (`0x6F28B2F0`) returns it |

Helpers (CONFIRMED): `vt+0x124` `0x6F28B1F0` SetLife (a non-positive value becomes `+0.0`, then the value is set);
`vt+0x128` `0x6F28B230` AddLife; `vt+0x144` SetMax writes the pair `(+0x80 unchanged, new max)` and clamps the life.
The refresh helpers `0x6F2AB460` / `0x6F2AB5E0` raise the unit event `0xD01E6` through SEH (FS-based); in the harness
they are **stubbed** (no cell is written by them in the model; UNVERIFIED that they change nothing else).

Steps (the model is `crates/sim/src/heroes.rs`, `life_sink`):

1. `value == +-0.0` returns at once (`0x6F2AC450` compares with `0.0` first).
2. `d = floor(value + 0.5)`: the helper `0x6F6EF330` rounds the delta half up before anything else (CONFIRMED: 0.5 to 1,
   2.5 to 3, -2.5 to -2, -0.4 to 0).
3. `min = 0.0` when the flag bit 8 is set, else `1.0`.
4. **Mode 1 (permanent false): proportional.** `ratio = life / max` (`0` when `max` is zero, `0x6F020640`), SetMax
   to `max + d`, then `life = floor(newmax * ratio + 0.5)`, raised to `min`, then SetLife.
5. **Mode 2 (permanent true): absolute, and mode 3 with `d >= 0`.** `d >= 0` (zero included; the x87 test falls
   through on equal, NaN takes the falling branch): SetMax to `max + d`, then `AddLife(d)`. `d < 0`: SetLife to
   `max(life + d, min)` first (the life is read before the maximum moves), then SetMax to `max + d` (which clamps the
   life from above).
6. **Any other mode, and mode 3 with `d < 0`:** only the maximum moves (`max + d`), the life is clamped to it.

Observed examples (life, floor, max, delta, mode -> life, max): (10, 0, 100, +5, 2) -> (15, 105); (10, 0, 100, +5, 1)
-> (11, 105) [105 x 0.1 = 10.5 rounds to 11]; (50, 0, 80, +5, 1) -> (53, 85); (100, 0, 100, -30, 2) -> (70, 70);
(0, 0, 100, +5, 1) -> (1, 105) [flag clear: minimum 1]; (50, 1, 100, -2.5, 2) -> (48, 98).

**Clamps (CONFIRMED):** the maximum setter clamps the life to `[floor, max]` with the inverted-pair rule: if `max < floor`
the life becomes `max` (90-point grid, `0x6F477400` through `0x6F4A9390`). The life setter `0x6F4A92B0` clamps with the
lower bound tested first and no inverted-pair test (CONFIRMED by vector case 83: an inverted pair gives the floor).

Verification: 800 cases (valid states, floor <= life <= max), **799 bit-exact and 1 that differs only in the sign of a
zero** (case 496: mode 2, life 0, delta -3, flag set: the original gives `+0.0`, the model `-0.0`; the sign of a zero
through the min path is UNSURE). Numeric mismatches: 0. The test asserts at most 4 zero-sign differences.
Invalid states (life below the floor, which the original reaches only from invalid input) are not modelled.

### 10.3 Application to the unit (what the model does)

- **Hit points**: `delta = after.hit_points - before.hit_points` (the derived bonuses with factor `1.0`), through the
  sink with `mode = permanent + 1` (10.2). The "before" value is the derived bonus of the previous attributes, which
  assumes the stored value at `+0xA4` equals it (LIKELY, see 10.5).
- **Mana**: placeholder kept (current mana moves by the change, clamped to `[0, max]`). UNVERIFIED (sink `0x6F278F50`).
- **Armor**: the total derived armor is stored on the unit (`UnitObj::hero_armor`) and added to the type armor by
  `combat::unit_defense` (`armor = type armor + bonus`, only when the bonus is non-zero). LIKELY: the original's armor
  path adds the delta to the object at `+0x6C` and then `0x6F201D50`; the armor read by the damage path is `+0xE0`.
- **Move speed**: the delta of the move bonus is added to `UnitObj::move_speed`, which the mover reads (LIKELY; the
  original's move chain `0x6F202F50` / `0x6F201D50` is not fully decoded).
- **Regeneration** (`UnitObj::hero_regen`) and **attack speed** (`UnitObj::hero_attack_speed`): stored totals only.
  The simulation has no regeneration tick (LIKELY absent). The attack speed reader in the attack cycle is the setter
  chain `0x6F208880` -> `0x6F0C9090` (the attack ability setter), which is not decoded; the cooldown is not changed.
- **Attack damage** (`UnitObj::hero_attack`, total): `damage::SpecDamage::raw` adds `r2i(hero_attack)` of the attacker to
  the weapon's dice bonus before the dice roll. CONFIRMED that `GetDamageAmount` sums two bonus fields
  (`this+0xA0[w] + this+0xAC[w]`, passed as one int, attack-damage.md section 3.1). That the hero bonus is one of
  those fields, and the truncation of the stored real, are UNSURE.

### 10.4 Global `+0x29C` (the hit-point factor): not found

- The strength setter reads `FUN_6F2834E0()` and passes `*(object + 0x29C)` as the factor. `0x6F2834E0` returns 0 when
  the global `DAT_6FAB65F4` is null (the game object created by `FUN_6F0074F0` at start-up); otherwise it calls the
  unit's vtable slot `+0xEC` and looks the result up in the global (`0x6F3A1650`). So the factor belongs to an object
  found at run time, not to a fixed global.
- No writer of that field was found in the decompile (the offset `0x29C` is used for many other fields).
- The model uses `1.0` (LIKELY neutral). Needs a live process to read the value.

### 10.5 Open items (this section)

1. The stored old values (`+0xA4`, `+0xB0`, `+0xC8`, `+0xAC`, `+0xBC`) at creation: the model assumes they equal the
   derived bonus of the initial attributes (no creation writer was found). A hero created with a strength bonus and
   changed later moves by the change only; if creation applies the bonus, this matches.
2. Mana sink `0x6F278F50`: run it like 10.2 (mana cells, mana vtable slots).
3. The factor `+0x29C` (10.4).
4. The attack-speed combination with the cooldown (`0x6F0C9090`).
5. The refresh helpers `0x6F2AB460` / `0x6F2AB5E0` (event `0xD01E6` through SEH): run them with an FS segment.

## 11. Open items (priority)

1. Kill experience split: the value `F` and the candidate loop (section 7.1). Needed for any XP gained in play.
2. The per-recipient level walk in `0x6F20B080` (section 7.1), Unicorn-checked with stubs.
3. Hero ability slots and the ability-level rules of the skill points (sections 4 and 7).
4. Derived bonuses (section 7.2): the Misc values and the unit fields they set.
5. Run the level loop and the strip rule in Unicorn with a stubbed hero object (the level counter is the only
   external read on those paths).
6. Side effects of the attribute setters (refresh calls `0x6F332810`, `0x6F209210`, `0x6F2092A0`).
