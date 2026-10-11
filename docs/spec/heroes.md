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

The numeric rules of the kill split are now resolved and checked in Unicorn: see section 8.2 (amount, split, engine)
and 8.3 (candidate selection, structure only).

### 7.2 Attribute bonuses (partly read)

- **Strength setter core** `0x6F20A160` (`this` = hero, arg = delta): base `+0x94` += delta; the shared refresh
  `0x6F332810`; the attack ability refresh `0x6F2087B0` when unit `+0x1E8` is set; then a call with the global object
  (`0x6F2834E0()`, field `+0x29C`) and `0x6F209090`. The derived bonus formula (StrHitPointBonus etc.) is in the
  callees `0x6F209000` / `0x6F209090` and the agility path `0x6F20A1E0`, which were not read to the end.
- **Intelligence setter core** `0x6F20A260`: section 6 (counter, then the same refresh pattern).
- The Misc keys (`StrAttackBonus`, `StrHitPointBonus`, `StrRegenBonus`, `AgiAttackBonus`, `AgiDefenseBonus`,
  `AgiAttackSpeedBonus`, `IntManaBonus`, `IntRegenBonus`) are still not read into the model. Which unit fields they
  set, and the rounding, are open. Not modelled.

## 8. Kill experience split and hero gate

Labels: CONFIRMED = read in the disassembly and reproduced in Unicorn with 0 mismatches; LIKELY = read, not run;
UNSURE = not settled. Harness: `tools/emu/hero_kill_xp_emu.py` (modes A and B), check: `crates/sim/tests/kill_xp_vectors.rs`
(`GAME_DLL` and `KILL_XP_VECTORS` set). Model: `sim::heroes` (`Gain`, `experience_engine`, `KillVictim`, `KillEnv`,
`kill_amount`, `distribute_kill`).

Runs: mode A (the engine `0x6F20B080`) 300 + 3000 cases, mode B (amount, split and the engine calls for every
candidate) 300 + 3000 cases; **0 mismatches**. The synthetic inputs cover handicap, walk, range, bonus slots, summoned
victims, illusions, suspended heroes, level-ups and the skill-point cap. Every case runs the real code below the
entries; only the callees listed in the harness answer from synthetic data (Misc readers, the level and factor tables,
player accessors, the upgrade-based bonus lookups, the victim's ability, and side effects that carry no numbers).

### 8.1 Hero gate (CONFIRMED)

Every hero native first finds the unit, requires its type id to start with an uppercase letter, the unit not to be
removed, and the unit not to be an illusion (`+0x5C` bit 30). A failing call returns its default. The model applies
the same gate (`natives/hero.rs`).

### 8.2 The numbers (CONFIRMED by the Unicorn runs)

**Amount `K` (`0x6F2A0230`, victim = dying unit, an int).**
1. Level `L`: `0x6F2A0210` gives the hero counter + 1 when the victim has a hero object (unit `+0x1F0`), else the unit
   level (`0x6F29DCA0`). An ability found by `0x6F0787D0` (rawcode from `0x6F113CC0`) overrides it when its virtual
   flag at slot `0x36C` answers non-zero: `L := ability +0xE4`.
2. `L < 1` gives `K = 0`. Otherwise the table is chosen by the type: the type id starts with an uppercase letter and
   the illusion bit is clear gives **hero** tables (`MaxHeroLevel`, `GrantHeroXP`, Misc key read at `0x6F876208`);
   anything else gives **unit** tables (`MaxUnitLevel`, `GrantNormalXP`, key `0x6F876218`). `L := min(L, max)`.
3. `G = grant[L - 1]`. If the victim's `+0x248` bit 2 is set (summoned), `K = f2i(mul(i2r(G), SummonedKillFactor))`
   (real Misc key `0x6F92F288`), otherwise `K = f2i(i2r(G)) = G`.

**Split (`0x6F2A3190`, record built by `0x6F2A46F0`).** The chosen list is the in-range list (record `+0x14`) when it
is not empty, otherwise the global list (record `+0xE0`). `N` is its count. `F = div(i2r(K), i2r(N))` is computed once
(softfloat), and when `0 < F` every entry of the list gets the engine call with value `F`, the handicap flag set, the
range flag set, and the walk flag set when the victim's owner index is 12 or above (`vf59` = owner, `cmp 12; setge`).
The order of the list does not change any number (each recipient depends only on `F`, `N` and its own state).

**Engine (`0x6F20B080`, kill flags).** Per recipient:
- a. Nothing when `F == 0.0` or the hero is suspended (`+0xEC`).
- b. Handicap: `v = mul(F, H)` with `H` the owner's handicap, player `+0x2A4` (the player is `0x6F2834E0(unit)`).
- c. Upgrade bonus: `B1 = 0x6F40B340(player, 'HERO', 11)`. Walk the nine rawcodes at `0x6FA6F044` (`1NWT`..`9NWT`)
   while `B1 != 0`, `i < 9`, and slot `i` exists (`0x6F40B380` of the unit's list). A non-zero code with `k = levels(code)
   != 0` sets `bonus = value(code, min(k, B1) - 1)`; the last one found wins. If `bonus != 0`: `v = mul(v, add(1.0, bonus))`.
- d. Plain gain (victim owner below 12): `XP += f2i(round(v))`, `round(x) = floor(x + 0.5)` (`0x6F6EF330`).
   Walk gain (victim owner 12 or above): `X = i2r(XP)`, `c` = counter. If `c < MaxHeroLevel - 1`, loop:
   `T = factor[c]` (HeroFactorXP / 100, table field `+8`); stop if `T == 0`; `X += mul(v, T)`; `c += 1`;
   `Nf = i2r(need(c))`; stop unless `Nf < X`; `v = div(X - Nf, T)`; `X = Nf`; stop when `c >= MaxHeroLevel - 1`.
   Then `XP := f2i(round(X))`: the walk **sets** the experience, it does not add to it.
- e. Tail (shared with the plain `AddHeroXP`): while `need(c + 1) <= XP` and not at maximum: level up, skill point
   `+1` when the new counter is below `MaxHeroLevel`. At maximum `XP := need(c + 1)`. When the hero leveled, skill points
   are capped at `sum over the 5 hero ability slots of max(0, max - min)` (`0x6F207C80`, `0x6F207CC0`) when that is lower.

Open for the amount/split: none found in the disassembly.

### 8.3 Candidate selection (structure read, the numbers are NOT run)

- Killer and dying: `0x6F2A4880` calls the builder `0x6F2A46F0` (dying, killer). No record when the killer is absent,
  when `0x6F285AC0(killer, dying)` is non-zero (the dying owner's bit in the killer owner's `+0x38` mask, CONFIRMED
  structure; LIKELY an ally test), or when the killer has flag `0x10000` (building, LIKELY) and the Misc key
  `BuildingKillsGiveExp` is off (`0x6FAB4268`, read by `0x6F26E1xx`). Killer owner = `vf59` of the killer.
- Range: `R = HeroExpRange` (Misc real, `0x6F009EE0`), `V = mul(R, R)` (record `+0x0C`, R is not divided by 32 here).
  A candidate is in range when `D <= V`, `D` = squared distance of the normalised positions of the dying unit and of the
  candidate (`0x6F28B990`, `dist2` `0x6F020000`). **The normaliser `0x6F4743A0 / 0x6F4742F0` is not understood** (it
  reads the map object), so the units of `D` against `R` are UNSURE.
- Candidates are all heroes of the global hero category list (`0x6F46D0B0` with category `0x41486572`, from
  `0x6F2075E0`), in list order. Each candidate is eligible when: the flags `+0x5C & 0x40000100 == 0`; the candidate's
  owner bit is set in the killer owner's `+0x68` mask (`0x6F3A34B0`, the shared-experience alliance, LIKELY); and when the
  Misc key `MaxLevelHeroesDrainExp` (`0x6FA70E98`) is off, the hero is not at maximum level (`0x6F207F10`).
- `GlobalExperience` (`0x6FA70E94`): when on, every eligible candidate is added to the global list (`+0xE0`); every
  eligible candidate in range is also added to the in-range list (`+0x14`).
- Misc values at runtime: the loader `0x6F26E1xx` reads `BuildingKillsGiveExp`, `GlobalExperience` and
  `MaxLevelHeroesDrainExp` into `0x6FAB4268`, `0x6FA70E94`, `0x6FA70E98`. The file's own values (1, 1) are overwritten
  at load; the Misc values are inputs (UNSURE for 1.26a defaults).
- Not checked: dead heroes (no life test seen in the filter; whether the global list keeps dead heroes is UNSURE),
  the normaliser, the `0x100` flag meaning, the alliance mask names (`+0x38`, `+0x68`).

### 8.4 Model status

- `sim::heroes::{experience_engine, distribute_kill, kill_amount}` implement 8.2 with `Gain` (handicap, walk, range).
  Not wired into the death path: the simulation does not track the killer (`GetKillingUnit` is null) and the candidate
  selection in 8.3 is not verified, so a hook would be guesswork. The death path and the combat code are untouched.

## 9. Model changes

- `sim::heroes`: the builder (`build`), the lookup (`XpTables::need_xp`), `add_xp`, `set_xp`, `set_level`,
  `strip_levels`, `modify_skill_points`, `attribute`, `intelligence`, `rebase`.
- `HeroObj` has `str_base`, `str_plus`, `agi_base`, `agi_plus`, `int_base`, `int_plus`. A new hero copies the attributes
  of its type (`Game::new_hero`). The engine fills them from the unit records (`STR`, `AGI`, `INT` and their gains)
  and the experience tables from the `[Misc]` layers when the reciprocal table is present (`engine::data::install`).
- `gamedata::misc::HeroMisc` (`MiscData::hero`): the hero keys, read as integers (decimal, malformed = 0) and reals
  (S2R, default 0.0). LIKELY for the integer reader.
- `GetHeroInt` and `SetHeroInt` are registered (`Support::Likely`).

## 10. Open items (priority)

1. Kill candidate selection (section 8.3): the position normaliser `0x6F4743A0` / `0x6F4742F0` and the units of the
   range test, dead heroes in the global hero list, the alliance mask names. The numeric split is done (section 8.2).
   Wiring it into the death path needs the killer in the simulation (`GetKillingUnit` is null today).
2. The per-recipient level walk in `0x6F20B080` (section 7.1), Unicorn-checked with stubs.
3. Hero ability slots and the ability-level rules of the skill points (sections 4 and 7).
4. Derived bonuses (section 7.2): the Misc values and the unit fields they set.
5. Run the level loop and the strip rule in Unicorn with a stubbed hero object (the level counter is the only
   external read on those paths).
6. Side effects of the attribute setters (refresh calls `0x6F332810`, `0x6F209210`, `0x6F2092A0`).
