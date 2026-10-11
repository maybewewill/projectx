# Economy pass 3: harvest, deposit, upkeep, refunds, training (Game.dll 1.26a)

Status: harvest carry, mine withdrawal, deposit, income tax (upkeep), cancel refund and the lumber swing are
DECODED from the instructions (CONFIRMED) and implemented in `crates/sim/src/economy.rs`. Training progress,
spawn and rally, and the nearest-hall choice are NOT decoded in this pass (section 7).

Labels: **CONFIRMED** = read from the disassembly (address given) and, where marked, reproduced in Unicorn.
**LIKELY** = strong inference from code shape or a name. **UNSURE** = guess or not traced.

Sources: the decompile export already in the scratchpad (chunk files, `index.tsv`), and capstone listings of
each function from its entry (`scratchpad/py/dis.py`, not in the repo). The decompiler loses ECX arguments
and misplaces some stack slots, so every control-flow claim below was checked against the instructions.

## 0. Corrections to passes 1 and 2

| Earlier claim | Correction | Basis |
|---|---|---|
| economy.md 1.4 / 2.5: the changer's "percent path" is a percent of holdings | It is the **upkeep tax**: the rate is a stored percent (integer), and the tax is `rate * amount / 100` of the amount **earned**, subtracted from the income | `0x6F4167D0` (flag A) reads the rate at object `P + (R+16)*16`; `0x6F40F620` writes that rate (CONFIRMED) |
| economy.md 2.5 / economy-2 1.8: the tax is not applied to income | It is applied on every deposit through flag A (deposit passes flags 1,1,1) | `0x6F047C80` calls `0x6F416A00/40(amount, 1, 1, 1)` (CONFIRMED) |
| economy-2 1.3: a lumber hit adds **1** | A hit adds the int at `this+0x1D8`, which is set from data at ability init (`0x6F0557D0`, f2i of a float field). The per-hit value is data; 1 is only the common case | `0x6F060780` pushes `[esi+0x1D8]` as the amount of `0x6F055610` (CONFIRMED, asm) |
| economy-2 1.3: `+0x1D8` is the harvest target object | `+0x1D8` is the **per-hit lumber amount** (int). The target is `0x6F0475F0` | `0x6F0557D0` (CONFIRMED), `0x6F060780` (CONFIRMED) |
| economy.md 6.2 / economy-2 1.4: `0x6F02D660` raises an event | It adds to a stat on its ECX object, with a clamp only for keys 1 and 2. It is a statistic counter, not an event. Not modelled | `0x6F02D660` body (CONFIRMED); which stat is UNSURE |
| economy-2 1.4: the mine is taken by `0x6F09E220` | Confirmed, and it is the **mine decrement** (`remaining -= taken`, `taken = min(request, remaining)`) | `0x6F09E220` (CONFIRMED) |
| economy-2 1.2: the carry-add amount is unclear | `0x6F055610(worker, amount, lumber_flag)`: the amount is the taken/hit value, the flag 0 = gold, 1 = lumber (CONFIRMED) | `0x6F060890` push order, `0x6F060780` push order |

## 1. Resource store and the changer

### 1.1 Layout (CONFIRMED)
- The stored gold and lumber values are x10 and live at player object `P + (R+4)*16`, with R = 1 gold, 2 lumber:
  `P+0x50` gold, `P+0x60` lumber. Read/write helpers `0x6F473170` / `0x6F473140` take the sub-object in ECX.
- Upkeep rates (percent, integer): `P+(R+16)*16`: `P+0x110` gold, `P+0x120` lumber. Written by `0x6F40F620`.
- Ally share table: `P+0x1E4 + 4*i` (gold, i = 0..11) and `P+0x1E4 + 0x34 + 4*i` (lumber). LIKELY ally sharing.
- Owner slot: byte at `P+0x30` (CONFIRMED).
- Cap: 10,000,000 in x10 units (`0x989680`). Negative results clamp to 0 (the `sar/not/and` sequence, CONFIRMED).

### 1.2 Wrappers (CONFIRMED)
`0x6F416A00` (gold) and `0x6F416A40` (lumber): amount x10, call the changer with `(R, amount*10, flags)` and
return `credited / 10`. ECX = the player object. The three flags are the arguments after the amount.

### 1.3 Changer `0x6F4167D0(P; R, A10, fA, fB, fC)` (CONFIRMED structure from the instructions)
1. `A10 == 0`: return 0.
2. Tax (fA): `tax10 = (rate[R] * A10) / 100`, rate = the value at `P+(R+16)*16` (divisor 1 for R = 1, 2; the
   `(R+12) in {1,2}` test gives divisor 10 only for R = -11, -10, not used). Wrapping 32-bit multiply, truncating
   divide (`imul` plus the magic `0x51EB851F` divide). The event flag fC fires a statistic with `tax10` (not modelled).
3. Ally share (fB), slots 0..11 in order, skipping the owner byte: `share = pct[R][i] * A10 / 100` (of the
   **original** A10), clamped to what is left, then `changer(ally_i, R, share, fA=1, fB=0, fC=0)` and `left -= share`.
   The loop runs while `left != 0`. Only R = 1 (gold) and R = 2 (lumber) have a table.
4. Tax from what is left (fA): `tax = min(tax10, left)`, `left -= tax`.
5. Credit: if `left != 0`, the stock at `P+(R+4)*16` becomes `clamp(stock + left, 0, 10,000,000)` for R = 1, 2.
6. Return `left` (x10).

The statistic after step 3 (`R+22`, the shared total) and the event of step 2 and 5 are not modelled (UNSURE what
they count). They do not change any stock.

### 1.4 Tests and verification
- Unit tests in `economy.rs` (`ally_share_goes_first_...`, `income_is_taxed_...`, `plain_credit_...`,
  `owner_slot_is_never_shared_to`): hand-computed from 1.3.
- The changer needs the player object (sub-objects found through `0x6F03FA30`), so it was **not** run in Unicorn.
  Its integer steps are checked against the instructions only. UNSURE: the one-slot-per-player assumption (we use
  `players.len()`, the original has 12 slots).

## 2. Upkeep (income tax)

### 2.1 Tier (CONFIRMED, `0x6F40F620`, asm lines 6F40F628-6F40F657)
```
tier = 0; t = thr[0];  if t == 0 -> tier 0
loop: if thr[tier] >= food_used (signed) -> stop with tier
      tier += 1; if thr[tier] == 0 -> stop with tier (the zero entry)
```
The reader `0x6F00A2A0(i)` returns 0 for i > 9. The tax read clamps the index to 9 (`0x6F00A3B0`, CONFIRMED).
Food used is `0x6F40F5D0` (player `+0x90`, CONFIRMED in economy.md 3.1).

### 2.2 Defaults (CONFIRMED from the image and the Unicorn start-up run)
- UpkeepUsage row 0 at `0x6F876148`: 40, 70, then 10000 x 8.
- UpkeepGoldTax (`0x6FAAE378` after the initialisers): 0.0, 0.30 (`0x3E99999A`), then 0.60 (`0x3F19999A`) x 8.
- UpkeepLumberTax: all zero.
- The image holds zeros at the tax table; the start-up initialisers fill it (`tools/emu/runtime_consts.py` path).

### 2.3 Rate (CONFIRMED, asm 6F40F66B-6F40F681)
`rate = F2I(ROUND(MUL(tax_float, 100.0)))`, where 100.0 = `0x42C80000` at `0x6FAAE574` (CONFIRMED value,
attack-damage.md). Soft-float ops: MUL `0x6F6EEE20`, ROUND `0x6F6EF330` (= floor(x + 0.5)), F2I `0x6F6EEC20`.
Verified in Unicorn: 519 vectors on the tax table and random tax bits, **0 mismatches**
(`tools/emu/economy_vectors.py`, `crates/sim/tests/economy/vectors.rs`, test `upkeep_rate_matches_the_original_chain`).

### 2.4 Trigger (LIKELY)
`0x6F40F620` runs when food used changes (economy.md 2.4, one dispatch site). The Rust side calls
`UpkeepTable::refresh_rates` from the caller that updates food. UNSURE: the exact call sites.

## 3. Harvest: carry, trip, deposit

### 3.1 Worker state (CONFIRMED offsets)
- Carry amount `+0x1C4` (script units; 0 = empty). Carry type: bit `0x100` of `+0x20` (1 = lumber).
- Gold trip size `+0x194`, lumber carry cap `+0x198`: `0x6F0557D0` stores them as f2i of data floats.
- Lumber per hit `+0x1D8`: f2i of a data float (CONFIRMED, `0x6F0557D0`).
- Swing interval `+0x1D4` (float). Owner unit `+0x30`. Mining flag: bit `0x8000` of `+0x20`.
- Owner-doubling flag: bit `0x4` of the worker's byte `+0x5C` (CONFIRMED, `0x6F047C80`).

### 3.2 Carry add `0x6F055610(worker, amount, lumber)` (CONFIRMED asm and decompile)
1. `amount == 0`: return false.
2. If the carry is not empty and the type differs: deposit first (`0x6F047C80`), carry = 0.
3. `sum = carry + amount`; `cap = lumber ? +0x198 : +0x194`; `full = cap <= sum`; `carry = full ? cap : sum`.
4. Set or clear bit `0x100` for the type. Return `full`.
The gold trip ignores the return value; the lumber swing ignores it too.

### 3.3 Gold trip `0x6F060890` (CONFIRMED asm)
Gate: mining flag set; the target `0x6F0417D0(target)` valid; the mine is the `Agld` ability of the target
(`0x6F0787D0(0x41676C64, ...)`, remaining at mine `+0x6C`).
1. `was_above = LowGoldAmount < remaining` (`0x6F009F90(0)` gives the threshold).
2. `request = (player flag 0x40 via 0x6F2D3290) ? 1 : worker +0x194`.
3. `taken = 0x6F09E220(mine, request)`: `if remaining < request: request = remaining; remaining -= request; return request`.
   The mine hook `owner.vt+0x124(0.0)` runs when the mine reaches 0 (UNSURE what it does).
4. `0x6F055610(worker, taken, gold)`.
5. If `taken != 0` and the unit is visible to the local player (`0x6F3A3660`): if `remaining <= 0` raise the
   depleted notice (event `0xDE`, sound GoldMineCollapseSound, text Goldminedestroyed, advisor ping); else if
   `was_above && remaining <= threshold` raise the low notice (event `0xDF`).
Ids and keys: `0xDE` and `0xDF` (economy.md 6.2, CONFIRMED in the call sites here).

### 3.4 Deposit `0x6F047C80` (CONFIRMED asm)
Gold path (bit `0x100` clear): fire event `0x23` on the worker (`0x6F27B4F0`), owner vtable `+0x8C(4,0,1)`.
Lumber path: event `0x24`, the same `+0x8C`, and `amount = 1` when the player flag `0x40` is set (`0x6F2D3290`).
Both paths: the player object is `0x6F3A1650(owner_index)`; if the worker has bit `0x4` at `+0x5C`, the amount
doubles when the player's `0x10000` flag is set (`0x6F2D44C0`); then `0x6F416A00/40(amount, 1, 1, 1)`, i.e.
the **income changer** with tax and share (section 1.3). Floating text when the credit is positive (not modelled).
The carry is cleared at the end.

Return trigger: the worker deposits when it reaches its return target (`0x6F047E00`: range predicate
`0x6F4766E0` with the constant at `0x6FAAEC94`, then `0x6F047C80`; LIKELY the arrival rule, CONFIRMED calls).

### 3.5 Lumber swing `0x6F060780` (CONFIRMED asm)
1. `interval = max(+0x1D4, 0.01)` (x87 compare, NaN keeps the value). Start the next swing timer (event `0xD01B2`)
   on the worker's timer list `+0x1DC`. The handler re-arms it each time (LIKELY one-shot per swing; the start call
   is the first thing in the handler).
2. Target `0x6F0475F0`; if `0x6F0471F0(target, 0, 1) == 0` (valid): the hit. (The sound and effect calls are not
   modelled. A player flag `0x40` makes the swing animation shorter (`0.1`), not modelled.)
3. Hit: target vtable `+0x120` (tree hit with I2R of `+0x1D8`, not modelled beyond the call).
4. `0x6F055610(worker, +0x1D8, lumber)`, then flag `0x4000` set on `+0x20`.

Implemented as `Worker::lumber_swing` on a `TimerQueue<SwingTimer>` (one-shot re-arm, `Worker::swing_interval`).
Test: hits per agile second are the same for 1 and 40 ticks per turn (`swing_handler_runs_on_the_agile_clock_regardless_of_speed`).
The tick is 0.005 s (`lockstep::TICK_INCREMENT_BITS`, spec lockstep.md section 9).

## 4. Cancel refunds `0x6F062320` (CONFIRMED asm, Unicorn-checked arithmetic)

Train branch (the slot holds a train, not a revive):
- rate = `TrainRefundRate` (`0x6F009EE0`, data float). Gold cost `0x6F29B800` (+0x20), lumber cost `0x6F29B830` (+0x24).
- gold refund = `F2I(MUL(I2R(gold_cost), rate))`, credited through `0x6F416A00` with flags `0, 0, 0`; lumber likewise
  through `0x6F416A40`. Gold first, then lumber.
- Refund only when the slot's bit in mask `+0x9C` is clear; otherwise a counter `+1` (not modelled).
- Revive slots use `ReviveRefundRate` and `0x6F0340A0` (not modelled).
- Research slots use `ResearchRefundRate` with the research cost `0x6F2B0260/0x6F2B0280`, the same arithmetic.

Verified: `refund_units(cost, rate) = F2I(MUL(I2R(cost), rate))` over **2,961 Unicorn vectors** (costs 0 to 20,000 and
random rates, plus 16,777,217 / 16,777,219 for the I2R 24-bit rounding), **0 mismatches**
(`tools/emu/economy_vectors.py`, test `refund_matches_the_original_truncated_product`).

The rate itself is data (economy.md 4.3, the `0x6FAAE470` default is 0.0). The refund is truncating (F2I) and untaxed.

## 5. Food helpers (CONFIRMED)
- `0x6F023BA0` (queue-time food check): `need = type.+0x30C()`; if need != 0 then `0x6F416A80(need)`, which returns
  false when `cap < used + need` (unsigned), unless the owner's type flag `0x10` says it is exempt.
- `0x6F023BD0` (food add): `0x6F40F5E0(+food_used of the type, or its negative)`.
- Food cap `0x6F40F850` = min(stat `+0x80`, stat `+0xA0`, FoodCeiling) (asm-level check of the min: CONFIRMED).
  Rust: `economy::food_cap`, `economy::food_room`.

## 6. Training queue (partial)

Confirmed:
- Queue class `CAbilityQueue` (`0x6F05DF60`), capacity 7 at `+0x28` and `+0x30` (economy.md 4.1, CONFIRMED).
- Train tick: `0x6F053D80` starts a periodic timer of **0.1 s** (`&0x6FAAE4A0` = 0.1f) with event `0xD01C1`, armed
  by event `0xD02A8` in `0x6F053DC0`. The tick body `0x6F043A20` sets the busy flag `0x200`, acquires the owner's
  current-item reference (`0x6F02DC60`, `+0xE0`) and calls `0x6F021500`. No progress counter is updated there.
- Refund arithmetic: section 4.

Not decoded (UNSURE, a dynamic check is needed):
- Where the per-tick progress is added and how `bldtm` (`+0x18`) and the game-speed step combine. The 0.1 s
  timer is the only clock found. The progress counter read by `0x6F40B340` (index 1) is the research one.
- The payment order at queue time (gold and lumber `0x6F416A00/40` with negative amounts, then food).
- Spawn position, rally point and the trained-unit event ids (`EVENT_PLAYER_UNIT_TRAIN_START/CANCEL/FINISH`).

Nothing about training progress is implemented. `TRAIN_QUEUE_CAPACITY = 7` is a constant only.

## 7. Nearest town hall (NOT FOUND)
- The deposit happens on arrival in range of the return target (3.4). The target itself is chosen by the order
  code (`0x6F0475F0`, `0x6F0471F0` checks flag `0x40` on the target and a route through `0x6F26D590`).
- Distance metric and tie-break: not traced. The range predicate used for the arrival is `D < (range/32 + a + b)^2`
  (attack-cycle-2.md, CONFIRMED there), so "nearest" is not a ranking here. UNSURE.

## 8. What the code does and where it is
- `crates/sim/src/economy.rs`: `change`, `credit`, `UpkeepTable` (`tier_for_food`, `tax_percent`, `refresh_rates`),
  `refund_units`, `cancel_train_refund`, `food_room`, `food_cap`, `Mine::take`, `Worker` (`carry_add`, `deposit`,
  `gold_trip`, `lumber_swing`, `swing_interval`).
- `crates/sim/tests/economy.rs` + `crates/sim/tests/economy/vectors.rs` (generated, 2,961 refund and 152 upkeep vectors,
  and the default constants).
- `tools/emu/economy_vectors.py`: the generator (runs the original in Unicorn).
- No natives were added or changed: the economy natives (`natives/economy.rs` is the waygate and resource module)
  still use the old resource model. Wiring the changer into `Game` is a follow-up; the changer is a free function on
  a slice of `PlayerEconomy`.

## 9. Verification (this pass)
- `cargo test --workspace` without and with `GAME_DLL`: 779 passed, 0 failed each (the 19 `economy` unit tests and 3
  replay tests included).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- Unicorn: 2,961 refund vectors and 152 upkeep vectors, 0 mismatches. Plus the 3 constants (100.0, the tax table,
  the upkeep percent constant), all matched. The start-up runner reports `failed: 1` out of 2,061 initialisers;
  the cause was not checked (it did not touch the functions used here).

## 10. Open items (priority order)
1. Training progress: find the function that adds to the progress counter on `0xD01C1` (the `0x6F043A20` call chain
   and the queue object vtable). Then the `bldtm` step and game-speed independence.
2. Nearest hall: trace the return-target choice (`0x6F0475F0`, `0x6F0471F0`) and its distance rule.
3. Statistic counters (`0x6F02D660` targets, the `R+22` shared total): which stat ids the scoreboard reads.
4. The stat behind the owner-doubling and one-per-trip flags (`0x6F2D3290` flag `0x40`, `0x6F2D44C0` flag `0x10000`):
   their names in the object data.
5. Dynamic check on the original: break on `0x6F4167D0` during a deposit, compare the credit with the Rust model.
