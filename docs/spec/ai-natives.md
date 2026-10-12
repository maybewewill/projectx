# AI natives (table `ai`): status and blockers

Scope: the 124 natives of table `ai` (`docs/spec/natives-1.26a.tsv`, registered by `0x6F2E3D90`) and the four
main-VM AI natives (`StartMeleeAI`, `StartCampaignAI`, `CommandAI`, `PauseCompAI`). The model is in
`crates/sim/src/ai.rs` and `crates/sim/src/natives/ai.rs`. Levels: CONFIRMED (the body shows the value or effect),
LIKELY (the mechanism is read, a detail is inferred), UNSURE (not fully decoded, or the model chose the value).

Behaviour is written in plain words. The bodies were read in a Ghidra decompile kept outside the repo.

## 1. Counts (2026-10-12, after S19; the older figures are in the parentheses)

| | Count |
|---|---|
| AI natives in table `ai` | 124 |
| Bound (`implemented: true`) | **124** (115 after S15, 100 after lane H, 97 before) |
| Not bound (`implemented: false`, logged and return the default) | **0** (9 after S15, 24, 27) |
| Level of the bound rows: CONFIRMED / LIKELY / UNSURE | 44 / 54 / 21 (a grep of the table also counts the 4 main-VM rows) |
| Raised to CONFIRMED by a Unicorn run | the bodies of 5 town natives (section 6), the assault request flow and the creep-camp chooser (section 9) |

S19 bound the last 9 (section 10). The list below is the state after S15, kept for the history. The 9 that were unbound: `GetHeroId`, `GetHeroLevelAI`, `GetTownUnitCount`, `GetNextExpansion`, `GetExpansionPeon`, `SetExpansion`,
`GetBuilding`, `GetMegaTarget`, `IgnoredUnits`. A 900 s run of orc vs human calls three of them (`GetExpansionPeon`
x188, `GetMegaTarget` x9, `GetNextExpansion` x342) and gets the defaults.

The test `no_ai_native_is_left_unbound` replaces the old pin (`unbound_ai_natives_are_pinned`). Lane G bound 9: the five town natives (section 6) and four captain/threat reads (6.4).

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

## 7. Production and order natives (2026-10-11, lane H)

Code: `crates/sim/src/ai_manager.rs` (what the natives do), `crates/sim/src/production.rs` (training, construction,
research, harvesting), tech data in `crates/engine/src/objdata.rs` and `data.rs`. Tests:
`crates/sim/tests/ai_production.rs`, `crates/engine/tests/ai_production.rs`. The bodies were read statically (IDA).
There is no Unicorn run: every body walks the AI's unit lists and ability objects, which the harness does not model.
No dynamic tracing was done; none is needed for what is claimed here, but the stand-ins in 7.5 would need it.

### 7.1 What the original does (CONFIRMED structure, from the decompile)
* `SetProduce(n, type, town)` `0x6F2DD450`: n = 0 returns true. Otherwise n requests of `0x6F2D8130`; the first failed
  request returns false. A structure type (`IsStructure` meta value) goes to the build request `0x6F2D2B70`, anything
  else to the train request `0x6F2D5B90`. The order is issued inside the native, before it returns.
* Train request `0x6F2D5B90`: a hero type the player already has (alive or dead, list at player `+0x2B0`) returns true
  (`0x6F2D2D80`, revive path). Otherwise the AI player's units are walked in list order. A unit qualifies when the usable
  test `0x6F2D5940` passes (not under construction, not busy: the queue ability's slot 804 and the upgrade ability's
  counter are clear), its train ability accepts the type (slots 828 / 624) and, when a town is given and its slot is
  set, the unit belongs to that town. The first one gets the train order (order id = the type code, replace, drain).
  Insufficient money or food is not looked at: the order is refused later and the native still returns true.
  The model reads "busy" as "queue not empty" (LIKELY): the AI never queues behind a running head.
* Build request `0x6F2D2B70` and `0x6F2D2880`: gates first (AI not paused `0x6F2D1A20`, a per-AI agile float equal to 0,
  requirements of the type `0x6F40B370` / `0x6F40B380`). Then the units are walked in list order for one whose AI mode is
  0 (idle), 5 or 6 (harvesting), whose build ability accepts the type, and that belongs to the requested town. The
  site comes from `0x6F2E50E0` (build-spot finder around the town position). No site sets a flag and the request fails
  (a delayed message is posted). With a builder and a site the unit gets the build order (`0x6F294B30`). A hall tier is
  the same path with the hall as the "builder". The haunted mine `ugol` uses an acolyte ability order.
* `SetUpgrade(tech)` `0x6F2DD4B0`, body `0x6F2D5A00`: the native always returns 0. The first usable building whose
  research ability accepts the tech, with an empty research counter and a passing start check (slot 800), gets the
  research order.
* `ClearHarvestAI` `0x6F2DD5B0`: for every unit of the player with an AI record, bit 1 (gold this cycle) becomes bit 4
  (gold last cycle), else bit 4 is cleared; bit 2 becomes bit 8 the same way.
* `HarvestGold/Wood(town, n)` `0x6F2DD5E0` / `0x6F2DD630`, body `0x6F2D9710`: n <= 0 does nothing; a town slot above 3 or
  empty does nothing; for gold the town needs a completed hall (or a completed mine of the kinds that stand alone).
  Wood on town 0 first (`0x6F2D8940`, first block) sends every idle harvester to wood; the count only decreases, it does
  not stop the loop (CONFIRMED quirk). Then (`0x6F2D8940`, second loop) units of the same town that were on that resource
  last cycle (bit 4 / 8) and still harvest (mode 5 / 6) keep it without a new order, one per count. Finally
  (`0x6F2D8C20`) other units that can harvest the resource, are idle or harvesting and are not yet assigned this cycle
  take a new order, in list order, until the count is used.
* `GetUnitCount` `0x6F2DD050` = typed count with mask `0x36` (completed and in progress) plus `0x6F2D1C10` (workers in AI
  mode 2 heading to build the type); `GetUnitCountDone` `0x6F2DD010` = typed count with mask `0x30`.

### 7.2 What the sim does now
* All of the above, on `Game`: `ai_manager::{set_produce, set_upgrade, harvest, clear_harvest, unit_count}`. A queued
  trainee counts as an in-progress unit of its type from the queueing on (LIKELY: the scripts rely on it).
* Training (`Game::train_unit`): `economy::TrainQueue` (seven slots, cost at queue time, food when the head starts),
  timer = `bldtm`, the unit appears south of the building; the food reserved by the head is handed to the created unit.
* Construction (`start_build_job`, `place_building`): the worker walks to the site through the order system; on arrival
  the cost is paid (shape of `0x6F0EDA70`), the building is created in progress, grows from 10 % to 100 % linearly and
  completes after `bldtm` with the registration transition (food made at the finish). The builder of a night elf or
  undead building is consumed. Hall tiers (`Upgrade=`) change the unit's type in place at the end of `bldtm`.
* Research (`start_research`): `UpgradeData.slk` cost `base + mod * level`, time `timebase + timemod * level`, the
  player's tech level rises at the end.
* Harvesting (`assign_harvest`, `step_harvester`): walk to the mine or tree, work, walk to the nearest completed hall,
  deposit through `economy::Worker::deposit` (income changer, upkeep tax); the gold trip is `Worker::gold_trip` on the
  `Mine` record. Trees come from `war3map.doo`; the pathing type bytes of `war3map.wpm` give the buildable area.
* Bound as a side effect: `GetUnitBuildTime`, `GetUpgradeGoldCost`, `GetUpgradeWoodCost` (24 AI natives are unbound now).

### 7.3 w3boot, orc computer on (2)Circumvention
`w3boot ... --computer 1:orc --steps 12000 map` prints one status line per game minute. Selected lines:
* 60 s: 8 peons (3 trained), 4 on gold, 340 gold deposited, altar and two burrows under construction.
* 180 s: 12 peons, blademaster, barracks under construction, 1,290 gold and 230 lumber deposited.
* 300 s: 13 trained, 5 built; 2 grunts, a head hunter, war mill, 2 burrows; food 27/30.
* 600 s: 19 trained, 8 built; 6 grunts, 3 head hunters, 4 burrows; 4,650 gold and 1,400 lumber deposited.
* 1200 s: 28 trained, 12 built; stronghold (hall tier), second hero, spirit lodge, bestiary, 7 researches, 9,430 gold
  and 2,190 lumber deposited. 0 aborted AI threads. Human vs orc (both computers) also runs; undead and night elf stall.

### 7.4 Not done
Expansions (`GetNextExpansion` and its neighbours stay unbound), attack waves and captains (the commands are only
logged), hero revive, `MergeUnits` / `ConvertUnits`, item and shop buying, undead (blight, ghoul lumber) and night elf
(wisp, entangled mines) production.

### 7.5 Stand-ins (UNSURE)
* Build spot: **decoded in section 8** (2026-10-12, `crates/sim/src/ai_site.rs`). The ring search of this section
  (rings of 64 units around the town hall, footprint half-size 128 / 96 / 64 by type) is kept only as the fallback
  `find_build_site_rings` for maps or types without a pathing grid, a pathing texture or a playable area.
* Builder among equals: the first eligible unit in creation order (CONFIRMED order rule); the town membership test is
  not modelled, any worker may serve any town.
* Times: training and construction = `bldtm` (the progress timers are not decoded), research = `UpgradeData` time,
  1.0 s inside the mine, 10 hits of 1.0 s per lumber trip, 10 gold or lumber per trip, 50 lumber per tree. Walking uses
  the existing order system (its speed-to-step conversion is itself a stand-in).
* Hero requirement tiers (`Requires`, `Requires1`, ...) follow the number of heroes owned; the food ceiling is 100.
* Trees are the doodads whose second type letter is `T` (the category column is `D` for them).
* The `ProdStats` totals are per game, not per player.

## 8. The AI build-spot search (2026-10-12, lane S14)

Replaces the ring stand-in of 7.5 for structures. Code: `crates/sim/src/ai_site.rs` (the port), the host adapter and the
call site in `crates/sim/src/production.rs` (`find_build_site`, `SiteHostImpl`), the data install in
`crates/engine/src/data.rs` (`install_sites`). Harness: `tools/emu/ai_site_emu.py` (Unicorn, real code of the routines
below), vectors `crates/sim/tests/vectors/ai_site_seed1.tsv` and `ai_site_hard1.tsv`, replay test
`crates/sim/tests/ai_site_vectors.rs`. Constants (`tools/emu/ai_site_consts.py`, start-up values): `0x6FAB4EB0` = 1600.0,
`0x6FAB4EB4` = 1/32, `0x6FAB4EB8` = -99999.0, `0x6FADB3F8` = 32.0, `0x6FAB68A4` = 32.0, `0x6FAB68A8` = 16.0.

### 8.1 Call path (CONFIRMED, static)
`SetProduce` -> build request `0x6F2D2B70` -> `0x6F2D2880`. That body walks the AI player's units for a builder
(AI mode at `+0x194` of 0, 5 or 6, as in 7.1; its build ability accepts the type; when a town slot is given the builder
belongs to it), takes the builder's town
(`0x6F275D90` -> hall reference at `+0x48`, `0x6F4786B0` resolves it), and calls the search
`0x6F2E50E0(this = AI state, &status, &centre_x, &centre_y, type, &out_xy, builder)` with the **position of the
town's hall** as the centre. The status float is `1.0` (found) or `-1.0`. On success the builder gets the build order
(`0x6F294B30` at the returned point; `0x6F294D40` for the haunted mine). On failure the caller keeps the failed site
index in a three-entry list and tries the next candidate (not modelled). Other callers of the search, not read:
`0x6F152D60`, `0x6F2D7F90`.

The wrapper `0x6F2E50E0`: builds the buffer (`0x6F2E1630`), marks the structures (`0x6F2DF650`), copies the buffer
(`0x6F2E4810`), runs the search with the reachability check on (`0x6F2DF6F0`, third argument 1). Only when that
fails (`-1.0`) **and** a reachability query failed during it (buffer `+0x20` set), it runs the search again on the
copy with the check off. The "second pass" flag `a6` of the buffer builder is `GetUnitCount(type) >= 2`
(`0x6F2D1C10` plus the typed count of mask `0x36`).

### 8.2 The window (`0x6F2E1630`, CONFIRMED by Unicorn)
A 101 x 101 window of 32-unit cells whose world origin is `centre - 1600.0` (softfloat sub; the origin in cell units is
`origin * (1/32)`). Two byte grids per cell: the *value* grid and the *state* grid (5 untouched, 6 queued, 7 done,
4 taken). Value cell: `5` when the cell has the **same cliff level** as the centre cell (`0x6F0128C0` at the playable
minimum's cell numbers) and is not unwalkable (`0x6F468F90(&x, &y, 2)` at the cell's lower-left corner, advanced by
softfloat adds of 32.0 per cell and row); `0x9C` unwalkable; `0x9E` another level. The queue is a ring of 10,201 pairs
with the centre cell (50, 50) in it.

### 8.3 Marking (`0x6F2DF650` -> `0x6F2DF4A0` -> `0x6F3E0D30` -> `0x6F2DF420` -> `0x6F2DF380`, CONFIRMED)
The units within a circle of 1600 of the centre (`0x6F2A3270` builder, case 9 range, flag 2) are visited by the callback
`0x6F2DF4A0`. Its inputs are two fields of the **unit type record**: `+0x1B8` = the `buffType` enum
(`0x6F262E60`: `""` or `_` 0, `townhall` 1, `resource` 2, `factory` 4, `buffer` 8, other 0) and `+0x1BC` = `buffRadius`
(UnitData). The pair (radius `r`, tier) is: townhall (r = trunc(buffRadius), tier 0); resource (trunc, 1);
factory (trunc, 2); buffer (trunc, 3) except that when the building being placed is a scout tower `hwtw` or ziggurat
`uzig` the radius is **15** (the AI already has two or more) or **5** (fewer); any other enum (including 0:
houses, farms, graveyards) is tier 3 with `r = 6`. A unit marks **every texel of its pathing texture**
(`pPathingFootprint`, the size of the TGA named by `pathTex`; a type without one is never passed in, so mobile units do
not take part, UNSURE which flag test of `0x6F26E490` enforces that): the footprint walk `0x6F3E0D30` starts at the
unit position (orientation from the Misc `BuildingAngle`: 90 -> 1, 180 -> 2, 270 -> 3, else 0, through
`i2r(f2i())`), walks rows then columns with softfloat adds of +-32 from the first texel centre, resetting the
coordinate the column step moves after each row. Each texel position becomes a window cell
(`f2i(position * (1/32) - cell origin)`) and `0x6F2DF380` runs: the cell itself becomes `0x9D` when its **signed**
value is above -99 (so a free cell under a footprint becomes `0x9D`, an unwalkable `0x9C` stays); then every cell of
the square of Chebyshev radius `r` takes the marker's tier when its signed value is above the tier (so a cell keeps the
smallest tier that covers it; negative cells never change). The result does not depend on the order of the units.

### 8.4 The search (`0x6F2DF6F0`, `0x6F2DF680`, CONFIRMED by Unicorn)
Pop a queue cell (x, y); world corner `origin + i2r(x) * 32.0` (softfloat; y alike); snap the corner by `0x6F3E0B50`
(8.5); the snapped point's window cell (i, j) = `f2i((1/32) * (point - origin))`. Inside the window:
* value `5` and state not 7: if the reachability check is on and fails, set the failed flag and skip; else if the
  placement test `0x6F2A50C0` passes, mark the cell 4 in both grids and **return the point**.
* value `0..3` (signed): if the tier's bucket is empty and the state is not 7: same checks; on success the bucket
  keeps the point (first candidate per tier only).
Then the neighbours of the popped cell (x, y) are queued when their state is 5 (set to 6), in the order
(x-1, y), (x-1, y-1), (x-1, y+1), (x+1, y), (x+1, y-1), (x+1, y+1), (x, y-1), (x, y+1), each only inside 0..100; then
the snapped cell gets state 7. The queue is first in first out, so the search is a breadth-first walk from the centre
and the first free cell in that order wins. When the queue empties the buckets are tried from tier 3 down to 0 (so
being next to a buffer is preferred over being next to a resource, factory or town hall); the winner's cell is marked
4 and its point returned; no bucket: `-1.0`.

### 8.5 The snap (`0x6F3E0B50`, CONFIRMED)
Clamp into the playable area (`0x6F469490`), then per axis: the integer part truncated to a multiple of 64 toward zero,
plus +-32 when bit 1 of the footprint size on that axis is set (sign follows the side of zero), converted back, plus
+-16.0 when the footprint size on that axis is odd (0.0 when even). The sizes are (w, h) for orientation 0 and 2 and
(h, w) for 1 and 3.

### 8.6 What is not decoded, and the model used
The four engine predicates are stubbed in the harness and modelled in the sim (`SiteHostImpl`):
* cliff level of a cell (`0x6F0128C0`: terrain level `0x6F742820`, ramps `0x6F0118C0`) - the model has no cliffs: every
  cell is level 0 (UNSURE; buildings can be chosen across a cliff when a ramp connects).
* the unwalkable test (`0x6F468F90`, mask 2 of the cell word's top byte) - the wpm byte's bit 0x02 or a tree (2 x 2 cells).
* reachability (`0x6F475560(from = hall, to = candidate, 1, 15, builder's footprint objects, ...)`: it erases the
  builder's own pathing footprint from the pyramid (`0x6F488330`, flag 1), issues the path request `0x6F49A8F0` from the
  hall's cell to the candidate with a tolerance radius argument 15, restores the footprint and returns the request's
  result; `-1` = no path, LIKELY) - modelled as an 8-neighbour flood fill over walkable cells from the hall's cell
  (UNSURE; the tolerance radius and partial paths are not modelled).
* the placement test (`0x6F2A50C0`, returns 0 when the building may stand: texture against the pathing map, units, mine
  and tree rules, visibility of the player) - every texel of the building's texture on a buildable walkable cell not
  covered by another structure's texture, mine or tree (the textures of structures are solid blue over their whole
  area, so the rectangle is exact for placement; UNSURE for the rest).
RNG: none of `0x6F2E50E0`, `0x6F2E1630`, `0x6F2DF6F0`, `0x6F2DF4A0`, `0x6F3E0B50`, `0x6F3E0D30` calls a random routine
(CONFIRMED by reading them); `0x6F2A50C0` and `0x6F475560` were not read to the bottom (LIKELY none).
Open: whether `0x6F26E490` (flag word `+0x5C` bit `0x10000`, not `0x80` unless `0x8000000`) selects structures only;
the other callers of the search; the builder loop's retry over town sites; that dead structures never take part.

### 8.7 Verification (CONFIRMED)
`tools/emu/ai_site_emu.py` runs the real `0x6F2E1630`, `0x6F2DF4A0` (with the real footprint walk, cell callback, cell marker),
`0x6F2E4810` and `0x6F2DF6F0` (with the real snap, clamp, neighbour push and softfloat library), the wrapper's retry
included, against hash models of the four engine predicates and of the type tables (stubs at the entries of
`0x6F0128C0`, `0x6F468F90`, `0x6F475560`, `0x6F2A50C0`, `0x6F32D0B0`, `0x6F29C300`, `0x6F29C330`, `0x6F020D30`, the unit
position getter). 500 random cases (map rectangle, centre, building type and texture size 1..16 x 1..16, angle in
0/45/89/90/180/270, 0..30 structures with `buffType` 0/1/2/3/4/5/8/16, radii -1..25, texture sizes 1..16) in three sets:
`ai_site_seed1.tsv` (330, ordinary), `ai_site_hard1.tsv` (110, placement rarely or never passes, reachability fails
often), `ai_site_dense1.tsv` (60, the window is covered by buffers so the tier buckets decide). The Rust port
(`sim::ai_site`) reproduces, for every case, the value grid and the state grid after the marking (FNV hash of all 10,201
bytes each) and the final status and point bit for bit: **500 / 500, 0 mismatches** (457 found, 43 failure exits, 68
second passes after a failed reachability query, 19 results taken from a tier bucket). Test:
`cargo test -p sim --test ai_site_vectors` (no DLL needed). Re-generate with
`GAME_DLL=... py -I tools/emu/ai_site_emu.py OUT.tsv N SEED [hard|dense]`.
Findings of the Unicorn pass that the static read got wrong: the cell marker compares the cell **signed** (`jle`), so a
free cell under a footprint becomes `0x9D`; the operand order of the softfloat mul differs between the cell index of the
search (`mul(1/32, diff)`) and the bucket cell (`mul(diff, 1/32)`).

### 8.8 In the sim, and the positions it gives
`Game::find_build_site` runs the port with the structures within 1600 of the anchor (live units and walking builders'
jobs that have a pathing texture) and falls back to the old ring search otherwise. The footprints are not written into
the wpm cells: later searches avoid earlier buildings because every structure is a unit (or a job) and takes part in the
marking and in the occupancy of the placement model, in the same way as in the original (where the units of the player
mark the window). The pathing pyramid is not touched (as in the original: the footprint is stamped by the unit's own
registration, not by the AI).
`w3boot ... --computer 0:orc --computer 1:human --steps 3000 --buildings (2)Circumvention.w3x`, 300 s (the `*` marks a
building still under construction):

| orc, slot 0 (hall at -1024, 3776) | | human, slot 1 (hall at 1472, -4288) | |
|---|---|---|---|
| altarofstorms | (-1568, 3808) | altarofkings | (1504, -4832) |
| trollburrow | (-1568, 3296) | humanbarracks | (1216, -3776) |
| trollburrow | (-1184, 3232) | farm | (960, -3776) |
| orcbarracks | (-512, 3776) | farm | (1728, -3776) |
| warmill | (-800, 3232) | farm | (1984, -4288) |
| trollburrow * | (-1568, 4256) | farm | (1984, -4032) |

At 600 s the orc adds a voodoolounge (-512, 3264) and a burrow (-288, 4256); the human adds a blacksmith (1984, -4608),
three more farms ((1984, -3776), (896, -4288), (1984, -4992)), an arcanevault (704, -3520) and a lumber mill under
construction (2272, -4256). No two buildings coincide; the nearest neighbours are 256 apart (two 6 x 6 or 4 x 4 textures
plus their buffers). Before the change (rings): orc burrows at (-1088, 3520) and (-896, 3520), altar at (-1280, 3520),
all within 300 of the hall.

### 8.9 Open
* The cliff level of a cell (`0x6F0128C0`) and ramps: the model returns level 0, so a spot below a cliff that a ramp
  connects to the hall is not rejected as the original would.
* The path request behind `0x6F475560` (tolerance radius 15, the builder's movement type) and the full placement test
  `0x6F2A50C0` (including the mine and tree rules and visibility) are modelled, not decoded.
* Which structures are enumerated (the flag test of `0x6F26E490`) and whether trees and other destructables take part
  (they do not: the callback reads a unit type record).
* The builder loop of `0x6F2D2880` that retries other town sites after a failure; the two other callers of the search.
* Pathing texture pixel flags beyond the whole-texture rectangle (the placement model treats every texel as blocking).

## 9. Captains, assault requests, creep camps and targets (2026-10-12, S15)

Code: `crates/sim/src/captains.rs` (the model), `crates/sim/src/natives/ai.rs` (the bindings), the Unicorn harnesses
`tools/emu/captain_assault_emu.py` and `tools/emu/creep_camp_emu.py`, their vectors in `crates/sim/tests/vectors/` and
the replay `crates/sim/tests/captain_vectors.rs`. Behaviour tests: `crates/sim/tests/ai_natives.rs` (three new). The
bodies were read in IDA (the user's `game.dll.i64`); nothing of the pseudo-C is in the repo.

### 9.1 What the original has
* The AI state holds an attack captain (reached by `0x6F2CFEB0` from the reference at `+0x2AC`) and a defense captain
  (`0x6F2CFED0`). A captain is a mover of its own: position getter (vtable slot `0xB8`), home, goal point and goal
  radius, mode word `+0x64`, flag word `+0x6C`, enemy references `+0x114`/`+0x120`. The members follow it through the
  group routines `0x6F2AB2D0/0x6F2AB3E0/0x6F26EF10`. CONFIRMED structure; the mover itself is not modelled.
* `InitAssault` (`0x6F2D4DF0`) sets bit 1 of the attack captain's flag word (CONFIRMED). `CaptainIsFull` reads it.
* `AddAssault(qty, type)` (`0x6F2DD9C0` -> `0x6F2D96F0` -> request `0x6F2D6EA0`) returns the request's value: true when
  the quantity was met, otherwise bit 1 is cleared and the result is false (CONFIRMED; the older reading "no return
  value" in section 3 was wrong, the body tail-calls the request). `AddDefenders` is the same request on the defense
  captain (`0x6F2D96D0`).
* **Request control flow, CONFIRMED by Unicorn (`captain_assault_emu.py`, 600 cases, 0 mismatches):** the type has an
  optional morphed partner (head hunter -> berserker `ohun`/`otbk`, druid of the claw -> bear form `edoc`/`edcm`, druid
  of the talon -> crow form `edot`/`edtm`, spirit walker -> ethereal form `ospw`/`ospm`, gargoyle -> stone form
  `ugar`/`ugrm`). The passes run in this order, each for the type and then for the partner: group pass (`0x6F2D4F20`),
  recruit pass flag 1 (`0x6F2D50A0`), recruit pass flag 0. After every pass the quantity is tested for zero (true).
  A pass lowers the quantity by one per unit and returns at once when it was 1 (`(*q)-- == 1`); a quantity that starts
  at 0 or below goes negative and never matches (quirk, kept).
* **Group pass** (LIKELY from the decompile): units of the type that are in this captain's group count toward the
  quantity; while the captain is attack-moving (mode 2) a member with life below 50 % (constant `0x6FAAE4D4` = 0.5) is
  released instead. **Recruit pass** (LIKELY): units of the type of the AI player, not already in this group, skipping
  units that are in transports, hidden or under construction (flags `0x42000000`), workers whose AI mode is not 0 or 5,
  units in an attack order (`0xD000F`, `0xD000E`) and units of another captain whose mode word is not lower than this
  captain's; flag 1 takes healthy units, flag 0 the rest. A recruited unit joins and gets a move order (the original
  issues the order built by `0x6F294A40` / `0x6F294D40`).
* **`RemoveInjuries`** (`0x6F2DDDA0` -> `0x6F2EEE50`, CONFIRMED structure): non-hero units of the attack group with life at
  most 50 % (constant `0x6FAAE4D0`) leave it, join the defense captain and are sent to its home.
* **Readiness** (`0x6F2E91A0`, flags (1,1) / (1,0) / (0,1) for `CaptainReadiness` / `...HP` / `...Ma`): soft-float life
  and mana percentages of the group, heroes and other units read separately. UNSURE how the two are combined; the model
  takes the totals and the smaller of the two for the combined native.
* **Attack-move** (`0x6F2EC7D0` XY, `0x6F2EC700` unit): mode word `+0x64` = 2, retreat bit (bit 2) cleared, the captain
  mover is sent (`0x6F474D40`). `AttackMoveKill` does nothing when the captain's current target is the same unit
  (`0x6F2E7530`, CONFIRMED), which is why the scripts can call it every 3 s.
* **Home and goal tests** (`CaptainIsHome` `0x6F2DD870` -> `0x6F2E77C0`, `CaptainAtGoal` `0x6F2DD930` -> `0x6F2E77F0`, both
  `0x6F2E76E0`): the captain's position against a point, with a radius of 500.0 (`0x6FAB4F08`) for home and the goal
  radius for the goal (200.0 = `0x6FAB4F2C` is stored by the attack-move). The compare scale is UNSURE (the body
  squares the radius and mixes the constants 20000.0 and 5000.0).
* **Creep camps.** A unit is registered as a camp creep when it is created (`0x6F2E52F0`): not removed (`+0x5C & 0x100`),
  not a structure, owned by player 12 (the neutral hostile player), and `+0x248 & 4` clear. `0x6F2E51D0` puts it into the
  nearest camp with fewer than 12 members (`0x6F2E38A0`) if the squared pathing-cell distance to that camp's first living
  member is below `CreepCampPathingCellDistance`^2 (**26** in the patch's `Units\MiscData.txt`; a missing key reads 0),
  else a new camp. The camp level (`0x6F2E1C50`) is the sum of the members' levels (`0x6F2A0210`: hero level + 1, else the
  type level), doubled for members with flag `+0x5C & 0x20000000` and `+0x200 <= 0` (the model reads that as flying).
* **`GetCreepCamp(min, max, flyers_ok)`** (`0x6F2E3D50` -> chooser `0x6F2E3980`, CONFIRMED by Unicorn,
  `creep_camp_emu.py`, 600 cases, 0 mismatches): walks the camps in list order; skips camps with no members or that the
  usability test `0x6F2E1D50` rejects; takes the path cost to the camp's first living member (`0x6F4758D0`; search limit 5000,
  lowered to the world bound + 100); a camp beyond the horizon (start 999999) is ignored; a camp whose level is above `max`,
  or that has flyers when flyers are not allowed, pulls the horizon down to its cost + 200 (when that is lower) and drops the
  candidates beyond it; a camp in `min..=max` competes for the lowest cost (strict, first wins), a camp below `min` is the
  fallback. The in-range pick wins over the fallback. The native returns the first living member of the chosen camp.
* `UnitInvis` (`0x6F2E07C0`) is the negation of a vision query; a unit without the invisibility flag `0x1000000` passes
  the gate (CONFIRMED), so the sim, which has no invisibility, returns false.

### 9.2 What the model does (and where it is UNSURE)
* `captains.rs`: `Captain` (members, home, stage point, mode, goal, flags, combat state, departure log), the six-pass
  `assault_flow`/`request`, `remove_injuries`, `readiness`, `is_home`/`at_goal`, `attack_move_xy`/`attack_move_kill`,
  `go_home`, `update`. The group is driven with ordinary orders: members get `attack-move` (order `0xD0014`) to the target's
  position; idle members are re-ordered every second, and within 900 units of a unit target they get a direct attack
  order. The original's leader mover and formation are not modelled (UNSURE).
* `update` runs once per AI step before the AI VMs: prunes dead members, sets the combat flag (a member has a living attack
  target; the original: an enemy reference of the captain resolves), ends the move when the goal unit is dead, and sets
  `TownThreatened` from a scan (an enemy fighting unit of a real player within 1500 units of one of the AI's halls; the
  original's scan is not decoded, UNSURE).
* Camps are built lazily at the first `GetCreepCamp`/`GetExpansionFoe` from the neutral hostile units that exist then (the
  original registers at creation; the same result when no creep is created later). Cost = straight distance in cells * 24
  (UNSURE: the unit of `0x6F4758D0`); levels come from the unit type level (the pick record), flyers from the movement type.
* Targets (all UNSURE, straight-line stand-ins for the path cost): `GetEnemyBase` = the nearest enemy town hall, else the
  nearest enemy structure (`StartGetEnemyBase` computes it at once, `WaitGetEnemyBase` is false); `GetEnemyExpansion` = the
  latest town hall of an enemy that has two or more; `IsTowered` = more than one enemy structure with an attack within 900
  units of the hall; `CreepsOnMap` = any living neutral hostile non-structure unit; `GetExpansionX/Y/Foe` use the nearest
  gold mine not within 900 units of a hall, and the camp nearest to it within 1100 units. `SuicidePlayer(Units)` attack-moves
  the group to the nearest structure (unit) of the player when its gate passes.
* Not bound: `GetMegaTarget` (masked read of an AI state field written by a threat scan that is not decoded),
  `IgnoredUnits`, `GetBuilding`, `GetNextExpansion`, `GetExpansionPeon`, `SetExpansion`, `GetTownUnitCount`, `GetHeroId`,
  `GetHeroLevelAI`.

### 9.3 Run: `w3boot --computer 0:orc --computer 1:human`, (2)Circumvention, 9000 steps (900 s)
`crates/engine/src/bin/w3boot.rs` now prints, per computer, the captain state each game minute, every departure of the
attack captain with its unit types, the first player-versus-player kill and the kills table. 0 aborted AI threads.
* First attack wave: **orc at 330.1 s**, 4 units (blademaster, 2 grunts, a head hunter) against a creep camp; **human at
  358.5 s**, 6 footmen against a creep camp. Later waves: orc 6 units at 455.1 s and 7 at 558.3 s; human 7 at 475.7 s.
* Both AIs go for creep camps first, as the melee scripts do (no allies, no siege, no enemy expansion, so
  `SingleMeleeAttack` ends in `CreepAttackEx`). The second waves of both aim at the same camp in the middle of the map; the
  groups meet there: **first player-versus-player kill at 597.7 s**. Totals at 900 s: p0 killed 6 units of p1 and 6 creeps,
  p1 killed 5 units of p0 and 7 creeps, creeps killed 1 unit of p0 and 2 of p1. Deaths per player: p0 6, p1 8, creeps 13.
  The player-versus-player fight is a meeting at a camp, not an attack on a base: nothing in this run targets the enemy hall.
* Observations: the sim has no life regeneration, so survivors stay injured (lowest member life 3-4 % late in the run);
  after about 700 s both groups shrink to a few injured units and the waves stop (no regeneration, no reinforcement beyond
  the production script). The AI counts that depend on the fixed 100 ms cadence of the model are UNSURE (lockstep.md 10).

### 9.4 Open items
* The captain mover, formation, the retreat rule (bit 2 is never set), the threat scan, the readiness combination, the real
  path-cost unit, camp registration order at creation, the usability test `0x6F2E1D50`, the mega target and the remaining
  unbound natives of section 1.
* Unicorn-isolable next: the camp creep assignment `0x6F2E38A0` (needs the Misc reader stubbed), the readiness `0x6F2E91A0`
  (needs a unit list), the at-goal compare `0x6F2E76E0`.

## 10. S19: captain routines, expansions and the last nine natives (2026-10-12)

Code: `crates/sim/src/captains.rs` (`goal_limit`, `within_goal`, `readiness_with`, `assign_camp`, `next_expansion`,
`expansion_peon`, `mega_target`, `ignored_units`, `first_building`, `ai_hero`, `town_unit_count`), the bindings in
`crates/sim/src/natives/ai.rs`. Harnesses (Unicorn, real code of the routine, helpers stubbed): `tools/emu/captain_goal_emu.py`,
`captain_ready_emu.py`, `camp_assign_emu.py`; vectors `crates/sim/tests/vectors/captain_goal_seed1.tsv`,
`captain_ready_seed1.tsv`, `camp_assign_seed1.tsv`, replayed by `crates/sim/tests/captain_vectors.rs` (the readiness test
needs `GAME_DLL` for the reciprocal table). The bodies were read in IDA on the user's `game.dll.i64`; nothing of it is in
the repo.

### 10.1 Checked in Unicorn (0 mismatches)
* **Captain position test `0x6F2E76E0`** (`CaptainIsHome` -> `0x6F2E77C0`, `CaptainAtGoal` -> `0x6F2E77F0`), **600 cases**.
  True iff `V >= D`: D is the soft-float squared distance (`0x6F6EF3B0`) from the captain's position (vtable slot `0xB8`)
  to the point; V is `20000.0` (`0x6FAB4F50`) when the radius is below `100.0` (`0x6FAAE574`), else
  `add(mul(mul(2.0, r), r), 5000.0)` (constants `0x6FAAE504`, `0x6FAB4F54`). So the radius 500.0 of "home" means about 710.6
  world units and the default goal radius 200.0 about 291.5 (the older reading "distance <= radius" was wrong; the
  squared compare of section 9.1 is settled). UNSURE: the captain's position is the centre of the members in the model; the
  normaliser `0x6F4743A0` that converts the slot's result was stubbed.
* **Captain readiness `0x6F2E91A0`** (`CaptainReadiness`, `...HP`, `...Ma`), **600 cases** (random groups of 0..12 units
  with heroes, illusions and mana users). The routine reads only its first stack argument (the life flag); the wrappers
  push (1,1), (1,0), (0,1), so **`CaptainReadiness` equals `CaptainReadinessHP`** (life) and only `...Ma` is mana. Heroes
  (type code with an upper-case first letter, illusion flag `0x40000000` at `+0x5C` clear) and other units have separate
  life sums; mana is pooled. Every sum starts at 1.0; the others' maximum life also gets `(captain+0xB8 - captain+0xBC) * 400`
  (units the captain should hold minus the group size: missing units count as 400 maximum life with no life). Result
  `f2i(div(mul(sum, 100.0), max_sum))`; with the life flag the smaller of the hero and the others' percentage. The old model
  (min of life and mana for the combined native, no hero split, no missing-unit term) is replaced. UNSURE: the field
  `+0xB8` has no model (the sim uses 0 missing).
* **Creep camp assignment `0x6F2E38A0`**, **600 cases**: the squared limit is `I2R(d * d)` with `d` = Misc
  `CreepCampPathingCellDistance` (cached at `0x6FAB4EBC`), camps with 12 members or without a living first member are
  skipped, the strictly nearest wins (first on ties). The sim's registration already matched; it now calls the same function.
  UNSURE: the distance routine `0x6F477070` -> `0x6F476860` (stubbed) is taken as the squared cell distance.

### 10.2 The retreat rule (bit 2 of the captain flags), decoded, not run
The bit is set at exactly one place, `0x6F2ED560(point, ..., flag)`, when called with the flag from `0x6F2EDB80`, which
is called twice from `0x6F2D2160` for the attack and the defense captain. `0x6F2D2160` is called by the handler of an
ability cast (`0x6F14CD30`) when the caster, a unit of a captain's group, uses a teleport-like ability and its life is at
most the constant at `0x6FAAE594`: the group then retreats to the cast point. Nothing else sets the bit; the scripts
read it with `CaptainRetreating`. The clearing: attack-move clears it (section 9.1), and the go-home step `0x6F2EC670`
(`CaptainGoHome`) clears it when the group is home (`0x6F2E77C0`) and at least half of the group (record `+0xC4`, "units
home", against `+0xBC / 2`) is home; if not home it issues the move order `0xD0012` toward the home point. The sim has no
town portal / teleport use by AI units, so the bit stays 0 there (CONFIRMED rule, no trigger in the model); `go_home` now
follows the clearing rule. In 40 minutes of the runs below `CaptainRetreating` was called ~40 times and always false, which
matches this.

### 10.3 Expansions
* **`GetNextExpansion`** (`0x6F2E10D0`): CONFIRMED structure. It keeps a cache (record `+0x19C`) with a timer and
  re-ranks when the timer is idle: candidates are the town sites whose mine holds more than 600 gold, that no AI of the
  player or an ally already holds in a town slot (`0x6F2DF2A0`), and where no town hall or haunted mine stands (callback
  `0x6F2DF330`). Three classes by path cost from the defense captain's home (`0x6F4758D0`, limit 5000, lowered by the
  world bound + 100): reachable with no player's unit near the site (callback `0x6F2DC600`), reachable with one near,
  unreachable (nearest by straight distance). The native returns the site index, -1 when none. The model ranks by straight
  distance and treats every site as reachable (UNSURE), returns the index of the registered town site that holds the mine.
* **`GetExpansionPeon`** (`0x6F2D1F40`): CONFIRMED structure. From the AI's units that are alive, have a build ability that is
  not currently building and whose type flags are not both `0x30000000`, the nearest by path cost (limit 700) to the
  expansion's mine, in three classes: AI mode 0 or 7 (idle), then mode 5/6 split by a type flag bit, then none. The model:
  idle workers, then gold harvesters, then lumber harvesters, straight distance.
* **`SetExpansion(peon, id)`** (`0x6F2D8130` -> `0x6F2D7F90`): CONFIRMED structure: gate `0x6F2D1A20`; the peon leaves its captain
  (`0x6F2EC850`); the peon's build ability must accept the type (vtable `+0x270`); the AI timer value at `+0x188` must be zero;
  the build-spot search `0x6F2E50E0` runs centred on the expansion site (`+0x3C`, `+0x44` of the site record); a failure
  (status below 0) starts a retry timer (message `0xD0244`, `0x6FAAE528`) and returns 0; for the haunted-mine type `ugol` the order is
  the haunt order `0x6F294D40` on the mine, otherwise `0x6F294B30` at the found point; returns 1. The model uses the
  S14 search ([`ai_site`]) centred on the mine and `start_build_job`; returns whether the job started.
* **`GetExpansionFoe`** (`0x6F2E15C0` -> the group builder with a default callback): unchanged (camp nearest to the
  site within 1100).
* **`GetTownUnitCount(type, town, only_done)`** (`0x6F2DD0C0`, no function in IDA, read as disassembly): `0x6F2D1C10(type)`
  (workers on their way to build it: state 2 and the build ability accepts the type) plus `0x6F2D5430(type, town,
  only_done)`: the AI's units of the type whose AI record's town reference (`+0x48`) equals the site of the given town slot
  (slot above 3 or empty gives 0); with the flag set, units with `+0x5C & 0x2000000` (under construction) are left out. The
  model counts units within 1024 of the town's anchor.

### 10.4 The other natives
* **`GetMegaTarget`** (`0x6F2DDD60`): reads the reference at AI state `+0x2C4` (`0x6F2D6500`). It is written by `0x6F2D7EC0`
  (a message handler `0x6F2DBEE0`, message `0xD0243`): only when `SetWatchMegaTargets` set the flag `0x2000`, a scan of all
  units (callback `0x6F2D18D0`) adds strengths (hero: `(level+1)/2 + 4`, other: the type level, a few structure types fixed
  1/3/4) to the totals of own, allied (own + allies) and enemy (and per enemy player); with allied > enemy and own >= 20
  the first player index (not self) with a recorded key building and own > 2 * that player's strength gives the target.
  Model: re-evaluated at each call; the key building is an enemy town hall (the original sets it inside a branch whose
  guard `0x6F26DE50` is not decoded, UNSURE).
* **`IgnoredUnits(type)`** (`0x6F2DCE80`): the AI's units of the type (group builder) whose AI record (`0x6F275D90`) is
  missing or has none of its three references (`+0x30`, `+0x3C`, `+0x48`) resolving. The model: not in a captain and not
  harvesting or building (UNSURE).
* **`GetBuilding(player)`** (`0x6F2E2E80`): null for a null/removed player; the group builder returns the first
  structure that does not have the ability `Buns` (callback `0x6F2DC120`; `Unsummon` puts it on). The model remembers the
  buildings `Unsummon` was called on.
* **`GetHeroId`** (`0x6F2DDF50`) / **`GetHeroLevelAI`** (`0x6F2DE0E0`): read the AI's hero record (`+0x1F0`) and, inside a hero-level
  callback, the hero being levelled. The model takes the first living hero of the AI. Note: the engine calls the
  function given to `SetHeroLevels` when a hero levels up; the sim does not do that yet, so AI heroes learn no skills
  (the two natives are only read by `SkillArrays`, which that call would run).

### 10.5 The build-spot search and the cliff level
The search host answered level 0 for `0x6F0128C0`. It now asks `Game::terrain()` (`TerrainGrid::cell_level`, S13) and the
cell numbers start at the whole-map corner (`Game::world_bounds()`, terrain.md section 1) instead of the playable minimum. On
(2)Circumvention the building positions of section 8.8 are unchanged (every cell inside the windows has the centre's level,
or the same ones differ only outside the playable window), so the 1200 s run below gives the same spots.

### 10.6 Runs (release build of `w3boot`, 12000 steps = 1200 s, SpecDamage, seed 0)
`--computer 0:orc --computer 1:human`, (2)Circumvention: 0 aborted AI threads, 9103 logged calls.
* Waves: orc 1 (336.2 s, 4 units: blademaster, 2 grunts, head hunter, against a creep camp); human 4 (362.7 s 6 footmen, 479.9 s 7,
  708.8 s 10, 846.2 s 10), all at creep camps. **No base attack by either AI**: no wave has an enemy structure as its goal.
  The orc's next waves never start (see the reasons below).
* **Expansions**: the human takes one (a town hall at (3520, -64) with two guard towers at about 600 s: `GetNextExpansion` 113
  calls, `GetExpansionPeon` 1, `SetExpansion` 1, `GetTownUnitCount` 1032). The orc never does: `GetExpansionFoe` is non-null
  in all 165 calls (a creep camp is at the site), `StartExpansion` returns at once, `GetExpansionPeon` is never called.
* Kills: p0 -> creeps 3, p1 -> creeps 10, creeps -> p0 1, creeps -> p1 4. **No player-versus-player kill** (S15 had one at
  597.7 s because its readiness model let the second waves meet in the middle; with the life-only readiness above the orc
  waits until its group is healthy, and the sim has no life regeneration, so it never is: the survivors stay at 5 % life).
* At 1200 s: orc fortress, 14 peons, 6 grunts, 3 head hunters, 3 berserkers, 2 demolishers, hero; human keep, 16 peasants,
  8 footmen, 4 riflemen, hero, 2 guard towers (+ the expansion).

`--computer 0:orc --computer 1:human --computer 2:undead --computer 3:nightelf`, (4)TwistedMeadows: 0 aborted threads, 13585
logged calls. Orc: 2 waves (380.8 s 2 grunts, 410.0 s 6), one expansion (SetExpansion x1); human: 1 wave (197.0 s, Mountain King
and 2 footmen); **undead and night elf do not progress** (undead: 5 acolytes, 2 ghouls, 10615 gold, 0 lumber, food 9/20, no hero;
night elf: 7 wisps, 4100 lumber, 140 gold; no wave, none of the natives above called): their economy in the production model is
stuck (undead gets no lumber, night elf has no entangled mine); this is older than S19 and not in the natives. Kills: orc ->
creeps 7, human -> creeps 2, creeps -> human 1; no player kill. With `0:orc 1:human 2:orc 3:human` instead: waves p0 2, p1 1, p2 1, p3 1;
expansions: p0 one; kills: p0 -> creeps 7, p1/p2/p3 -> creeps 2 each; again no base attack and no player kill.

### 10.7 Open items (in order of effect on the AI's play)
1. **Life regeneration of units** (and mana): not in the sim; groups never recover, so `CaptainReadiness >= readyPercent` stops
   new waves after the first fight. This is the main reason that no AI attacks a base in 1200 s.
2. The hero-level callback (`SetHeroLevels`) is never run: AI heroes learn no skills.
3. Undead and night elf economy/production (undead lumber, night elf entangle and moon-well building).
4. The captain mover (leader and formation), the retreat trigger (needs unit teleport use), the threat scan behind
   `TownThreatened` (record `+0x164`, writer not found), the path-cost unit of `0x6F4758D0`, the mega-target timer and its
   key-building branch, the `+0xB8` field of the captain record, the expansion cache timer.
5. Wave composition after the first wave depends on the same readiness gate.

## 11. S21: undead and night elf economy (2026-10-12)
Haunted mine (acolytes gather beside it), entangled mine (wisps inside), ghoul and wisp lumber and the Graveyard drop-off are modelled; see docs/spec/economy-5.md (disassembly only, no Unicorn). Item 3 of 10.7 is closed: the four-race run on (4)TwistedMeadows now has every race gathering (0 aborted threads).
## 12. S22: why the waves stopped (2026-10-12)

Repro: `w3boot --computer 0:orc --computer 1:human`, (2)Circumvention. Three separate causes, found by logging the AI
native calls (`--ai-log`) and the captain members' state; none of them was in a captain native.
1. **Dead hero blocks the wave loop.** `setup_force` -> `AwaitMeleeHeroes` (common.ai) loops `Sleep(1)` until
   `GetUnitCountDone(hero_id) > 0`. The orc's first hero died in the first fights and nothing revived it (the revive
   path of `SetProduce`, `0x6F2D2D80`, only returned true), so the attack thread slept for the rest of the game
   (`CaptainRetreating` was the last attack native called). Fix: `Game::revive_hero_at` / `dead_hero_of`
   (`production.rs`): `SetProduce` of an own dead hero queues a revive at an altar that can train the type; the queue item
   finishes by reviving (full life, 0 mana, at the altar). **UNSURE model**: cost and time = type cost / build time times
   `0.4 + 0.1 * level` (level 1..10), no food; the original's formula and the restore rules are not decoded (IDA was not
   reachable in this session).
2. **Attacking an invulnerable neutral.** The human group acquired an owner-15 structure with 150000 life and the
   invulnerable ability and swung at it for ever (`CaptainInCombat` true, `lowest hp 1.00`). The sim's attack
   validation had no invulnerability check. Fix (`combat.rs`, from the cut-off S22 branch): `attackable` = valid and not
   invulnerable, used for target retention, the swing gate and acquisition. UNSURE: the original matches the target's
   classification against the weapon `targs` (`0x6F0CBD70` -> `0x6F0C93E0`), not read here.
3. **Stuck group.** A group standing at the nearest approach to a wall got `code 0, one point` (path to its own start)
   from the 400-pop request on every re-plan and never moved (790 units short of the camp, attack order, no route).
   Fix (`orders.rs::plan_route`): such a request is retried once with a 20000-pop budget (UNSURE: the original has
   no such retry that was seen). `GetCreepCamp` now also skips camps whose first living member the search cannot reach
   (`Game::path_reachable`, 200000 pops; stand-in for the unreachable answer of the path cost `0x6F4758D0`).

Other findings: a base attack in `SingleMeleeAttack` needs `has_siege` (orc: 2 catapults/raiders/batriders or level >= 40; human
analogue) and, without air units, daytime 4..12; otherwise the script only goes for creeps and expansions. `CaptainInCombat`
being false for the orc while its group sat idle was correct (no target); the "combat true forever" of the human was cause 2.

Run (release `w3boot`, 18000 steps = 1800 s, SpecDamage): 0 aborted threads. Orc waves 3 (336 s, 1044 s, 1437 s; the last two
target an enemy structure: first the camp, then the human town hall at (1472,-4288) with catapults); human waves 6 (288 ... 829 s).
Kills: p0 -> p1 56, p1 -> p0 25, p0 -> creeps 5, p1 -> creeps 15, creeps -> p0 2 / p1 3. Both AIs attack each other's base area;
no game ended (no player lost all buildings by 1800 s; the sim has no defeat check wired here).
Open: the original revive formula; whether the human attacks the orc base (its last goal is the orc camp area); the
`first 400-pop` stall may hide a pathing defect worth a Unicorn comparison of a stuck request; invulnerable-target rule.
