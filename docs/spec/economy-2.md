# Economy pass 2 - harvest, training tick, construction (Game.dll 1.26a)

Status: PARTIAL. Harvest carry and deposit are traced. Nearest-hall selection, the mine decrement amount, the lumber per-trip value, the training tick body, spawn/rally and construction placement are NOT yet confirmed. Labels as in economy.md: CONFIRMED / LIKELY / UNSURE.

Correction to pass 1: CAbilityHarvestBase vtable slot 22 (0x6F024E80) is a type getter (it returns a static descriptor at 0x6F8772C0), not harvest logic. The deposit logic is at 0x6F047C80 instead.

## 1. Gold and lumber harvest

1.1 Carry state (ability object). CONFIRMED offsets.
- Carried amount: +0x1C4 (0 means empty). Carry flag: bit 0x100 of +0x20 set means carrying lumber, clear means gold.
- Per-trip capacity: +0x194 for gold, +0x198 for lumber. These are the second and third fields in the harvest ability (loaded from data, not in the DLL, LIKELY).
- Owner unit pointer: +0x30. Harvest target object: +0x1D8 (the mine or tree).

1.2 Carry add (0x6F055610, args: ability, amount, lumber flag). CONFIRMED.
- If the ability is already carrying the other resource, it first calls the deposit (0x6F047C80) and resets the carry.
- New carry = old carry + amount, capped at the per-trip capacity (+0x194 or +0x198).
- Sets or clears bit 0x100 for lumber/gold, stores the new carry, and returns true when the capacity was reached (the cap check). This is the "full" signal.
- Calling with amount 0 is used to test or reset the flag (CONFIRMED from the call at 0x6F060890).

1.3 Lumber swing tick (0x6F060780). CONFIRMED structure.
- Reads the swing interval from +0x1D4, floored at 0.01 (the constant at 0x6FAAE480, which is 0.01 as float). This is the per-swing timer.
- Fires event 0xD01B2 (the swing/animation event) through 0x6F4778F0.
- Gets the harvest target (0x6F0475F0, reads a flag bit 0x80 on the order object, and the target id via 0x6F4786B0).
- Calls the target's virtual slot 0x120 to get the hit result. On a successful hit it calls 0x6F055610 with amount 1 on the carry object at +0x1D8.
- So one successful swing adds 1 unit of lumber to the carry. The per-swing tree damage and the tree's own life are NOT in this function. They are read by the target's slot 0x120 (not traced). UNSURE for damage per swing; CONFIRMED that the carry step is +1 per hit in this path.
- Sets bit 0x4000 on +0x20 after a successful hit.

1.4 Gold trip check (0x6F060890). CONFIRMED structure.
- Looks up the gold mine ability by the four-character id 'Agld' (0x41676C64) through 0x6F0787D0. Its remaining gold is read at mine offset +0x6C.
- Picks a per-trip amount from the worker's +0x194 (gold), or 1 when the lumber flag is set, then gets the mine via 0x6F09E220 and calls 0x6F055610 with amount 0 to test the carry. If the mine's remaining gold is below the threshold from 0x6F009F90, it fires the 'low' advisor ping (key PingAdvisorColor). The mine remaining value is therefore the number the decrement reduces, but the decrement site is not yet traced.
- 'Agld' is confirmed as the gold mine id used by the DLL. 'Agmb' (the second mine id from the task) does not appear in the decompile as a literal. UNSURE whether it is used by this DLL.

1.5 Deposit (0x6F047C80). CONFIRMED structure.
- If the ability has no carry (+0x1C4 == 0), it returns.
- Gold path (bit 0x100 clear): fires event 0x23 (gold deposit), calls the owner's virtual slot 0x8C with (4, 0, 1), reads the carry through the owner's slot 0xEC, sets the owner's player index from 0x6F3A1650, and calls 0x6F416A00 with the carried amount and flags 1, 1, 1.
- Lumber path (bit 0x100 set): fires event 0x24, then uses 0x6F2D3290 as a check, then calls 0x6F416A40 with the same flags.
- If the credited amount is positive and 0x6F3A3660 says the player should see it, it shows floating text through 0x6F025050 (the GoldTextColor / LumberTextColor keys).
- Clears the carry at +0x1C4 and runs the cleanup 0x6F021270.
- The three flags 1, 1, 1 mean: the percent path, the ally-share path and the event path are all on in the changer (see section 2).

1.6 Callers of 0x6F416A00 and 0x6F416A40 (gold and lumber changers). CONFIRMED list.
- About 35 call sites in chunk_001, chunk_004, chunk_018. Negative amounts are costs (train, build, revive), positive amounts are income or refunds. Two positive call sites in chunk_001 (around 34065 and 34108) are refunds.
- The deposit site at 0x6F047C80 (chunk_001 around 12520) is one of the income callers, with flags 1, 1, 1.

1.7 Nearest town hall. NOT TRACED. The deposit path has no explicit hall search. The hall is the owner unit reached through +0x30 and the virtual call. The actual nearest-hall choice is likely in the movement/order code (0x6F0475F0 target finder, 0x6F0471F0 target validation, which checks flag bit 0x40 on the target and a route via 0x6F26D590). Ties not determined. UNSURE.

1.8 Income tax (upkeep) application. PARTIAL.
- Deposit calls 0x6F416A00/0x6F416A40 with flagA = 1 (percent path). In 0x6F4167D0 the percent path computes an amount from a stat read on the calling object (the object in the ECX register at the call), then subtracts that part from the amount before crediting (remainder credited, event raised for the remainder when the event flag is set).
- Calling convention (CONFIRMED from disassembly): the wrapper pushes the resource id 1 or 2, the scaled amount (amount x 10), then the three flags, and leaves ECX as the caller's object. So the changer's first stack argument is the resource id. In the pass-1 spec I wrote the mapping loosely; this is the correct order.
- LIKELY that this percent path is the upkeep tax on income (the stored tax rate written by 0x6F40F620 is what the stat holds). Not confirmed: the scale (divide by 10 for some ids) and which object is in ECX have not been checked. UNSURE. Next step: find the ECX object for the deposit call and compare with 0x6F40F620's writes.

1.9 Mine decrement and depletion. PARTIAL.
- Mine remaining gold is at +0x6C of the 'Agld' ability object (CONFIRMED read). Decrement site not found yet. The depletion events are 0xDE and 0xDF (pass 1). UNSURE where they fire.

## 2. Changer 0x6F4167D0 (correct argument order)

- Inputs: resource id (1 gold, 2 lumber), amount x 10, flag A (percent path), flag B (ally share), flag C (event on remainder).
- Amount 0 returns 0.
- Flag A: amount becomes (stat x scaled amount) / 100, where the stat is read from the calling object and divided by 10 only for two special ids. The result is subtracted from the amount.
- Flag B: loops over the 12 player slots (skipping the owner), using the per-player share percentages at +0x1E4 (gold) and +0x1E4 + 0x34 (lumber). LIKELY ally sharing.
- Flag C: when the remainder is positive, raises event 0x6F02D660 with the remainder.
- Credits the remainder to the stat, clamped to 10,000,000 for resource ids 1 and 2.

## 3. Training tick (CAbilityQueue / CAbilityBuildInProgress)

3.1 Timer. CONFIRMED.
- The 0.1 second constant is at 0x6FAAE4A0 (value 0.1 as float, from the CRT start-up run). Timers are started with 0x6F4778F0 using that constant and event 0xD01C1 (periodic tick). Event 0xD02A8 starts the periodic timer, 0xD01C1 is the tick handler (0x6F053DC0).
- Functions 0x6F053D80 and 0x6F053DC0 are the start and tick handlers. Which class owns them is not yet confirmed (LIKELY the construction or training progress object).

3.2 Progress accessor. CONFIRMED signature only.
- 0x6F40B340 and 0x6F40B370 take (object id, stat index). Used with the stat indices 0xB and 0x12 for the progress or time data. Meaning of the stat indices is UNSURE.
- The research completion (pass 1, 0x6F059400) uses 0x6F40B340 with index 1 as its progress count.

3.3 Cost and food timing. PARTIAL (see pass 1 section 3.4).
- Food room check (0x6F416A80) runs at queue time through 0x6F023BA0. Food used is added through 0x6F023BD0 (positive). Costs are charged through 0x6F416A00/40 with negative values.
- Exact order (queue add versus training start) is still UNSURE.

3.4 Spawn position and rally. NOT TRACED.

## 4. Construction placement and build timer

4.1 Command table. CONFIRMED.
- 0x6F023BF0 is the cancel and build command table (CmdCancel, CmdCancelBuild, CmdCancelTrain, CmdCancelRevive, CmdBuild, CmdBuildHuman, CmdBuildOrc, CmdBuildNightElf, CmdBuildUndead, CmdBuildNaga).
- 0x6F0240A0 (ConstructionDamageRefundPenalty) runs a pre-check (0x6F147B00) and then reads its key.

4.2 Placement validation and build timer. NOT TRACED in this pass. The pathing specs (docs/spec/pathing-build.md) are the place to start; the build timer is probably in the CAbilityBaseBuild progress object, using the 0.1 s tick from section 3.1 (LIKELY).

## 5. Ordering rules (new)

- Harvest hit (CONFIRMED): swing timer, target hit (slot 0x120), carry add of 1 (lumber path), capacity check, then the swing event.
- Deposit (CONFIRMED): event 0x23 or 0x24, owner sounds, changer with flags 1, 1, 1, floating text, carry cleared.

## 6. Open items

1. Find the ECX object for the deposit call (the player or the stat object) and confirm the upkeep tax scale in 0x6F4167D0 flag A.
2. Find the mine decrement site (writes to 'Agld' +0x6C) and the lumber per-trip value.
3. Find the nearest-hall rule in the order or movement code (around 0x6F0475F0 and 0x6F0471F0).
4. Trace the training tick owner of 0x6F053DC0 and the spawn/rally code.
5. Trace construction placement (pathing) and the build timer.
