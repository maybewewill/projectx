# Pending common natives: findings (pass 1)

Scope: the `NotImplemented` rows of `crates/sim/src/natives/pending.rs` and the corpse, guard, stock and
random-pick natives of the work list. Addresses are from `docs/spec/natives-1.26a.tsv`. Confidence:
CONFIRMED (read from the disassembly and checked), LIKELY (read from the decompile, not checked), UNSURE.

Result of this pass: no native changed from `NotImplemented`. Each one below depends on state the model does
not keep, or on a subsystem that is not read yet. The reasons in `pending.rs` now name the missing piece.

## 1. Player unit counts (`GetPlayerUnitCount` 0x6F3C9820, `GetPlayerTypedUnitCount` 0x6F3C9850, `GetPlayerStructureCount` 0x6F3C98C0)

- CONFIRMED (disassembly): the native resolves the player handle (`0x6F3BD4D0`, returns the player object or 0).
  If the player is missing, or its word at `+0x2D0` is 0, the result is 0. Otherwise the word at `+0x2D0`
  is the counter object, and the count is `+0x60` of that object, plus `+0x5C` when the flag argument is non-zero
  (`0x6F3DC410`: `return [obj+0x60] + (flag ? [obj+0x5C] : 0)`).
- CONFIRMED: `GetPlayerTypedUnitCount` (type, flag1, flag2) returns 0 when the player is missing or has no counter.
  Otherwise it maps the type id through `0x6F3A8D10` (-1 is 0) and walks the type tree with
  `0x6F4038D0` using the flags `0x10`, `0x16` (flag1 set) and `0x08` (flag2 set).
- LIKELY: the `+0x5C`/`+0x60` pair are "units being built" and "completed units" (the flag adds the first),
  but the increment and decrement sites are not found. Until they are, the model cannot tell which units a
  count includes (dead, removed, under construction).
- UNSURE: whether a dead unit is counted; the walk `0x6F4038D0` and the counter update sites are the next step.

## 2. World bounds (`GetWorldBounds` 0x6F3D0D60)

- CONFIRMED (decompile): the native allocates a rect handle as its last step (`0x6F430C80`), like the other
  handle-returning natives. It calls about ten helpers (`0x6F3B9180`, `0x6F3DB8A0`, `0x6F4C8520`, `0x6F001EC0`,
  `0x6F472C20`, `0x6F48AB80`, `0x6F3B6500`, `0x6F00F560`, `0x6F3DB8F0`, `0x6F3A8060`), reads `+0x70` of the
  global object and `+0x54` of the pathing object, and passes the results to the rect builder.
- NOT DONE: the bounds are not the playable area (`location.rs`) by construction; the source of the four
  coordinates is not identified. Implementing it needs those helpers read one by one.

## 3. Random picks (`ChooseRandomCreep` 0x6F3B3660, `ChooseRandomNPBuilding` 0x6F3B3680)

- CONFIRMED (decompile): the natives call `0x6F00F520` (the stream-setup helper, with a fixed argument) and then the
  walks `0x6F29FD90` (creep, stream 32) and `0x6F294740` (neutral building, stream 34).
- The walks filter candidates by creep / neutral building flags of the unit types. The `UnitTypes` registry does
  not carry those flags (they come from the unit data tables). Blocked on the unit type data, not on the RNG.

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
