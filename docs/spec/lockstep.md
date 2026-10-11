# Replay time slots, turns and simulation steps (Game.dll 1.26a)

Status: pass 1, 2026-10-11. Sources: the decompile export (chunk_022, chunk_025, chunk_023) and the
disassembly of the tiny wrappers the export skips (read from the user's own Game.dll, SHA1
c5d8624b...). No Unicorn run of the functions below yet (see section 6).

Labels: **CONFIRMED** = seen in the disassembly or decompile at the address given.
**LIKELY** = strong inference from the code shape. **UNSURE** = not resolved.

Corrections to earlier notes:
- game-loop.md 1.1 asked what the 16-bit word of a 0x1E/0x1F block means. It is the byte length
  of the payload (CONFIRMED). The time increment is the first 16-bit word **inside** the payload.
- game-loop.md 1.2 said +0x628 is "current game time". In replay mode it is a byte cursor
  (CONFIRMED, see 1.2 below). The sim clock is not +0x628.

---

## 1. Where a time slot is consumed

### 1.1 Replay pump (CONFIRMED)
- `0x6F54EB00` (CNetData replay pump). For block ids 0x1E and 0x1F it reads a 16-bit length
  (`0x6F4C2C70`), copies that many payload bytes (`0x6F4C2FD0`), then calls `0x6F54CAD0` with
  the block id, the payload pointer and the length. Block 0x1F also ends the pump loop.
- Its "due time" comparisons (+0x628, +0x62C, +0xAC8) are buffering control, not sim time.
  Replay mode copies +0xACC (bytes consumed so far) into +0x628 at the top of the pump (CONFIRMED).

### 1.2 Enqueue and the time increment (CONFIRMED)
- `0x6F54CAD0` (enqueue an event record, `0x6F54C490` allocates it). For ids 0x1E and 0x1F only:
  1. A reader over the payload reads the **first 16-bit word** (`0x6F6516C0` → `0x6F4C2C70`).
     This word is the **time increment in milliseconds** (LIKELY from the public W3G layout; the
     code makes it the first payload word, CONFIRMED).
  2. If the reader did not run past the payload end, the increment is added to **CNetData+0x225C**
     (the pending-milliseconds counter). CONFIRMED (`add [esi+0x225c], eax` under the `ja` guard).
  3. Then `0x6F5491B0` hands the record to the queue object at +0xF38 (`0x6F548FF0`, CONFIRMED call;
     queue semantics UNSURE).

### 1.3 Applying a slot's commands (CONFIRMED structure, meaning partly UNSURE)
- `0x6F5516E0(this, reader, player, flag)` applies one player's command sub-stream:
  - if player == 0, +0x1C68 is incremented;
  - `uVar6 = 0x6F550730(reader, player)` applies the commands (a loop over action bytes with a
    big switch on the action id, CONFIRMED);
  - `uVar6` is added to the player's bucket at `+0x284 + player*0x304` and **subtracted from
    +0x225C** (CONFIRMED).
  - when the flag argument is nonzero, +0x2258 := (7·(+0x2258) + uVar6) >> 3, a moving average
    of the slot size (CONFIRMED), and the per-player counter at +0x2E0 is reduced by uVar6
    (CONFIRMED). When the player is the local player (+0x610) and the flag is set, it calls
    `0x6F543E50/0x6F537820/0x6F6504F0` (CONFIRMED).
- **UNSURE**: what `0x6F550730` returns. Its epilogue does not set EAX, so the value is whatever
  the last virtual call left in EAX. Therefore we cannot say for certain that the value credited
  to the buffer is the slot's time increment. LIKELY it is intended to be the milliseconds of the
  slot, because +0x225C and +0x284 are used as milliseconds below (see 2.2).
- Player index is capped at 16 (`0x6F54C490`/`0x6F65C900`, CONFIRMED from earlier pass).
- Player order: each player's sub-stream is applied in the order it appears in the slot (LIKELY).
  Player 0 is special-cased in the parser (`0x6F550730`, CONFIRMED). No sort by player id
  (CONFIRMED: no sort call on this path).

### 1.4 Command block id 0x22 (checksum) (CONFIRMED path, content UNSURE)
- Block 0x22 goes to `0x6F54E140` → `0x6F54CC10`. The handler asks a CDataStore-like object for
  a serialised record (vtable slot 0x28), then enqueues it with id 0x22 through `0x6F54CAD0`
  **without** a time increment (CONFIRMED: only 0x1E/0x1F add to +0x225C).
- The decompile of `0x6F536C30` prints `"[CSyncData] (%s)"` and `"csum[%08X], count[%u]"`, from
  vtable slot +4 of the sync object and a second field (CONFIRMED). So a sync record carries a
  checksum and a count. Where the value is computed and which slot it is written to: UNSURE
  (see 4).

---

## 2. Converting milliseconds into game time

### 2.1 Frame driver `0x6F553470` (CONFIRMED structure)
Called once per frame with a time argument (GetTickCount-like). Constants read from the image:

| Item | Value | Source |
|---|---|---|
| Consumption unit | 25 ms | literal 0x19 |
| Threshold | consume while accumulator > 24 | literal 0x18 |
| Accumulator cap | 4000 | `0x6F553470` clamp to 4000 |
| Freeze rule | if more than 200 ms of real time pass inside the loop, accumulator := 0 | `GetTickCount` |
| Speed table | `{3, 4, 5}` (int32) at `0x6FA89154`, index = speed setting | CONFIRMED values |
| Speed setting | 0 = slow, 1 = normal, 2 = fast; any value ≥ 2 becomes 2 | `0x6F53E890` (CONFIRMED) |
| Speed factor | float = table[speed] / DAT_6F957640 (stored at +0x22AC) | `0x6F53E890` |

- Frame accumulator +0x2250 += (+0x22B4 / +0x22B8) × (frame delta in ms), where the delta is
  `now - previous` (previous at +0x224C) and the ratio is normally 1/1 (CONFIRMED). The
  "scaled by latency" factor from table `0x6F9595F8` (7 floats) is used only in one branch
  (CONFIRMED structure, meaning UNSURE). The accumulator is clamped to 4000.
- Loop, while +0x2250 > 24 (CONFIRMED):
  1. If +0x284 (player 0 buffer, ms) < 25, pull more turns through `0x6F553440` (which calls
     `0x6F54EB00`, the pump) and stop if nothing arrives.
  2. Subtract 25 from +0x284 and from +0x2250.
  3. If the game state (+0x278) is 4 or 5: `n = table[speed] × m` where `m = 0x6F5BE0C0()` if
     `0x6F5BE0B0()` is nonzero and 1 otherwise (both read the global at `0x6FACCEA0`; their
     meaning is UNSURE, probably a replay or fast-forward factor). Then `n` times: +0x1C70 += 5,
     dispatch the `CNetEventGameTick` (id 0x40090092) to the observers at this+8, +0x22C0 += 5
     with wrap at 250 (CONFIRMED).
  4. If the state is 6 and +0x610 == 1, +0x2250 += 25 back (CONFIRMED).
- After the loop, if any dispatches happened, one `CNetEventIdleTick` (0x40090093) is sent with
  the total count × 5 (CONFIRMED). If the frame delta is nonzero, one `CNetEventAsyncIdleTick`
  (0x40090094) with the delta (CONFIRMED).

### 2.2 What this means (LIKELY)
- Game time is **25 ms units** of buffered turn time, paced by wall clock at 1× speed.
- A turn with increment `T` ms adds `T` to the buffer; it is consumed in units of 25 ms. The
  remainder below 25 ms carries to the next turn (CONFIRMED arithmetic: the buffer is an integer
  and the subtraction is exact).
- Per consumed unit the engine dispatches `table[speed]` game-tick events. At normal speed that is
  4 per 25 ms, i.e. 16 per 100 ms.

### 2.3 The open point: where one simulation step (0.1 s) comes from (SUPERSEDED by section 7)
- Section 7 (pass 3) replaces the "one step per 100 ms of consumed time" model: the tick is 5 ms of
  game time (LIKELY), so steps per 100 ms of turn time are 0.6/0.8/1.0 for slow/normal/fast (LIKELY).
- The text below is kept for history.
- AGENTS.md section 4.3h says `0x6F3A2EC0` advances the agile clock by 0.1 s per call, and that the
  step is 100 ms (the constant `0x6FAAE4A0` = 0.1f, CONFIRMED by an earlier pass).
- The `CNetEventGameTick` listeners were not located. If every game-tick dispatch were one 0.1 s
  step, normal speed would run 16 steps per 100 ms, which contradicts the constant.
- Therefore the mapping from game-tick dispatches to simulation steps is UNSURE. Options:
  (a) a listener counts the dispatches and steps every 16 (normal) ticks, i.e. every 100 ms of
  consumed time; (b) the 0x1C70/0x22C0 counters are the step clock (5 units per tick, 250 wrap).
  Both give 100 ms per step at normal speed. Our model uses (a): **one simulation step per 100 ms of
  consumed time**, with a remainder carry in ms. This is LIKELY, not confirmed.

---

## 3. Order of operations inside one turn

1. The turn's time increment is added to the pending counter when the block is received
   (CONFIRMED, 1.2). The commands are parsed and applied per player when the queued record is
   applied (CONFIRMED, 1.3). Receipt and application are different moments.
2. The per-frame driver (2.1) consumes buffered time in 25 ms units and dispatches game ticks.
   The commands of a turn therefore run before the game time they cover is consumed (LIKELY).
3. The step bracket `0x6F65C7D0` / `0x6F65C840` (game-loop.md 1.4) applies queued net events on
   exit from the outermost step, in list order (CONFIRMED).
4. UNSURE: whether the commands of a turn run strictly before the first step of that turn, or
   after the last step of the previous turn. The Rust scheduler applies a turn's commands before
   the steps it produces (LIKELY, matches 2 and 3, not confirmed).

---

## 4. The 0x22 checksum block

- Handler `0x6F54E140` → `0x6F54CC10` (CONFIRMED). It enqueues a serialized sync record without a
  time increment (CONFIRMED).
- The printed format `csum[%08X], count[%u]` in `0x6F536C30` (CONFIRMED) says the record holds a
  32-bit checksum and a count. The checksum is produced by a vtable call on the CSyncData object.
- Not located: the writer of the 0x22 record (the sender), what the checksum covers (LIKELY the
  state after the steps of that turn), and the CRC or other algorithm. game-loop.md 2.x already
  says the same. UNSURE.
- Replays: the 0x22 blocks are only compared, never used to advance time (LIKELY).

---

## 5. Summary table

| Question | Answer | Confidence |
|---|---|---|
| Turn increment location | first u16 of a 0x1E/0x1F payload, in milliseconds | CONFIRMED (code path), LIKELY (format) |
| Accumulation | pending += T on receipt; buffer(+0x284) += credited value on apply | CONFIRMED / credited value UNSURE |
| Conversion | 25 ms units, wall-clock paced at 1x, buffer >= 25 required | CONFIRMED |
| Ticks per unit | table {3,4,5}[speed] (slow/normal/fast) | CONFIRMED values, multiplier m UNSURE |
| Steps per 100 ms | 4 units; 1 step per 100 ms of consumed time | LIKELY (contradiction in 2.3) |
| Remainder carry | integer ms, carried | CONFIRMED arithmetic |
| Order: commands vs steps | commands before the steps they produce | LIKELY |
| Player order | stream order, player 0 special | LIKELY |
| 0x22 checksum | serialized record with csum and count, no time | CONFIRMED path, content UNSURE |

---

## 6. Verification status

- Not run in Unicorn: `0x6F553470` needs the whole CNetData object and the GetTickCount hook.
  A vector test of the speed table is possible from the image (`GAME_DLL`); it is in the Rust test
  suite. The 25 ms conversion in `crates/sim/src/lockstep.rs` is therefore **not** verified against
  the original; it follows the code above.
- `DAT_6F957640` is zero in the file and set at startup (see tools/emu/runtime_consts.py). Its value
  is not used by the Rust model.

---

## 7. Pass 3 (2026-10-11): the game-tick chain and steps per turn

Sources: disassembly (capstone) and a Ghidra 11.4.2 decompile of 0x6F553470, 0x6F5378E0,
0x6F549620, 0x6F548F70, 0x6F301690, 0x6F470C70, 0x6F46E520, 0x6F46E9D0, 0x6F46E370,
0x6F46FFE0, 0x6F3A2EC0, 0x6F53E890, 0x6F5BE070/090, 0x6F2E0B70 (scratch export, not committed).
The listener thunk at 0x6F2F4950 has no function entry in Ghidra; it is read from bytes.

### 7.1 Per 25 ms unit: the tick count (CONFIRMED)
- 0x6F553470 (frame driver) consumes +0x2250 in 25 ms units while the state at +0x278 is 4 or 5.
- Per unit: `n = SPEED[+0x22A8] * m`, with `SPEED = {3,4,5}` at 0x6FA89154.
- `+0x22A8` is set by 0x6F53E890 to `clamp(setting, 0, 2)` (CONFIRMED); the same routine stores
  `+0x22AC = SPEED[idx] / DAT_6F957640` (a float; the double divisor is set at startup, value UNSURE).
- For each of the n ticks, the driver does: `+0x1C70 += 5`; dispatch event 0x40090092 to the
  observers at CNetData+8; `+0x22C0 += 5`, and if the result is above 250 it subtracts 250.
  (CONFIRMED from the disassembly and the decompile.) So +0x22C0 cycles through 5..250 (period 50 ticks).
- After the loop: one idle event 0x40090093 with the total `5 * n_total`, and one async idle event
  0x40090094 with the wall-clock delta, if any (CONFIRMED).

### 7.2 The multiplier m (CONFIRMED structure, runtime values UNSURE)
- `0x6F5BE0B0()` = `(0x6F53E670() == 0) ? G.+0x258 : 0`, with `G = 0x6FACCEA0`.
- `0x6F5BE0C0()` = `(0x6F5BE0B0() == 0) ? 1 : G.+0x10`.
- The driver: `m = (0x6F5BE0B0() != 0) ? 0x6F5BE0C0() : 1`. So `m = G.+0x10` only when `G.+0x258 != 0`
  and the test below is 0. Otherwise `m = 1`.
- `0x6F53E670()`: reads CNetData through manager slot 0xD (+0x10, +8). If there is no net object it
  returns 0. Otherwise it reads the 4-character tag at `+0x2288`: `0x4C4F4F50` ("POOL") and
  `0x4E4F4E45` ("ENON") return 0, any other tag returns 1 (CONFIRMED values; meaning of the tags UNSURE).
- So the multiplier is active for "no net object" or the POOL/ENON tags. Values of G.+0x10 and
  G.+0x258 are written through ECX-relative calls (e.g. 0x6F5BDF90, not decoded). UNSURE; for normal
  play probably m = 1.

### 7.3 The listener chain (CONFIRMED path, the increment source UNSURE)
- Registration: `0x6F301690(enable)` calls 0x6F549620 three times (context 0, player 0) for
  0x40090092 → handler 0x6F2F4950, 0x40090093 → 0x6F3011B0, 0x40090094 → 0x6F3011F0. The
  disable path (`0x6F545B40`) is the same shape. Callers: init 0x6F3A2ED0, and 0x6F5C0CD0/0x6F5C0DF0.
- Handler 0x6F2F4950 is a this-adjusting thunk: `add ecx, 0x14; call 0x6F470C70; mov eax,1; ret`
  (CONFIRMED bytes; no Ghidra function there, so the real `this` is the registered object + 0x14).
- 0x6F470C70 (`__fastcall(p)`, CONFIRMED decompile): if the agile manager (global 0x6FAB7368) +0x38 is 0,
  it calls 0x6F46E9D0(p) on queue A (manager+0x40), then 0x6F46F390(p), 0x6F46F3A0(p) and 0x6F470740(p)
  on the world sub-objects (world 0x6FAB73D8 +0x14 / +0xBC / +0x164, LIKELY the movement and other
  per-tick systems, see movement-3.md).
- 0x6F46E9D0 → 0x6F46E520(p = float pointer T, edx = dispatch object) (CONFIRMED):
  `tmp = add(now, *T)` (LIKELY the operator is softfloat add 0x6F6EF000, see softfloat.md);
  if `tmp < limit(+0x48)` then `now := tmp`, run the pump, return 1; otherwise the horizon branch.
  So the float behind T is an **increment per call**, not an absolute target (LIKELY, from the add).
- 0x6F46E370 (pump, CONFIRMED decompile): runs while the top expiry is at or below the target, sets
  `now := expiry` during each callback, and restores `now := T` at the end (agrees with 4.2).
- 0x6F46FFE0 (queue B, CONFIRMED): compares the cursor 0x6FAB7380 with `*T` as an absolute target.
  So the same pointer means an increment in 0x6F46E520 and an absolute target in 0x6F46FFE0.
- **Correction**: 0x6F3A2EC0 is not a per-frame update. It is the game-start initializer (it registers
  the listeners with 0x6F301690, builds the world, registers natives at 0x6F3D4020, and calls
  0x6F470C70(&0.1) and 0x6F46FFE0(&0.1) once). Its callers 0x6F3A3AD0 and 0x6F3A49E0 are wrappers
  (CONFIRMED callers, meaning UNSURE). The claim "0x6F3A2EC0 advances the clock by 0.1 per call" is withdrawn.
- **Unresolved**: the thunk passes `registered object + 0x14` as T, so the per-tick increment is the
  float at that object + 0x14, not the constant 0x6FAAE4A0 (which is used only by the init call and
  queue B). The registered object is found through the observer hash (0x6F548F70 stores the handler at
  entry+0x10 and the context at entry+0x14) and the dispatch slot 4 of CNetData+0xC/+0x18 (CONFIRMED that
  the dispatch calls slot 4 of both sub-objects). Its value is not determined. UNSURE.

### 7.4 Game time per tick (LIKELY)
- +0x1C70 gains 5 per tick (CONFIRMED). If +0x1C70 is game time in ms, one tick is 5 ms of game time
  (LIKELY). The 5 is then the tick length, not a per-step count.
- Under that reading, per 25 ms unit of turn time: slow 15 ms, normal 20 ms, fast 25 ms of game time
  (times m). Per 100 ms of turn time: 60 / 80 / 100 ms of game time (times m).
- A simulation step is 100 ms of game time (the agile step, 0.1 s: softfloat constant 0x6FAAE4A0,
  used as an increment in the init call). This is the open link (UNSURE).

### 7.5 Steps per 100 ms of turn time (answer, with confidence)
| Speed | Ticks per 25 ms | Game ms per 100 ms turn (m=1) | Steps per 100 ms turn (m=1) | Confidence |
|---|---|---|---|---|
| slow | 3 | 60 | 0.6 | LIKELY (depends on 7.4) |
| normal | 4 | 80 | 0.8 | LIKELY |
| fast | 5 | 100 | 1.0 | LIKELY |
- The earlier model (one step per 100 ms of turn time at every speed) is **contradicted** by 7.1: it
  would need 16 steps per 100 ms at normal, or 1 step per 16 ticks, which 7.4 does not support.
- The multiplier m scales all three rows (CONFIRMED structure, value UNSURE).
- Under the earlier 16-ticks-per-step guess the normal row would be 1.0; the code gives 0.8 under 7.4.

### 7.6 Summary table (pass 3)
| Question | Answer | Confidence |
|---|---|---|
| Ticks per 25 ms unit | SPEED[clamp(setting)] * m | CONFIRMED |
| +1C70 per tick | +5 | CONFIRMED |
| +22C0 wrap | cycles 5..250 (period 50 ticks) | CONFIRMED |
| Multiplier m | G.+0x10 if G.+0x258 and net tag POOL/ENON or no net; else 1 | CONFIRMED structure, values UNSURE |
| Listener for tick | thunk 0x6F2F4950 (+0x14) → 0x6F470C70 → queue A pump | CONFIRMED path |
| Per-tick increment | float at registered object +0x14 | UNSURE (object not identified) |
| Per-call 0.1 s | 0x6FAAE4A0 only in init and queue B; not per tick | CONFIRMED (correction) |
| Game ms per tick | 5 | LIKELY |
| Sim step | 100 ms game time | LIKELY |
| Steps per 100 ms turn at normal | 0.8 (m=1) | LIKELY |

### 7.7 Verification status
- No Unicorn run of 0x6F553470 yet: it needs the CNetData object and the hub. The tick arithmetic
  (7.1, 7.4) is small enough to test in Rust with the constants above; `crates/sim/src/lockstep.rs`
  now implements it (5 ms per tick, speed table, multiplier input) and tests it.
- Next step to close the open link: identify the registered object of the 0x40090092 entry (the
  observer hash at CNetData+0xC/+0x18, entry+0x14) and read the float at its +0x14, then compare with
  the agile step (0x6FAAE4A0 = 0.1). This needs a Unicorn run with the observers populated.
