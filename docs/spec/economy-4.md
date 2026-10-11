# Economy pass 4: training queue (`CAbilityTrain`), Game.dll 1.26a

Status: PARTIAL. The premise of the previous pass (a 0.1 s train tick on 0xD01C1) is wrong. The queue layout, the
slot shift, the event dispatcher, the start and blocked paths, the completion and the spawn order are read from the
code. **The progress timer that finishes a unit, the use of `bldtm`, the gold/lumber check and the nearest town hall
are NOT found.** Labels: **CONFIRMED** = read from the disassembly or decompile at the address given, and where marked
checked in Unicorn; **LIKELY** = strong inference from code shape or a name; **UNSURE** = guess or not traced.

Sources: decompile export (scratchpad `decomp/`), capstone listings from each entry, the RTTI map
(`scratchpad/vtables.tsv`, `vfunc_names.tsv`), dword reference search over the image, Unicorn
(`tools/emu/train_queue_emu.py`, uses `tools/emu/jasscomp/jcomp.py`).

## 0. Corrections to earlier passes

| Earlier statement | Correction | Basis |
|---|---|---|
| economy-3 §6 and the task: the train tick is a 0.1 s timer, event 0xD01C1, armed by 0xD02A8 in `0x6F053DC0`; its body `0x6F043A20` sets busy flag 0x200 | **Wrong.** `0x6F053D80`, `0x6F053DC0`, `0x6F053E20` are methods of **`CBuffAuraRegenLife`** (vtable `0x6F880ABC`, slots 197, 3, 15). `0x6F043A20` (the tick body) is the aura pulse: it applies a buff id stored at `this+0xD0` to the owner, with busy flag 0x200. It has nothing to do with training | RTTI class of the vtable; `0x6F053DC0` is referenced only from that vtable (dword search); its direct callers are `0x6F053D80`/`0x6F053DC0`/`0x6F043B70` (aura code) |
| economy.md 4.1: the queue is `CAbilityQueue` (`0x6F05DF60`, capacity 7) | The **train** queue is `CAbilityTrain` (vtable `0x6F8E6964`, 211 entries). `CAbilityQueue` (`0x6F87C384`) is its base. Capacity 7 is the same | RTTI names; `0x6F05DF60` constructor (economy.md) |
| economy-3 §4 (refund): the refund bit is in mask `+0x9C` | In `CAbilityTrain` the count is at `+0x9C` and the per-slot bit mask at `+0xBC` (see §1). The refund-bit semantics are still UNSURE | `0x6F1652A0` (CONFIRMED) |
| economy-3 §6: the progress counter is read by `0x6F40B340` | That is the research progress (`0x6F059400`). Not the train progress | `0x6F059400` decompile |

## 1. Queue layout (`CAbilityTrain`) — CONFIRMED

- Used-slot count: dword `+0x9C` (`this[0x27]`).
- Slots: 7 dwords, type ids, at `+0xA0 + 4*i`. Slot 0 is the head (`+0xA0`).
- Per-slot bit mask: dword `+0xBC` (`this[0x2F]`), one bit per slot, moves with the slots.
- Capacity 7 (CONFIRMED by the constructor, economy.md 4.1, and by the slot count).

### 1.1 Shift (queue removal) `0x6F1652A0` = vtable slot 208 (`0x340`) — CONFIRMED
For a removed index `i` below `count-1`: slot `j` takes slot `j+1` for `j` from `i` to `count-2`, and the mask
bit `j` takes bit `j+1`. The last used slot `count-1` is set to 0 and its mask bit cleared.
**The count is not changed here** (the caller changes it).
- Unicorn: **3,000 random cases** on the original (count 2..7, index 1..count-1, random slots and masks):
  0 mismatches (`tools/emu/train_queue_emu.py`, vectors in `crates/sim/tests/economy/train_vectors.rs`).
- If index 0 is removed and the new head id (`+0xA0`) is non-zero, the function calls vtable slot 210 (`0x348`),
  which is the head start `0x6F1653E0`. CONFIRMED (asm `0x6F1652A0`, decompile). For index 0 with no new head it runs
  the UI clear path (not isolated).

## 2. Event dispatcher `CAbilityTrain::OnEvent` = `0x6F177FA0` (vtable slot 3) — CONFIRMED

| Event | Action |
|---|---|
| `0xD01C1` | release the listeners at `+0x94` and `+0x98` (`0x6F02CA60` twice), then start the head: `vt210(0)` = `0x6F1653E0` |
| `0xD01C4` | completion: `vt209(...)` = `0x6F1733A0` (section 4) |
| `0xD000A` | an order to slot `i` (`param_2[0x11]`, id check against `+0xA0+4i`): `vt204(i, 1, 0)` = `0x6F165680` (the cancel of a slot, it contains the `TrainRefundRate` key) — LIKELY |
| `0xD01A1` | `0x6F165200` (not read) |
| other ids | the base handlers (`0x6F15B8A0`, the child-slot path `0x6F173190`) |

Event `0xD01C1` therefore is **not the progress tick**. It is a retry: the listeners that a blocked head arms
(section 3.2) are released on it, and the head is tried again.

## 3. The head start `0x6F1653E0` = vtable slot 210 (`0x348`) — CONFIRMED structure, LIKELY meaning

Arguments: ECX = queue, EDX = slot index (0 for the head; the body reads the head id at `+0xA0` regardless).

### 3.1 Food room (`0x6F023BA0` = slot 196, `0x310`) — CONFIRMED
Returns **1** when the type needs no food, or when `0x6F416A80` says the room is there
(`used + need <= cap`, `cap` = `0x6F40F850`, the minimum of three supplies); **0** when the room is not there.
`0x6F416A80` skips the test when the owner type has flag `0x10` (exempt).

### 3.2 Food OK (asm `0x6F165434`: `test eax,eax; jne 0x6F16555D`)
Path `0x6F16555D` (`ret 4` at `0x6F1655E0`, it does not arm a timer):
1. `vt314(player, type, 1)` = `0x6F023BD0` (slot 197): adds the food use of the type to the player: `+need` for
   flag 1 (CONFIRMED; the type is read through the global "current type" of `0x6F29BAF0`, so the head is assumed; LIKELY).
2. Start notice: sound `0x25`, owner `vt+0x8C(4,0,1)` (the same call as the deposit), then the start hook
   `0x6F298BE0`, which fires **event key `0x80245`** through `0x6F285E10` (CONFIRMED key).

### 3.3 Food blocked (fall-through, `0x6F16543C` ff.)
Reads the type's food and the cap, sets the state, then creates **two listeners on `0xD01C1`**:
`0x6F02D7D0(&out, 0xD01C1, this, 0)` (kind 4) into `+0x94`, and `0x6F02D800(...)` (kind 5) into `+0x98`.
Both go through `0x6F40FAF0` to `0x6F473180` (IntegerListener, `Integer.cpp`) or `0x6F4774E0` (FloatModifier).
What value they listen to is **UNSURE**. The reading that fits the code: they wait for the food state to change,
then `0xD01C1` restarts the head (section 2). The asm jumps are read directly, not inferred from the decompile.

## 4. Completion `0x6F1733A0` = vtable slot 209 (`0x344`), on `0xD01C4` — CONFIRMED order, spawn parts LIKELY

1. Owner = `+0x30` (fallback `0x6F472890`). Owner `vt+0xB8` (slot 46) gives the owner position (3 floats), through
   `0x6F4743A0`.
2. Placement `0x6F29E280(pos, rally_or_null, flag)` (flag = `iVar4 == 0x10`, a race/type test, UNSURE):
   - a direction index 0..3 from a three-way test on three constants (`0x6FAAE470`, `0x6FAAE570`, `0x6FAAE580`
     against the eps `0x6FAAE614` = 2e-7); when a rally is given, the index is `angle / 90` capped at 2 (`0x6F1D2AB0`);
   - the building size radius `0x6F29C2A0`; with no rally point the base is the world origin fields `DAT_6FAB7368 +0x6C/+0x70`
     (the pathing origin, LIKELY);
   - the search is `0x6F473D10(out, pos, radius, index, 1, validator, ...)`, with validator `0x6F26F180` (flag 1) or
     `0x6F2AB130` (flag 0). **The order of the candidate positions is in `0x6F473D10`, not read.**
3. Unit creation `0x6F29F990(pos, facing = 0.0, flags 0x404, 0, -1)`. The new unit gets flags `|= 0x300` (`+0x64`),
   then `0x6F2858B0` (owner), `0x6F333A60`, the finish hook `0x6F298E60` (LIKELY the finish notice; key not read).
4. Rally: if the building has flag `0x80` (`+0x20`) and a non-empty rally list (`+0x9C` count, `+0xA0` entries),
   `0x6F04B920` calls `0x6F04B3E0(new unit)` for each valid rally entry. This issues the rally order to the new
   unit. **The order id is not read** (LIKELY a move-type order).
5. UI: `0x6F0542F0`; if the count is 1 `0x6F333040`, else `vt205` = `0x6F164EA0` (count 1 → `0x6F333010`,
   else `0x6F15B7C0`).
6. Remove slot 0: `vt208` = `0x6F1652A0` (section 1.1), which starts the next head (CONFIRMED).
7. `0x6F021270` (cleanup).

**Failure path** (no placement, `0x6F1733A0` after the search): sound `0x25`, error text `0x6F030920`, no removal. The
head stays queued. CONFIRMED structure.

## 5. Payment and food: the validator `0x6F172FC0` = vtable slot 200 (`0x320`) — CONFIRMED structure

This is the queue-time check. It returns a result code (0 = accepted):
- owner = `+0x30`; the owner's player object via `0x6F3A1650` (`vt+0xEC` gives the owner index);
- `vt+0x32C()` (slot 203, `0x6F17B0A0`) is tested first; when it is non-zero and the order is `0xD000A` (the cancel
  order), the result is 0 or `0xDD` by the check `0x6F3A35C0` (decompile arithmetic `(-(x != 0) & 0xFFFFFF23) + 0xDD`);
- for a listed type, `vt+0x32C(count-1)` non-zero gives **code 0x20** (LIKELY "queue full": slot count-1 is occupied);
- type list lookup (`0x6F32D9A0`): when the id is listed, `vt+0x308(type) == 0` → `0xDD` (not trainable here);
  `vt+0x304(type) == 0` → `0xB7` (requirements; `vt+0x304` = `0x6F172F30`, not read);
  `vt+0x300(owner, type) != 0` → its code (`0x6F172ED0`, not read);
- **food**: `need = 0x6F29BAF0(type)`; if `need != 0` and `0x6F00A210` (FoodCeiling) `< used + need`
  → **code 7**. Note: this uses FoodCeiling alone, while the room check of `0x6F416A80` uses the minimum of
  three supplies. The two disagree; which one the queue uses is still open (the model takes a cap from the caller);
- `vt310(type) == 0` → **code 3** (no food room, see 3.1).
- Otherwise `0x6F044C90` (generic, not read).

**Gold and lumber are not checked here.** The cost payment is not in this function either. Two facts point to
payment at queue time: the cancel refund `0x6F062320` applies to every train slot, running or not (economy-3 §4),
and the refund is the full cost times the rate. So the cost is taken when queued (LIKELY). The place of the
shortage check (the "not enough gold" refusal) is UNSURE.

Food is taken when the head starts (section 3.2, CONFIRMED). Food is given back when a running head is cancelled
(LIKELY, `0x6F023BD0` with flag 0 is the reverse call). When a unit finishes, the food stays used (LIKELY: the unit owns it).

## 6. Question 1: where the progress is kept and advanced — NOT FOUND

Confirmed facts that constrain it:
- The completion event is `0xD01C4` (section 2). The head's food and the start notice are set at start (section 3.2).
- No timer is armed in `0x6F1653E0`'s OK path (section 3.2) or in the start hook `0x6F298BE0` (its only timer-like call
  is `0x6F62A580` to test a listener, not a timer).
- `bldtm` (type `+0x18`): the getter `0x6F29B7A0` reads the *current* type (global, through `0x6F4C8520`), not the head.
  No reader of `bldtm` in the train paths was found in this pass.
- Routines that arm `0xD01C4` (`0x6F477A80` with the time in ESI, called by `0x6F0D30D0`, a base method of about 200
  vtables; `0x6F0479C0`, `0x6F0AA7D0`, `0x6F0CDC70`, `0x6F0D30D0`, `0x6F0FD3D0`, `0x6F134EA0`, `0x6F05A060`): checked by name only,
  none is confirmed to be the train. `0x6F07F040` (vtable of a buff class, slot 211) arms `0xD01C1` and forwards to
  `0x6F0D30D0`; it looks unrelated but was not fully read.

Not done: the dynamic check. On the original under Wine, break on `0x6F1733A0` (completion) and on `0x6F1653E0` (start),
and read the call stack. That gives the caller and the arming routine.

## 7. Question 4: nearest town hall — NOT DONE

The deposit goes to the target chosen by `0x6F0475F0` (economy-2 §1.7); nothing new was read. No distance metric or
tie-break is known.

## 8. What the model in `crates/sim/src/economy.rs` does

`TrainQueue` (queue of up to 7 `TrainItem`s, head first, a `refund_mask`, `TimerQueue<TrainTimer>` for the head):
- `enqueue`: capacity (CONFIRMED), food room with a cap from the caller (CONFIRMED check, cap is the caller's choice),
  resource check (UNSURE), cost paid (LIKELY);
- `try_start` / `retry`: food reserved and timer armed for `build_seconds` (**the timer length is UNSURE; the model
  takes `bldtm` as the length**); blocked when there is no room (CONFIRMED: the blocked head waits);
- `complete` (on `0xD01C4`): `Finished`, shift (CONFIRMED), next head starts (CONFIRMED);
- `cancel`: refund by `cancel_train_refund` (CONFIRMED arithmetic), refund bit (UNSURE meaning), head cancel gives food
  back and cancels the timer (LIKELY), shift, next head starts (CONFIRMED);
- `shift_out`: the shift of 1.1 (checked in Unicorn, 3,000 cases).

Constants: `TRAIN_EVENT_START/CANCEL/FINISH` = 0x45/0x46/0x47 (LIKELY, the unit-level ids; the start notice key
0x80245 is CONFIRMED); `TRAIN_EVENT_RETRY` = 0xD01C1, `TRAIN_EVENT_COMPLETE` = 0xD01C4 (CONFIRMED dispatch).

Not wired into natives or `Game` (another agent is editing `natives/economy.rs`).

## 9. Verification (this pass)

- Unicorn: 3,000 shift cases on the original (`tools/emu/train_queue_emu.py`), 0 mismatches; the asserts also check that the count is unchanged.
- `cargo test -p sim --test economy_train`: 12 passed (the shift vectors, the shift rules, enqueue refusals and payment, start and blocked start, retry, completion, head and middle cancel, refund bit).
- Full workspace test and clippy: see the commit message and the report.

## 10. Open items (priority order)

1. Find the progress timer: dynamic breakpoint on `0x6F1733A0` (completion), read the stack; or trace `0x6F473D10` to see the candidate order.
2. What the two listeners at `+0x94`/`+0x98` (kinds 4 and 5, `0x6F40FAF0`) listen to: the food integer or the progress.
3. `bldtm` (type `+0x18`): which reader uses it for a queued unit (the global-current getter `0x6F29B7A0` is not it).
4. The order id of the rally order (`0x6F04B3E0`) and the finish notice (`0x6F298E60`).
5. The validator's `0x6F172ED0` (slot 192) and `0x6F172F30` (slot 193): the requirement and cost checks.
6. The cap used by the queue food check: FoodCeiling (`0x6F00A210`) or the minimum of three (`0x6F40F850`).
7. Nearest town hall (section 7).

## 9. Pass 2 (2026-10-11): the train's progress setter is the base one (CONFIRMED structure, Unicorn for the arithmetic)

- `CAbilityTrain` does not override vtable slot 191 (`0x2FC`): it runs `CAbilityBaseBuild::vf191` = `0x6F0239B0`.
  (The construction class overrides it at `0x6F054690`, construction.md section 12.2.)
- `0x6F0239B0(p)` writes `this+0x78 * (100 - q) / 100` (q = p clamped as unsigned to 100) through the property at
  `this+0x7C` (`0x6F477350`). CONFIRMED in Unicorn over 6,180 vectors (`tools/emu/base_progress_emu.py`), and the Rust
  port `sim::construction::remaining_after_percent` matches all of them. This is the **remaining** part of a total
  stored at `this+0x78`.
- Consequence for the train: the progress is a percent-remaining of a stored total. The total is LIKELY the head's
  build time (`bldtm`) and the remaining value is what the timer or modifier at `+0x7C` reads. Both the total's writer
  and the driver of `p` are NOT found in this pass, so the train timer length stays UNSURE and `economy::TrainQueue`
  is not changed.
- Completion of a train remains `0xD01C4` (sections 2 and 4). The construction completion uses key `0x80241`
  (construction.md section 12.4). The two completions are different keys; the train's own finish key is not
  confirmed beyond `0xD01C4`.
- Still open: the writer of `this+0x78` for the train, the driver of `p`, the listener of `+0x7C`, nearest town hall
  (section 7).
