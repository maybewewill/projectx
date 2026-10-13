# Human hero abilities in Warcraft III 1.26a (Game.dll): Thunder Clap, Bash, Avatar, Divine Shield, Resurrection, Water Elemental (S24)

Scope: the concrete classes of six Human hero abilities, the level-record fields they read, the buffs they apply, the
random streams, the target selection and the order of effects, wired into `crates/sim` on the cast core of
`abilities-5.md`. Model: `crates/sim/src/human_heroes.rs` (plus the hooks in `cast.rs`, `combat.rs`, `auras.rs`,
`natives/order.rs`). Tests: `crates/sim/tests/human_heroes.rs` (12 behaviour tests), `human_heroes_vectors.rs`
(3 vector checks, 400 cases each). Unicorn harness: `tools/emu/human_heroes_emu.py`. Addresses are 1.26a; no code
or tables are copied.

Labels: **CONFIRMED** = read in the machine code (and Unicorn-run where a count is given); **LIKELY** = strong
inference; **UNSURE** = a stand-in in the model; **NOT FOUND** = searched, not located; **NOT DONE** = not modelled.

## 0. Class identity (CONFIRMED, Unicorn: slot 7 of every vtable run, 11/11)

| Code | Class | Vtable | Cast shape |
|---|---|---|---|
| `AHtc` | `CAbilityThunderClap` | `0x6F8F67C4` | no target (immediate), area effect |
| `AHbh` | `CAbilityBash` | `0x6F90A414` | passive, attack hooks |
| `AHav` | `CAbilityAvatar` | `0x6F90998C` | no target, self buff |
| `AHds` | `CAbilityDivineShield` | `0x6F894AEC` | no target, self buff, cancellable |
| `AHre` | `CAbilityResurrection` | `0x6F91C1CC` | no target, area effect |
| `AHwe` | `CAbilityWaterElemental` | `0x6F89BF84` | no target, summon |
| `AHbz` | `CAbilityBlizzard` | `0x6F910CAC` | channel (NOT DONE) |
| `AHmt` | `CAbilityMassTeleport` | `0x6F88BEFC` | target + group (NOT DONE) |

Buff classes (slot 7): `BHtc` `CBuffThunderClap` `0x6F8E79B4`; `BPSE` `CBuffPause` `0x6F8B3BEC` (Bash's stun is a
**pause** buff, not `BSTN`); `BHav` `CBuffAvatar` `0x6F9039A4`; `BHds` `CBuffDivineShield` `0x6F894794`; `BHwe`
`CBuffWaterElemental` `0x6F8EBDBC` (the timed life of the water elemental, one of the seven special timed life ids);
`BHbz` `CBuffBlizzardAoe` `0x6F905484`.

All six cast classes share the cast body of `abilities-5.md` (slot `0x338` -> `0x6F054880`, stages, payment in the effect
body `0x6F05C840`, cooldown by flag `0x200` and message `0xD01BE`). The class effect hangs on slot `0x3C0` (immediate
form, here `0x6F05C3A0` for Thunder Clap and Resurrection and a class function for the others) which dispatches by the
target kind to slots `0x420` (point), `0x424` (unit), `0x428` (no target) and `0x41C`.

Level-record fields are read through the getters of `abilities-3.md`: DataA `0x6F02E9E0`, DataB `0x6F02EA50`, DataC
`0x6F02EAC0`, DataD `0x6F02EB30`, DataE `0x6F02EBA0`, Area `0x6F02E900`, Dur `0x6F02E760`, HeroDur `0x6F02E7D0`.
The extra duration term at ability `+0x9C` is read like in `abilities-5.md` section 5 (0.0 in the model, UNSURE).

## 1. Thunder Clap (`AHtc`)

* Entry: slot `0x428` (`0x6F1608B0`, CONFIRMED). It reads Area, takes the caster's position, spawns the shockwave art
  (presentation) and calls slot `0x434` (`0x6F176CD0`, shared with Storm Bolt) which builds a group around the
  position with the builder `0x6F2A3270` and runs the per-unit callback `0x6F1607B0` over it. Order = the range query's
  cell walk (`groups-events-2.md`; CONFIRMED structure).
* Per unit (`0x6F1607B0`, CONFIRMED order): a refusal test on the target (`vf 0x11C` and the ability's target check
  `0x6F28D750` against `vf 0x308`), then **damage = DataA** with the class word `0x8000` (slot `0x42C`, type 15 = sonic)
  from the caster through the target's damage slot `0x120`; then, if the target resolves and its flag word `vf 0x114`
  has bit 8 clear, the slow `0x6F18A930`.
* Slow (`0x6F18A930`, CONFIRMED): duration `HeroDur` for a hero target (hero test `0x6F1D2660`, Unicorn-checked in
  `abilities-5.md`), otherwise `Dur` plus the `+0x9C` term; **nothing happens when the duration is zero**. The buff
  `BHtc` is looked up on the target (`0x6F0787D0`); a found buff is refreshed through its slot `0x360`, otherwise a new one
  is created (`0x6F188B30`) and applied through slot `0x354` with **movement `-DataC`** and **attack speed `-DataD`**
  (the sign bit is flipped on each value) and the source.
* Model: `thunder_clap` in `human_heroes.rs`. Targets = units in `Area` around the caster, in the model's cell-walk
  order, alive, enemy of the caster, not a structure, not flying (UNSURE: the stock `targs` text is not read; this is
  the usual "ground, enemy"). Damage through `unit_damage_target` (type 15), a survivor gets `apply_slow_as(BHtc, ...)`
  of the aura system (strongest-wins is not used: the slow buff keeps one entry per code and replaces its factors, like
  `abilities-5.md` for Slow). Mana, cooldown and the five events come from the cast core.
* Not modelled: the per-owner array the callback fills (`array[vf 0xEC] = target`, used by the notification after the
  group; UNSURE meaning), the 0x40 flag of the damage record (UNSURE), the shockwave presentation.

## 2. Divine Shield (`AHds`)

* Cast (slot `0x3C0`, `0x6F0B64C0`, CONFIRMED structure): clears flag `0x8000`; duration = `HeroDur` if the **caster**
  is a hero (`0x6F1D2660(owner)`), else `Dur + (+0x9C)`; looks up the buff `BHds` on the caster: a found buff gets the
  set-duration rule (slot `0x350` = `0x6F0D4EF0`, `buffs.md` section 1: never shorten), otherwise a new buff is created
  and applied. The ability keeps a pointer to its buff (`+0xE4`). If the "can cancel" test `0x6F089A50` is false the
  ability sets flag `0x10000` and bumps its `+0x44` counter (UI).
* Buff removal effect (slot 207 = `0x6F07D1F0`, CONFIRMED): decrements the unit's invulnerability counter (`+0xE8`; when it
  reaches zero the invulnerable bit `0x8` of the flag word is cleared) and then the magic immunity counters
  (`0x6F077320` subtracts one from `+0x15C`, and from `+0x160` for the flag argument). The matching **apply** side was
  read for Avatar (section 4: `0x6F07A3A0` adds one to the same counters) and is taken as symmetric here
  (LIKELY; the apply function of the buff was not read).
* Cancel: `0x6F089A50` is true when `DataA` is not zero **and** a Misc test passes (`0x6F009F90` with the string at
  `0x6F88E388`, not decoded). The model uses `DataA != 0` only (UNSURE for the Misc part). The order `undivineshield`
  removes the buff; the id 852056 is from the public order lists (UNSURE).
* Model: `apply_divine_shield` adds the buff with `BuffEffect::DivineShield` (positive predicates), `invuln_count += 1`,
  `magic_immune += 1`; the undo is in `finish_buff`. `Game::unit_is_invulnerable` now also tests `invuln_count > 0`; the
  placeholder damage model honours the invulnerability gate of the target side (`attack-damage.md` section 11: the classes
  `0x10`, `0x20`, `0x4000000`, `0x400000` pass it).

## 3. Bash (`AHbh`)

Bash is a passive with four hooks on the ability (CONFIRMED by reading the vtable slots; the call sites that invoke the
slots were not located, the order below follows the data flow):

1. **Slot 73 (`0x6F1B2EC0`), the swing starts against a target.** Order, CONFIRMED:
   the armed flag (`0x200`) is cleared first; no target -> stop; the owner has the buff `BOwk` (Wind Walk; code from
   `0x6F15A110`) -> stop; the ability is disabled (`+0x3C > 0`) or not enabled (slot `0x268`) -> stop; the target
   check of the ability (`0x6F02E000`, the level's target flags) fails -> stop; a unit-flag test on the target
   (`0x6F26DE50`, UNSURE meaning, not modelled) -> stop; **the roll**; on success the flag `0x200` is set, a
   notification (`0x6F27B460(owner, 0xE)`) is sent and the counter `+0x84` is set to 1, on failure the counter is
   incremented.
2. **Roll = pseudo-random distribution** (CONFIRMED, **Unicorn: effective chance 400/400, compare 400/400, draw
   400/400, 0 mismatches**, `tools/emu/human_heroes_emu.py`, vectors in `crates/sim/tests/data/human_heroes/`):
   * `0x6F1A3470`: `effective = I2R(counter) * T(DataA)` with the soft multiply.
   * `T(p)` (`0x6F1D21F0`): index `i = f2i(p * 0.2 + 0.5)` (soft mul by `0x3E4CCCCD`, soft add of 0.5), clamped to
     `0..=20`; `T = DIV(I2R(n[i]), I2R(10000))` with n = `0, 38, 147, 320, 547, 819, 1126, 1461, 1815, 2184, 2562, 2953,
     3333, 3781, 4203, 4607, 5000, 5799, 6666, 7706, 10000` (the table is built by a static-local initialiser on the
     first call; one entry per 5 percent step). So the chance of the n-th swing since the last proc is `n * T`: the
     standard PRD.
   * The draw is **stream 3** (`0x6F2853D0` with EDX = 3): `R = add(0.0, mul(1.0, U))` where `U` is the stream's
     `random_fraction` (one generator step); proc iff `R <= effective` (`0x6F01FD60`: `!(effective < R)`, NaN never
     procs).
3. **Slot 77 (`0x6F1A3590`), the damage roll of the swing.** The stored bonus (`+0x80`) is cleared; if the ability is not
   armed, stop. `DataB` (the damage multiplier): if not zero, `stored = damage * DataB` and the record gains
   `stored - damage` (through the record's bonus transfer `0x6F255720`, class `0x20000`); `DataE` not zero sets the
   record flag `0x400` (UNSURE meaning); the ability's art override (`0x6F040760`) is stored at record `+8`
   (presentation, not modelled).
4. **Slot 85 (`0x6F1CC530`), the hit lands on `target`.** Requires the armed flag. Takes the stored bonus (clears it).
   **Pause:** duration `HeroDur` if the target is a hero else `Dur` (+`+0x9C`); if it is not zero and the target
   resolves, the buff `BPSE` (`0x6F0D2280`) is refreshed (slot `0x360`) or created and applied (slot `0x358`). The
   stored bonus, when not zero, is shown as a text tag (presentation). **Flat damage:** `DataC` not zero and the owner
   is not an illusion (`+0x5C & 0x40000000`) -> a damage of `DataC` from the owner to the target, class `0x20000`
   (type 17, `DAMAGE_TYPE_FORCE`; slot `0x348` returns the class), not an attack.
5. **Slot 74 (`0x6F1B3060`)** clears the armed flag (and the notification) when armed.

Model (`bash_on_attack_start`, `bash_modify_damage`, `bash_on_hit`, `bash_clear`, hooked in `combat.rs`): the start hook
runs at the execute point before the damage roll, the multiplier after it, the hit hook after the main damage when the
target survived (instant attacks and missile arrivals), and the flag is cleared after the hit, on an evaded or missed
attack and at the next swing's start. UNSURE: the call points of slots 73/74/77/85 in the attack cycle (the original may
roll at the swing start rather than at the execute point, and may call the hit hook before the main damage), the target
filter (the model uses "enemy"), the unit-flag test (not modelled), the illusion flag test uses `UnitObj::illusion`.
The pause buff is applied through `apply_stun_as(BPSE, ...)`, which counts a stun like `BSTN` (UNSURE: that the pause
class stops orders and attacks the way a stun does; its class was not read beyond the code).

## 4. Avatar (`AHav`)

* Cast (slot `0x3C0`, `0x6F1CA180`, CONFIRMED structure): a preliminary routine (`0x6F1B69E0`) sets flags `0x8000` and
  `0x20000` on the ability, starts the duration timer (message `0xD01C4` at `+0xF0`) and, on the first activation,
  updates the caster's state (a state change, presentation or dispel: NOT FOUND). Then a `BHav` buff is created
  (`0x6F1C7F80`, always a new one, no lookup) and applied with the six values **(owner, duration, DataA, DataB,
  `f2i(DataC)`, DataD)**. The duration is **`Dur` + `+0x9C`** always (no hero test).
* `CBuffAvatar::apply` (slot 214, `0x6F1A03B0`, CONFIRMED): the values go to four stored cells (DataA at `+0xD4`, DataB at
  `+0xDC`, the integer at `+0xE4`, DataD at `+0xE8`), then three stat sinks and the immunity:
  * DataA -> `0x6F2769C0`: **adds to the unit's armor cell** (`+0xE0`, the same one the damage routine reads);
  * DataB -> `0x6F2AC450(unit, DataB, mode 3)`: the unit **life sink** in mode 3 (`heroes.md` section 10, Unicorn-checked
    there): a non-negative rounded delta raises the maximum and the life by it;
  * `f2i(DataC)` -> `0x6F0C6C10(attack ability, value, -1)`: the **attack damage bonus** (the sink the Command Aura
    uses);
  * `0x6F07A3A0(owner, 1)`: adds one to the magic immunity counters `+0x15C` and `+0x160`; on the first count it also
    removes a buff of a code returned by `0x6F0D9B60` and runs a dispel routine (`0x6F079FE0`); the selection of that
    dispel is NOT FOUND (the model does not dispel).
  Then presentation (scale and model change through unit slots `0x8C`/`0x90`) and the notification `0xB`.
* Remove (slot 207, `0x6F1A0480`, CONFIRMED): the same sinks with the negated values (`-DataA`, `-DataB`, `-int`) and one
  magic immunity count off.
* Field meaning (LIKELY from the sink each value reaches): A = armor bonus, B = hit point bonus, C = damage bonus
  (integer), D = a stored flag (magic immunity switch, UNSURE: the stored cell is not read by the sinks above).
* Model: `apply_avatar` / `undo_avatar`: `bonuses.armor += A`, the hero life sink in mode 3 with B, `bonuses.damage +=
  f2i(C)`, `magic_immune += 1`; undone exactly. Expiry through the buff timer (message `0xD01C4`, `buffs.md`). UNSURE:
  whether the original's immunity counter is the only effect of "magic immunity" (damage gates in `damage.rs` read
  Misc `MagicImmunesResistDamage`; the sim has no spell-immunity check on buff application yet).

## 5. Resurrection (`AHre`)

* Cast (slot `0x428`, `0x6F1D1FA0`, CONFIRMED structure): `max = f2i(DataA)`, the area is `Area`, the centre is the
  caster's position. The group builder `0x6F2A3270` runs the callback `0x6F1D1720` (flag arguments `4` and `0x1F`) which
  collects candidate corpses into 20-byte records; the records are **sorted** with `0x6F4C6210`; then slot `0x438`
  (`0x6F1B5A60`, the revive of one unit) is called for the first `min(max, count)` records.
* Candidate rule (`0x6F1D1720`, partly read): the caster itself is accepted without further tests; otherwise a unit is
  considered when its flag word has `0x400000` (a corpse) and the ability's target check passes, then the two players
  must be related by `0x6F3A3530` (an alliance test, so allies and own units, not enemies), the target must carry the
  corpse buff `Acar` (code from `0x6F15BA20`) and a record is filled (buff, a flag from `+0x1FC == 0x10`, the unit's
  position). NOT FOUND: the callback that fills the sort keys (`0x6F1CFF70`).
* Sort (`0x6F19EA20` for `AHre`, slot `0x430` returns 1): corpses whose owner index equals the record's `+0x10` field
  (the caster's own player) come first; otherwise the secondary key is `0x6F19E970` (record `+4` descending, then `+0xC`
  with zero first). The meaning of `+4` and `+0xC` is not known.
* Revive (`0x6F1B5A60`, CONFIRMED order): a corpse that is a "fresh" hero death (flag `0x10` of `+0x5C`) is first
  checked; the `Agyc` buff (graveyard timer) is removed (`0x6F0827D0`); the unit is revived with a **life fraction of
  1.0** (`vf 0x180(...)` with the fraction on the stack) at its own position; if slot `0x434` of the ability returns
  true the unit is marked summoned; the event for the reviver (JASS `RESURRECT`, `GetReviveUnit`) fires (not modelled).
* Model: `resurrection`: dead, not removed units within `Area` of the caster, not enemies of the caster, the caster's
  **own** corpses first (UNSURE: the sort keys), then the others in arena (creation) order (UNSURE), the first `f2i(DataA)`
  of them get `dead = false`, life = max life (`revive_unit`). Mana of the revived unit is left as it was (UNSURE), no revive
  event is fired (NOT DONE), no corpse buff or decay timer exists in the sim (every dead, not removed unit is a corpse).

## 6. Summon Water Elemental (`AHwe`)

* Cast (slot `0x3C0`, `0x6F163390`, CONFIRMED structure): the unit type comes from the level's `UnitID` (`+0x44`, slot
  `0x3E8`), the count is `f2i(DataA)`, the duration is `Dur` + `+0x9C` (**no hero test**), `Area` is read but not used by the
  loop. For each unit: the registration flags are `0x502` when the duration is zero and `0x2` otherwise (so a timed
  summon does not use food), the position comes from `0x6F1D26B0` (around the caster: the caster's position plus an
  offset from `0x6F4AFCD0`, which may draw randomness: NOT FOUND), the unit is created by `0x6F1D27A0`, linked to the
  ability (`0x6F052EC0`), and the slot `0x3E4` (`0x6F18C730`) applies the **timed life buff** through the creator
  `0x6F07B170` with the duration; finally the summon event `0x80254` is posted (`0x6F299390`).
* Model: `water_elemental`: `f2i(DataA)` units of the level's `UnitID` for the caster's owner at the caster's position
  (UNSURE), `summoned = true`, `apply_timed_life(unit, 'BHwe', Dur)`; the timed life expiry kills them (`buffs.md`
  section 3). Not modelled: the summon event, the placement offsets, food flags.

## 7. Blizzard (`AHbz`) and Mass Teleport (`AHmt`): NOT DONE

Located only: classes (section 0), the Blizzard channel end handler `0x6F1AFBE0` (clears the channel state and stops
the wave timer at `+0x144`) and its wave-state setter (`0x6F1AFCE0`, stores a time computed from the missile speed
and the delay: `max(a, b)` through the agile setter), the init `0x6F197E90`, the buff `BHbz` (`CBuffBlizzardAoe`).
The wave scheduling, the damage per wave (building factor, random target cells) and the Mass Teleport group walk are
not decoded; no sim code exists for them.

## 8. Order ids and routing (UNSURE)

Thunder Clap 852594, Avatar 852086, Divine Shield 852090, undivineshield 852056, Resurrection 852044, Water Elemental
852097 are from the public order-id lists, like the three spells of `abilities-5.md` section 7. `natives/order.rs`
routes `IssueImmediateOrderById` with such an id (the unit must own the ability) to `human_heroes::immediate_order`;
the native returns whether the cast started. The cast core was extended with the no-target form (`CastTarget::None`).

## 9. Verification

| Check | Original | Cases | Mismatches |
|---|---|---|---|
| class identity (slot 7) of 6 abilities and 5 buffs | the getters | 11 | 0 |
| Bash effective chance (DataA bits, counter bits, incl. NaN, inf, negatives, huge counters) | `0x6F1A3470` -> `0x6F1D21F0` | 400 | 0 |
| Bash proc compare (draw, chance bits incl. NaN) | `0x6F01FD60` | 400 | 0 |
| Bash draw on stream 3 (random stream states) | `0x6F2853D0` | 400 | 0 |

Rust: `human_heroes_vectors.rs` (the three vector checks; the first and last need `GAME_DLL`), `human_heroes.rs` (12
behaviour tests: Thunder Clap area/slow/durations/mana, Avatar sinks and undo, Divine Shield invulnerability and
cancel, Resurrection selection and area, Water Elemental count and timed death, Bash always-proc, stream-3 draw
accounting and counter). The rest of the numbers (damage, armor sink, life sink mode 3, timed life) reuse earlier
Unicorn-checked pieces (`attack-damage.md`, `heroes.md` section 10, `buffs.md`).

## 10. Open

* The attack cycle's call points for the four Bash slots; the target filter and the unit-flag test of Bash.
* The `+4`/`+0xC` sort keys and the corpse record of Resurrection; the corpse model (decay, `Acar`, `Agyc`).
* The placement of summoned units; the summon and revive events.
* The dispel and the state change of Avatar's first activation; the Misc half of the Divine Shield cancel test.
* Blizzard and Mass Teleport.
