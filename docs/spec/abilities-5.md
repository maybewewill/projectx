# Spell casting core in Warcraft III 1.26a (Game.dll): readiness, event sequence, mana, cooldown, Dur versus HeroDur (pass 6)

Status: the open items of abilities-4 are **resolved structurally** (decompile of the shared cast body and of the
three concrete classes); the arithmetic pieces are Unicorn-checked (6,000 cases, 0 mismatches). Implementation:
`crates/sim/src/cast.rs`, tests `crates/sim/tests/spell_cast.rs` and `spell_vectors.rs`, harness
`tools/emu/spells_emu.py`.

Labels: **CONFIRMED** = read in the decompile (and Unicorn where a count is given); **LIKELY** = strong inference;
**UNSURE** = a stand-in in the model; **NOT FOUND** = searched.

## 0. Corrections to earlier passes

| Earlier statement | Now |
|---|---|
| abilities-4 section 0/4: spells do not read `Cool` through the getter | **Corrected.** `Cool` is read through the ability's virtual slot `0x2EC` (= `0x6F02E890`) inside the effect body `0x6F05C840`; the direct-call scan could not see it. |
| abilities-2 section 1.2: `UnitResetCooldown` changes no ability field | **Corrected.** The walk calls `0x6F037E10` on every ability that passes the `'AAsp'` class test: when the cooldown flag (`+0x20 & 0x200`) is set and more than **0.01 s** (constant `0x6FAAE480`, runtime value `0x3C23D70A`) is left, the cooldown timer is cancelled and re-armed with 0.01 s (message `0xD01BE`). Net effect: the ability is ready 0.01 s after the call. |
| abilities-4 section 11: mana comparison "some stored float equals the value" | **Corrected.** `0x6F283C70` is `mana >= cost` (section 3). |

## 1. The cast sequence (CONFIRMED structure)

Entry: order message -> `0x6F074C60` (message switch) -> `0x330` forwarder -> **`0x6F054880`** (slot `0x338`, shared
by all spells) -> slot `0x340` (`0x6F0290E0`) -> slot `0x378` (`0x6F074080`, the cast start).

* `0x6F054880` refuses when the caster's flag word `+0x5C` has `0x100000` or `0x200000`, or its counter `+0x54` is
  above zero (LIKELY: the "cannot act" states, stun among them). The model refuses a stunned caster.
* The readiness check is **`0x6F050880`**, which returns an error code: `221` ability disabled (`+0x3C > 0` or
  the owner flag `0x40000000`), **`209` not enough mana** (slot `0x370` = `0x6F029080` false), **`210` cooling down**
  (ability flag `0x200` set and the sign bit clear), `185` (`+0x44 > 0`, UNSURE meaning), `184` (slot `0x268`
  false), `84`. Order: 221, 209, 210, 185, 184, 84. `0x6F04FED0(stage)` is the recheck used at every stage: it
  calls `0x6F037DE0` first (= mana ok and flag `0x200` clear), then the range and target tests.

Stage functions (each posts its JASS event through a unit method, and arms the next stage with a CAgentTimer):

| Stage | Function | Event (key, player-unit / unit table) | Next |
|---|---|---|---|
| 1 | `0x6F071570` | CHANNEL `0x6F29A0F0` (`0x80310` / `0x80321`) | timer `0xD01BB` after the delay `+0x84`, else stage 2 at once |
| 2 | `0x6F06C180` | CAST `0x6F29A280` (`0x80311` / `0x80322`) | timer `0xD01BC` after the delay `+0x8C`, else stage 3 at once |
| 3 | `0x6F063760` | EFFECT `0x6F29A410` (`0x80312` / `0x80323`), then **`0x6F05C840`** (pay, effect, cooldown) | a channel time `+0x94` would arm `0xD01BD`; else finish at once |
| finish | `0x6F05BCD0` -> `0x6F050AF0` | FINISH `0x6F29A5A0` (`0x80313` / `0x80324`) | |
| end | `0x6F050950` | ENDCAST `0x6F29A730` (`0x80314` / `0x80325`) | |

The keys are the JASS event ids `272..276` (player-unit, `0x110..0x114`) and `293..297` (unit, `0x121..0x125`) plus
`0x80200` (CONFIRMED by the immediates inside the posters). Stages 2 and 3 run the readiness check again with their
stage number (`0x6F04FED0(2)`, `(3)`); a failure ends the cast through `vf924(0)`.

**Not decoded: the sources of the delays.** `+0x84` and `+0x8C` are floats of the ability object whose writers I did
not find. The model reads `+0x84` as the level's `Cast` field and `+0x8C` as the unit type's `castpt` (UNSURE; this
matches the observed game behaviour: EFFECT, payment and cooldown happen after the cast point). `+0x90/+0x94` is a
float pair (channel time); `0x6F022270` tests `0 < +0x94` and `|0 - +0x94| >= 0.001` (`0x6FAAE47C`). The model has
no channelled spell.

## 2. The effect body `0x6F05C840` (CONFIRMED order)

1. Cost: slot `0x3D4` (`0x6F069170`, the level's integer `Cost`) -> `I2R`; **if non-zero, the mana payment
   `0x6F283DB0`** (section 4).
2. Eye candy and notifications (`0x6F052FB0`, `0x6F040710`): presentation only.
3. The class effect (slot `0x3C0` = `0x6F05C3A0` dispatches by the target kind; for a unit target it calls
   **slot `0x424`**, section 6).
4. **Cooldown**: `Cool` is read through slot `0x2EC` (`0x6F02E890`) with the current level; when the ability is not
   removed (`+0x20 & 8` clear), the sign bit is clear, slot `0x314` is zero and `0 < Cool`, slot `0x3A4`
   (`0x6F050B70`) runs: sets flag `0x200`, calls `0x6F021270` (UI refresh) and arms a CAgentTimer with message
   **`0xD01BE`** and timeout `Cool` (timer at ability `+0xD0`). The handler for `0xD01BE` is slot `0x3A8`
   (`0x6F050BC0`): cancels that timer, clears flag `0x200`.

So the cooldown is **per ability object**, runs on the agile clock, and is a flag plus a one-shot timer. The level
builder already clamps a negative or NaN `Cool` to 0.0 (abilities-3). The ready check is the flag.

## 3. The unit-side mana check `0x6F283C70` (Unicorn 2,000 cases, 0 mismatches)

`BOOL __thiscall(unit, float *cost, int flag)`. Reads the thread-context cheat flag (`0x6F4C34D0(13)` -> bit `0x20`
of a byte: a "no cost" cheat returns true), the agile mana property at `unit+0xB8` (`0x6F4773A0`), optionally adds
the contributions of the unit's ability list (`+0x1DC/+0x1E0`, slot `0x120`; none for the three spells), and returns
`mana >= cost` as an x87 compare in double precision. **NaN on either side gives false.** If `flag & (unit+0x5C >> 2)
& 1` the other value `unit+0xCC` is used instead of the mana (a different resource; not modelled, never set for these
spells). The cast passes the cost value `add(I2R(Cost), 0.0)` (abilities-4 section 10) and `flag = 1`.

`cast::mana_check` = this rule. Vectors: `tests/data/spells/unit_check.txt` (mana, cost, flag, result).

## 4. The mana payment `0x6F283DB0` (Unicorn 2,000 cases, 0 mismatches)

`(unit, out, &cost, flag)`: with the cheat flag the payable amount is 0.0. Otherwise it starts as the cost, walks
the unit's abilities of kind 52..66 (cost modifiers, `slot 0x104` kind, `slot 0x108` apply; none for these spells;
when one claims the cost the amount becomes 0.0), then:

* `out = (cost - payable) + min'(payable, mana)`, the soft sub/add, where **`min'` replaces the payable only when
  `mana < payable` is an ordered compare** (a NaN mana keeps the cost);
* the mana property is written with `mana + (-min')` (soft add) through the agile setter `0x6F477350`.

So a unit with less mana than the cost pays all it has; the pre-check normally prevents that. `cast::pay_mana`
returns `(out, new mana)`. Vectors: `payment.txt` (mana, cost, out, new mana). The payment happens **in the effect
body, after the EFFECT event and before the class effect**, not at the order.

## 5. Dur versus HeroDur (CONFIRMED in two effect slots)

`0x6F1FC2A0` (Slow) and `0x6F181C70` (the Storm Bolt launcher) both do: if `0x6F1D2660(target)` is true, the
duration is `HeroDur` (`0x6F02E7D0`); otherwise `Dur` (`0x6F02E760`) **added by the soft add to the float at ability
`+0x9C`** (a duration bonus whose writer I did not find; 0.0 in the model, UNSURE).

`0x6F1D2660(unit)` (Unicorn 2,000 cases, 0 mismatches): false for a null unit; true when the **first character of
the unit type id (byte `+0x33`) is `A..Z`** (hero ids are capitalised), else true when either of two helper tests is
true (`0x6F1D2630()`, `0x6F0766A0(unit)`; meaning not decoded, LIKELY "treated as a hero" cases). `cast::is_hero_target`.

## 6. The class effects (slot `0x424`)

| Class | Slot `0x424` | Effect |
|---|---|---|
| `CAbilityThunderBolt` (`AHtb`) | `0x6F18F680` -> `0x6F181C70(ability, isHero)` | builds an effect record (damage = `DataA` at `+0x74`, damage class word from slot `0x42C` = **`0x8000`**, the duration of section 5 as the buff time) and launches a missile (`0x6F176C80` -> `0x6F2C58E0`). Damage and the stun buff come on arrival. |
| `CAbilityHolyBolt` (`AHhb`) | `0x6F08EB20(target)` | `v = DataA`. The relation table `0x6F88E1D4` (read with the target's undead state: race Undead or class bit 2, and two relation tests of the owners) gives +1 heal, 0 nothing, -1 damage. Heal: `v` through the heal modifiers (abilities of kind 68..82, none here) and the life adder; an ethereal target (`+0x158 > 0`) multiplies by Misc `EtherealHealBonus`. Damage: `v / 2.0` (constant `0x6FAAE504` = 2.0, runtime value) with damage class **`0x2000`** (divine) from the caster through the target's damage slot `0x120`. |
| `CAbilitySlow` (`Aslo`) | `0x6F1FC2A0` | duration of section 5; the buff gets **-DataA** (movement) and **-DataB** (attack speed) as deltas (`0x6F1FC190`); the buff id is the level's BuffID (`Bslo`). |

The table for Holy Light, as read (rows are indexed by `v3 | 2*v2`; non-undead column then undead column): for a
non-undead target `v2 == 0` (not an enemy) heals and `v2 == 1` does nothing; for an undead target `v2 == 1` damages,
and `v2 == 0` damages unless `v3 == 1`. `v2 = (0x6F3A3430(owner, player) == 0)`, `v3 = 0x6F3A3700(...)`. The model
uses `v2 = enemy` and `v3 = "same owner"` (UNSURE for the allied undead).

The Slow and Storm Bolt effects reach the buff creator through `0x6F042B20` (level record setup) and
`0x6F1FC190` / `0x6F176C80`; the buff stacking rule on a second cast is NOT FOUND (the model replaces a buff of the
same code, UNSURE).

## 7. What the Rust model does (`crates/sim/src/cast.rs`)

* `begin_cast(game, rt, caster, base, target)`: the order entry (alive, has the ability, not stunned, readiness 209
  then 210, live target, range) and stage 1; the delays are timers (`Payload::Spell`). `natives/order.rs` routes a
  target order whose id maps to a spell ability here; the native returns whether the cast started. Order ids of
  the three spells (`852095` thunderbolt, `852089` holybolt, `852075` slow) are from public order lists: UNSURE.
* Events: CHANNEL, CAST, EFFECT, FINISH, ENDCAST on the owner's player-unit table and the caster's unit table, with
  caster, ability code, target unit and target x/y in the firing, so `GetSpellAbilityUnit`, `GetSpellAbilityId`,
  `GetSpellTargetUnit/X/Y/Loc`, `GetTriggerUnit` answer (they were already bound). `GetSpellAbility` has no object.
* Payment, cooldown (`Game::is_cooling`, `cooldown_remaining`, `reset_cooldowns` = `UnitResetCooldown`), buffs
  (`BSTN` stun, `Bslo` slow) with expiry timers; a stunned unit does not walk or fight (`combat_tick`, `step_unit`);
  a slowed unit's `move_speed` is the base times `1 + delta`, restored when the last slow ends.
* Storm Bolt's missile is a plain distance over speed (1000/s, UNSURE; the flight arithmetic is open in missiles.md).
* Not modelled: walking into cast range (the cast is refused out of range), channelled spells, the free-cast cheat,
  cost modifiers, interruption by a new order other than another spell, the attack speed part of Slow (stored in
  the buff, not read by the attack cycle), buff stacking, mana-property events.
* Data: `SpellLevel` (cast, dur, hero_dur, cost, cool, area, rng, DataA..DataI) from the ability table
  (`engine::objdata` fills it from `AbilityData.slk` through `gamedata::abilitytype`).

## 8. Unicorn (tools/emu/spells_emu.py)

| Check | Original function | Cases | Mismatches |
|---|---|---|---|
| unit mana check (random mana/cost bit patterns incl. NaN, inf, denormals; both flag values) | `0x6F283C70` | 2,000 | 0 |
| mana payment (reported amount and new mana) | `0x6F283DB0` | 2,000 | 0 |
| hero test (first byte, two helper results) | `0x6F1D2660` | 2,000 | 0 |

Stubs: the agile mana getter `0x6F4773A0` and setter `0x6F477350` (the agile object needs the world clock), the two
helpers of the hero test. Everything else, including the soft add/sub, runs from the DLL. Tests pin the results in
`crates/sim/tests/spell_vectors.rs` (derived values only, no game bytes).

## 9. Open

* The sources and writers of `+0x84`, `+0x8C`, `+0x94`, `+0x9C`, `+0x44`.
* The cast/channel/effect delays against real castpt/Cast data (no real-map run was made).
* The order id table (`+0xAC`) of the real ability rows, the buff creator and stacking, the missile flight.
* The dispatcher of `0xD0148..0xD014E` / `0xD0160` notifications posted by `0x6F074080` (presentation: animation start).
