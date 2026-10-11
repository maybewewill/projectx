# Heroes: experience, levels, skill points, attributes (Game.dll 1.26a)

Scope: the hero experience tables, experience gain and level changes, skill points, and the strength and agility
getters and setters. The intelligence getter and setter, the kill-experience distribution, and the derived attribute
bonuses are **not** decoded (sections 6 and 7).

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

## 6. Intelligence: not decoded

- **GetHeroInt** → `0x6F3C68C0` → `0x6F277850` → `0x6F0DB3A0` (CONFIRMED path). The function takes the unit (not the
  hero sub-object), reads a gain at unit `+0xDC` (real), and returns `c1 + trunc(gain * c2)` where `c1` and `c2` are
  two level counters read through two different embedded references of the unit (`+0x7C` and `+0x6C`) (CONFIRMED by
  disassembly).
- What is open: if both references are the same counter, the base is the level counter itself and an intelligence of
  about 20 at level 1 cannot come from it. So `c1` or the base lives elsewhere (UNSURE). The native and the setter
  (`0x6F277980` → `0x6F20A260`, which adds to a counter, not to a base) are therefore not implemented.

## 7. Not decoded (kill experience, derived bonuses, abilities)

- **Kill experience** (`0x6F20B080` with the distribution branch): when the sixth argument is non-zero, the gain is
  computed from a per-level table of the victim's `GrantHeroXP` / `GrantNormalXP` (lookups `0x6F32E020/0x6F32E040`
  and a 9-entry string table at `0x6FA6F044`), with a range check (the seventh argument) and a handicap multiplier
  read from the owner player (`+0x2A4`). The unit-kill caller that sets these arguments, the "HERO" object list
  (`0x6F40B340`), who receives the experience (range, allies, split), and the rounding of the split are not read.
  `HeroFactorXP`, `HeroExpRange` and `SummonedKillFactor` are read by the builder or by other functions that are not
  decoded here.
- **Derived attribute bonuses** (`StrAttackBonus`, `StrHitPointBonus`, `StrRegenBonus`, `AgiAttackBonus`,
  `AgiDefenseBonus`, `AgiAttackSpeedBonus`, `IntManaBonus`, `IntRegenBonus`): read from `[Misc]` by functions in
  `chunk_004`, `chunk_007`, `chunk_010`, `chunk_015` (the functions `0x6F20A160`, `0x6F20A1E0`, `0x6F20A260` apply
  them through the hero's attack and movement abilities, `+0x1E8` and `+0x1EC`). The values and the formulas are not
  read; they are not modelled. The `[Misc]` reader keeps them out of the hero model for now.
- **Hero ability slots** (five codes at `+0xF4`): their levels bound the skill-point cap and the strip rule. The
  model has no hero ability slots, so the caps are off (section 4).
- **Revive, skill selection, proper names**: unchanged from the earlier pass (see `natives/hero.rs`).

## 8. Hero gate (CONFIRMED)

Every hero native first finds the unit, requires its type id to start with an uppercase letter, the unit not to be
removed, and the unit not to be an illusion (`+0x5C` bit 30). A failing call returns its default. The model applies
the same gate (`natives/hero.rs`).

## 9. Model changes (this pass)

- `sim::heroes`: the builder (`build`), the lookup (`XpTables::need_xp`), `add_xp`, `set_xp`, `set_level`,
  `strip_levels`, `modify_skill_points`, `attribute`, `rebase`.
- `HeroObj` gained `str_base`, `str_plus`, `agi_base`, `agi_plus`. A new hero copies the attributes of its type
  (`Game::set_hero_attributes`, `Game::new_hero`). The engine installs the attributes from the unit records and the
  experience tables from the `[Misc]` layers when the reciprocal table is present (`engine::data::install`).
- `gamedata::misc::HeroMisc` (`MiscData::hero`): the hero keys, read as integers (decimal, malformed = 0) and reals
  (S2R, default 0.0). LIKELY for the integer reader.

## 10. Open items (priority)

1. Kill experience distribution (section 7): needed for any XP gained in play.
2. Intelligence getter and setter (section 6).
3. Hero ability slots and the ability-level rules of the skill points (sections 4 and 7).
4. Derived bonuses and their application to HP, mana, attack and regeneration (section 7).
5. Run the level loop and the strip rule in Unicorn with a stubbed hero object (the level counter is the only
   external read on those paths).
