# Economy and production (Game.dll 1.26a) - analysis notes

Status: PARTIAL. Several core paths are confirmed (resource storage, upkeep tables, food counters, queue capacity, refund keys, research completion). The gold-mine harvest loop, the nearest-hall rule, tree harvesting, the training timer tick, and construction placement are NOT yet traced. Those sections say so and list the next functions to trace.

Labels: CONFIRMED = read directly from the decompile or the PE image, with the address. LIKELY = strong inference from the code shape or a name. UNSURE = guess or not traced.

Method: decompile chunks (scratchpad/decomp), PE image reads (pefile), and a unicorn start-up run for values that the CRT initialisers fill in .data (tools/emu/runtime_consts.py, run from a scratch venv). No project files were changed.

## 0. Global facts

- Gold and lumber are stored in the player object x10 (CONFIRMED, section 1). Food values are stored raw (CONFIRMED, section 3).
- Several economy numbers are data-driven. The DLL reads them by key from the MiscData file (for example UpkeepUsage, UpkeepGoldTax, FoodCeiling, TrainRefundRate). Values are only present in the user's MPQ. Where the binary holds a fallback, the fallback is given. CONFIRMED (key list), LIKELY (MiscData source).
- Ability and unit numbers (harvest carry, mine amount, build time, queue times) are not in the DLL. They come from the unit and ability data tables, which are not in this binary. CONFIRMED that the DLL has no such literals in these paths, LIKELY that they are data-table fields.

## 1. Player resource state

1.1 Natives. SetPlayerState is at 0x6F3C9AA0 and GetPlayerState at 0x6F3C9B00. Both first check the player (0x6F3BD4D0) and a state id in range 0 to 0x19. CONFIRMED.
- Set: state ids 15 and 16 are multiplied by 10 in the native before the write. The write goes through 0x6F3A0EB0. CONFIRMED.
- Get: states 1 and 2 are divided by 10. States 15 and 16 are divided by 10 again. State 0 and states above 2 are not scaled. CONFIRMED.
- Enum names (1 gold, 2 lumber, 4 food cap, 5 food used, 6 food cap ceiling, 15 and 16 gathered totals, and the upkeep rate states) are LIKELY from the standard JASS player-state enum. The repo has no table for them.

1.2 Storage (0x6F3A0EB0, the only caller is SetPlayerState). CONFIRMED.
- Multiplier: 10 for states 1 and 2, 1 otherwise (state 0 is 1).
- Clamp: for states 1 and 2 only, if the scaled value is greater than 10,000,000 (unsigned compare), it becomes 10,000,000. Negative values become 0. So the raw cap is 10,000,000, which is 1,000,000 gold or lumber in script units.
- Writes the result to the player's resource sub-object through 0x6F473140.

1.3 Resource change helpers. CONFIRMED.
- 0x6F416A00 (gold) and 0x6F416A40 (lumber) multiply the amount by 10, call the common changer 0x6F4167D0 with resource id 1 (gold) or 2 (lumber), and divide the result by 10 before returning.
- Negative amounts are charges, positive amounts are refunds or income. Call sites show both directions (for example chunk_001 lines 10422, 11820, 13979, 17641, 26775; refunds at 34065 and 34108).

1.4 Common changer 0x6F4167D0. CONFIRMED structure, UNSURE details.
- Arguments: resource id, the amount already scaled x10, plus three flags (percent path, ally-share path, event path). Its first argument is passed in a register, so the exact signature is not fully pinned down.
- Percent path: when the flag is set, the amount becomes current_resource_in_script_units x amount / 100 (integer). This is a "percent of what you hold" path, not the upkeep tax. An event (0x6F02D660) is raised when the event flag is set.
- Ally-share path: when set, it loops over 12 player slots, skipping the owner, using per-player share percentages stored at offsets 0x1E4 (gold) and 0x1E4 + 0x34 (lumber). The leftover is then given to the owner. LIKELY the "share resources with allies" rule (UNSURE).
- Final write: clamped to 10,000,000 for resource ids 1 and 2 (same clamp as 1.2). CONFIRMED.

1.5 Food room check (0x6F416A80). CONFIRMED structure.
- Returns 1 when the player can afford `food_needed` more: the check is cap >= used + need, where cap is 0x6F40F850 and used is 0x6F40F5D0.
- It is skipped when the owner's flag at unit-object offset 0x30 bit 0x10 is set (LIKELY: unit type flag "does not use food").
- Used by 0x6F023BA0 (see section 3.4).

## 2. Upkeep (income tax by food used)

2.1 Config keys. CONFIRMED.
- UpkeepUsage: 10 food thresholds per race row (read by 0x6F00A2A0, cache at 0x6FA4F740).
- UpkeepGoldTax: 10 floats, one per tier (read by 0x6F00A3B0, cache at 0x6FAAE3C8, returns the entry for the requested tier).
- UpkeepLumberTax: 10 floats, one per tier (read by 0x6F00A4D0, cache at 0x6FAAE3F0).
- **Correction 2026-10-11 (runtime-constants audit):** the cache's "not loaded" marker is the float -1.0 (0x6FAAE464) in the first cell: the reader compares `DAT_6faae464 == DAT_6faae3c8` and reloads only when they are equal. The start-up dump shows the first cell is -1.0 at run time (0.0 in the file image). Once loaded, the cells hold the MiscData values, or the built-in defaults copied from the block at 0x6FAAE378..0x6FAAE39C (start-up values 0.0, 0.3, then 0.6 for the remaining eight tiers).
- Each reader only loads from MiscData when a config table is present (flag check at 0x6F002790). Otherwise it uses the built-in defaults below.
- The tooltip code (0x6F358F80 and 0x6F358F00) shows each tier as "100 minus tax" percent, so the tax value is a percent taken off income. CONFIRMED.

2.2 Defaults.
- UpkeepUsage fallback rows start at 0x6F876148, 10 dwords per row (stride 0x28). The row index is DAT_6FA4F6E8, normally 0 (LIKELY). CONFIRMED values for row 0: 40, 70, then 10000 for the other 8 entries. Row 1: 50, 80, then 10000 x8. Row 2 starts at 0x6F876198 and is not an upkeep row (see 3.3).
- UpkeepGoldTax fallback (CRT-filled, measured with the emulator): tier 0 = 0.0, tier 1 = 0.30, tiers 2 to 9 = 0.60. CONFIRMED.
- UpkeepLumberTax fallback: all zero (the file image and the CRT run both leave 0x6FAAE3A0 to 0x6FAAE3C4 at zero). CONFIRMED.

2.3 Tier selection. LIKELY.
- Food used is the owner's counter (section 3.1). Walk the threshold list in order and stop at the first entry whose threshold is greater than or equal to food used, or at a zero entry. That index is the tier. The gold and lumber tax for that tier are then stored into the player's upkeep-rate states (0x6F40F620).
- With the default row 0: food used 1 to 40 is tier 0 (0 percent tax), 41 to 70 is tier 1 (30 percent, income 70 percent), 71 and above is tier 2 (60 percent, income 40 percent). LIKELY (depends on row 0 being used).
- The 10000 entries make tiers beyond the last real one effectively unbounded.

2.4 Refresh. CONFIRMED.
- 0x6F40F620 recomputes both tax values. It is called from one dispatch site (chunk_018 around line 12912, case 6 of a player-state switch). LIKELY it runs on every food-used change.

2.5 Applying the tax to income. NOT FOUND.
- The only path that reads the stored rate for a resource change is 0x6F4167D0, and that path is the percent-of-holdings path (1.4), not income tax. So the tax is not applied there.
- The income deposit (harvest return, bounty, sell, etc.) is still to be traced. Candidate: chunk_001 line 12520 (a gold add with flags 1, 1, 1). UNSURE.
- Rounding for any tax step: integer division on the x10 values (LIKELY, consistent with the tooltips and the other resource paths).

2.6 FoodCeiling. CONFIRMED.
- The key string FoodCeiling is at 0x6F8761D0. It is read by 0x6F00A210 (index 0), clamped to at least 1, and cached at 0x6FA4F73C.
- Fallback when the key is absent: the dword at 0x6F876198 + index x 4. For index 0 that is 90, for index 1 that is 100. CONFIRMED values, LIKELY the default index is 0.

## 3. Food accounting

3.1 Counters on the player object. CONFIRMED offsets, LIKELY names.
- Food used: the sub-object at +0x90. Getter 0x6F40F5D0. Add with peak tracking: 0x6F40F5E0 (adds to the sub-object, then raises the peak stored at +0x24C if the new value is higher).
- Food cap: 0x6F40F850 returns the minimum of three values: the sub-object at +0x80, the sub-object at +0xA0, and FoodCeiling (0x6F00A210). CONFIRMED structure. LIKELY that +0x80 is the supply made by buildings and +0xA0 is an extra cap (UNSURE).
- Food made add: 0x6F03F970 (the food-made counterpart). Sub-object is UNSURE.
- Stored raw, not x10. The /10 getters 0x6F40F6F0, 0x6F40F710, 0x6F40F730 are for gold and lumber display, not food. LIKELY.

3.2 Per-type values. CONFIRMED.
- The unit type record stores food used at offset +0x5C and food made at +0x60. Getters 0x6F29BAF0 and 0x6F29BB20 read them through the type lookup 0x6F29A8C0.
- Gold cost is at +0x20 (getter 0x6F29B800) and lumber cost at +0x24 (getter 0x6F29B830).

3.3 Unit-level update. CONFIRMED (cross-checked with docs/spec/unit-core.md).
- 0x6F0F7EA0 handles the unit's flags. Bit 0x200 subtracts the type's food used (0x6F29BAF0 through 0x6F40F5E0). Bit 0x800 subtracts the type's food made (0x6F29BB20 through 0x6F03F970). Bits 0x100 and 0x400 add them back on owner change or creation.

3.4 Queue-time food. CONFIRMED structure.
- 0x6F023BA0 is the train/build check: when the target type needs food, it calls 0x6F416A80 and refuses if there is no room.
- 0x6F023BD0 adds (or with a flag, removes) the type's food used to the owner through 0x6F40F5E0. LIKELY called when a unit starts training and when training is cancelled or finishes.
- Exact timing (queue add versus start) is UNSURE.

3.5 SetPlayerState interaction. CONFIRMED.
- SetPlayerState only writes the stored values for the states listed in section 1 (gold, lumber, the gathered totals). It does not touch the food counters at +0x80, +0x90 or +0xA0. Food cap and food used are read-only from script (GetPlayerState for food returns the sub-object values). CONFIRMED.

## 4. Training queue (CAbilityQueue)

4.1 Class and capacity. CONFIRMED.
- CAbilityQueue vtable is at 0x6F87C384 (218 entries). Constructor is 0x6F05DF60. It sets the vtable, then sets the field at object offset +0x28 to 7, and the field at +0x30 to 7.
- So the queue capacity is 7 slots (CONFIRMED value, LIKELY meaning). Some UNSURE overlap with other classes at +0x28, so the meaning is confirmed only for CAbilityQueue.

4.2 Slot layout. CONFIRMED offsets, LIKELY meanings.
- Slot i has a unit-type or upgrade id at +0xA4 + 4i.
- A 3-dword record per slot at +0xC4 + 12i (an id plus a mask; -1 means none).
- Slot progress or flag at +0x11C + 4i.
- Count of entries is the field at +0x28 (the refund code checks index against it).

4.3 Cancel and refund. CONFIRMED structure, data-driven rates.
- 0x6F062320 handles removing slot i. If i is at or beyond the count it does nothing.
- Research slot: refund with the key ResearchRefundRate, then add gold and lumber via 0x6F416A00 and 0x6F416A40.
- Train slot: refund with the key TrainRefundRate using the type's gold and lumber cost (+0x20 and +0x24). If the slot is a revive (id not zero in the revive table), it uses ReviveRefundRate and 0x6F0340A0 for the revive costs.
- Refund math: cost x rate, with the rate read as a softfloat and converted to integer (0x6F6EEE20 and 0x6F6EEC20). Truncation is LIKELY.
- Rate values: data-driven. The fallback is the float at 0x6FAAE470, which is 0.0 in the file image and not touched by the CRT run. UNSURE whether a missing MiscData key means no refund or a default; treat the rate as data.
- The percentage actually shipped by Blizzard is NOT in the DLL.

4.4 Progress timing and spawn. UNSURE.
- The per-tick timer for training is not traced. The research timer (section 5) is the nearest model.
- Spawn position and rally point are not traced.
- Trained-unit event id is not traced.

## 5. Research and upgrades (CAbilityResearch)

5.1 Completion. CONFIRMED structure.
- 0x6F059400 is run per tick for the active research. The active research id is stored at +0x94 and its target time or count at +0x98.
- Progress is read by 0x6F40B340(id, 1). When progress reaches the target:
  - fires event 0x80261 through 0x6F62A570, with the research object,
  - clears the research flag through 0x6F40FD70,
  - shows the completion message (string id 0x80),
  - calls 0x6F298AB0 with the research id, which is the effect-application hook,
  - clears +0x94 and +0x98.
- The cancel or refund path is 6F062320 (section 4.3). The research refund key is ResearchRefundRate, and there is also an UpgradeRefundRate key.

5.2 Costs and levels. UNSURE.
- RelativeUpgradeCost keys appear at 0x6F0E44A0 and 0x6F0F1C70. Level-based cost formulas are not traced.
- The research-to-level effect in 0x6F298AB0 is not traced.

## 6. Gold mine and lumber harvesting

6.1 Classes. CONFIRMED.
- CAbilityGoldMine vtable at 0x6F892454 (190 entries). Its mine state object is MineShaft (vtable with 24 entries). The mine state sub-object is at mine offset +0x70.
- Harvest family: CAbilityHarvestBase (vtable 0x6F8825B4, 209 entries), CAbilityHarvest, CAbilityHarvestLumber, CAbilityHarvestReturn, CAbilityAcolyteHarvest (vtable 194 entries).
- Save and load of mine fields is at 0x6F0A3950 and 0x6F0A74F0 (not the harvest logic). CONFIRMED.

6.2 Mine events. CONFIRMED ids, firing site UNSURE.
- Event 0xDE = mine destroyed, with sound key GoldMineCollapseSound and text key Goldminedestroyed.
- Event 0xDF = mine low, with sound key GoldMineLowSound and text key Goldminelow.
- Routed through 0x6F333400 and 0x6F2F6950. The code that fires 0xDE and 0xDF is not traced.

6.3 Owner duration. CONFIRMED key, LIKELY meaning.
- GoldMineOwnDuration is read in 0x6F0ACB00 (CAbilityGoldMine, vtable slot 32). It sets the mine state to 15 (+0xAC) and starts the owner-duration timer. LIKELY the timer that collapses a mine after its owner leaves.

6.4 Not yet traced (UNSURE). Mine gold amount and decrement, carry amount, trip time, nearest town hall selection and tie-break, tree life per hit, lumber carry capacity, upkeep application to the deposit (see 2.5). The next functions to read: the CAbilityHarvestBase slot 22 (0x6F024E80) and slot 41 (0x6F069490), the CAbilityHarvestReturn helpers, and the mine sub-object vtable slot 0x18 (called by 0x6F0A3860). The "Treeoccupied" and "Minenotentangleable" strings mark the failure cases for trees and mines.

## 7. Construction

7.1 Classes. CONFIRMED.
- CAbilityBaseBuild (vtable at 0x6F87925C, 203 entries), CAbilityBuild (vtable at 0x6F8B677C, 212 entries), race builds CAbilityHumanBuild, CAbilityOrcBuild, CAbilityNightElfBuild, CAbilityUndeadBuild, CAbilityNagaBuild, CAbilityNeutralBuild, CAbilityItemBuild, CAbilityBuildInProgress, and CAbilityRetrain.

7.2 Refund keys. CONFIRMED.
- ConstructionRefundRate is read in 0x6F031090. If the building flag 0x400 is set it returns the fixed value at 0x6FAAE470 (zero in the file image, so no refund). Otherwise it reads the key and multiplies the building cost (through the type lookup at 0x6F472890 and the unit-type vtable slot 0x130).
- ConstructionDamageRefundPenalty is read in 0x6F0240A0 (uses FUN_6F147B00 as a pre-check, then the penalty key).
- CancelBuild and the cancel command table are at 0x6F023BF0. The cancel string keys are CmdCancel, CmdCancelBuild, CmdCancelTrain, CmdCancelRevive.

7.3 Not yet traced (UNSURE). Placement validation (pathing footprint, the owner check for build, the tree and mine checks). The pathing specs in docs/spec/pathing-build.md should be checked first. Construction progress timer, worker behaviour per race (the build order and the "return to work" rule), and the cancel-percent values (data-driven).

## 8. Ordering rules gathered so far

- Charge order for a queued unit (LIKELY): gold and lumber first (negative amounts through 6F416A00 and 6F416A40), then food check (6F416A80), then food used added (6F023BD0), then the slot is written.
- Cancel order (CONFIRMED): slot index check, then refund (cost x rate, gold then lumber), then food removed (6F023BD0 with the removal flag), then the slot is cleared.
- Research completion order (CONFIRMED): event 0x80261, clear research flag, message, effect hook 6F298AB0, clear +0x94 and +0x98.
- Upkeep refresh (LIKELY): food used change, then 6F40F620 recomputes tax tier and stores rates.

## 9. Open items (in priority order)

1. Trace the CAbilityHarvestBase per-tick function and the mine sub-object vtable slot 0x18 (to get mine amount, carry, trip, and nearest hall).
2. Find the income deposit call site and confirm where the upkeep tax percent is applied.
3. Trace the training timer tick, the spawn and rally code, and the trained-unit event.
4. Confirm the owner of the food-made sub-object (+0x80 or another) by reading 0x6F03F970.
5. Trace placement validation and construction progress.
6. Confirm the research effect hook 0x6F298AB0 and the level cost formula.
