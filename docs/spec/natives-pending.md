# Pending common natives: findings (pass 1)

Scope: the `NotImplemented` rows of `crates/sim/src/natives/pending.rs` and the corpse, guard, stock and
random-pick natives of the work list. Addresses are from `docs/spec/natives-1.26a.tsv`. Confidence:
CONFIRMED (read from the disassembly and checked), LIKELY (read from the decompile, not checked), UNSURE.

Pass 1 result: no native changed from `NotImplemented`. Pass 2 (2026-10-11) implemented the three player unit count
natives (section 1); world bounds (section 2) and the random picks (section 3) are decoded further and stay pending.

## 1. Player unit counts: RESOLVED (pass 2, 2026-10-11)

Implemented: `GetPlayerUnitCount`, `GetPlayerStructureCount`, `GetPlayerTypedUnitCount` (`natives/player.rs`), on the
counters of `crates/sim/src/unit_counts.rs`. Checked against the original in Unicorn (`tools/emu/unit_counts_emu.py`:
1,900 counter operations over two seeds and 3,800 typed walks, 0 mismatches; vectors in
`crates/sim/tests/vectors/unit_counts_seed1.tsv`).

- CONFIRMED (disassembly): the counter object is `player+0x2D0`. Totals: `+0x54` structures in progress, `+0x58`
  structures completed, `+0x5C` units in progress, `+0x60` units completed. `GetPlayerUnitCount(p, flag)` =
  `+0x60` (+`+0x5C` if the flag is set) (`0x6F3DC410`); the structure native uses `+0x58` (+`+0x54`) (`0x6F3DC430`).
- CONFIRMED: per-type record (created on first touch by `0x6F4036B0`): `+0x1C` completed (flag clear only), `+0x20`
  in progress (flag clear), `+0x24` in progress (flag set), `+0x28` total (completed and finished), `+0x30` class word.
- CONFIRMED: the five update functions and their wrappers (player is `ECX`; wrapper `0x6F40FD30` add completed,
  `0x6F40FD70` remove in progress, `0x6F40FDB0` finish, `0x6F40FDF0` remove completed, `0x6F40FE30` begin in progress).
  Effects: add completed = total+1, `+0x1C`+1 (flag clear), totals+1; remove in progress = `+0x20`/`+0x24`-1, totals-1;
  finish = total+1, in progress-1, `+0x1C`+1 (flag clear), totals: in progress-1 and completed+1; remove completed =
  total-1 and completed-1 (the `+0x1C` count is not decreased); begin = in progress+1 (total unchanged).
- CONFIRMED: class word. `0x6F29D2F0` tests that the type exists in the unit database (any valid type gets class 8 when
  not a structure). `0x6F32CC80` (a flag at `+0xB4` of a type table at `0x6F938A50`) gives class 4 (structure, LIKELY
  the `isbldg` column). `0x6F2B0000` (predicate through `0x6F261CC0` and `0x6F25C820`) gives class 2: not counted in
  the totals. The predicate's meaning is UNSURE.
- CONFIRMED: the typed count `0x6F4038D0`: value of the record (flag 0x10 total; flag 1 `+0x1C`; `+0x20` with flag 2;
  `+0x24` with flag 4), then the first child list (unless flag 0x20), then the second list when flag 8 (with flag 2 on).
  A type with no record still walks its lists. The natives set 0x10, 0x16 (includeIncomplete) and 0x8 (includeUpgrades).
- LIKELY: the list sources. List 1 = tables `0x6F938DA8` (count, `0x6F32D440`) and `0x6F938FA8` (`+0x16C` count, `+0x170`
  array); list 2 = table `0x6F938ED4` (`+0x178`, `+0x17C`) and `0x6F938F54` (entry 0's inner list). The unit column
  behind them is UNSURE (list 2 is likely the `upgrades` column). The sim reads them from
  `UnitTypes::set_count_links` (empty until the engine fills them); the building flag is `UnitTypes::set_building`.
- CONFIRMED: unit registration state `unit+0x64`: bit 0x8 = registered as in progress, bit 0x10 = registered as completed.
  Bits 0x1/0x2 are the state (in progress / complete); in consistent states 1 goes with 8 and 2 with 0x10.
- Creation: `0x6F29F990` (the unit builder, flags 0x502) registers a completed unit: `add completed` with flag 0 (CONFIRMED
  call graph; in the model: `count_add_completed`). Units are not created in progress (no construction model).
- Removal: `0x6F29D320` undoes the registration (in-progress entry if bit 0x8, else completed entry if bit 0x10). Its
  callers `0x6F2A4810`, `0x6F2A4880`, `0x6F2A01C0` sit in the event dispatcher `0x6F2A7E60`; which native reaches
  them is UNSURE (the model removes on `RemoveUnit`).
- Owner change `0x6F2A4BE0` (`SetUnitOwner` calls it with 1): removes the old owner's entry (by bits 0x8/0x10), then adds
  to the new owner (by bits 1/2) with flag 0. Model: `count_move_owner`. CONFIRMED structure, LIKELY the full effect.
- Deaths: `KillUnit` (life 0) does not change the counts. A dead unit stays counted until it is removed (LIKELY; the
  death-to-removal path is not traced).
- Construction: `0x6F179000` (flags 0x101) and `0x6F1793E0` (0x102) call `0x6F29F850` (begin/complete). They are LIKELY the
  construction start and finish of buildings. Not modelled (no construction in the sim).
- UNSURE: the guard `DAT_6FAB65F4` in the typed native (assumed set); the string-to-type rule (the sim takes a
  four-character string as a raw code when that type exists, else a type name; the original reads the converted value
  from the call frame through `0x6F4C4630`).

## 2. World bounds (`GetWorldBounds` 0x6F3D0D60): NOT RESOLVED (pass 2)

- CONFIRMED (decompile of the body): the native calls `0x6F3B9180`, `0x6F3DB8A0` (result stored in a local),
  `0x6F4C8520` (the hash, as in the handle code), `0x6F001EC0` (a global object; its `+0x70` is passed to `0x6F3DB8A0`),
  `0x6F472C20`, `0x6F48AB80` (the pathing object; its `+0x54` is passed to `0x6F3B6500`), `0x6F00F560` and `0x6F3DB8F0`,
  then builds the rect and allocates the handle last (`0x6F430C80`).
- The decompile does not keep the argument flow of these helpers (the values in the rect are set through stack slots the
  export loses). The source of the four numbers is not identified. Candidate (UNSURE): the pathing rect at `+0x54` is
  `(y0, x0, y1, x1)` (pathing-build notes), which would be the grid extent, not necessarily the world bounds.
- Next step: disassemble the native (not the export) and read the four stores into the rect.

## 3. Random picks (`ChooseRandomCreep` 0x6F3B3660, `ChooseRandomNPBuilding` 0x6F3B3680): PARTLY RESOLVED (pass 2)

- CORRECTION: `0x6F00F520` is not a stream setup. Its body is `call 0x6F01F5A0; mov al, [eax+0x1d0]; ret`: a byte getter
  (the arguments are ignored). The stream comes from the draw itself (below).
- CONFIRMED (disassembly): the shared draw `0x6F285320` takes the stream in `ECX` and the count in `EDX`. It takes one
  step of the stream (`0x6F011390`) and returns `high32(r * n)` (`mul`, then a 32-bit shift). `ChooseRandomItem` uses it
  with ECX = 0x23 (stream 35). The creep walk calls it with ECX = 0x20 (stream 32); the neutral walk with ECX = 0x22
  (stream 34). The draw is therefore the one of `ChooseRandomItem`.
- CONFIRMED (decompile + disassembly): the unit-type list is a global linked list (head `DAT_6FAB4468`, next link at
  `DAT_6FAB4460 + 4 + node`). Each node is the type record: rawcode at `+0x14`, level at `+0x68` (`level`), `special` at
  `+0x290`, `isbldg` at `+0x28C`, a race enum at `+0x1C0`, `hostilePal` at `+0x294`, `nbrandom` at `+0x298`, the
  movement type at `+0x1A8`.
- CONFIRMED creep walk (`0x6F29FD90`, level argument): the rawcode's first byte must not be an uppercase letter; race
  enum `+0x1C0` = 8; `isbldg` 0; `special` 0; level = the argument or the argument is -1. The candidates are appended
  to a growable array (`0x6F09EAF0`, CONFIRMED). The walk's end calls the draw with stream 32.
- CONFIRMED neutral walk (`0x6F294740`): race `+0x1C0` = 7; `isbldg` != 0; `special` 0; `hostilePal` 0; `nbrandom` != 0.
  Candidates appended the same way; the draw uses stream 34.
- NOT DECODED: (a) the race enum values 7 and 8 (the `race` column text is read now, `UnitTypeData::race` in gamedata,
  but the text-to-enum mapping is not in any table read so far); (b) the tileset check in the creep walk (a wildcard
  string test through `0x6F6EB690` when the second argument is set, with a Storm import `Ordinal_571`); (c) the movement
  type checks in the creep walk (two calls of `0x6F29C240`, which returns `+0x1A8`).
- Status: the natives stay NotImplemented until (a) to (c) are decoded. Implementing them with a guess would change the
  candidate set on some maps, so no native is registered.

## 4. Corpses and blighted goldmines (`CreateCorpse` 0x6F3C5350, `CreateBlightedGoldmine` 0x6F3CD630)

- The corpse type and the goldmine type are passed in registers the decompile does not show. The corpse flags
  (0x200, 0x1000) and the goldmine amount come from ability and unit data that are not in the registry.
- Both create units through the unit builder (`0x6F24F1F0`), so they also consume a handle id last. The builder
  path is in `unit-core.md`; the missing part is the type argument.

## 5. Guard positions (`RecycleGuardPosition` 0x6F3CB800, `RemoveGuardPosition` 0x6F3CB820, `RemoveAllGuardPositions` 0x6F3CB840)

- CONFIRMED (decompile of the bodies): each checks the unit (`0x6F3BDCB0`) or player (`0x6F3BD4D0`) handle and
  then calls one helper (`0x6F2E5410` for recycle, `0x6F2DCF00` for remove, `0x6F2DCEC0` for remove-all). No
  rng draw and no handle allocation.
- The helpers operate on the guard list of the AI (creep leash). The model has no guard list. Implementing these
  without it would be a fake, so they stay pending.

## 6. Player removal, crippling and owner transfer (`RemovePlayer` 0x6F3C9DC0, `CripplePlayer` 0x6F3C1310, `SetPlayerUnitsOwner` 0x6F3C12F0, `CachePlayerHeroData` 0x6F3C9DE0)

- `RemovePlayer` calls `0x6F41F220` with the result argument after the player check. The argument of
  that call is the game-result record (UNSURE: read from the argument order, not from the callee).
- `CripplePlayer` needs the force enumeration helper `0x6F473170` and the crippling setter `0x6F416670`
  (flag = the boolean argument). The crippled state is not in `PlayerObj`.
- `CachePlayerHeroData` calls `0x6F41B8E0` and `0x6F416B70` (decompile). `SetPlayerUnitsOwner` (0x6F3C12F0) calls
  `0x6F40B1D0` with its integer argument after the player check. None of these callees is read yet.

## 7. Event getters (`GetWinningPlayer` 0x6F3BB3E0, `GetRescuer` 0x6F3C2390, `GetSellingUnit` 0x6F3C2560, `GetOrderTargetDestructable` 0x6F3C2F10)

- Each reads a field of the current event payload. The events that carry them (victory/defeat, rescue, sell,
  destructable order target) have no firing site in the model (see `event.rs`). Without a firing site, the getter
  can only return null, which would hide the missing event. They stay pending.

## 8. Player score (`GetPlayerScore` 0x6F3C9B70)

- CONFIRMED (decompile): the native takes a score kind 0..24 (`param_2 < 0x19`) and reads one of 25 counters
  from the score object (`0x6F40B230`, `0x6F40B240`, `0x6F40B260`, ...). The counters are not kept in the model.

## 9. Selection, stock, cheat, level change

- Selection (`ClearSelection`, `SelectUnit`, `SyncSelections`, `GroupEnumUnitsSelected`): local interface state
  (the local player object's selection set, guarded by its interface flag). They must not change simulation
  state, so they stay pending until a local-player interface object exists.
- Stock natives (`AddUnitToStock` ...): the per-shop arrays of the unit object and the default lists are not
  modelled.
- `Cheat` runs the chat command parser (`0x6F7B0DA0`), `ChangeLevel` the campaign loader (`0x6F3A4C70`). Neither is read.
