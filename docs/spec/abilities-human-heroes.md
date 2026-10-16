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
| `AHbz` | `CAbilityBlizzard` | `0x6F910CAC` | point target, spawns the effect `XHbz` (section 11) |
| `AHmt` | `CAbilityMassTeleport` | `0x6F88BEFC` | unit target, group teleport (section 12) |

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
  (`0x6F077320` subtracts one from `+0x15C`, and from `+0x160` for the flag argument).
  The **apply** side (slot `0x31C` = `0x6F07D180`) is now read and **CONFIRMED by Unicorn, 400 cases, 0 mismatches** (V1,
  `tools/emu/spell_buff_emu.py --only divine_shield`, `crates/sim/tests/data/unit_spells/divine_shield.txt`,
  `spell_buff_vectors.rs`; the real cast slot `0x6F0B64C0`, the real apply and the real counters `0x6F0796F0` / `0x6F07A3A0`
  on a fake caster; stubbed: buff lookup / allocator, base apply, purge removal, dispel walker, cancel test): the set-duration
  rule (hero test on the CASTER: `HeroDur`, else `Dur` + `+0x9C` by soft add), then the purge-debuff lookup and removal,
  then the dispel walker with `(0,1,0,0,0,1,1,0,1)`, then the invulnerability count (`+0xE8` plus one, unit flag `0x8` set
  when it becomes 1), then the magic immunity counters (`+0x15C` plus one, `+0x160` plus one); when either immunity counter
  becomes exactly 1 the purge removal and the same dispel walker run a second time (idempotent, so the model's single dispel
  has the same net effect). The ability gets flag `0x10000` exactly when the cancel test fails
  (`human_heroes::divine_shield_apply_counters`).
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
* **Field meaning and values: CONFIRMED by Unicorn, 400 cases, 0 mismatches** (V1, `tools/emu/spell_buff_emu.py` check
  `avatar`, `crates/sim/tests/data/unit_spells/avatar.txt`, `spell_buff_vectors.rs`; the real cast slot `0x6F1CA180`, the real
  `CBuffAvatar` apply slot 214, the real helpers `0x6F196A40/60/80` and the real damage counter `0x6F0C6C10`; stubbed: the
  timer start, the buff allocator and registration, the holder objects, the armor / life sinks and the immunity counter,
  which record their arguments; `human_heroes::avatar_values`): duration = `Dur` + `+0x9C` by soft add, no hero test; `DataA`
  reaches the armor sink `0x6F2769C0` unchanged; `DataB` the life sink `0x6F2AC450` with mode 3 (unchanged value);
  both weapon slots' damage counters (mask -1) grow by `F2I(DataC)`; `DataD` is stored in the holder at `+0xE8` and nothing
  reads it (so the "magic immunity switch" reading is wrong: the immunity count is added unconditionally, once, with
  argument 1). The remove slot 207 (negated values) was read, not run.
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
* **Sort comparator** `0x6F19EA20` (+ `0x6F19E970`) is now CONFIRMED (Unicorn 400 cases, 0 mismatches,
  `human_heroes::resurrection_compare`): a different owner than the other record: the record whose owner equals its
  `+0x10` field (the caster's player) goes first; then the larger `+4` key first (a wrapped difference test), then the
  record with a zero `+0xC` flag first, then the smaller `+8` real first (a NaN difference puts the first record last),
  then the larger agent id first. What fills `+4`, `+8`, `+0xC` is still NOT FOUND (`0x6F1CFF70`), so the model keeps
  them equal and the order is "caster's player first, then the larger agent id" (UNSURE; before S30 the model used
  creation order).
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

## 7. Blizzard (`AHbz`) and Mass Teleport (`AHmt`)

Decoded in S30, see sections 11 (Blizzard) and 12 (Mass Teleport).

## 8. Order ids and routing

**CONFIRMED (S30, Unicorn: `tools/emu/human_heroes2_emu.py`)**: vtable slot `0x308` of each class is `mov eax, id`:
Blizzard 852089 (`0xD0079`), Mass Teleport 852093, Thunder Clap **852096**, Avatar 852086, Divine Shield 852090,
Resurrection **852094**, Water Elemental 852097, Holy Bolt **852092**, Thunder Bolt 852095. The earlier values for Thunder
Clap (852594), Resurrection (852044) and Holy Light (852089, which is Blizzard's) were wrong and are fixed (cast.rs,
human_heroes.rs, tests); Mirror Image in `orc.rs` is 852123 (it had 852093). `undivineshield` 852056 is still from the
public list (UNSURE). Slot roles (re-checked after the coordinator's note): for these non-autocast classes slot `0x308` returns the cast id and
slot `0x348` is the generic stub; for autocast classes (Slow) slot `0x348` is the cast id (852075) and `0x308` the
autocast/toggle id (852076). The nine ids above equal the coordinator's table (`ability_orders.py`).
`natives/order.rs` routes `IssueImmediateOrderById` with an immediate spell id to `human_heroes::immediate_order`; point
and unit target orders go through `spell_for_order` into the cast core (`CastTarget::Point` / `Unit`).

## 9. Verification

| Check | Original | Cases | Mismatches |
|---|---|---|---|
| class identity (slot 7) of 6 abilities and 5 buffs | the getters | 11 | 0 |
| Bash effective chance (DataA bits, counter bits, incl. NaN, inf, negatives, huge counters) | `0x6F1A3470` -> `0x6F1D21F0` | 400 | 0 |
| Bash proc compare (draw, chance bits incl. NaN) | `0x6F01FD60` | 400 | 0 |
| Bash draw on stream 3 (random stream states) | `0x6F2853D0` | 400 | 0 |

| Blizzard parameter hand-over `0x6F1B9E90` -> `0x6F1B9BB0` (random level records) | the real method | 400 | 0 |
| Blizzard shard placement `0x6F1AF6E0` (permutation, 2 offsets per shard, final global and stream-4 states) | the real function | 400 | 0 |
| Mass Teleport sort comparator `0x6F026120` | the real function | 400 | 0 |
| Resurrection sort comparator `0x6F19EA20` | the real function | 400 | 0 |
| order ids (vtable slot `0x308`) of 9 classes | the getters | 9 | 0 |

Rust: `human_heroes2_vectors.rs` (the four vector checks; the placement needs `GAME_DLL`), `human_heroes_blizzard_mt.rs`
(4 behaviour tests), harness `tools/emu/human_heroes2_emu.py`.

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

## 11. Blizzard in detail (S30)

Three objects, found by RTTI and the message dispatch (CONFIRMED = read in the machine code and, where a number is
given, Unicorn-run):

* `CAbilityBlizzard` (`0x6F910CAC`): the point form (slot `0x418`, `0x6F1C2650`) builds an `XHbz` effect (type lookup by
  the code from `0x6F197A00`), calls `0x6F1B9E90` (parameter hand-over) and arms a `0xD01C1` timer on the ability
  (`+0x144`, an autocast scan, not the waves). The ability's own handlers `0x6F14CEF0/0x6F14CFC0` are the autocast scan.
  The channel end (slot `0x388`, `0x6F1AFBE0`) destroys the effect (link at `+0x134`).
* `CEffectBlizzard` (`XHbz`, `0x6F8FB97C`): the message handler `0x6F1C2610`: `0xD01B0` -> `0x6F1B6C00` (visual wave),
  `0xD01B1` -> `0x6F1B9DB0` (damage wave), `0xD01C1` -> `0x6F1A1000` (launch the next shard missile; art only).
* `CBuffBlizzardAoe` (`BHbz`, `0x6F905484`, parent `CBuffAoe`): a buff whose duration is `interval - 0.01` and whose
  float `+0xCC` holds the wave damage; see "Not found" below. `BHbd` (`CBuffBlizzard`) is the sister buff.

**Parameters** (`0x6F1B9E90` -> setup `0x6F1B9BB0`, CONFIRMED, Unicorn 400 random level records, 0 mismatches):

| Level field | Use |
|---|---|
| `Cast` (`+0x04`) | wave interval (`+0x80`) |
| `DataA` | number of waves, `f2i`, at least 1 (`+0x78` visual counter, `+0x84` damage counter) |
| `DataB` | damage per wave and unit (`+0x8C`) |
| `DataC` | shards per wave, `f2i`, clamped to `1..=32` (`+0x98`) |
| `DataD` | building factor (`+0xAC`) |
| `DataF` | cap on the total damage of one wave (`+0x94`), 0 = none |
| `DataE`, `Dur` (+ ability `+0x9C`), `HeroDur` | optional debuff properties (`+0x354`, `+0x35C`, `+0x364`); Blizzard has Dur 0 |
| `Area` | damage radius (`+0x74`) and the placement radius |
| `BuffID[1]`, `BuffID[0]` | `+0x370` (`BHbz`), `+0x374` (`BHbd`) |

The Cast field is both the cast time of the cast core and the wave interval.

**Timeline** (CONFIRMED structure): the setup runs the visual wave at once (`0x6F1B6C00`: presentation, then
`+0x78 -= 1`, re-arm `0xD01B0` with the interval while `> 0`, then the placement `0x6F1AF6E0`) and arms the damage timer
`0xD01B1` with **0.9 s** (the cell `0x6FAAE4F0`, start-up value 0.9). Each damage wave decrements `+0x84` and re-arms with
the interval while `> 0`. So the damage waves are at `0.9 + k * interval`, `k = 0..DataA-1`; the visual waves at
`k * interval`. The damage wave first tests the effect's flag word `+0xC0` (the ability's targs word) and skips the
damage when it is zero; the stock `targs` of Blizzard is `_` (empty), so this test cannot be the targs word as read, and
the model damages regardless (UNSURE).

**Shard placement** (`0x6F1AF6E0`, CONFIRMED, Unicorn 400 cases, bit-exact permutation, offsets and final generator
states): `n = +0x98` shards. The shuffle is a Fisher-Yates over `0..n` using the **global generator**, one step per
`i = 1..n-1`, `j = high32(step * (i + 1))`. All other draws are **stream 4** (`add(0.0, mul(1.0, U))`): with `n > 5` the
last shard is at a random point of a square (`x = 0.4*Area*(U-0.5)`, then `y`), the other `m` shards sit on a circle of
radius `Area/2` at angles `base + i * (2*pi/m)` (`base = 2*pi*U`), each plus a jitter `0.4*Area*(U-0.5)` in x then y.
Draw count per wave: `(n>5 ? 2 : 0) + 1 + 2m` on stream 4 and `n-1` global steps. The shard launcher `0x6F1A1000` takes
`perm[remaining-1]` and spawns a missile art object at the offset; it carries no damage.

**Damage wave** (`0x6F1B9DB0` -> area helper `0x6F2C31D0` -> callback `0x6F1B6CB0` -> `0x6F1AF950`; numerics read, not
Unicorn-run, LIKELY): per unit in the area `d = DataB`; when `DataF != 0` and `DataF < d * n` then `d = DataF / n` (soft
`mul`/`div`, `n` = a candidate count chosen by the record's relation kind in `0x6F2C31D0`; the model uses the number of
enemies in the area); a structure (`unit+0x5C` bit 16) or a unit with `unit+0x20` bit `0x200000` takes `d * DataD`; the
result goes to the `BHbz` buff creator (`0x6F1CADD0`) with the interval and the damage: an existing buff keeps the larger
value ("strongest wins", so overlapping Blizzards do not stack). The damage class word is `0x200` (`0x6F1B45F0`), i.e.
type 9 `DAMAGE_TYPE_COLD`.

**Not found**: where `BHbz`'s stored value becomes life loss (the buff has no timer of its own; the inherited `CBuffAoe`
hooks `0x6F0D4CB0` / `0x6F0D1B00` were read only partly), the filter word `0x40303006` of the area helper, the category
counts, whether a stun or order change of the caster ends the channel (the model ends the effect when the caster dies).

Model: `human_heroes::blizzard_*` (params, placement, damage numerics) and the effect timers (`SpellMsg::Human`). The
model deals the damage directly at each damage wave (no buff, no strongest-wins), enemy units and structures.

## 12. Mass Teleport in detail (S30)

Class `CAbilityMassTeleport` (`0x6F88BEFC`). Data (stock): `Area` 700, `DataA` 24 (units teleported), `DataB` 3 (casting
delay), `DataC` 1 (clustering), unit target (`ground,structure,vuln,invu,player,neutral,ally`), range 99999.

* **Start** (unit form, slot `0x424`, `0x6F0565D0`, CONFIRMED structure): removes three buffs, flags the caster (`+0x60 |
  0x200`) and the destination unit, stores the two positions (`+0x138`, `+0x13C`), creates the teleport art, arms a `0xD01C1`
  timer of **1.0 s** on the ability (`+0x124`; its handler `0x6F0263F0` only reports progress to the unit). The point form
  (slot `0x418`, `0x6F032CB0`) resolves a unit at the point and calls the unit form.
* **Delay**: the level init (slot `0x80`, `0x6F060AE0`) copies **`DataB`** into the property at `+0x90`; the model uses
  it as the channel time (LIKELY: the consumer in the cast framework was not read).
* **Completion** (slot `0x39C`, `0x6F069D60`, CONFIRMED structure): `limit = f2i(DataA) - 1`; the group builder
  `0x6F2A3270` (filter word `0x40303006`, centre = the **caster's** position, radius `Area`) runs the per-unit callback
  `0x6F060B30` -> `0x6F056890`, which skips the caster and any unit failing `0x6F0485F0(caster)`, and appends a 20-byte
  record: the unit, `f2i(squared 3D distance to the caster)` (`add(add(x*x, y*y), z*z)` by soft arithmetic,
  `0x6F0201B0`), a level key (hero: the hero ability level `+0x8C`, `0x6F26EB30`; other units: the type record value
  `+0x68` capped by Misc `MaxUnitLevel`, or `MaxHeroLevel` for hero-letter ids, `0x6F29DCA0`), the hero flag and the
  "same player as the caster" flag. **When the record count is above `limit` as an unsigned compare (so a `DataA` below 1
  never trims), the list is sorted with `0x6F026120` and cut to `limit`.** Then clustering is decided by Misc
  `MassTeleportCluster` and `DataC != 0` (`0x6F0327E0`), and every record (and the caster) is moved by `0x6F056980`
  -> `0x6F048AE0` to the destination unit's position; with clustering the unit keeps its offset (UNSURE: the offset
  base, read from the argument flow only).
* **Sort comparator** `0x6F026120` (CONFIRMED, Unicorn 400 cases, 0 mismatches): own units first, then heroes first,
  then the larger key first, then the smaller distance first, then the larger agent id first. It never returns 0 for
  different units, so the sort algorithm is irrelevant. `human_heroes::teleport_compare`, `teleport_select`.
* Model: the effect is created at the effect stage, `DataB` seconds later the caster (if alive) and the destination
  must be alive; candidates are the caster's allies and own non-structures in `Area` (UNSURE: the filter word is not
  decoded), the hero key is the hero level, the other keys are 0 (UNSURE), the agent id is the arena index (UNSURE).
  The cast is not interrupted by orders or stuns (NOT DONE).

## 13. What S30 did not raise to CONFIRMED

Thunder Clap (target set filter `targs`, the per-owner array), Divine Shield (the apply side of the buff, the Misc half of
the cancel test), Avatar (the first-activation dispel) and Water Elemental (placement `0x6F1D26B0` / `0x6F4AFCD0`) keep the
labels of sections 1 to 6; the time went into Blizzard, Mass Teleport and the Resurrection sort. Nothing in them changed
except the order ids of section 8.

## 14. The shared channel lifecycle (S35)

Read from the machine code (CONFIRMED = read in disassembly; Unicorn numbers where a count is given). Implemented once
in `cast.rs` (`channel_time`, `complete_cast`, `end_cast`, `process_interrupts`) and used by Blizzard and Mass Teleport.

**Flags of the ability word `+0x20`** (names are mine): `0x1` cast active, `0x4` the unit's order queue is held,
`0x800` the cast animation runs, `0x1000` channel hook active, `0x2000` casting by order (selects the order id getter
`0x368` instead of `0x348`: slot `0x314` is just this bit), `0x4000` ENDCAST pending, `0x40000` finish succeeded,
`0x40000000` backswing/channel phase.

**Stage 1** (`0x6F071570`): sets `0x1000`, clears `0x4000`, posts CHANNEL (`0x6F29A0F0`), then calls slot `0x384` (class
channel-start hook; the default is a `ret`, only `ANcl` implements it), then the readiness check. The stage-1 delay is
the property at `+0x80`: its value is the level's `Cast` (base init `0x6F05C6C0` through `0x6F01F850`) -- **Blizzard
overwrites it with 0.0** at the end of its level init `0x6F197E90`, so a Blizzard cast starts at once and its `Cast` is
only the wave interval (the model had waited `Cast` seconds before; fixed, CONFIRMED by disassembly).

**Stage 3 tail** (`0x6F063760`, CONFIRMED): after EFFECT and the effect body (mana, class effect, cooldown) the property
at `+0x90` (value cell `+0x94`) is the **channel time**:

| Source | Value |
|---|---|
| default (base init `0x6F05C6C0`, when slot `0x35C` is 0) | the unit type's `castbsw` (`0x6F29BD70`, unit type record `+0x70`) |
| Blizzard (`0x6F1B9E90`, end) | `mul(I2R(f2i(DataA)), Cast)` = waves times the interval (**Unicorn 400/400**, `human_heroes::blizzard_channel_time`) |
| Mass Teleport (level init `0x6F060AE0`, slot `0x35C` returns 1) | `DataB` |

`0x6F022270` (value above 0.0: ordered `c < p` as an x87 compare, then not `0.001 > |p - c|`) and `0x6F022340`
(differs from `FLT_MAX` = `0x6FAAE624`: not `0.001 > |c - p|`) decide: not above zero -> slot `0x39C(1)` at once
(**Unicorn 400/400** each, `cast::prop_above`, `cast::prop_differs`); otherwise slot `0x398` (default `0x6F050A60`: sets
bit 8 of the flag word of the object the unit's `+0x19C` handle names; no simulation effect found), then, unless the
value is `FLT_MAX`, a `0xD01BD` timer of that many seconds on the cast's stage timer (`+0xBC`). The FINISH event of a
spell therefore comes `castbsw` after the effect for an ordinary spell (the earlier model fired it with the effect; the
`spell_cast.rs` test is updated), and `waves * Cast` after the effect for Blizzard.

**Message `0xD01BD`** (dispatch table at `0x6F074E00`, handler list `0x6F074C60`): slot `0x39C(1)`. Mass Teleport's
class override `0x6F069D60` teleports and then falls into the default `0x6F05BCD0` (success path: tells the unit the
order finished with `0xD015F`/`0xD0160`, then `0x6F050AF0`): **FINISH event (`0x6F29A5A0`), the channel-end hook (slot
`0x388` when flag `0x1` is set), the unit's order drain**. The hooks: default `ret`; Blizzard `0x6F1AFBE0` (resets the
unit animation, destroys the effect through the handle at `+0x134`, cancels the autocast timer, then posts ENDCAST
`0x6F050950`); Mass Teleport `0x6F056AC0` (ENDCAST first, then clears flags `0x80000`/`0x100000` on the caster and the
destination and ends the teleport art). ENDCAST for the other classes is not posted by a visible hook (`0x6F050950` has
34 callers, mostly class overrides): the model keeps firing it after FINISH (LIKELY).

**Interruption** (message `0xD0144` -> slot `0x3A0` = `0x6F05C020` -> `0x6F050BE0`, CONFIRMED structure): the message
carries an order object; its id (`+0x34`) is compared with the ability's own order id (`0x368` when flag `0x2000`, else
`0x348`). When it **differs**: flag `0x1000` is cleared and slot `0x388` runs, the stage timer `+0xBC` is cancelled, the
casting flags are cleared and the animation is reset. The same id leaves the cast alone. No FINISH is posted on this
path; the hook posts ENDCAST. Where the unit posts `0xD0144` is the order dispatcher (`orders.md` section 5: every
replacing order and stop). The model (`note_order_replaced`, `process_interrupts`): a replacing order (including stop)
of another id ends the cast; **death or removal of the caster and a stun of a channelling caster** end it (LIKELY from
game behaviour; the buff-side calls were not read); **silence does not** end a started channel (LIKELY). The processing
runs after every timer event and tick (the original reacts inside the order call; the difference is at most one tick).

Open: the unit-side meaning of slot `0x398`; who posts ENDCAST for classes without a hook; whether a re-issue of the same
order id restarts a channel in the original (the model restarts the cast, ending the old one).

## 15. Blizzard damage delivery (S35)

**Data of the shipped 1.26a patch** (`War3Patch.mpq`, `Units\AbilityData.slk`; the older archives differ): `AHbz`:
`targs` `_`, `Cast` 1, `Dur`/`HeroDur` 0, `BuffID` `BHbd,BHbz`, `EfctID` `XHbz`. `AHmt`: ability `targs`
`ground,structure,vuln,invu,player,neutral,ally`.

**Argument capture** (Unicorn: the real `0x6F1B9DB0` against a fake effect, `tools/emu/human_heroes3_emu.py`): the damage
wave calls the area helper `0x6F2C31D0` with: a shape list `{area (+0x74), value (+0x8C)}` (count 1), the damage class
`0x200` (effect slot `0x88`), **the effect's flag word `+0xC0`** (= the ability's flag word `+0x4C`), the caster, the
owner index `+0xBC`, the callback `0x6F1B6CB0` and the effect as context; the circle centre is `(+0x64, +0x6C)` (x, y).

**The flag word** (`targs` table in the file at `0x6FA6F478..0x6FA6F608`, 8-byte records name/value; values read as data):
air `0x4`, alive `0x1000000`, ally `0x4000`, ancient `0x80000000`, bridge `0x400`, dead `0x2000000`, debris `0x100`,
decoration `0x200`, enemy `0x10000`, friend `0x6000`, ground `0x2`, hero `0x400000`, invulnerable `0x200000`, item `0x20`,
mechanical `0x8000000`, neutral `0x8000`, none `0x1`, nonancient `0x40000000`, nonhero `0x800000`, nonsapper `0x10000000`,
notself `0x1E000`, organic `0x4000000`, player `0x2000`, sapper `0x20000000`, self `0x1000`, structure `0x8`, totem/ward
`0x10`, tree `0x40`, vulnerable `0x100000`, wall `0x80`. The normaliser `0x6F2552F0` adds the default of every group the
word leaves empty (relations: all; alive/dead: alive; classes: ground+air+structure; vulnerable/invulnerable:
vulnerable; hero, ancient, sapper, organic/mechanical: both). So `_` = `0xFDD1F00E` means: **alive, vulnerable (not
invulnerable), ground/air/structure, all relations including self and allies**. Where the original keeps allies out of a
Blizzard is not found (the damage target side has a relation case, `attack-damage.md` section 11.6); the model hits
enemies only (UNSURE). Mass Teleport's group filter `0x40303006` (nonancient, player, vulnerable, invulnerable, air,
ground; item 17 of the op list `0x1D`) = **units of the caster's own player**, alive, not structures, not ancient,
vulnerable or invulnerable alike.

**The per-unit callback `0x6F1AF950`** (CONFIRMED structure; numbers **Unicorn 400/400** against the real function with a
fake effect, target and buff, `human_heroes::blizzard_unit_damage`, `blizzard_buff_replaces`): arguments (unit, `n`, pointer
to the value `d`).
1. A non-immune target (`unit+0x15C <= 0`) with a non-zero optional debuff duration (`HeroDur` for heroes, else `Dur`;
   0 for the stock ability) gets the `BHbd` debuff through effect slot `0x80` (not modelled; the stock ability skips it).
2. **Cap**: `DataF != 0` (NaN counts as non-zero) and `cap < mul(I2R(n), d)` (ordered) -> `d = div(cap, I2R(n))`.
3. The sign flips when effect slot `0x8C` is true (0 for Blizzard).
4. **Building factor**: a structure (`unit+0x5C` bit 16, test `0x6F26DE50`) or a unit with `unit+0x20` bit `0x200000`
   (`0x6F26DE80`, meaning unknown; the model treats it as false) takes `mul(DataD, d)`.
5. Effect slot `0x84` (`0x6F1CADD0`) gets or creates the **`BHbz` marker buff** on the unit (duration `interval - 0.01`,
   `0x6F0D6040`: `sub(interval, 0.01)` when the interval is above zero, else a zero-length buff that ends at once). A
   **new** marker makes the callback return **1** (slot `0x84` returned 0); an **existing** one keeps its
   value unless `existing < d` (ordered), in which case the stored value (`+0xD0`) is replaced and the duration refreshed
   through slot `0x350`; the callback returns **0**.
6. The area helper applies the damage only when the callback returned non-zero (`0x6F2C3597`: `test eax, eax`, then the
   record builder `0x6F2556E0` and the target's damage slot `0x120`; `0x6F2BFED0`): **a unit that already carries `BHbz`
   takes no damage from this wave**. Consequences: two overlapping Blizzards do not stack within the marker window
   (`interval - 0.01`); successive waves of one Blizzard (`interval` apart) each hit.
7. Damage record: amount `d`, class word `0x200` (type 9) from the effect (slot `0x88`), plus bit `0x10` when `d == 0.0`
   exactly; a negative `d` takes the heal path (`0x6F277C20`, `-d`; not modelled). The record goes through the target side
   `0x6F2A40D0` (`attack-damage.md` section 11): class `0x200` is not the armor class `0x10`, so only the type table and
   the min-1 rule apply.

`BHbz` itself (`CBuffAoe` slots: `0x354` returns 1, so its apply/remove hooks `0x6F0AE0B0`/`0x6F0AE0D0` do nothing; the
`0x44/0x48/0x4C` overrides are save/load of the stored value) deals **no damage of its own** (the earlier "life loss from
the buff" question is answered: there is none). `n` in step 2 is the number of enumerated units of the same relation kind
(the area helper's kind buckets `+0x4C..+0x54`, kind from the record `+8`); the model uses the number of enemy targets
(UNSURE).

Model: `human_heroes::blizzard_damage` (marker map `State::markers`, spell buff `BHbz`), tests
`two_blizzards_do_not_stack_and_invulnerable_units_are_skipped`, `human_heroes3_vectors.rs`.

## 16. Mass Teleport: delay, filter, placement (S35)

* **Channel delay** (CONFIRMED, **Unicorn 400/400**: the real level init `0x6F060AE0` with the base init stubbed): the
  property at ability `+0x90` (the channel time of section 14) is set to the level's **`DataB`** unchanged. The teleport
  runs when that `0xD01BD` timer ends (completion `0x6F069D60`), so it is cancelled by an order, a stun or the caster's
  death before that (section 14). The completion also requires flag `0x80000` on the ability (set at the start), a live
  destination whose `+0x5C` bit `0x100` is clear, and `0x6F0485F0(caster)` to accept the caster; otherwise it only
  finishes.
* **Group filter** (CONFIRMED: argument capture of the call at `0x6F069E95`, the `targs` table of section 15): the group
  builder `0x6F2A3270` gets op `0x1D` with the word `0x40303006` (nonancient, player, self, vulnerable, invulnerable, air,
  ground), the caster as reference and player -1. The normaliser `0x6F2552F0` (**Unicorn 400/400**,
  `human_heroes::targs_normalise`) turns it into `0x7DF03006`: **units of the caster's own player** (and the caster itself,
  which the callback `0x6F056890` skips), alive, **no structures** (the class group is given, so structure is not
  defaulted in), not ancient, invulnerable ones included. Allied units of other players are NOT taken (the earlier model
  took allies). The per-unit callback also asks `0x6F0485F0` (the "can be moved" test: not owned by someone else, not a
  unit with a channelling order id `0xD0278`, not flag `+0x60 & 0x200` = already being teleported, not a ward/structure
  class; error codes `0xDD`, `0x6C`, `0xBA`, `0x5C`, `0x95`, `0x69`) -- only the owner and structure parts are modelled.
* **Selection**: the limit `f2i(DataA) - 1`, the record layout and the comparator are section 12 (unchanged, Unicorn 400).
* **Placement** (CONFIRMED structure by running the real `0x6F056980` with the move call captured): with the cluster
  flag every unit is handed the **destination position pair itself** (`0x6F048AE0(unit, &dest, &dest, 0, ...)`), so they
  stack on the destination; without it the unit gets an offset-based pair (`*A2 + *A4` of the helper's arguments; the
  formula behind them was not decoded: the model keeps the caster-relative offset, UNSURE). **The cluster flag** is Misc
  `MassTeleportCluster` (read with default 0, `0x6F009F90` with `push 0`) **and** `DataC != 0` (`0x6F0327E0`); the key is
  not in the shipped `MiscData.txt`, so in the stock game clustering is OFF whatever `DataC` says (the earlier model
  clustered on `DataC` alone and kept offsets for it, the opposite). `Game::set_mass_teleport_cluster` carries the key.
  The move itself (`0x6F048AE0`, 14 arguments: pathing-aware placement, the transported unit's order cleanup) is not
  decoded; the model clamps to the playable area and stores the position.
* Order: the caster is moved first, then the sorted records (`0x6F06A023..0x6F06A063`, stride 20 bytes).

## 17. Resurrection, the magic-immunity apply, Banish, Phoenix and what stays open (S35)

**Resurrection corpse records** (`0x6F1D1720` callback, record builder `0x6F1C1B90`, CONFIRMED by reading; the distance
Unicorn 400/400): a non-hero unit (type id first character not `A`..`Z`) accepted by the ability's flag word
(`air,ground,dead,friend`) becomes a 20-byte record `{unit, key, real, flag, owner}`: `key` (`+4`) = `0x6F29DCA0(unit)` =
the type's `level` (0 when not positive, at most Misc `MaxUnitLevel`), `real` (`+8`) = the **squared 2D distance to the
caster** (`0x6F020210` 3D difference then `0x6F020000` = `x*x + y*y`, both soft arithmetic), `flag` (`+0xC`) = 0, owner
index (`+0x10`) = the caster's player. The sort comparator of section 5 therefore orders: the caster's own corpses first,
then **higher unit level first, then nearer first**, then the larger agent id. The model uses exactly this (the earlier
"all keys equal, larger agent id first" order was wrong; the `human_heroes.rs` test is updated). Hero corpses take another
path through the `Acar` corpse buff (`0x6F1CFF70` -> `0x6F1C9780` over up to ten buff references) and are not modelled: the
model skips heroes (UNSURE whether the stock game can revive them).

**Divine Shield apply** (`CBuffDivineShield` slot `0x31C` = `0x6F07D180`, CONFIRMED by reading; `0x6F0D9B60` = `'Bprg'`):
base apply (slot `0x320`), then the purge debuff `Bprg` is removed, then the dispel walker `0x6F079FE0`, then
`0x6F0796F0` (invulnerability count `+0xE8`, bit `0x8` of the flag word) and `0x6F07A3A0(unit, 1)` (magic immunity counts
`+0x15C`/`+0x160`; on the first count the same purge removal and dispel run again). **Avatar** runs the same removal and
dispel on its first immunity count (`0x6F07A3A0`). The walker (Unicorn: the real function against fake buff objects with
random predicate answers, **400/400**, `human_heroes::dispel_selects`): the nine stack arguments are physical, magic,
undispellable-ok, aura-ok, typed, positive, negative, type-arg, timed-ok; Divine Shield and Avatar call it with
`(0,1,0,0,0,1,1,0,1)`: **magical buffs of both polarities, no auras, nothing undispellable, timed life included**.
`Game::magic_immunity_dispel` applies it (the sim knows the predicates of stun, slow and Thunder Clap slow only, as
negative magical buffs: UNSURE for other codes). **Cancel**: slot `0x318` of `CAbilityDivineShield` (`0x6F0AE2C0`)
returns **852091** (`0xD007B`) when `0x6F089A50` allows it (so the earlier id 852056, which is `undefend`, was wrong), and
`0x6F089A50` needs `DataA != 0` **and** Misc `CanDeactivateDivineShield` (default 0, absent from `MiscData.txt`): in the
stock game the shield cannot be cancelled. `Game::set_can_deactivate_divine_shield` carries the key.

**Thunder Clap** (patch data: `targs` `ground,neutral,organic`, `Dur` 5, `HeroDur` 3, `BuffID` `BHtc`): the flag word
normalises to ground-only classes (no air, no structure), organic, relation `neutral`; what `neutral` means against the
player alliance masks in `0x6F25D480` (a mask pair at `+0x38`/`+0x78` of the player object) was not decoded, so the
enemy rule of the model stays UNSURE. The slow application (`0x6F18A930`): duration hero test then `HeroDur` / `Dur + (+0x9C)`
(section 1), movement `-DataC`, attack speed `-DataD`; an existing `BHtc` is refreshed through its slot `0x358`, a new
one is created and applied through slot `0x354`.

**Water Elemental placement** (`0x6F163390` loop, `0x6F1D27A0`, `0x6F1D26B0`, `0x6F4AFCD0`, read, not Unicorn-run):
each unit is created by `0x6F29F990` with facing = the caster's facing (or Misc `BuildingAngle` times pi/180 when the
flag argument is zero) at a position `0x6F1D26B0` computes as the caster's position plus a rotated offset
(`0x6F4AFCD0` builds a rotation from an angle and the axis `(0,0,1)`); the offset argument comes from the caller's
vector call and was not resolved, so the model keeps the caster's position (UNSURE). Registration flags `0x502`
(duration 0) / `0x2`.

**Banish** (`AHbn`, order **852486**, unit target; patch data `Dur` 12/24/36, `HeroDur` 5/7/9, `DataA` 0.5): unit effect
`0x6F1CA2B0`; values **Unicorn 400/400** (`human_heroes::banish_values`): duration = `HeroDur` for a hero else
`add(Dur, +0x9C)`, movement factor `-DataA`, attack factor `-DataB`. Buff `BHbn` (`CBuffBanish` < `CBuffEthereal` <
`CBuffSpeed`): apply = speed apply (`0x6F0D6460`), remember the caster, stop a running attack (`0x6F2A4B80`), ethereal
counter +1 (`0x6F07A340`); removal: counter -1 and a zero-damage hit of class `0x210` from the caster. Model: spell buff
`BHbn` (slow factors) + `ethereal += 1` + `combat_end`; the removal lowers the counter.

**Summon Phoenix** (`AHpx`, order **852489**, no target): `CAbilitySummonPhoenix` is a `CAbilityWaterElemental`; the
effect `0x6F11BA10` only registers a callback and enters the Water Elemental summon loop; the post-create hook
`0x6F128BE0` stores the summoner in a buff of the new unit and starts **no** timed life. With `Dur` 0 in the data the
Phoenix is permanent. Model: `human_heroes::phoenix` = the shared `summon` body.

**Siphon Mana** (`AHdr`, order **852487**, unit target) and **Flame Strike** (`AHfs`, order **852488**, point target)
are decoded only structurally and are NOT modelled: Siphon Mana is a channel (`Dur`/`HeroDur`, timer message `0xD01C1`,
per tick the range test `0x6F4766E0` with `Area`, then slot `0x3EC` = `0x6F04F7A0`), whose transfer rules are driven by the
Misc keys `DrainTransfersLife`, `DrainTransfersMana`, `DrainGivesBonusLife`, `DrainGivesBonusMana` (all default 0, absent
from the shipped `MiscData.txt`) and the level fields `DataA..DataG`; the per-unit effect is `0x6F0B6900` (read as far as
the flag decoding, not the transfer arithmetic). Flame Strike creates the effect object `XHfs` (`CEffectFlameStrike`, a
`CEffectDeathAndDecay`) and applies the damage-over-time buff `BHfs` (`CBuffFlameStrike` < `CBuffLifeDoT`); `Cast` 1.33 is
its stage-1 delay as for ordinary spells, `DataA` damage, `DataB` interval 0.33 (LIKELY meanings). **Brilliance Aura**
(`AHab`) and **Devotion Aura** (`AHad`) are passive aura emitters already modelled in `auras.rs` (buffs.md sections 4 and
5: armor `DataA` and mana regeneration `DataA` through the aura buffs `BHad`/`BHab`); S35 added no change.

**Open**: the offset placement of Mass Teleport units and Water Elemental summons, what `neutral` selects in the
relation masks, the Acar hero-corpse path of Resurrection, the Siphon Mana and Flame Strike arithmetic, the unit-side
meaning of slot `0x398`.

## 18. V5 verification (Unicorn against the original, 0 mismatches required)

* **Thunder Clap (`AHtc`) CONFIRMED.** The vtables of `CAbilityThunderClap` (`0x6F8F67C4`) and `CAbilityStomp` (`0x6F8F6C04`)
  were compared slot by slot: `0x428` (`0x6F1608B0`), `0x42C` (`0x6F18F880`), `0x434` (`0x6F176CD0`), `0x418`, `0x424`, `0x330`,
  `0x338`, `0x348` are the same functions; only `0x430` (the slow `0x6F18A930` here, the stun `0x6F18ABF0` for Stomp) and
  `0x308` (order id) differ. So the area filter and the per-unit body `0x6F1607B0` are War Stomp's (orc spec section 13).
  Run: `THUNDER_VT=1 tools/emu/stomp_emu.py` (the real callback on a fake Thunder Clap ability, slot `0x430` recorded) 400
  cases, 0 mismatches (`tests/data/human_heroes/thunder_unit.txt`, test `thunder_clap_per_unit_body_matches_the_original`),
  plus the slow's own run (400 cases, section 17). **Bug fixed:** the model filtered "enemy, no building, no flyer" by hand
  and sent the full `DataA` to spell-immune units; `thunder_clap` now uses `orc::stomp_word` / `area_predicate` (enemy and
  neutral relations, `targs` ground/organic bits) and the zero-record path (amount 0.0, flags `0x50`, no slow) for an
  invulnerable or spell-immune unit. Default `targs` `ground,neutral,organic` (`THUNDER_CLAP_TARGS_DEFAULT`).
* **Summon Water Elemental (`AHwe`) CONFIRMED (loop).** The real summon loop `0x6F163390` (slot `0x3C0` of
  `CAbilityWaterElemental`, `CAbilityScout`, `CAbilitySpiritOfVengeance`) ran in Unicorn on a fake ability with a real level
  record: 450 cases (150 per vtable), 0 mismatches (`tools/emu/v5_summon_emu.py`, `tests/data/human_heroes/summon_loop.txt`,
  test `summon_loop_matches_the_original`). Count `F2I(DataA)`; duration `Dur + ability+0x9C` (soft add, no hero test); creator
  flag `0x502` for a zero duration else `2`; creator arguments (owner index, the `Area` pointer, constant 1, flag) and the
  per-unit order link `0x6F052EC0`, class slot `0x3E4` (pointer to the duration), release. The same wrapper `0x6F1D27A0` as
  Feral Spirit, so the placement is `orc::feral_place` (caster + rotation by the facing times `(Area, 0, 0)`; Feral's `P` run,
  400 cases). **Bugs fixed:** the model put the units on the caster ("the offset argument was not resolved": it is `Area`) and
  passed the facing in radians where `build` takes degrees. `summon` (also used by Phoenix, Scout, Spirit of Vengeance and the
  neutral water elementals) now uses `feral_plan` / `feral_place`. Not run: the class slot `0x3E4` bodies (buff creation, Unicorn
  22 cases for the timed-life apply in `abilities-2.md`), the unit creator's own placement search.
