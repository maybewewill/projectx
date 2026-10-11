# AI natives (table `ai`): status and blockers

Scope: the 124 natives of table `ai` (`docs/spec/natives-1.26a.tsv`, registered by `0x6F2E3D90`) and the four
main-VM AI natives (`StartMeleeAI`, `StartCampaignAI`, `CommandAI`, `PauseCompAI`). The model is in
`crates/sim/src/ai.rs` and `crates/sim/src/natives/ai.rs`. Levels: CONFIRMED (the body shows the value or effect),
LIKELY (the mechanism is read, a detail is inferred), UNSURE (not fully decoded, or the model chose the value).

Behaviour is written in plain words. The bodies were read in a Ghidra decompile kept outside the repo.

## 1. Counts (2026-10-11, after lane G)

| | Count |
|---|---|
| AI natives in table `ai` | 124 |
| Bound (`implemented: true`) | 97 |
| Not bound (`implemented: false`, logged and return the default) | 27 |
| Level UNSURE in total | 38 (27 unbound + 11 bound: the 7 older ones, 4 captain/threat idle values) |
| Level CONFIRMED / LIKELY (all bound rows, 128 rows incl. the 4 main natives) | 43 / 47 |
| Raised to CONFIRMED by a Unicorn run | 0 as a row label; the bodies of 5 town natives are Unicorn-checked (section 6) |

The 27 unbound natives are pinned by a test (`unbound_ai_natives_are_pinned`). A change in the list has to change
that test on purpose. Lane G bound 9: the five town natives (section 6) and four captain/threat reads (6.4).

## 2. The unbound natives and what they need (written for the 36 of the first pass; 9 are bound now, see section 6)

Every body first takes the AI state of the calling player (`FUN_6f4786b0`, the agent of the player's AI), and then
calls one of the routines below. The "needs" column names the state or subsystem that the model does not hold.

### 2.1 AI town and expansion model (map AI data, town slots)
Towns are the four town slots of the AI state. Each slot holds a town hall, mines, peons and an expansion site.
None of this is in the sim, and the expansion table is filled from the map's AI data.

- `GetGoldOwned` (`0x6F2DE000`): sums a value over the town slots (`FUN_6f2d1210`), not the player's gold.
- `GetMinesOwned` (`0x6F2DDFD0`): counts the mines of the town slots (`FUN_6f2d11a0`).
- `TownWithMine` (`0x6F2DE030`): index of the first town with a mine (`FUN_6f2d5380`).
- `TownHasMine(i)` (`0x6F2DE060`): mine test on slot `i` (`FUN_6f2d12b0`).
- `TownHasHall(i)` (`0x6F2DE0A0`): hall test on slot `i` (`FUN_6f2d1310`).
- `GetTownUnitCount(type, town, flag)` (`0x6F2DD0C0`): units under construction (`FUN_6f2d1c10`) plus the units of the
  type in the given town (`FUN_6f2d5430`, which walks the town's slot list).
- `GetNextExpansion` (`0x6F2E2BC0`): next expansion index (`FUN_6f2e10d0`).
- `GetExpansionPeon` (`0x6F2DD530`), `GetEnemyExpansion` (`0x6F2DD4F0`), `GetExpansionFoe` (`0x6F2E2E40`): town and
  expansion lookups (`FUN_6f2d1f40`, `FUN_6f2d1e50`, `FUN_6f2e15c0`), each returning a unit handle.
- `GetExpansionX` (`0x6F2E2DA0`), `GetExpansionY` (`0x6F2E2DF0`): the expansion record's centre (`FUN_6f2e1560/1590`).
- `SetExpansion(unit, index)` (`0x6F2E0570`): expansion placement (`FUN_6f2d8130`).

### 2.2 Captain groups (captain record `+0x6C` is modelled; the group is not)
The captain record (`FUN_6f2cfeb0`) holds the flag word (modelled) and a group size at `+0xBC` (not modelled, no
writer found).

- `CaptainIsEmpty` (`0x6F2DDB50`): true when the group size at `+0xBC` is 0.
- `CaptainGroupSize` (`0x6F2DDB90`): the value at `+0xBC`.
- `CaptainReadiness` (`0x6F2DDA40`), `CaptainReadinessHP` (`0x6F2DDA90`), `CaptainReadinessMa` (`0x6F2DDAD0`):
  readiness of the group from its units (`FUN_6f2e91a0`, flags 1/1, 1/0, 0/1).
- `CaptainIsHome` (`0x6F2DD870`): the group's home test (`FUN_6f2d1080`, which calls the captain move test
  `FUN_6f2e76e0`). The body has no return value of its own; the boolean comes from that test (UNSURE).
- `CaptainInCombat(flag)` (`0x6F2DD8A0`): captain combat state (`FUN_6f2d0ff0`).
- `CaptainAtGoal` (`0x6F2DD930`): the goal point test (`FUN_6f2d1090`).

### 2.3 AI threat and goal records
- `TownThreatened` (`0x6F2DD8E0`): two town threat masks (`+0x164`, `+0x168`) of the AI state; the test is
  `mask168 & mask164 != 0xFFFFFFFF` and a non-null AI state. The masks have no writer in the model.
- `WaitGetEnemyBase` (`0x6F2DDCF0`): reads the enemy-base result at `+0x2EC` of the AI state (no writer; the
  search `FUN_6f2db410` is not modelled, and the original's timing of it is open).
- `GetEnemyBase` (`0x6F2DDD20`), `GetMegaTarget` (`0x6F2DDD60`): base and mega-target lookups (`FUN_6f2d1cd0`,
  `FUN_6f2d6500`), each a masked read of an AI state field plus an agent-to-unit conversion.

### 2.4 Scans through the group builder (callback decoded, filter not)
The group builder `FUN_6f2a3270` takes a callback, a user word, a search mode and a range source. The callbacks are
not exported as functions by the first analysis; they were decompiled in a fresh import (see section 4).

- `CreepsOnMap` (`0x6F2DC0F0`): the builder runs with a fixed mode and type (0x1B, 8, 0x1F). Its callback
  (`0x6F2DC0E0`) sets a flag on the first unit it is given and returns 0, so the result is "any unit matched". The
  builder's match rule (which units the mode 0x1B/8 walk accepts) is not decoded.
- `GetBuilding(player)` (`0x6F2E2E80`): the builder runs over the player's buildings. The callback (`0x6F2DC120`)
  calls `FUN_6f0787d0` with the code `Buns`; when that call gives 0 it stores the unit and returns 0 (stop), otherwise it
  returns 1 (continue). `FUN_6f0787d0` is not decoded.
- `IgnoredUnits(type)` (`0x6F2DCE80`): the builder runs with the AI's player and a type. Its callback (`0x6F2D3360`)
  counts the units that match none of three masked tests (`+0x30/+0x34`, `+0x3C/+0x40`, `+0x48/+0x4C`), using
  `FUN_6f275d90` and `FUN_6f4786b0`. The three masks are the unit's order and state words, and the builder's rule is
  not decoded.
- `IsTowered(unit)` (`0x6F2E0950`): counts the units near `unit` with an attack ability at `+0x1E8` and a predicate
  `FUN_6f0c6f70`; the radius comes from the unit's vtable slot `0xEC` with the constant 1500.0. Result is "count
  greater than 1". Blocked by the builder's rule, the predicate, and vtable slot `0xEC`.
- `GetCreepCamp(type, x, y)` (`0x6F2E3D50`): picks the best camp from the camp list (`FUN_6f2e3980`), using distances
  and the camp's record. The camp records are map data that the sim does not load.

### 2.5 Vision gate
- `UnitInvis(unit)` (`0x6F2E07C0`): the native returns the negation of `FUN_6f3a38f0(player, unit, 1, 4, 0)`. With
  that flag set the body calls the gate `FUN_6f3a15f0`: a unit that does not have flag `0x1000000` at `+0x5C` passes
  (so it is not invisible), and a flagged unit passes only when its owner is in the player's 16-bit mask at
  `+0x2E0` (from `FUN_6f3a1650`) or in the mask `FUN_6f27a380(1)`. The per-player record `FUN_6f3a1650` and the mask
  `FUN_6f27a380` are not decoded. (The sim's vision grid is a stand-in for unit sight, so even the cell path would not
  be exact yet.)

### 2.6 Hero records
- `GetHeroId` (`0x6F2DDF50`): finds the AI's hero through `FUN_6f0db4a0` (a key) and `FUN_6f0787d0` (the hero search),
  then compares owners; falls back to the AI state's field at `+0x30`. Both helpers are not decoded.
- `GetHeroLevelAI` (`0x6F2DE0E0`): `(FUN_6f473170() - hero+0x90) + 2` on the AI's hero record (`+0x1F0`). The hero
  record and `FUN_6f473170` are not decoded.

### 2.7 Production and upgrade costs
- `GetUnitBuildTime(type)` (`0x6F2DD190`): searches the player's production records (`FUN_6f2d2fb0`) first, then the
  type data. The production records are not modelled.
- `GetUpgradeGoldCost(tech)` (`0x6F2DD2A0`), `GetUpgradeWoodCost(tech)` (`0x6F2DD2C0`): base plus increment times
  (level minus 1), from the upgrade cost records (`FUN_6f2afaa0/6f2afb20` gold, `FUN_6f2afba0/6f2afc20` wood). The
  level comes from `FUN_6f2dd130`, which adds 1 for an in-progress research (`FUN_6f40b310`). The upgrade cost
  records are not loaded by `gamedata`.

### 2.8 Captain group helpers
Covered in 2.2.

## 3. The bound UNSURE rows (7)

These are bound, but their bodies are not fully decoded:

- `GetUnitCount` and `GetUnitCountDone`: the AI's own-unit walks (`FUN_6f2d1c10` under construction, `FUN_6f2d5430`
  town walk). The model counts live owned units.
- `SetProduce`, `MergeUnits`, `ConvertUnits`, `AddAssault`, `AddDefenders`: the requests (`FUN_6f2d8130`, `FUN_6f2d2580`,
  `FUN_6f2d2440`, `FUN_6f2d96f0`, `FUN_6f2d96d0`) are not modelled. `ConvertUnits`, `AddAssault` and `AddDefenders`
  have bodies with no return value; the value of the boolean in the original is whatever the register holds at
  return, which the model cannot reproduce (returns false).

## 4. What this pass checked

- Decompile of the 36 unbound bodies and of their direct callees that are exported (the chunk files).
- Four callbacks that are not exported as functions (`0x6F2DC0E0`, `0x6F2DC120`, `0x6F2DDC90`, `0x6F2D3360`), and
  `0x6F2D0FF0`, `0x6F3A38F0`, `0x6F2E3980`, `0x6F2D6500`, `0x6F2E15C0`, `0x6F2E1560`, `0x6F2E1590`, `0x6F2D1CD0`,
  `0x6F2D1080`, decompiled in a fresh Ghidra import with function creation at those addresses (no analysis pass;
  the scripts live in the scratchpad, not in the repo).
- No Unicorn run. The bodies of the 36 all reach state (town slots, captain records, expansion records, production
  records, upgrade cost records, hero records, vision per-player records) that is not in the model, so there is no
  isolated function to run without a harness for that state. The 7 bound UNSURE rows have the same dependencies.

## 5. Plan for the 36

1. Town and expansion model (2.1, 4 natives on towns, 8 on expansions/bases): needs the map AI data and the town
   slot record. Lives in the engine's map load, not in `ai.rs`.
2. Captain groups (2.2): needs the group membership and its size, and the group's readiness and home test
   (`FUN_6f2e91a0`, `FUN_6f2e76e0`).
3. Group-builder rule (2.4): decode the builder's match rule (`FUN_6f2a3270` mode 0x1B/8/0x1F, 0x18, 0x17, 0x1C, 0x11)
   and the callbacks' helpers. This unlocks CreepsOnMap, GetBuilding, IgnoredUnits and IsTowered together.
4. Upgrade cost records (2.7): load the upgrade cost data in `gamedata` (outside the AI files).
5. Hero records (2.6) and the vision per-player record (2.5) once the hero and vision models are exact.

## 6. Lane G: running melee AIs, and the town model (2026-10-11)

### 6.1 What `w3boot` does now
`w3boot --computer SLOT[:human|orc|undead|nightelf] [--ai-log N]` makes a slot a computer player before the first
step (the player's raw controller and race are set on the game; `main` runs at the first step and the melee start
calls `StartMeleeAI`). On `(2)Circumvention.w3x` with real `common.ai` and the four race scripts, for 12,000 steps
(20 game minutes), all four races start (`human.ai`, `orc.ai`, `undead.ai`, `elf.ai`), 0 aborted threads, 0 AI natives
called but not implemented. The peon thread (`peon_assignment`, `HarvestGold(TownWithMine(),..)`), the build loop
(`SetProduce`, ...) and `PlayGame`'s `SleepForever` run. The AI does not progress past that: its orders
(`HarvestGold`, `SetBuildUnit`, ...) are only logged, nothing builds or trains, so the later paths (expansions,
attack waves, hero logic) are not reached yet. That is the next blocker, not a missing native.

Two compiler fixes were needed before any race script even started (`BuildFailed` events):
* `common.ai` has `native Sleep` and `constant integer SLEEP` (same name, case-insensitive): a native may now share a
  name with a global (`Ctx::declare_native`).
* `undead.ai` has `function undead_mine` and `common.ai` has `constant integer UNDEAD_MINE`: a function may now share
  a name with a *constant* global. UNSURE: the emulated original compiler (`tools/emu/jasscomp`, entry `0x6F458900`
  with the AI caller's arguments) rejects both pairs, but it also rejects every `native` line (the native registry
  of that harness is not set up), so its answer is not decisive; the retail scripts do run in the retail game, which
  is the evidence used. A function that reuses the name of a *plain* global stays an error (harness o17, o20, q05).

### 6.2 The town model (`crates/sim/src/ai_towns.rs`)
CONFIRMED structure (decompile of `0x6F2D11A0`, `0x6F2D1210`, `0x6F2D1310`, `0x6F2D12B0`, `0x6F2D5330`, `0x6F2D5380`,
`0x6F2DEC10`, `0x6F2D0C00`):
* One map-wide list of **town sites** (array `0x6FAB4E90`, count `0x6FAB4E74`). A site record holds a hall reference
  at `+0x48`, mine references at `+0x54` and `+0x60`. A reference is an (id, tag) pair; it resolves when the agent
  exists and its removed flag is clear (a dead unit still resolves).
* Each AI state has four **town slots** at `+0x1B8` (empty = -1, else a site index). All helpers reject a slot index
  above 3 first.
* Adding a site to an AI (`0x6F2D5330`): the slot already holding the site, else the first empty slot, else nothing
  (returns 4).
* A gold mine is a unit with the ability `Agmb` (code `0x6F082470` returns); its amount is that ability's `+0x6C`.
* `GetMinesOwned` = live mine references over the four slots' sites (a site in two slots counts twice).
  `GetGoldOwned` = the sum of the `Agmb` amounts of the same mines (not the player's gold). `TownHasHall(i)` = the
  slot's hall resolves. `TownHasMine(i)` = either mine reference of the slot's site resolves.
  `TownWithMine` = the first slot whose first mine resolves and: human/orc (player race 1, 2): the hall resolves and
  its unit word `+0x5C` bit `0x2000000` (structure under construction) is clear; night elf (4): the mine is not under
  construction and its type is `egol`; any other race: the mine is not under construction. -1 when none.
* Unicorn check (`tools/emu/ai_towns_emu.py`, vectors `crates/sim/tests/vectors/ai_towns_seed1.tsv`, replayed by
  `crates/sim/tests/ai_towns_vectors.rs`): the six real bodies on 300 random synthetic states, **6,426 checks, 0
  mismatches** (mines 300, gold 300, with-mine 300, hall 2,100, mine 2,100, add-slot 1,326). Stubs: the reference
  resolver `0x6F4786B0` and the ability finder `0x6F0787D0`; everything else is the real code. The emulator caught
  one error of mine on the way: the entangled mine is `egol`, not `emol`.

UNSURE, and used by the model instead of the original's rule:
* **When** units are registered. The original registers a gold mine when the unit enters play (`0x6F2E52F0`,
  `0x6F284B40`) and the AI's own halls through its unit handler (`0x6F2DBB90`), plus a start site from the AI's start
  location (`0x6F2DA810`) and a start peon (`0x6F2D7710`). The model registers lazily, in object order, the first time
  a town native runs: every mine unit (a unit with a resource node, or type `ngol`, `ugol`, `egol`), and the town
  halls (class bit `0x40`) and haunted mines of players that run an AI (started, or a computer slot). Order of
  creation decides slot 0 (the first hall).
* **Which site** a unit joins. The original asks the path-cost routine `0x6F4758D0` (limit 80 for the hall search in
  `0x6F2DF170`, 150 and 5000 elsewhere) and takes the cheapest below the limit; the model takes the nearest site
  within `SITE_RADIUS` = 1024 world units (straight line), else makes a new site.
* The sim creates a resource node (amount 0) for every mine-type unit at creation, so `SetResourceAmount` works on
  it; nothing in the sim did that before (the engine's map loader did not register nodes, so `GetResourceAmount`
  and the new gold sum were 0 on real maps). The default amount of a fresh mine is not read from the ability data.
* Night elf: a plain `ngol` is never an entangled mine, and the sim has no entangle, so `TownWithMine` is -1 for the
  elf until a mine converts.

### 6.3 Still unbound (27) and why
* Expansion, 8: `GetNextExpansion` (`0x6F2E10D0`), `GetExpansionX/Y`, `GetExpansionPeon`, `GetExpansionFoe`,
  `GetEnemyExpansion`, `SetExpansion`, `GetTownUnitCount`. They rank candidate sites by path cost
  (`0x6F4758D0` → `0x6F475560`), with side effects on cached fields (`+0x19C` of the AI state), and `GetExpansionFoe`
  runs the group builder `0x6F2A3270` with a callback. Not isolable without the path-cost routine on a live map.
* Group-builder scans, 5: `CreepsOnMap`, `GetBuilding`, `IgnoredUnits`, `IsTowered`, `GetCreepCamp`.
* Captain: `CaptainReadiness`, `CaptainReadinessHP`, `CaptainReadinessMa` (soft-float readiness over the group's
  units, `0x6F2E91A0`: needs the group), `CaptainIsHome`, `CaptainAtGoal` (captain position against home/goal).
* Records: `GetHeroId`, `GetHeroLevelAI`, `GetUnitBuildTime`, `GetUpgradeGoldCost`, `GetUpgradeWoodCost`,
  `WaitGetEnemyBase`, `GetEnemyBase`, `GetMegaTarget`, `UnitInvis`.
Scripts that call them (grep over the five retail scripts): `CaptainReadiness` 2, `GetCreepCamp` 2, `GetExpansionFoe`
2, `GetHeroId`, `GetEnemyExpansion`, `GetMegaTarget`, `IgnoredUnits` 1 each, all in `common.ai`. None was reached in a
20-minute run, because the AI builds nothing yet (6.1).

### 6.4 The four idle reads
`CaptainIsEmpty` / `CaptainGroupSize` read the captain record's group size at `+0xBC` (CONFIRMED bodies `0x6F2DDB50`,
`0x6F2DDB90`); `CaptainInCombat(attack)` is true when either of the two enemy references of the attack (true) or
defense captain (`0x6F2D0A00`: `+0x114`, `+0x120`) resolves; `TownThreatened` is true when the player record's
reference at `+0x164` resolves (`0x6F2DD8E0`). The sim has no writer for these fields (no captain groups, no combat
tracking, no threat scan), so they read the fields of `AiControl` (`captain_group_size`, `captain_in_combat`,
`town_threatened`), which are idle (0, false, false) unless a host sets them. UNSURE.
