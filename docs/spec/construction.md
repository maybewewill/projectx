# Construction (buildings under construction), Game.dll 1.26a

Status: PARTIAL. The payment, the pending refund, the counter and food transitions and the cancel refund
arithmetic are read and checked in Unicorn. **The progress tick, the HP growth law, the placement validation,
the completion event ids and the race differences are NOT found.** Labels: **CONFIRMED** = read from the
disassembly/decompile at the address and (where marked) checked in Unicorn; **LIKELY** = strong inference;
**UNSURE** = guess or not traced.

Sources: decompile export (scratchpad `decomp/`, chunks 000, 001, 004, 007, 012, 013, 017, 018, 019),
RTTI names (`scratchpad/vtables.tsv`, `vfunc_names.tsv`), capstone listings, Unicorn harnesses
`tools/emu/construction_counts_emu.py` and `tools/emu/construction_pay_emu.py`.

## 0. Corrections to earlier passes

| Earlier statement | Correction | Basis |
|---|---|---|
| natives-pending.md section 1: `0x6F179000` (flags 0x101) and `0x6F1793E0` (0x102) are the construction start and finish | **Wrong.** They are the start and finish of an **upgrade in progress**: `CAbilityUpgradeInProgress` vf212 and vf206 (RTTI). The start body sets `this+0x98` to the unit's type and `this+0xA8` to the new type, and tests the instant flag `0x100` of `this+0x20`. Building construction is in `CAbilityBuild` and `CAbilityBuildInProgress` | RTTI names, decompile of `0x6F179000` and `0x6F1793E0` |
| Task premise: the build timer is in `CAbilityBaseBuild` on the 0.1 s tick of economy-2 | **Not confirmed.** `0x6F045600` (vf205 of `CAbilityBuildInProgress`, 125 lines) is the in-progress tick that uses the 0.1 f constant `0x6FAAE4A0` and the aura-style life setters. Its link to the build timer is not shown. economy-4 already corrected the 0.1 s tick for training to the aura pulse | `0x6F045600`, `0x6F05FF50` decompile |
| economy.md 7.2: refund rate in `0x6F031090` | Confirmed as read. The flag `0x400` rule is in the body | `0x6F031090` decompile |
| Pass 1 (sections 5, 7, 11): the in-progress tick `0x6F045600` is "vf205 of CAbilityBuildInProgress", and `0x6F05FF50` is "vf205" | **Wrong names.** `0x6F05FF50` is **vf212** (the start: slot `0x350`); `0x6F060020` is **vf205** (the completion, slot `0x334`); `0x6F045600` is not a vtable entry: it is called by the start (vf212), by the percent setter (vf191, section 12) and by the resume (vf59). The tick updates the owner's life modifiers, it is not a timer | vtable index of `CAbilityBuildInProgress` (`vtables.tsv`, `vfunc_names.tsv`), asm of `0x6F05FF50`, `0x6F060020`, `0x6F054690`, `0x6F054760` |
| Pass 1 (section 7): completion event = CONSTRUCT_FINISH 28 (LIKELY from common.j order) | **Not confirmed.** Completion calls `0x6F2983A0`, which fires the event key `0x80241` (CONFIRMED key, section 12.4). The JASS constant for that key is not in the upload | `0x6F2983A0` decompile, asm of `0x6F060020` |

## 1. Classes (CONFIRMED, RTTI)

- `CAbilityBaseBuild` (vtable `0x6F87925C`, 203 slots), `CAbilityBuild` (`0x6F8B677C`, 212 slots), race
  subclasses `CAbilityHumanBuild` (`0x6F8B7744`), `CAbilityOrcBuild` (`0x6F8B7A9C`), `CAbilityNightElfBuild`
  (`0x6F8B7DF4`), `CAbilityUndeadBuild` (`0x6F8B814C`), `CAbilityNagaBuild` (`0x6F8B84A4`),
  `CAbilityNeutralBuild` (`0x6F8B73EC`), `CAbilityItemBuild` (`0x6F8C3174`).
- `CAbilityBuildInProgress` (`0x6F87A4DC`, 216 slots), `CAbilityBuildQueue` (`0x6F87ED54`, 208 slots).
- The unit's build ability is at `unit+0x1F4` (fixed slot, unit-core.md; LIKELY).
- Dispatch of `CAbilityBuild` = slot 3 `0x6F10D900` (60-line switch, CONFIRMED). Events it handles:
  `0xD0144` (stop: slot 206 `0x6F0F7910`), `0xD0161` (`0x6F0F7790`), `0xD0179` (placement check
  `0x6F0EDA70`, flag 0), `0xD017A` (`0x6F0F7950`, sets `+0x20` bit `0x100`), `0xD01A2` (`0x6F0F7780`),
  `0xD029C` (`0x6F0EDA70`, flag 1: restore), `0xD02A5` (`0x6F0EE250`, placement/start), `0xD02A6`
  (`0x6F0EE4B0`, start notice). Others go to the base `0x6F023CD0`.
- `CAbilityBuildInProgress` slot 3 `0x6F054650`: `0xD01D0` = `0x6F045110(1,0)`, `0xD01D1` = `0x6F045110(0,1)`
  (pause / resume of the in-progress tick; CONFIRMED dispatch, the body of `0x6F045110` not read).

## 2. Payment (CONFIRMED structure; Unicorn 3,000 cases, 0 mismatches)

Placement check `0x6F0EDA70(event, flag = 0)`:
1. `gold = FUN_6F29B800()` (type `+0x20`, goldcost), `lumber = FUN_6F29B830()` (type `+0x24`).
2. Read the stored gold through `0x6F473170` (the ECX object is not identified; LIKELY the stored gold); if
   `gold > stored / 10` → refused. Then the same read for lumber; if `lumber > stored / 10` → refused.
   (The stored values are x10, so the compare is in whole units.) UNSURE which object the second read uses.
3. Covered: `0x6F0D9400(gold, lumber)` pays through the changers `0x6F416A00(-gold)` and `0x6F416A40(-lumber)`
   (each wrapper multiplies by 10 inside, `0x6F4167D0` with flags 0,0,0: no tax, no share).
4. Store the paid amounts at `this+0x94` (gold) and `this+0x98` (lumber); call `0x6F03FEF0`; set bit 31 of
   `this+0x20`.
5. Refused: `0x6F0ED9F0(-1, 0, 0)` shows the message (`0x6F03FF20`) and clears bit 31. Nothing is taken.

Pending refund `0x6F0F7750(-1)` (called by the stop path `0x6F0F7910` for every stop, and by `0x6F0F7780`):
zeroes `+0x94`/`+0x98` and calls `0x6F0ED9F0(-1, gold, lumber)`: message, then (if either is non-zero) the
owner's player index through the owner's vtable slot 25 (`+0x64`), `0x6F3A1650` (set player), and the two
changers with the **plain** amounts. **No rate is applied**: a build cancelled before placement refunds in full.
CONFIRMED (Unicorn 982 refunds, 0 mismatches). Note: the stop path also runs `0x6F26F900` when `+0x20` bit
`0x100` is set (not traced).

Gold and lumber are not taken at the moment the worker reaches the site; they are taken when the order is
accepted at the check above (the placement event `0xD0179` / `0xD02A5`). That the order-start event is the
trigger for the check is LIKELY.

## 3. Registration of the building: `0x6F29F850(unit, flags, old, new)` (CONFIRMED; Unicorn 1,000 cases, 0 mismatches)

Called with ECX = the unit. The original first reads the owner (`unit` vtable slot 59, `+0xEC`) and calls
`0x6F3A1650`. Then, with `unit+0x64` = the registration word `reg`:

- If `flags & 0x40 == 0`:
  - `reg & 8` (registered in progress): `0x6F40FD70(old, 0)` = remove in progress; clear bits `0x8` and `0x1`.
  - else `reg & 0x10` (registered completed): `0x6F40FDF0(old)` = remove completed; clear bits `0x10`, `0x2`.
  - if `flags & 1`: `0x6F40FE30(new, flags & 0x20)` = begin in progress; set `0x9`.
  - else if `flags & 2`: `0x6F40FD30(new, flags & 0x20)` = add completed; set `0x12`.
- If `flags & 0x1000 == 0` (food part, reads the updated `reg`):
  - `reg & 0x800`: food made of the type subtracted (`0x6F03F970(-fmade)`), clear mask `0xC00`.
  - `flags & 0x100`: food used added (`0x6F40F5E0(fused)`), set `0x300`.
  - `reg & 0x200`: food used subtracted (`0x6F40F5E0(-fused)`), clear mask `0x300`.
  - `flags & 0x400`: food made added (`0x6F03F970(fmade)`), set `0xC00`.
  - `reg = (reg & ~clear) | set`.

The wrappers `0x6F40FD30/FD70/FDF0/FE30` call the `unit_counts` methods of unit-counts (`0x6F403770`,
`0x6F4037B0`, `0x6F403860`, `0x6F403890`), already checked (natives-pending.md section 1). The Rust
transcription is `sim::construction::transition`; `apply_counts` maps the effects onto `PlayerCounts`.

Consequences (CONFIRMED from the code, the meaning LIKELY):
- Start of an in-progress building = flags `0x101` (begin, food used), with old = new = the type.
- Finish = remove in progress and add completed (`0x102` for an upgrade; no food made). A building finish
  with the food made needs bit `0x400` (`0x502` is the creation flag set of `0x6F29F990`); **which flags a
  building finish uses is UNSURE**.
- Cancel or removal of an in-progress building = flags `0`.
- `old` is removed and `new` is added: the upgrade uses two types. A plain build uses one type twice.
- The food made of a building is applied at its completion only (`0x400`), not at the start (LIKELY).
- Quirk (CONFIRMED by the code, kept): the in-progress removal uses flag 0 always, so a unit added with the
  flag set (`0x20`) is removed as if it were the flag-clear kind.

## 4. Cancel refund of a building under construction (CONFIRMED structure)

`0x6F031090` (ConstructionRefundRate): if `this+0x20` has `0x400`, the refund is the value at `0x6FAAE470`
(= 0.0). Otherwise it reads the key `ConstructionRefundRate` (`0x6F009EE0`), and if the option check
`0x6F009F90(0)` is set, multiplies the building cost (type through `0x6F472890` and the unit-type vtable slot
`0x130`, then `0x6F0223C0`, UNSURE what it does) by the rate with softfloat. The rounding is the train refund's
arithmetic `F2I(MUL(I2R(cost), rate))` (CONFIRMED for trains, economy.md; `sim::economy::refund_units`). The
key's value is in the user's MiscData (not in the image). `sim::construction::construction_refund` implements it
with the rate as an input. `ConstructionDamageRefundPenalty` `0x6F0240A0`: pre-check `0x6F147B00`, then the
penalty key (not traced further).

## 5. Progress and HP growth (PARTLY FOUND in pass 2, see section 12; the driver of the progress is NOT found)

- The in-progress object `CAbilityBuildInProgress` has the tick `0x6F045600` (called from vf205 `0x6F05FF50`).
  Its body reads `this+0x78` (a float, LIKELY the progress), compares it with 0.0, multiplies with `0.1`
  (`0x6FAAE4A0`) through the owner's vtable slot `0x130`, and uses two-listener life/mana modifiers (`this+0xF8`,
  `+0x100`, `+0xE0`, `+0xE8`). The mapping to bldtm seconds and to HP is not shown.
- The unit life setter path and the progress are not linked yet. The sim's `hp_fraction(progress)` is a
  **placeholder** (linear 0 to 1). Do not treat the HP law as bit-exact.
- Needed next: a dynamic breakpoint on `0x6F045600` under Wine (see the watch list) or a decompile of the
  caller that starts the timer with `bldtm` (`UnitBalance` `bldtm`, type `+0x18`, read by the getter
  `0x6F29B7A0` for the current type only).

## 6. Placement validation and pathing (NOT FOUND, partly)

- Validation: the placement query and its validators are the slot-143/145/156 family of `CAbilityBuild`
  (`0x6F0F7980`, `0x6F0EE1E0`, `0x6F0E2490`), where `0x6F0EE1E0` checks the owner and the type (`0x6F3A35C0`,
  `0x6F0D9510`, optional vf `0x300`). The cell test (pathing flags of the footprint) is not read. Return code
  `0xDD` = not buildable here (CONFIRMED value from the function).
- Pathing update: `0x6F475560` (unit footprint update, pathing-build.md 5.2) erases the footprint rects with
  `0x6F488330(rect, 1)`, runs the placement query `0x6F49A8F0` (radius 0x80), then recomputes with flag 0. Its
  callers include the unit creation natives. Whether `CAbilityBuild` placement calls it is NOT confirmed. The
  stamp of the new footprint into the cell words is not found (pathing-build.md 5.2, UNSURE).
- Not done: the placement footprint size (a type field or the texture; not found), creep and tree blocking.

## 7. Completion event and JASS ids (LIKELY, not in the image)

- Player-unit event ids follow the common.j order that matches the confirmed ones (ISSUED_ORDER 38, DEATH 20,
  TRAIN_START 32 in economy): CONSTRUCT_START 26, CONSTRUCT_CANCEL 27, CONSTRUCT_FINISH 28 (LIKELY from the
  order; common.j is not in the upload). Registration keys: `0x80200 + id` (player-unit).
- The unit-level ids (`0x80200 + id`) do not follow a fixed offset from the player-level ones (death: player
  20, unit `0x35`), so the unit-level ids for construction are UNSURE (the `+37` rule of ISSUED_ORDER and TRAIN
  would give `0x3F/0x40/0x41`).
- Event-response getters (GetConstructingStructure, GetCancelledStructure, GetConstructedStructure) are not
  traced. The completion routine `0x6F2A1F20` (1,015 lines, calls `0x6F29F850`) is the likely place that fires
  the events; not read.

## 8. Race differences (NOT FOUND, partly)

- Each race has its own build class (section 1), so the differences are in the class's virtual functions.
  Not compared yet. The worker's "return to work" rule and the Undead/Night Elf specifics (for example, the
  Night Elf build through `CAbilityNightElfBuild`) are UNSURE. The generic model is the only one implemented.

## 9. Implemented in the sim (crates/sim/src/construction.rs)

- `transition`, `apply_counts`: the registration transition, checked against 1,000 vectors
  (`crates/sim/tests/vectors/construction_counts_seed7.tsv`, `crates/sim/tests/construction.rs`).
- `place_check`, `PendingCharge::{record, refund}`: the payment and pending refund (checked in Unicorn, 3,000
  cases; the Rust version is covered by unit tests, not by the same vectors).
- `construction_refund`: the cancel arithmetic with the rate as an input (UNSURE the rate and the cost scale).
- `ConstructionTimers`: a construction on `TimerQueue` (completion = build time in seconds, UNVERIFIED),
  `progress` and `hp_fraction` (placeholders).

## 10. Verification

- `tools/emu/construction_counts_emu.py --cases 2000 --seed 1`: 2,000 cases, 9,521 logged calls, 0 mismatches;
  `--cases 1000 --seed 7 --vectors ...`: 1,000 cases, 4,837 calls, 0 mismatches (the file is the test vector).
- `tools/emu/construction_pay_emu.py --cases 3000 --seed 1`: 2,018 placements (1,649 paid), 982 pending refunds,
  0 mismatches.
- Rust: see the commit message for the test counts.

## 11. Open items (priority order)

1. The progress timer and the HP law: a dynamic breakpoint on `0x6F045600` (and on the caller that arms the
   build timer) under Wine, reading `this+0x78` and the life written.
2. Which building flags a finish uses (`0x502`-style with the food made, or `0x102`), from the completion routine
   `0x6F2A1F20`.
3. The stored-resource object of `0x6F473170` (gold vs lumber, the ECX object).
4. The placement validators (`0x6F0F7980`, `0x6F0E2490`) and the footprint size.
5. The completion event ids and getters; the unit-level construct ids.
6. Race differences (the virtual functions of the race classes).

## 12. Pass 2 (2026-10-11): the percent setter, completion, and the open driver

Scope of this pass: dynamic checks (Unicorn) of the in-progress percent handler, its completion branch and its
tick gate, plus the completion event key. The timer that drives the progress was NOT found, so the build time
in seconds still does not map to the progress (section 5 remains open for the timer).

### 12.1 Class slots (CONFIRMED, RTTI names `vfunc_names.tsv`, index `vtables.tsv`)
- `CAbilityBuildInProgress` vf212 `0x6F05FF50` = start (slot `0x350`): sets the owner's flag `0x2000000` on the unit
  (`+0x5C`), calls `0x6F056190`, then the tick `0x6F045600`.
- vf191 `0x6F054690` = **percent setter**, one integer argument `p` (slot `0x2FC`), section 12.2.
- vf205 `0x6F060020` = **completion** (slot `0x334`), section 12.3.
- vf59 `0x6F054760` = resume: `0x6F0485C0()`, then if flag `0x800` is clear and `0x6F022060()` is false the tick
  runs and it returns; otherwise it calls the completion (slot 205). CONFIRMED structure (asm/decompile, not run).
- vf3 `0x6F054650` = dispatch: `0xD01D0` pause (`0x6F045110(1,0)`), `0xD01D1` resume (`0x6F045110(0,1)`).

### 12.2 The percent setter `0x6F054690(p)` (CONFIRMED in Unicorn, 1,515 vectors, 0 mismatches)
Owner = `this+0x30` (fallback `0x6F472890`). With `x = I2R(p)`:
1. `0x6F025BD0(p)` is called first (not read, its body is not part of the life law).
2. `t2 = x / 100.0`, `t3 = 0.1 + t2` (soft-float ops `0x6F6EFE10`, `0x6F6EF000`; constants `0x6FAAE574` = 100.0,
   `0x6FAAE4A0` = 0.1).
3. The owner's slot `0x130` (`CUnit::vf76`, `0x6F28B080`) is called with the out pointer and returns a pointer to a
   float `A`. The result is `life = A * t3` (soft MUL, `A` first). `A` is read through the current agent
   (`0x6F4773E0` copies the agent float at `+0x84`); what that float is in game terms is UNSURE.
4. The owner's slot `0x124` (`CUnit::vf73`, `0x6F28B1F0`, clamps at zero) is called with the life pointer.

So the life written for progress `p` is `A * (0.1 + p/100)` (truncating soft-float). With the probe owner value
`A = 1000.0`: p = 0 -> 100.0, p = 1 -> 109.99999, p = 50 -> 599.99994, p = 100 -> 1099.99988 (before the
completion). The 0.1 start fraction is therefore a **10% start** of the stored value only if `A` is the maximum
life; the clamp of the setter is not checked and `A` is not identified as the maximum life.
Rust: `sim::construction::percent_life(factor, p, tables)`; vectors `crates/sim/tests/vectors/construction_progress.tsv`
(factor bits, p, life bits, completion, tick), test `crates/sim/tests/construction_progress.rs` (needs `GAME_DLL`).

### 12.3 Branches on p (CONFIRMED in Unicorn with the gate `+0x48` = 0 and 1)
- The **tick gate** is `this+0x48` (a pause counter: `0x6F045110` changes it, the tick `0x6F045600` tests `< 1`).
- `p != 100` and gate `<= 0`: the tick `0x6F045600` runs after the setter (Unicorn: tick called).
- `p == 100` and gate `<= 0`: the completion (slot 205, `0x6F060020`) runs, no tick (Unicorn: completion called).
- `p == 100` and gate `> 0`: the flag `0x800` is set (`+0x20`) and nothing else runs; the completion is deferred and
  the resume (vf59) runs it when the gate clears (asm, not run).

### 12.4 Completion `0x6F060020` (CONFIRMED structure, asm and decompile)
- Owner flags `+0x5C` bit `0x2000000` cleared; two agent cancels (`0x6F477D30`); the owner's slot 59 read; then
  `0x6F28D490`/`0x6F28D4B0` tests, and `0x6F28B740(owner)`, which calls `0x6F024F00` and `0x6F46D1B0` (the unit
  list update, not read).
- Event: `0x6F2983A0(unit)` tests for listeners on key **`0x80241`** (`0x6F62A580`), and if any exist fires
  `0x80241` with the unit as trigger (`0x6F285E10`). This is the only caller of `0x6F2983A0`. So **completion fires
  key `0x80241`**. It is a 0x80200 + 0x41 key; the common.j constant is not in the upload, so the JASS name is UNSURE.
  The construct-finish id 28 of section 7 is NOT confirmed by this key.

### 12.7 The base percent setter `CAbilityBaseBuild` vf191 `0x6F0239B0` (CONFIRMED in Unicorn, 6,180 vectors, 0 mismatches)
- `CAbilityTrain` does not override slot 191, so the train runs this setter too (vtable index, section 12.1 method).
- `0x6F0239B0(p)` (ECX = this, one stack argument, ret 4): `q = p` if `p` is below 100 as an **unsigned** value,
  else `q = 100` (so negative `p` gives 100). `rest = I2R(100 - q)`, `f = rest / 100.0`, and the value
  `this+0x78 * f` (soft MUL) is written through `0x6F477350` on the embedded object at `this+0x7C`.
- Result: **the remaining part of the stored total**: `written = total * (100 - q) / 100` (truncating soft-float).
  Probe with total 1000.0: p = 0 -> 1000, p = 1 -> 990, p = 25 -> 750, p = 99 -> 10, p = 100 -> 0, p = 150 -> 0.
- Rust: `sim::construction::remaining_after_percent(total, p, tables)`; vectors `tests/vectors/base_progress.tsv`.
- So `this+0x78` is a total (for a train, LIKELY the train time `bldtm` of the head; UNSURE) and `this+0x7C` holds the
  remaining value. The object at `+0x7C` is a FloatMini-like property (the setter is the same as the one in
  abilities-2 section 2), and its listeners are not traced; the timer or modifier that reads the remaining value is
  the open link.

### 12.5 What is still open (priority)
0. **Who calls the percent setters** (vf191 of the build base and the construction override, both with `p` in 0..100)
   and the object at `this+0x7C` (its listeners, which timer it arms).
1. **Who calls the percent setter, and with which `p`**. The slot `0x2FC` calls found in the decompile belong to other
   classes (buffs, abilities). A dynamic breakpoint on `0x6F054690` under Wine, with the call stack, is the way.
2. The timer or message that advances `p` (the 0.1 s agile timer is the candidate, not confirmed).
3. The meaning of the owner's slot-0x130 factor `A` (the agent `+0x84` float) and the setter's clamp.
4. The JASS constant of key `0x80241` (`common.j`).
5. Train: `CAbilityTrain` runs the base setter (section 12.7) with its own `+0x78` total; the driver is open (economy-4 section 9).

### 12.6 Verification
- `tools/emu/construction_progress_emu.py`: setter vectors (1,515: 9 + 6 factors × 101 values), gate probes
  (completion at p = 100, tick at p = 37): 0 mismatches against `construction::percent_life`.
- `crates/sim/tests/construction_progress.rs`: passes with `GAME_DLL` (1,515 vectors).
