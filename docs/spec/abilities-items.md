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

S41 (this file, sections 8 to 15) adds the cooldown group, the potions, the regeneration items, the damage handlers, the
orbs, the figurines, the item auras, the Ankh and the borrowed spell classes. Unicorn tool of S41:
`tools/emu/item_cores2_emu.py` (vectors `crates/sim/tests/item_ability_vectors2.txt`, tests
`crates/sim/tests/item_ability_vectors2.rs`, behaviour `crates/sim/tests/item_abilities2.rs`).

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
* **Cooldown group** (CONFIRMED structure, S41, Unicorn `GR` 350): the group is the item type's `cooldownID`
  (`ItemData.slk`, a raw code, record `+0x70` of the item type: `0x6F2B8D70`). The unit's inventory ability keeps a table
  of group codes (`+0xC0`) with a timer object per entry (20 bytes at `+0xDC`); the start routine `0x6F0E3F30` (unit,
  item, `&cool`, `&cool`) reuses the entry with the same code or else the entry with the smallest remaining time, starts
  its timer with the cooldown, and tells every pack item of the group (`0x6F2B7040`) that it is cooling (item flag
  `0x200`). The routine is called from the item cast end `0x6F05C840` with the ability's `Cool`, **unless `ignoreCD`**
  (`0x6F2B8DA0`) zeroes it, and only when `Cool` is above 0.0 (soft compare `0x6F01FD40`): `item_abilities::
  group_cooldown_start`. The use gate is the item's flag `0x200` (`0x6F2B6CC0` returns no order while it is set): an item
  is blocked while its group cools, **also an `ignoreCD` item** (it only does not start the group). The model checks the
  holder's group cooldown in `start_use` and starts it at the cast end under the item type's `cooldownID` (skipped when
  the group id equals the cast ability id, whose own cooldown the cast core starts). Not modelled: the carry of a
  cooling group with a dropped item (`0x6F0F0F80` starts a timer on the item with the remaining time; the pickup
  `0x6F0F8C10` re-applies the holder's group).
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
| `AIms` | `CAbilityMoveSpeedBonus` | `0x6F0F4500` / `0x6F0F4560` | on the move ability (`unit+0x1EC`). S41 reading: with the Misc key `MoveSpeedBonusesStack` (absent from the shipped MiscData, so 0) the apply/remove only **recompute** the speed (`0x6F202FE0` -> `0x6F201D50` -> the full move speed computation `0x6F2019F0`); with the key set the ability marks itself (flag `0x80`) and its slots `0x2D4`/`0x2D8` (`0x6F0FAF70`/`0x6F0FAFB0`) store `DataA` into field `+0x28` of the speed record the computation passes along. How the unstacked bonus reaches the computation is **not decoded**, so the model keeps adding `DataA` to the unit's speed (**UNSURE**) |
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

### Regeneration (`AIrg`, area callback CONFIRMED, buff behaviour LIKELY)

`CAbilityRegeneration` (a `CAbilityRejuvination` child; unit effect `0x424` = `0x6F108AC0`, area `0x428` = `0x6F1088D0`
with the callback `0x6F108800`). The real data (potions and scrolls of rejuvenation and regeneration, `BuffID`
`BIrg,BIrl,BIrm`): `DataA` the life total, `DataB` the mana total, `Dur` 30/45 s, scrolls with `Area` 600. The model reuses
the Rejuvenation mechanics of `unit_spells.rs` (decoded in `abilities-unit-spells.md` section 7): the hero rule duration, the buff
for the duration + 0.01 s, a repeating 1.0 s tick (first after 1.0 s) of `DataA / dur` life and `DataB / dur` mana by soft
division, added through the life and mana adders; a refresh replaces the amounts. LIKELY from the class parentage and
the data meaning, not Unicorn-checked.

**V3 (Unicorn `RG`, 300 rows, `regeneration_area_callback_matches_the_original`): CONFIRMED** the scroll form's area callback `0x6F108800`:
a candidate is skipped when the ability's slot `0x3F0` answers non-zero (immunity), or when it is an illusion and `DataA`
is not zero while Misc `IllusionsCanRestoreLife` is off, or `DataB` is not zero while `IllusionsCanRestoreMana` is off (a NaN amount
counts as not zero); otherwise the apply `0x6F108620` gets the duration `HeroDur` for a hero else `Dur`, and `DataA` / `DataB` untouched
(`item_abilities::regen_area_callback`). The earlier model skipped every illusion when the life key was off. Decoded, not run: the
effect `0x424` passes `DataC & 1`, `DataC & 2`, `DataD != 0` and the `BuffID` entries 0..2 (`0x6F02EE20`) to `0x6F108620`, which picks the
buff `BIrm` / `BIrl` / `BIrg` (`0x6F0DAD00` / `0x6F0DAC80` / `0x6F0DAC00`) and its apply slot `0x36C` / `0x368` / `0x364`;
the per-tick amounts of those three buff classes are still the Rejuvenation stand-in above (LIKELY, stock rows have `DataD` = `DataE` = 1).

## 5. Town portal (`AItp`, UNSURE model)

`CAbilityItemTownPortal` (vtable `0x6F88C75C`, child of `CAbilityItemTeleport`): data `Cast` 5 s, `Area` 1100,
`DataA` 90, `DataB` 1, targets structures. The unit effect `0x424` reads the target's position and calls slot `0x418`
(`0x6F0610D0`) which runs the area group builder with the callback `0x6F0264B0` and then slot `0x434` (`0x6F049470`: the
cancel path of the item flag, see section 1). Not decoded: the callback and the teleport effect object. The model moves the
caster and the units chosen like Mass Teleport (`DataA` the count through `human_heroes::teleport_select`, `DataB` the
clustering flag) to the target; **UNSURE** in the field meanings and the delay.

## 6. NOT DONE (before S41; see sections 8 to 15 for what S41 did)

`AIrm` / `Arel` (regeneration items: a hero-only modifier on the mana/life property), `AIva`, `AIsr`, `AIcf`, the orbs (`CAbilityDamageBonusBase` family), `AIvi`/`AIvu`
(invisibility, invulnerability potions), `AIrc`, `AIrs`, `AIan`, `AIrd`, the figurines, `AIte`, `AIfl`/flags, `AIbl`/`AIco`,
`AIgl`. Their classes are listed in `docs/research/ability-atlas/factory.tsv`; the tracer decodes a slot in minutes.

## 7. Real data (checked)

`ItemData.slk` (1.26a patch): columns `abilList` (up to four ids), `cooldownID` (a **base** code), `ignoreCD`, `uses`,
`usable`, `perishable`, `powerup`, `Level` (capital L), `droppable`, `pawnable`, `sellable`. `AbilityData.slk` rows of item
abilities have `code` (the factory base) and `item` = 1. Example: Potion of Healing `phea` has `abilList` `AIh1` (base `AIhe`),
`uses` 1, `perishable` 1, `cooldownID` `AIhe`; `AIh1` has `Cool1` 20 and `DataA1` 250. About 120 distinct base codes
appear in the item table; 29 were modelled by S36 (sections 2 to 5) and S41 adds about 35 more (the families of sections 8 to 14, the
aura classes and the borrowed spell classes). The unmodelled ones with the most items are now the other orbs (`AIpb`, `AIcb`, `AIzb`, the
lightning orbs) and the build scrolls (`AIbl`, 8).

## 8. S41: potions of invisibility and invulnerability (`AIvi`, `AIvu`)

`CAbilityItemInvis` (`0x6F13C180`, slot `0x428`) and `CAbilityItemInvul` (`0x6F13C270`): the target (ability target else
owner) gets the buff `Binv` (`0x6F10F580` returns the code) or `Bvul` (`0x6F10F990`) through the buff finder `0x6F0787D0` and
its apply slot `0x350` with the duration: **`HeroDur` for a hero target, else `Dur` plus the ability's `+0x9C` term** by soft
add (CONFIRMED, Unicorn `VI` 350: `potion_buff_duration`). The buff classes are not decoded; the model makes `Bvul` add one
invulnerability count (the `Avul` ability's flag; `UnitMods::invuln`) and `Binv` a plain marker buff (no detection model
exists), both positive and magic (UNSURE flags). Real data: `AIv1/AIv2` Dur 120/180, `AIvu/AIvl` Dur 15/7, `Cool` 45.

## 9. S41: regeneration items (`Arel`, `AIrm`)

Persistent abilities; each creates a *forever modifier* on the unit's agile life/mana property (the way auras, items and
heroes add regeneration: `Game::add_life_regen_modifier` `0x6F02DC60`, `add_mana_regen_modifier` `0x6F02DD00`) and
releases it on removal. Without agile stats (no tables) the model falls back to the aura bonus fields.

* `Arel` (`CAbilityRegenLife`, apply `0x6F0FB0C0`, Unicorn `RL` 350): rate = the ability value `+0x88` = `DataA` (2.0 and
  1.0 in the stock rows), made unless the carrier is an illusion and Misc `IllusionsCanRestoreLife` (key CONFIRMED,
  `0x6F876B18`) is 0 (`0x6F0F4840`). The Misc ethereal factor sits behind slot `0x33C` which answers 0 for this class.
* `AIrm` (`CAbilityItemRegenMana`, apply `0x6F0FB370`, rate `0x6F0E64E0`): heroes only; rate = `(type regenMana + MiscIntRegenBonus
  * I2R(hero intelligence)) * DataA` (CONFIRMED, `RM` 350 and `RH` 350: add then multiply, `mana_regen_item_rate`,
  `hero_int_regen_term`); the illusion gate uses `IllusionsCanRestoreMana` (`0x6F876B30`, `0x6F0F4AE0`). **`IntRegenBonus` is read
  here** (`0x6F2082C0`; shipped value 0.05): the earlier statement that no `IntRegenBonus` is read was wrong, it is not
  read by the hero attribute setters. The Misc values are loaded by `gamedata::misc` and installed by
  `Game::set_item_misc`. Stock: `AIrm` DataA 0.5 (Sobi Mask), `AIrn` 0.25.

## 10. S41: damage handlers of items (`AIva`, `AIsr`)

Both are `CAbilityPersistentBonus`-family abilities with handlers on damage records. The record's `+0x20` is the attack
type row (0 = Spells), `+0x0C` its flags.

* `AIva` (`CAbilityVampireBonus`, post-damage slot `0x170` = `0x6F0F53A0`, Unicorn `VA` 350): after the pre-damage arming
  (`0x6F0F54A0`: the ability is enabled and not paused, the target filter `air,ground,enemy` accepts the victim, the record is armed) the
  carrier heals `DataA * damage` (soft multiply, **the compare `0 < damage` is an x87 compare that also passes a NaN**)
  through the life adder `0x6F277C20`, unless an illusion on either side blocks it while Misc `IllusionsCanRestoreLife` is
  0; the effect routine `0x6F053040` is called whenever the gates up to the damage compare pass (also for a blocked
  heal). `item_abilities::life_steal_outcome`, hooked in `combat.rs::hit_with_bash` (basic attacks). Stock: Mask of Death
  `DataA` 0.5. Not modelled: the victim immunity query `0x6F28D750`.
* `AIsr` (`CAbilityItemSpellReduction`, slot `0xFC` = `0x6F0F5730`, Unicorn `SR` 350 and `RS` 350): a record with a non-null
  pointer, flag `0x10` clear, row 0 (Spells) and a non-illusion carrier has its two amounts (`+0x10`, `+0x18`) multiplied
  by `1 - DataB` (`0x6F255800`): `spell_reduction_factor`, `scale_record`. Hooked in `Game::unit_damage_target` before the
  target side (the original runs it inside the target side before armor). Stock: Runed Bracers `DataB` 0.33.

## 11. S41: orbs (`AIfb`, `AIob`; others NOT DONE)

`CAbilityDamageBonusBase` family. Pre-damage slot `0x134` (`0x6F0F2B40`): skips records with flag `0x80`, requires the target
filter (`0x6F26D590` with the targs `ground,air,ward`) and no immunity answer, and arms the ability (`+0x20 |= 0x100`) unless the
carrier is an illusion. The post-damage slots `0x14C/0x150/0x154` act only on an armed ability (`0x6F0F2BE0`).

* `AIfb` Orb of Fire (`0x154` = `0x6F0F2CD0`): **corrected in S53 (section 17)**: the area body damages the enemies within `Area`
  except the victim with `[ability+0x88]` (= `DataA`), class `0x4000000`; the `[victim]+0x120` call re-delivers the incoming attack
  record, so the victim gets no second `DataA` (the S41 reading was wrong). Stock: `DataA` 5, `Area` 140.
* `AIob` Orb of Frost (`0x154` = `0x6F108FC0`, Unicorn `FR` 350): the buff `Bfro` (`0x6F1998D0`) with **`HeroDur` for a hero
  victim, else `Dur` (no extra term)** and the carrier's illusion bit: `orb_frost_duration`; the buff is the Frost Attack
  buff (`unit_spells::frost_apply`, Misc `FrostMoveSpeedDecrease/FrostAttackSpeedDecrease`). Stock: `Dur` 3, `HeroDur` 1.
* NOT DONE: `AIpb` Venom (`0x154` = `0x6F0F3170`, registers a callback `0x6F0F3020`), `AIcb` Corruption (`0x6F109B40`, buff
  `BIcb`), `AIzb` Freezing (`0x6F108E20`, buff `Bfre`), the lightning orbs (`AIsb`/`AIlb`) and the Orb of Darkness
  (`AIdn` is an `AIfb` row without damage or area). Their handlers have the same arming and a different effect.

## 12. S41: figurines (`AIfs`, `AIfr`, `AIfu`)

Every figurine row of `AbilityData.slk` has the code `AIfs` (the Skeleton figurine class; `AIfr` Rock Golem and `AIfu` Doom Guard
differ only in their unit-code getters). Effect `0x6F0B8A00` (slot `0x42C`): the unit type of `DataC` (slot `0x6F08C400`) times
`f2i(DataA)` and the unit type of `DataD` (`0x6F08C460`) times `f2i(DataB)` (a type equal to the empty marker `0x5F5F5F5F` is
skipped) go to the summon loop `0x6F0B8900(code, count, &dur, &area)` with `dur = Dur + [ability+0x9C]` (soft add) and `Area`
(CONFIRMED, Unicorn `FG` 350: `figurine_batches`). The loop creates the units for the owner, marks them summoned (`+0x5C |= 0x2000`)
and starts the timed life buff `BFig` with the duration (`0x6F07AD60`). The model places the units at the carrier (**UNSURE**: the
original asks the owner for spots within `Area`) with `apply_timed_life(BFig)`. The loader reads `DataC`/`DataD` as raw
codes (`ItemAbilityData::unit_codes`). The Skeleton figurine: 4 `nsce` + 4 `nsca`, `Dur` 120.

## 13. S41: item auras

Item rows whose `code` is an aura class (`AIae` -> `AOae` Endurance, `AIba` -> `AHab`, `AIcd` -> `AOac`, `AIad` -> `AHad`, `AIau`
-> `AUau`, `AIav` -> `AUav`, `AIar` -> `AEar`: 14 stock items) carry their own level data (the item aura values differ from the hero
spell's). The loader reads `Area`, `DataA..D`, `BuffID` and `targs` into `ItemAbilityData::auras`; the aura system treats the item
ability id as the class (`Game::aura_kind` consults the registered aliases), so taking the item attaches the ability and
`sync_auras` starts the emitter (2.0 s pulse, 0.5 s scan, immediate apply to the carrier); dropping it stops the emitter and the
buffs run out on their 4 s timers. The aura mechanics are those of `auras.rs` (S28 reading).

## 14. S41: the Ankh (`AIrc`) and borrowed spell classes

* `AIrc` (`CAbilityItemReincarnation`, death handler `0x6F1F1D50`): not a spell; when the carrier dies (not an illusion) the
  effect starts with the delay `DataA` (the stock 7.0, same as `AOre`), then **the item uses a charge** (`0x6F2B8390`, so the
  perishable Ankh is destroyed) and the unit gets flag `0x1000`. The model starts the same revival timer as `AOre`
  (`Game::start_reincarnation`) and uses the charge. UNSURE: `DataB` (500, clamped to at least 1.0) and `DataC` (-1, a negative
  value selects a unit stat) feed the effect (probably the restored life and mana), the pathing probe at the death point and
  the other gates; the model revives with full life.
* **Borrowed spell classes** (`item_abilities::DELEGATED`): item rows whose `code` equals a unit spell (`AEbl` Dagger of Escape,
  `AHds`, `AHhb`, `AHmt` Staff of Teleportation, `AHre`, `Arai`, wards `Aeye` `Ahwd`, `Alsh`, `Aprg`, `Aroa`, `AOcl`, `AUdp`, `AUin`)
  and the item classes that inherit a spell class unchanged (`AIrs` -> `AHre`: only the code/order getters are overridden;
  `AIrd` -> `Arai`: the same effect slot `0x424` = `0x6F0E6AA0`, only the autocast gate `0x380` differs; `AIdi` -> `Adis`: the same
  area slot `0x418` = `0x6F0EF320`) run that module's effect with the item's level record: `cast.rs` resolves the alias to the
  class code for the target gates and the effect chain (`effect_base`, `resolve_rec`). LIKELY (class identity from the atlas);
  not Unicorn-run per class. Passive borrowed classes (Bash `AHbh`, Critical Strike `AOcr`, Evasion `AEev`, Slow Poison `Aspo`)
  are NOT done: those modules look the ability up by its class code and its level data, which the item alias does not share.

## 15. S41 status

Unicorn cores (350 rows each, 0 mismatches): `GR` group start, `VI` potion durations, `RL` `RM` `RH` regeneration, `SR` `RS`
spell reduction, `VA` life steal, `FR` frost orb duration, `FG` figurine batches (`item_ability_vectors2.rs`).

NOT DONE or UNSURE after S41: `AIcf` Cloak of Flames (a `CAbilityPermImmolation` child; the passive pulse is driven by
`0x6F12C9B0` messages `0xD01A2`/`0xD01A7`/`0x80261` and a listener on the unit, not decoded); `AIms` (section 3); `AIsi` (no stock
item uses it); `AIgl` Glyph items; `AIbl` build scrolls (`CAbilityItemBuild`, area slot `0x418` = `0x6F107B10`); `AIan` Animate Dead
item (no Animate Dead class in the sim); `AIte` teleport (parent of `AItp`, no stock item); `AItp` stays the section 5 model;
`AIil` illusion, `AIco` command, `AIfa` flare, `AIdm` damage AoE and the remaining orbs; the item-level carry of a cooling group.

## 16. S45: the remaining item classes (41 codes)

Method: class identity and parents from the ability atlas (`docs/research/ability-atlas/factory.tsv`), the overridden slots
of each class (what the child changes relative to its RTTI parent), the stock rows of `AbilityData.slk` / `ItemData.slk`
(1.26a patch) for the data fields, and the existing modules reused through the parent. **No new Unicorn rows in S45**: the
numbers below are models. Everything here is **LIKELY** (class identity, reuse by parentage) or **UNSURE** (the behaviour);
nothing is CONFIRMED beyond what earlier sections covered. Model: `crates/sim/src/item_abilities.rs` (families
`Flag` .. `Unmodelled`), tests `crates/sim/tests/item_abilities3.rs` (they pin the model, not the original).

| Code (class) | Stock items | Model | Label |
|---|---|---|---|
| `AIfd` `AIff` `AIfg` `AIfh` (figurine children, override only slot `0x42C` = the unit-code getter; `AIfg` is the base) | none directly (rows use `AIfs`) | the section 12 figurine (effect slot `0x428` = `0x6F0B8A00`, Unicorn `FG`) | LIKELY |
| `AIfl` `AIfe` `AIfm` `AIfn` `AIfo` (`CAbilityFlag < CBonusBase`; children override only code/name getters) | capture flags (`flag`, `nflg`, `oflg`, `uflg`, `btst`) | passive marker, no stat effect (`AIfl` overrides the bonus slots `0x80`/`0x88` only) | LIKELY |
| `AIpi` (`CAbilityItemPermInvis < CAbilityPermInvis`) | none | permanent `Binv` marker buff while carried | UNSURE |
| `AIcf` (`CAbilityItemCloakOfFlames < CAbilityPermImmolation`, override `0x348`) | `clfm`, `shdt`: `DataA` 10, `Area` 160, `Dur` 1 | permanent buff `BIcf` and a repeating tick every `Dur` (1.0 if none): `DataA` Spells-row damage to enemies in `Area` of the carrier; stops with the item. The original drives the pulse from `0x6F12C9B0` messages (not decoded) | UNSURE |
| `AIcb` `AIpb` `AIlb` `AIzb` (`CAbility*DamageBonus < CAbilityDamageBonusBase`; post-hit slots `0x14C`/`0x150`/`0x154` differ) | `ocor`, `oven`, `olig`, none | after a basic hit on an enemy: `AIcb` buff `BIcb` armor `-DataA` for `HeroDur`/`Dur` (5 s); `AIpb` poison buff `Bpoi` (`DataA` per second, class 23, **5.0 s stand-in, the row has no duration**); `AIlb` `DataA` Spells-row damage; `AIzb` the Frost buff of `AIob` (no stock `Dur`, so nothing happens) | UNSURE |
| `AIlp` (`CAbilityLightningPurge < CAbilityPurge`), `AIsb` (`CAbilitySpellEffectBonus`, casts the spell named by `UnitID` on a hit with chance `DataB`) | `olig`, `odef`/`oli2`/`oslo`/`shcw` | **done in S53 (section 17)**: `AIlp` is the Purge effect, `AIsb` a pseudo-random proc on stream 36 | CONFIRMED |
| `AIdd` (`CAbilityPassiveDefense`, damage handlers `0xFC`/`0x134`/`0x160`; `DataA` 0.7 in `arsh`, 0.65 in `Aegr`) | `arsh` | **done in S53 (section 17)**: Pierce row times `DataA`, Spells and Magic rows times `DataE` | CONFIRMED |
| `AIda` (`CAbilityItemDefenseAoe`, area `0x428` = `0x6F0B6320`, callback `0x6F0B61D0`) | `spro` (`DataA` 2) and the `AIdb` row of `arsc` (`DataA` 2, `DataB` 150, `DataC` 100); `Area` 600, `Dur` 30 | **V3 correction, CONFIRMED (`DA`, 300 Unicorn rows, `tools/emu/v3_cores_emu.py`, test `scroll_of_protection_callback_matches_the_original`)**: the callback heals as well: a candidate that is not an illusion gets the life adder with `DataB` and the mana adder with `DataC` when `0 < DataB` or `0 < DataC` (x87 compares); a candidate not immune to the order gets the buff `Bdef` with `DataA` armor and the hero-rule duration (`HeroDur` for a hero, else `Dur` + the ability's `+0x9C`). Illusions get the buff but no heal. The old model ignored `DataB` / `DataC` and skipped illusions; `item_abilities::defense_callback`. Not modelled: the immunity answer `0x6F28D750`; the group builder's relation / `targs` filter (UNSURE, friends and the caster) | CONFIRMED (callback); area membership UNSURE |
| `AIdm` (`CAbilityItemDamageAoe`, `0x418`), `AIca` (`CAbilityItemCureAoe`, `0x418`) | none | `AIdm`: `DataA` damage to enemies in `Area`; `AIca`: negative dispellable buffs end on allies in `Area` | UNSURE |
| `AIta` `AItb` (detect aoe / by buff) | `crys`, `dust` | marker buff `Bdet` on the enemies in `Area` for the hero-rule duration (no detection model exists to read it) | UNSURE |
| `AIfa` (`CAbilityItemFlare`), `AIrv` (`CAbilityItemRevealMap`) | `fgun`, `pomn`/`gomn` | a visible fog modifier over `Area` around the target point (`AIrv`: a huge circle) for `Dur`, stopped by a one-shot timer | UNSURE |
| `AIct` (`CAbilityItemChangeTOD`, `EfctID` `XIct`) | `moon` | **done in S53 (section 17)** | numbers CONFIRMED, end of the effect UNSURE |
| `AIgl` (`CAbilityGlyph < CAbilityTome`, effect `0x428` = `0x6F082300`) | `gfor`, `guvi`: `UnitID` `Rgfo`/`Rguv`, `DataA` 1 | `AddPlayerTechResearched(owner, UnitID, f2i(DataA))` (reads level 1) | UNSURE |
| `AIbl` (`CAbilityItemBuild`, `0x418` = `0x6F107B10`) | 8 scrolls: `UnitID` = four units, one per race | the entry of the owner's race (1 human .. 4 night elf) created at the point, finished at once (`Dur` = construction time not modelled). Needs `ItemAbilityData::unit_lists` (the loader reads the `UnitID` list) | UNSURE |
| `AIpm` (`CAbilityItemPlaceMine`, `0x418`) | `gobm`: `UnitID` `nglm` | the unit created at the point for the caster's player | UNSURE |
| `AIco` (`CAbilityItemCommand`) | `ccmd`: `DataA` 5, `Rng` 750, `targs` non-hero enemy/neutral | the Charm body: owner change + stop when the creep level is at most `DataA` | UNSURE |
| `AIil` (`CAbilityItemIllusion`, `0x424` = `0x6F13DFD0`) | `will`: `Dur` 60, `DataB` 2 | a copy of the target (or the caster) as an illusion for `Dur`; damage dealt `DataA`, taken `DataB` | UNSURE |
| `AIpv` (`CAbilityItemVampirePotion`, `0x428`) | `vamp`: `Dur` 20, `DataA` 12, `DataB` 0.75 | **done in S53 (section 17)**: buff `BIpv`, `F2I(DataA)` damage and `DataB` life steal | CONFIRMED |
| `AIan` (`CAbilityItemAnimateDead < CAbilityAnimateDead`) | `sand`, `scul`: `Dur` 40, `Area` 900, `DataA` 6 | up to `DataA` dead non-hero units in `Area` revive under the caster's player for `Dur` (query order, placement at the corpse) | UNSURE |
| `AIrt` (`CAbilityItemRecall`, `0x418` = `0x6F06A870`), `AIte` (`CAbilityItemTeleport`, `0x418` = `0x6F060C10`, the parent of `AItp`) | `amrc`: `Area` 700, `DataA` 12 | the Town Portal model of section 5 (`DataA` count, `DataB` clustering) | UNSURE |
| `AIsh` (`CAbilitySpiritTroll < CAbilitySpiritWolf`, overrides code/order getters and `0x3E4`), `AImo` (`CAbilityItemMonsterLure < CAbilityWard`), `AIwb` (`CAbilityItemWeb`, own `0x424` = `0x6F0AB010`), `AIdc` (`CAbilityItemDispelChain`, own `0x424` = `0x6F102490`) | `axas`, `lure`, `silk`, `wneu` | run the unit spell module of the parent (Feral Spirit, the ward effect, Web, Dispel Magic) with the item's level record: `item_abilities::borrowed_class` | UNSURE (the last two override their own effect, so numbers may differ) |
| `AIfc` (`CAbilityItemFlyingCarpet`), `AIso` (`CAbilitySoulTrap`), `AIri` (`CAbilityItemRandomItem`), `AIrb` (`CAbilityRebirth < CAbilityTome`, `0x428` = `0x6F0F4EA0`) | `gsou`, `rreb` | not registered in Rust; **NOT DONE** | NOT DONE |

Other changes: `item_abilities::run_effect` takes the runtime (death events of S45 kills, the owner-change event of `AIco`);
`natives::misc::add_tech` is `pub(crate)`; `orc.rs` gets `Game::register_illusion`; the engine loader keeps the whole
`UnitID` list of a row (`ItemAbilityData::unit_lists`). Known simplification: two items giving the same marker buff
(`AIpi`) share it, and dropping one removes it.

## 17. S53: the last AI* codes and Unicorn confirmation of the item models

Harness `tools/emu/item_cores3_emu.py` (rows `crates/sim/tests/item_ability_vectors3.txt`, 350 per kind, 3,150 rows, 0
mismatches, tests `item_ability_vectors3.rs`), behaviour tests `item_abilities4.rs`. The tracer got `item_trace_emu.LAST_TRACE`
(the call trace is kept when a run raises).

**Level data getters** (checked with distinct values): `0x6F02E760` Dur, `0x6F02E7D0` HeroDur, `0x6F02E900` Area,
`0x6F02E9E0` DataA, `0x6F02EA50` DataB, `0x6F02EAC0` DataC, `0x6F02EB30` DataD, `0x6F02EBA0` DataE, `0x6F02EC10` DataF,
`0x6F02EC80` DataG, `0x6F02ECF0` DataH, `0x6F02ED60` DataI. (Static readings that took `0x6F02EA50` for DataA are shifted by one.)

| Code | Class / slots | Model | Label |
|---|---|---|---|
| `AIct` Moonstone (`moon`: `Dur` 30, `Cool` 70, `EfctID` `XIct`) | `CAbilityItemChangeTOD < CAbilitySpell`, `0x3C0` = `0x6F109EB0` | time of day `DataA + DataB / 60` (soft divide, soft add), duration `extra + Dur` (no hero rule), started only when the duration is above 0.0 (`0x6F104570` finds the effect object and calls `0x6F0F5690`: `SetFloatGameState(TIME_OF_DAY)` + one-shot timer `0xD01C1`). Expiry `0x6F0F56D0`: time of day := saved value (`+0x28`), `0x6F429920(.., 2)` (resume) | numbers **CONFIRMED** (`CT` 350); the saved value (taken at the first cast) and the clock suspension while it lasts **UNSURE** |
| `AIpv` Potion of Vampirism (`vamp`: `Dur` 20, `HeroDur` 20, `DataA` 12, `DataB` 0.75, `BuffID` `BIpv`) | `CAbilityItemVampirePotion`, `0x428` = `0x6F109F70` (carrier) | buff `BIpv` for the hero-rule duration with `F2I(DataA)` flat damage (holder `+0xD4`/`+0xD8`, getters `0x6F0FB6C0`/`0x6F0FB750` over the weapons) and life steal `DataB` (holder `+0xCC`/`+0xD0`); on a hit the buff heals `ratio * damage` (`0x6F0FB7D0`: victim not mechanical, enemy relation, damage above 0, illusion gates) | apply arguments **CONFIRMED** (`PV` 350), hit handler gates **CONFIRMED** (`VP` 350); the Misc keys at `0x6F8B4A5C`/`0x6F8B288C` read as off, buff flags UNSURE |
| `AIdd` Arcanite Shield (`arsh`: `DataA` 0.7, `DataE` 1) | `CAbilityPassiveDefense`, target-side handler `0xFC` = `0x6F098F90` | record amounts times `DataA` on the Pierce row (2), times `DataE` on the Spells and Magic rows (0, 4); the armed branch (`DataG`/`DataH`, flag `0x200`) is 1.0 in all stock rows. Hooked in `damage::side_of` (basic attacks) and `item_abilities::spell_reduction` (natives) | **CONFIRMED** (`DD` 350); the arming slot `0x160` is not modelled |
| `AIlp` Lightning Purge (`olig`, row `Dur` 3, `HeroDur` 0.3, `DataA` 2, `DataC` 35) | `CAbilityLightningPurge < CAbilityPurge`, unit effect `0x424` = `jmp 0x6F107E10` | the Purge effect of `unit_spells` (`borrowed_class`) | **CONFIRMED** (identical code) |
| `AIlb` Orb of Lightning (old) | `0x154` = `0x6F0F2F70` | **replaces the S45 damage model**: on an armed hit the carrier's `AIlp` ability is looked up (`'AIlp'` = `0x6F0DBE00`), and, unless its blocked query (slot `0x42C`) answers yes, its unit effect runs on the victim | **CONFIRMED** (`LB` 350); the blocked query is read as no |
| `AIfb` Orb of Fire | `0x154` = `0x6F0F2CD0` | area body `0x6F2C31D0` with one shape `{Area, DataA}`, count 1, class `0x4000000`, flags `targs | 0x10000`, source = the carrier, callback `0x6F0DC570` (everything except the victim) and the victim as context; then the same record to the victim (`flag = record flags & 0x100`) | **CONFIRMED** (`FB` 350) - the S41 "how the area is used" question is closed |
| `AIzb` Orb of Freezing | `0x154` = `0x6F108E20` | **replaces the S45 Frost model**: the stun buff `Bfre` (`CBuffFreeze < CBuffStun`) for the hero-rule duration; no stock row has a duration | **CONFIRMED** (`ZB` 350) |
| `AIcb` Orb of Corruption (`ocor`) | `0x154` = `0x6F109B40` | buff `BIcb` for the hero-rule duration with amount **`DataB`** (0.0 for an illusion carrier, which never arms anyway); the buff takes it off the victim's armor unless the victim is an illusion (`0x6F0F3A90`) | **CONFIRMED** (`CR` 350) |
| `AIms` Boots of Speed (`bspd`) | `CAbilityMoveSpeedBonus`, slot `0x184` = `0x6F0FF220` (returns `+0x88` = `DataA`) | the unit's flat bonus is the **maximum** over its abilities (`0x6F077590`, start 0.0, replace when below or unordered), added to the base before the multiplier (`0x6F2019F0`): boots do **not** stack. The Misc `MoveSpeedBonusesStack` branch of apply/remove stays off | **CONFIRMED** (`MS` 350) |

**More findings of S53 (all Unicorn, `item_cores3_emu.py`, 350 rows per kind):**

| Code | Result | Label |
|---|---|---|
| `AIsb` Orb of Slow / Lightning / Darkness / Shaman Claws (`CAbilitySpellEffectBonus`, slot `0x124` = `0x6F0F3DD0`, row `UnitID` names the spell: `AIos` Slow, `AIpg` Purge, `ANbs` Black Arrow) | the victim kind (`0x6F0E5080`: first character of the type code upper-case = hero 1, else flag `+0x248` bit 2 = summoned 2, else 0) picks the chance in percent: `DataB` normal, `DataC` hero, `DataD` summoned (Orb of Slow 15 / 5 / 35). The roll is the **pseudo-random distribution of Bash** (`human_heroes::prd_effective_chance`: table constant of the chance times the number of rolls since the last proc, one counter per kind in the ability at `+0xD0/+0xD4/+0xD8`, start 1) against RNG stream `0x24` (36): proc iff `draw <= effective` (`0x6F01FD60`); a proc sets flag `0x400` and resets the counter, a miss adds 1. On the post-hit slot `0x154` (`0x6F0FA3C0`) the child ability gets the victim as target and its end-cast `0x6F05C840` runs: the model casts the child's effect on the next timer tick (`TICK_PROC`, `cast::run_effect` with the child's class code and level). The engine loader reads the `UnitID` row as an alias. `ANbs` (Black Arrow) is not modelled by the sim, so Orb of Darkness procs do nothing yet | rolls and counters CONFIRMED (`SB`); the one-tick delay and the missing child gates (range, mana, cooldown) UNSURE |
| `AIil` Wand of Illusion (`will`) | the buff arguments are the hero rule duration of the TARGET (`Dur + extra` for non-heroes), `DataA` (damage dealt factor), `DataB` (damage taken factor), the caster (`IL` 350). Stock: `DataA` 0, `DataB` 2 | arguments CONFIRMED; spawn spot `0x6F1D26B0` and buff flags UNSURE |
| `AIwb` Web scroll | the unit effect `0x6F0AB010` is **byte-identical** to `CAbilityWeb`'s `0x6F0AADD0` (170 instructions, the only difference is the SEH table pointer): the borrowed Web model is the original code | CONFIRMED |
| `AIdc` Wand of Neutralization | `0x6F102490` is **not** Dispel Magic's area effect `0x6F0EF320`: it is a separate function (area `Area` 400, count `DataC` 8, relation flags from the target: `0x10000` enemy, `0x7000` friend, else `0x18000`), the creep spell `Ache` Chain Dispel has the same row. The S45 Dispel Magic model is only a stand-in | UNSURE (decoded, not modelled) |
| `AIco` Staff of Domination | effect `0x6F1CF8F0`: an illusion target (`+0x5C & 0x40000000`) gets its vtable `0x124` called with 0.0, any other target changes owner to the caster owner's player (`0x6F2A4BE0`); the validator `0x6F1CF8B0` hands `DataA` to the Charm level rule `0x6F1A2600` | owner change CONFIRMED structure; the illusion branch (not modelled) and the level rule UNSURE |
| `AIpb` Orb of Venom | `0x154` only registers a callback (`0x6F0F3020` through `0x6F076700`). The poison is the item's second ability **`Apo2`** (`Dur` 6, `DataA` 9, buff `BIpb`), applied by the poison attack module. The S45 stand-in (5 s `Bpoi`) is removed | LIKELY (callback body not decoded) |
| orbs, persistent part | `CAbilityDamageBonusBase::0x80` (`0x6F0FD580`) calls the attack bonus apply (`0x6F0FA7F0` -> `0x6F0F4100`, the `AIat` body), so every orb (`AIfb AIob AIcb AIpb AIlb AIzb AIsb`) also adds `F2I(DataA)` to the attack damage while carried (Orb of Frost +6). Added in S53 (`apply_passive`) | CONFIRMED structure (apply chain read); no Unicorn row for the apply itself |
| `AItp` / `AIrt` Town Portal / Recall | `0x6F0610D0` runs the area group builder `0x6F032E30` with the callback `0x6F0264B0`, which keeps the candidate nearest to the clicked point (squared cell distance, replaced only when strictly smaller; `[ctx+4]` stops after the first); the clicked unit is only the reference point of that search. The teleport `0x6F060C10` gathers units with the Mass Teleport flag word `0x40303006`, limit `f2i(DataA) - 1` (the Unicorn-confirmed `teleport_select` of Mass Teleport is the rule used), `DataB` is gated by the Misc key at `0x6F876AEC` | structure CONFIRMED; the nearest-destination search and the cluster direction not run (the model teleports to the clicked unit) - UNSURE |
| `AIcf` Cloak of Flames | slot `0x348` (`0x6F13B960`) hands `DataA`, `Area` and `Dur` to the buff apply `0x368` (`0x6F12C4D0`, a `CBuffImmolation < CBuffLightningShield`) which starts a 0.1 s timer family (`0xD01BA`); the damage tick of the buff was not decoded | UNSURE (unchanged) |

**NOT DONE after S53** (decoded, no behaviour, no stock row or too deep): `AIfc` (`CAbilityItemFlyingCarpet`: `0x428` finds the unit's
`'Amou'` ability and calls `0x6F0292A0(.., 0)`; no `AbilityData` row), `AIri` (`CAbilityItemRandomItem`: replaces the used item by a random
item of the same level through `ChooseRandomItemEx(any)` on stream 35 and `0x6F2B91D0`, excluding its own type; no stock row), `AIrb`
(`CAbilityRebirth < CAbilityTome`, `0x428` = `0x6F0F4EA0` creates a unit of the type held at `+0xCC` of the carrier at its position;
`rreb`), `AIso` (`CAbilitySoulTrap`, `gsou`: adds `CAbilitySoulTrapped` to the target hero, its effect lives in the revive path),
`AIdc`, the `AIdd` arming slot `0x160`, the Orb of Darkness child (`ANbs`).

## 18. V3 verification wave (Unicorn, `tools/emu/v3_cores_emu.py`, vectors `crates/sim/tests/v3_vectors.txt`)

Every row is the ORIGINAL code on fake objects (callees that need the object model stubbed and recorded). 300 rows per kind,
0 mismatches after the fixes listed. Tests: `crates/sim/tests/v3_vectors.rs` (the soft-division ones need `GAME_DLL`).

| Kind | Original | Model | Result |
|---|---|---|---|
| `DA` | Scroll of Protection callback `0x6F0B61D0` | `item_abilities::defense_callback` | CONFIRMED; **bug fixed**: `DataB` life and `DataC` mana were ignored, illusions were skipped from the buff |
| `MT` | Rune of Mana area callback `0x6F1269E0` (`AImr`) | `mana_area_callback` | CONFIRMED; **bug fixed**: the mana families use `IllusionsCanRestoreMana`, skip units with `|max mana| < 2e-7`; the heal rule was used for all three |
| `RT` | Restoration area callback `0x6F1E6130` (`AIra`) | `restore_area_callback` | CONFIRMED; **bug fixed**: an illusion is skipped only when a non-zero amount meets its Misc key off (life with `IllusionsCanRestoreLife`, mana with `IllusionsCanRestoreMana`) |
| `RG` | Regeneration area callback `0x6F108800` (`AIrg`) | `regen_area_callback` | CONFIRMED (callback only, see the Regeneration section) |
| `CB` (S36) | Heal area callback `0x6F0F0470` (`AIha`) | `area_callback_amount` | unchanged, CONFIRMED earlier |

The slot `0x42C` the three restore callbacks call answers 0 for all three classes (`xor eax,eax`), so the ethereal branch
(`+0x158 > 0` times a Misc factor) is dead for items. Stock data (`ItemData.slk` / `AbilityData.slk` of the 1.26a patch): `AIda` rows
`AIda` (`spro`: `DataB` = `DataC` = 0) and `AIdb` (`arsc`: 150 / 100); `AIdm` (damage aoe) has **no stock row** (`item` 1, all data `-`).
Stock coverage of the item base codes (count of items): `AIab` 36, `AIde` 21, `AIat` 20, `AIfb` 13, `AIrg` 11, `AIfs` 8, `AIbl` 8, `AImm` 7,
`AIml` 6, `Arel` 6, `AIhe` 5, `AIha` 5, `AIsb` 4, then single-digit counts.

## 19. V6 verification wave (Unicorn, `tools/emu/v6_emu.py`, vectors `crates/sim/tests/v6_vectors.txt`)

* **Confirmed rows added** for every item core of S36 / S41 / S53 that already carried a 0-mismatch run (`docs/spec/confirmed-abilities.tsv`):
  `AIhe AIma AIre AIha AImi AImn AIem AIaa AIlm AIxm AIam AIim AIsm AIab AIat AIde AIas AIml AImm AIsp AIvi AIvu Arel AIrm AIva AIsr AIob AIfs
  AIct AIpv AIdd AIlb AIfb AIzb AIcb AIms AIsb AIil` (plus the earlier `AIda AIdb AImr AIra AIrg`). Each row cites its harness kind and the
  case count of the vector file.
* **`AItp` Town Portal destination step** (`TN`, 350 rows, 0 mismatches): callback `0x6F0264B0` = `item_abilities::town_portal_step`: a candidate
  the ability's slot `0x3EC` refuses changes nothing and the search goes on; otherwise the squared distance to the clicked point (soft subtract,
  two soft multiplies, soft add) replaces the kept one when none is kept or when it is strictly smaller (a NaN never replaces); with the `first`
  flag the search stops after the first candidate that passed the refusal test. The refusal predicate, the gathering of the teleported group
  and the cluster direction are not covered; the cast model still teleports to the clicked unit (UNSURE).
* Dagger of Escape, Staff of Teleportation, item auras: the item rows are alias rows of the confirmed base classes (`AEbl`, `AHmt`, and `AIae
  AIba AIcd AIad AIau AIav AIar` -> `AOae AHab AOac AHad AUau AUav AEar`), so they run the confirmed code with their own level data.
* Not verifiable in this wave (decoded, see `abilities-neutral.md` s9): Ankh `AIrc`, Cloak of Flames `AIcf`, the wands `AIdc` (separate
  area function `0x6F102490`, not Dispel Magic) and `AIwb`.
