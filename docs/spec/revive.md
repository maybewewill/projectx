# Hero revive, unit counters at death, melee defeat (S27)

Game.dll 1.26a. Written from disassembly/decompile and Unicorn runs; no game bytes are quoted.

## 1. Revive cost (`0x6F0340A0`) and time (`0x6F030CF0`) — CONFIRMED (Unicorn)

Harness `tools/emu/revive_cost_emu.py` runs the real code with the Misc readers (`0x6F009EE0` real, `0x6F009F90` integer),
the hero level getter (`0x6F2A0210`), the type cost getters (`0x6F29B800` gold, `0x6F29B830` lumber) and the type build time
(`0x6F29B7A0`) hooked; the soft-float helpers run for real. 400 random cases (levels 0..60, random factors including -0,
huge limits, type costs up to x10): **0 mismatches** on gold, lumber and time (`crates/sim/tests/revive_vectors.rs`,
vectors in `crates/sim/tests/revive_vectors/data.rs`; needs `GAME_DLL` for the reciprocal table).

* Cost factor `t = ReviveBaseFactor + I2R(level - 1) * ReviveLevelFactor`, then `f = (ReviveMaxFactor <= t) ? ReviveMaxFactor : t`
  (x87 compare, unordered picks the cap). Lumber uses `ReviveBaseLumberFactor` / `ReviveLumberLevelFactor` and the same
  `ReviveMaxFactor`.
* Gold = `F2I(I2R(F2I(DIV(I2R(typeGold) * f + 0.5, 5.0))) * 5.0)` (constants `0x6FAAE4D0` = 0.5 and `0x6FAAE514` = 5.0 at
  run time), limited by `HeroMaxReviveCostGold` as an **unsigned minimum**; lumber the same with the lumber type cost and
  `HeroMaxReviveCostLumber`. The values are raw (the changers `0x6F416A00/40` multiply by 10 inside).
* Time: `t = I2R(level) * ReviveTimeFactor`, capped by `ReviveMaxTimeFactor` like the cost factor;
  `secs = I2R(F2I(DIV(I2R(buildTime) * t, 5.0))) * 5.0`; raised to at least 1.0; then limited by `HeroMaxReviveTime`
  (`secs <= max ? secs : max`). Result is a real in seconds.
* Code: `sim::revive::{cost, time}`; Misc keys in `gamedata::misc::ReviveMisc` (absent key = 0.0 / 0).

## 2. Revive start and finish — LIKELY (decompile read, not run)

* Order start `0x6F061AA0` (the altar's revive slot): pays gold/lumber through the changers (`-gold`, `-lumber`), takes **no
  food** on the hero branch (the `-(a3 != 0)` count change belongs to the non-hero branch `0x6F033F70`), registers the queue
  entry and arms the timer for the time above.
* Finish `0x6F061E70` (and `0x6F059E20`, the immediate variant): the dead hero object is moved to **the altar's position** (not
  next to it) by the unit vtable slot 96 (`+384`); then restore through `0x6F2A64A0`: life = `HeroReviveLifeFactor` times the
  maximum life (`vf304` with the property getter), mana = the type's starting mana when Misc `HeroReviveManaStart` is non-zero,
  else `HeroReviveManaFactor` times the maximum mana. `0x6F2A64A0` keeps the life at least 0.5 (x87 compare against `0x6FAAE4D0`)
  unless `|life| < 0.001` (`0x6FAAE47C`), in which case it reads the property getter instead (UNSURE which value).
  The hero level and experience are untouched (the object is only un-killed).
* Sim: `Game::revive_hero_at` pays and times by the formulas of section 1; `revive_unit` places the hero at the altar, restores
  as above (a life factor below 0.001 keeps the maximum, UNSURE) and re-registers the hero in the owner's counters
  (`count_add_completed`).

## 3. Unit counters leave at death — CONFIRMED structure

`0x6F2A4880` (the death handler that also runs the kill-experience chain `0x6F2A46F0`) ends in `0x6F29D320`, the leave-play
routine. `0x6F29D320` removes the unit from the owner's counters (completed entry flag `0x10` -> `0x6F40FDF0`, in-progress flag `0x8`
-> `0x6F40FD70`), refreshes food-like counters and the regeneration modifiers. So `GetPlayerStructureCount` falls **at death**,
not when the corpse is removed (the same routine is also on the removal path `0x6F2A01C0`). The sim removed the entry only in
`RemoveUnit`, so Blizzard.j's `MeleeCheckLostUnit` (`GetPlayerStructureCount(owner, true) <= 0`) never saw a count of 0 for a
player whose last buildings died. Fixed in `natives::unit::set_life` (`count_unregister` at the death), plus
`Game::production_unit_died`: an upgrade in progress of a unit that dies leaves its in-progress entry (else the target type
stayed counted as a structure; LIKELY, the original's ability removal cancels it). A train queue of a dying building is not
cancelled yet (open).

## 4. Melee defeat and victory — behaviour check

Blizzard.j `MeleeInitVictoryDefeat` registers per player: construct start/cancel, unit death (`MeleeTriggerActionUnitDeath`),
defeat, leave, alliance change triggers and a 2 s timer. The death action calls `MeleeCheckLostUnit`, which calls
`MeleeCheckForLosersAndVictors` when the owner's structure count is 0; losers get `MeleeDoDefeatEnum` (`CachePlayerHeroData`,
`RemovePlayer(p, DEFEAT)`, cripple and dialog natives), victors `MeleeDoVictoryEnum`. All natives involved exist; with the
counter fix the chain runs.

`w3boot --wipe SLOT@STEP` (test aid; repeatable) sets every structure of SLOT to zero life through the normal death path
(`ScriptHost::kill_units_zero_life`). `w3boot` now prints `result pN: game result R, slot state S, structures C` per player.

Runs on (4)TwistedMeadows, orc/human/undead/night elf computers, release build:
* 36000 steps (3600 s), no wipe: 0 aborted threads, nobody defeated: structures p0 24, p1 26, p2 15, p3 3, results all "none". Kills p0 -> p2 4, p0 -> p3 23, p2 -> p0 3, p3 -> p0 22.
* `--wipe 3@8000`: p3 defeated (result 1, slot state 2), others unchanged.
* `--wipe 3@8000 --wipe 2@8100 --wipe 1@8200`, 9000 steps: p1, p2, p3 defeated, **p0 victory** (result 0), the victory/defeat texts
  are displayed.

## 5. Stuck-path retry and camp reachability removed — decoded

* The request driver `0x6F49B9C0` calls the dispatcher `0x6F49A800` exactly once per request (gated by `0x6F49ACA0`, a per-walk
  request budget; it adds the pops used to a per-walk total). After the call it compares the **last path point with the
  goal**: equal clears flag `0x20000000`, different sets it (a partial path) and returns 1. There is no second request with a
  larger budget. The follower (`0x6F49DAC0` and the waypoint-reached code) asks again from the new position only when the
  stored list is exhausted, and a failed re-plan leaves a wait of at least 20 walks (movement-7.md section 4). The invented
  20000-pop retry of S22 is removed (`orders.rs::plan_route`).
* `0x6F2E1D50` (the camp usability test) walks the camp's 12 member slots and tests flag bits `0x1000000` / `0x4000000` with
  `0x6F27A320` against the asking player: invisibility/vision style checks. It does no path test.
* `0x6F4758D0` -> `0x6F475560` -> `0x6F49A8F0` is the **placement query** (nearest free cell search around the camp member with
  a size and a limit, the same one that unit creation uses), not a ground path search. The cost the camp chooser uses is that
  query's result; "beyond the horizon" means no free cell within the limit. S22's `path_reachable` (a full A* with 200000 pops)
  has no counterpart and is removed. The sim's camp cost stays the straight distance stand-in (UNSURE, ai-natives.md 9.2).
* Consequence: a group may stand at its nearest approach to a wall, like in the original; the AI relies on its own timeouts.
  In the 4-AI run above no group is stuck for more than a few waves; the earlier (2)Circumvention stall is not re-measured here.

## 6. Open

* Pop/path timing of partial paths when the nearest node is the start (does the original return code 0 and a one-point path, or
  flag a blocked state in the follower?). Needs a dynamic check.
* Revive: cancel refund (`ReviveRefundRate`, `0x6F0340A0` reused by `0x6F062320`), food, the restore call order into `0x6F2A64A0`
  are not run in Unicorn; the dead hero's death-time bookkeeping (e.g. experience) is untouched.
* Train queue of a dying building: in-progress entries are not released.
