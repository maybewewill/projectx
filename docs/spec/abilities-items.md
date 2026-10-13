# Item abilities (Game.dll 1.26a) (S36)

Scope: how an item's ability is used from the pack (`UnitUseItem*`), the charge, perishable-removal and cooldown rules,
and the item ability families that matter for melee games: restoration items, tomes (powerups), persistent stat bonuses,
speed items and (as a model) town portal. Model: `crates/sim/src/item_abilities.rs` (hooks in `cast.rs`,
`natives/item.rs`, `natives/hero.rs`, `kill_xp.rs`; loader `crates/engine/src/objdata.rs`). Tests:
`crates/sim/tests/item_abilities.rs` (behaviour), `crates/sim/tests/item_ability_vectors.rs` (Unicorn rows),
`crates/engine/tests/item_objdata.rs` (loader; `W3_ARCHIVES=a.mpq;b.mpq` runs the real tables).
Unicorn tools: `tools/emu/item_trace_emu.py` (call tracer, the decoding aid), `tools/emu/item_cores_emu.py` (numeric
cores). Addresses are 1.26a; no code or tables are copied. Method: capstone reading of the vtable slots of the classes
named by the ability atlas (`docs/research/ability-atlas/`), plus Unicorn runs of the original code on fake objects.

Labels: **CONFIRMED** = read in the machine code and run in Unicorn with 0 mismatches (the row count is given);
**LIKELY** = strong inference from the code; **UNSURE** = a stand-in; **NOT DONE** = not modelled.

## 0. The tracer (how the families were decoded)

`item_trace_emu.py` runs one virtual slot of an ability class on a fake ability object whose level record holds distinct
recognisable numbers (`DataA` = 101.75, `DataB` = 202.0, ...), runs the data getters (`0x6F02E500..0x6F02EE00`) and the
soft-float library for real, and stubs every other call made directly by the slot, recording target, ECX, EDX and the
stack arguments. The trace shows which level field reaches which sink. `item_cores_emu.py` then builds exact input cases
for the numeric cores and records what the original does; the Rust test recomputes every row with the model.

## 1. The use path

* `UnitUseItem` / `UnitUseItemPoint` / `UnitUseItemTarget` (`0x6F3C9090`/`9100`/`9170`, natives-damage-items.md section 3):
  the pack checks, then the order id is read from the item (`0x6F2B6CC0`: the first item ability that answers as usable;
  id = `0xD0028` + that ability's order index, `+0xAC`). The cast order ids of the item ability classes are constants (the
  `cast` getter, slot `0x348`; `docs/research/ability-orders/getters.tsv`): `AIhe` 852272, `AIma` 852276, `AIre` 852281,
  `AIha` 852273, `AImr` 852277, `AIra` 852282, `AIsp`/`AIsa` 852285, `AItp`/`AIte` 852286, `AIem` 852260, `AIlm` 852263,
  `AImi` 852264, `AImn` 852265, `AIsm` 852266, `AIam` 852257, `AIim` 852262, `AIaa` 852259, `AIgo` 852610, `AIlu` 852611.
  The model casts the item's first castable ability by its alias id through `cast::begin_cast` (the order system is not
  needed for the same result); the native returns whether the cast started (the readiness errors refuse it).
* **Charge routine `0x6F2B8390`** (CONFIRMED, `CH` rows, 350 Unicorn cases, 0 mismatches): item flag `0x1000` (no charges)
  skips the decrement; otherwise a count at or below 0 returns at once (nothing else happens, no UI refresh, no event) and
  any other count drops by one. Then, if the holder key resolves, the holder is told (`0x6F299750`: the use-item event, key
  `0x80257`; model: player-unit event `0x32` then unit event `0x57`, manipulating unit = the holder, manipulated item =
  the item). Then, when flag `0x1000` is clear, the item has flag `0x40` and its count is 0, the item is destroyed
  (`0x6F2B8330`: lifetime 0, unlink from the map, removal). Flag `0x40` is therefore **perishable** (`item_flags::PERISHABLE`;
  the meaning is inferred from this test and from `ItemData.slk` `perishable`: LIKELY). A count that reaches 0 does not set
  `0x1000` by itself.
* **When the charge is used** (LIKELY): the cast sequence ends in `0x6F050950` (ENDCAST), which, when the ability carries
  flag `0x4000` ("started by an item"), clears it and calls the charge routine on the item named by the ability's item key
  (`+0xA0/+0xA4`). The cancel path `0x6F049470` clears `0x4000` again when `0x6F0379A0` answers true. The model uses the
  charge when the cast **completed** (an interrupted or refused cast keeps the charge; UNSURE for a cast that is
  interrupted after the effect).
* **Cooldown group** (UNSURE): `ItemData.slk` `cooldownID` is a *base* ability code (all healing potions have `AIhe`, all
  mana potions `AIma`, ...; `ignoreCD` opts out). The model starts a cooldown under that code with the ability's `Cool`
  when the cast completes, and refuses a cast whose group is cooling (every item of the group shares it). The original keeps
  a timer (`0xD01BE`) on the item (`+0xE4`, `0x6F2B5CC0`) and a flag `0x200`; how the group is resolved is not decoded.
* **Pack hooks**: taking an item attaches its abilities to the unit (`0x6F0F8C10` path, CONFIRMED structure) and removing it
  takes them off (`0x6F0F0F80`, list at unit `+0xC0`); the persistent bonuses apply and come off with them (section 3).
  A powerup is consumed at pickup (`0x6F0F0EA0` -> `0x6F2B9600`); the model runs the castable effects of its abilities once
  on the taker (UNSURE: no mana, cooldown or charge).

## 2. Restoration items (castable)

Level fields: `DataA` .. read through the getters `0x6F02E9E0` (A) and `0x6F02EA50` (B) at the ability's level.

| Code | Class (vtable) | Effect slot | Behaviour |
|---|---|---|---|
| `AIhe` | `CAbilityItemHeal` (`0x6F8C9F64`) | `0x428` = `0x6F0E3990` | target = ability target else owner; the life adder `0x6F277C20` gets `DataA` |
| `AIma` | `CAbilityItemManaRestore` (`0x6F8DF81C`) | `0x428` = `0x6F119F10` | the mana adder `0x6F279140` gets `DataA` |
| `AIre` | `CAbilityItemRestore` (`0x6F91A1A4`) | `0x428` = `0x6F1DBB10` | life adder `DataA`, then mana adder `DataB` |
| `AIha` / `AImr` / `AIra` | area forms | `0x428` = `0x6F0F0530` / `0x6F126AA0` / `0x6F1E6250` | group builder `0x6F2A3270` around the target-or-owner position, radius `Area` (`0x6F02E900`); per candidate the callback (`0x6F0F0470` ...) |

* CONFIRMED (`FX` rows, 350, 0 mismatches): the three bodies hand exactly `DataA` (and `DataB`) to the adders, in this order,
  **with no ethereal factor**: the class slot `0x42C` that would select it answers 0 for all three classes (it is run for
  real in the harness, with random ethereal values and Misc bits).
* CONFIRMED (`CB` rows, 350): the area callback skips an immune candidate (`0x6F28D750`) and, when the Misc switch (key at
  `0x6F876B18`) is off, an illusion (flag `+0x5C & 0x40000000`); otherwise it gives `DataA` to the life adder; it always
  returns 1.
* LIKELY: the adders add and clamp to the maximum (the Heal model of unit-spells did the same; the life adder's modifier
  loop `0x44..0x52` has no ability in the model); the mana adder clamps the same way. UNSURE: the candidate relation
  (the group builder's flag words `0x1D`/`0x1F`): the model takes the caster's allies and the caster; immunity is not
  modelled.

## 3. Tomes and persistent bonuses

The tomes (`CAbilityTome` family, init `0x6F0E6580` and the attribute init `0x6F0E5D50`) read their numbers at **level 1**
whatever the ability level: the profile reader `0x6F01FA50`/`FAD0`/`FB50` builds the key `DataA`/`DataB`/`DataC` + `1`
(`push 1`); the value is `F2I` (truncating) of the real. The persistent bonuses read `DataA` of the current level
(`0x6F0E5780` -> `0x6F02E9E0`) into the ability's value (`+0x88`).

| Code | Class | Slot | Behaviour (Unicorn rows) |
|---|---|---|---|
| `AImi` | `CAbilityMaxLifeMod` | `0x428` = `0x6F0DE2F0` | life sink `0x6F2AC450(unit, I2R(n), mode 2)`, n = `F2I(DataA)` (`TM` 350) |
| `AImn` | `CAbilityMaxManaMod` | `0x428` = `0x6F0DE320` | mana sink `0x6F278F50(unit, I2R(n), mode 2)` (`TM`) |
| `AIem` | `CAbilityExperienceMod` | `0x428` = `0x6F0DE390` | experience engine `0x6F20B080(I2R(n), show 1, 0, 0, 0, **range 1**)` (`TM`); the model uses `heroes::experience_engine` with the range flag (upgrade bonus 0 in the model) |
| `AIlm` | `CAbilityLevelMod` | `0x428` = `0x6F0DE200` | levels up by `n` keeping the progress: with `L` the hero level, `need(l)` the need table (`0x6F207760`), `A = need(L-1)` (0 for `L = 1`), `B = need(L+n-1)`, `C = need(L+n)`: amount = `F2I((xp-A)/(B-A) * (C-B)) + (B - xp)`, handed to the experience engine as `I2R` (CONFIRMED, `LM` 350 rows, 0 mismatches; `level_mod_value`) |
| `AIxm`, `AIam`, `AIim`, `AIsm` | `CAbilityAttributeMod` + 3 children (share init and effect) | init `0x6F0E5D50`, effect `0x428` = `0x6F0DDDF0` | init stores `F2I(DataA/B/C)` at `+0x124/+0x128/+0x12C`; the effect calls the **agility** adder `0x6F20A1E0`(A), the **intelligence** adder `0x6F20A260`(B), the **strength** adder `0x6F20A160`(C), flag 0 (`AT` 350). Real data agrees: the editor's Tome of Strength `AIsm` has `DataC` = 1, Agility Bonus `AIa3` has `DataA` = 3 |
| `AIaa` | `CAbilityAttackMod` | `0x428` = `0x6F0DE190` | only with an attack ability: `0x6F0C6BD0(attack, n, mask -1)`: `n` added to both weapons' base damage bonus (`+0xA0`, `+0xA4`) (`TM`); the model adds to the unit's damage bonus |
| `AIgo`, `AIlu` | `CAbilityItemGiveGold/Lumber` | `0x428` = `0x6F0F4DE0` / `0F4E40` | the changer wrapper `0x6F416A00/40` with flags 0,0,0 (a plain credit, no tax or share) of `n`; a floating text for a local owner (not modelled) (CONFIRMED structure, not in the numeric rows) |
| `AIab` | `CAbilityAttributeBonus` (persistent) | init `0x6F0E57E0` | the same three adders with `F2I(DataA/B/C)` of level 1 on pickup, negated on removal (`AB` 350) |
| `AIat` | `CAbilityAttackBonus` | apply `0x6F0F4100`, remove `0x6F0F4170` | `0x6F0C6C10(attack, F2I(DataA), mask)`; removal the negative (`PB` rows). Flag `0x80` marks "applied" so a bonus applies once |
| `AIde` | `CAbilityDefenseBonus` | apply `0x6F0F4340`, remove `0x6F0F4390` | armor sink `0x6F2769C0(&value)`, removal with the sign bit flipped |
| `AIas` | `CAbilityAttackSpeedBonus` | `0x6F0F4230` / `0x6F0F4290` | attack speed sink `0x6F0C9090(&value, 0)`, removal sign-flipped |
| `AIml`, `AImm` | max life / mana bonus | init `0x6F0DDF70` / `0x6F0DDFF0`, remove `0xA4` | life/mana sink with `(DataA, mode 1)`, removal `(-DataA, mode 1)` (`PL` 350) |
| `AIms` | `CAbilityMoveSpeedBonus` | `0x6F0F4500` / `0x6F0F4560` | on the move ability (`unit+0x1EC`); with the Misc key `MoveSpeedBonusesStack` off it calls the speed adder `0x6F201D50`. **UNSURE**: the model adds `DataA` to the unit's speed |
| `AIsi` | `CAbilitySightBonus` | init `0x6F0DDEF0` | sight sink `0x6F27A4E0(&value)` (divides by the constant `0x6FAAE50C`, writes a property). **UNSURE**: the model adds `DataA` to both sight radii |

Sinks: `heroes::life_sink` / `mana_sink` (Unicorn-checked earlier) and the attribute adders go through
`natives::hero::change_hero_attributes` (the setter path of `SetHeroStr`: base `+= delta`, derived values refreshed, mode 1).

## 4. Speed items

`AIsp` (`CAbilityItemSpeed`, effect `0x424` = `0x6F1FC4C0`): duration by the hero rule (`HeroDur` for a hero target, else
`Dur` + the ability's extra term by soft add), then the buff apply of `CBuffSpeed` (`0x6F0D6500`, buff `Bspe`) with
`DataA` -> holder `+0xCC` -> **movement** sink `0x6F202FF0`, `DataB` -> holder `+0xD4` -> **attack speed** sink `0x6F0C9090`
(CONFIRMED, `SP` rows 350: the apply arguments `(dur, DataA, DataB)` bit-exact; the sinks by reading `0x6F0D59A0/59F0`).
`AIsa` (`CAbilityItemSpeedAoe`): area `0x428` = `0x6F1DE240` calls this per candidate (callback `0x6F1D6810`: immunity test,
then slot `0x424` on the candidate). The model applies `UnitMods { move_delta: DataA, attack_delta: DataB }` through
`apply_mods_buff` for the duration; the buff flags (positive, magic, dispellable) are UNSURE.

## 5. Town portal (`AItp`, UNSURE model)

`CAbilityItemTownPortal` (vtable `0x6F88C75C`, child of `CAbilityItemTeleport`): data `Cast` 5 s, `Area` 1100,
`DataA` 90, `DataB` 1, targets structures. The unit effect `0x424` reads the target's position and calls slot `0x418`
(`0x6F0610D0`) which runs the area group builder with the callback `0x6F0264B0` and then slot `0x434` (`0x6F049470`: the
cancel path of the item flag, see section 1). Not decoded: the callback and the teleport effect object. The model moves the
caster and the units chosen like Mass Teleport (`DataA` the count through `human_heroes::teleport_select`, `DataB` the
clustering flag) to the target; **UNSURE** in the field meanings and the delay.

## 6. NOT DONE

`AIrm` / `Arel` (regeneration items: a hero-only modifier on the mana/life property), `AIrg` (Replenishment, a
`CAbilityRejuvination` child), `AIva`, `AIsr`, `AIcf`, the orbs (`CAbilityDamageBonusBase` family), `AIvi`/`AIvu`
(invisibility, invulnerability potions), `AIrc`, `AIrs`, `AIan`, `AIrd`, the figurines, `AIte`, `AIfl`/flags, `AIbl`/`AIco`,
`AIgl`. Their classes are listed in `docs/research/ability-atlas/factory.tsv`; the tracer decodes a slot in minutes.

## 7. Real data (checked)

`ItemData.slk` (1.26a patch): columns `abilList` (up to four ids), `cooldownID` (a **base** code), `ignoreCD`, `uses`,
`usable`, `perishable`, `powerup`, `Level` (capital L), `droppable`, `pawnable`, `sellable`. `AbilityData.slk` rows of item
abilities have `code` (the factory base) and `item` = 1. Example: Potion of Healing `phea` has `abilList` `AIh1` (base `AIhe`),
`uses` 1, `perishable` 1, `cooldownID` `AIhe`; `AIh1` has `Cool1` 20 and `DataA1` 250. About 120 distinct base codes
appear in the item table; 28 are modelled (the codes of sections 2 to 5). The unmodelled ones with the most items are the
orb family (`AIfb` and siblings, 13 items), `AIrg` (11), the figurines (`AIfs`, 8) and the build scrolls (`AIbl`, 8).
