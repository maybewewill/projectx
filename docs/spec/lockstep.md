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

---

## 8. Pass 4 (2026-10-11): the per-tick increment, the listener object, the 0.1 s "step"

Sources: decompile export (chunk_014, chunk_019, chunk_020, chunk_022), capstone disassembly of
the image (Game.dll SHA1 c5d8624b...), Unicorn start-up values (`tools/emu/runtime_consts.py`).
Result in one line: the **tick is 5 ms of game time (LIKELY, now with a second independent
reading)**; **the float that the listener adds per tick is NOT identified** (UNSURE); **there is no
engine-side "simulation step" event** in the tick path (CONFIRMED by enumeration of the dispatched
ids).

### 8.1 The divisor DAT_6F957640 is 5.0 in the file (CONFIRMED)
- The symbol is a **double** (read as `(float)_DAT_6f957640` in the decompile). Its bytes in the
  image are low dword `0x00000000` at `0x6F957640` and high dword `0x40140000` at `0x6F957644`,
  i.e. the double **5.0**. It is set in the file; it is not written at start-up.
- Code references to it (image scan of .text for the absolute address): `0x6F52F393`,
  `0x6F53E8DB` (speed setter), `0x6F55069E` (CNetData constructor). All three are reads. So the value
  is 5.0 from the file, not from the initialisers. (`runtime_consts.py` printed only the low dword
  and showed 0, which is the low half, not the value. Corrected here.)
- Consequence (CONFIRMED arithmetic): the speed factor stored at +0x22AC by `0x6F53E890` and by the
  CNetData constructor (`4 / DAT`, the value for normal speed) is `SPEED[idx] / 5`:
  slow 0.6, normal 0.8, fast 1.0.

### 8.2 Why this points to 5 ms per tick (LIKELY)
- Per 25 ms of turn time the driver dispatches `SPEED[idx]` ticks (CONFIRMED, lockstep.md 7.1).
  So game milliseconds per turn millisecond = `SPEED * t / 25` with `t` the tick length. Setting
  that equal to the stored factor `SPEED / 5` gives **t = 5 ms** exactly. The divisor and the
  counter increment (+0x1C70 += 5, CONFIRMED) are the same 5 ms.
- A speed-scaled per-tick increment would make the two clocks disagree: the counter +0x1C70 would
  advance 5 per tick at every speed while the agile clock advanced `0.8 * 0.005` per tick at normal.
  So the per-tick float is **not** speed-scaled (LIKELY, from the consistency argument; the float
  itself is unverified).
- The multiplier `m` (lockstep 7.2) scales ticks, not the tick length.

### 8.3 The tick dispatch and the listener thunk (CONFIRMED)
- Frame driver `0x6F553470`, tick branch (disassembly `0x6F553762..0x6F553780`): `ECX = esi + 8`
  (esi = CNetData), `add [esi+0x1C70], 5`, `call 0x6F5378E0(&event)`, then `add [esi+0x22C0], 5`
  with wrap at 0xFA. The event object is `CNetEventGameTick` (its `vftable`, built on the stack), id `0x40090092`,
  with a float payload from `DAT_6FACC8CC`.
- `0x6F5378E0(this = CNetData+8, event)`: calls slot 0x10 of `[this+4]` and of `[this+0x10]`,
  returns their OR (CONFIRMED). These are two sub-objects of the per-player observer record.
- Registration `0x6F301690(enable)` (CONFIRMED bytes): for each of 0x40090092/93/94 it calls
  `0x6F549620` with **ECX = event id, EDX = handler** (0x6F2F4950 for the tick), and **two stack
  arguments, both 0**: the context and the player index.
- `0x6F549620` (CONFIRMED): `this = [manager slot 0xD + 0x10] -> [+8] + player*0x304 + 8`. For player
  0 this is **CNetData+8 = CNetObserver record 0**. It then calls `0x6F548F70(event, handler, context)`.
- `0x6F548F70` (CONFIRMED, disassembly): builds the key `(event, handler)` with `0x6F544F80`
  (64-bucket hash at `this+0x34`, bucket = `event & 0x3F`). If absent, it allocates a node from the pool
  at `this+0x1C` and stores **node+0x10 = event, node+0x14 = handler, node+0x18 = context**. It then calls
  `[this+4]`'s vtable slot 2 with `(event, event)` (registration notice). If present it only updates the
  context.
  - So the **context stored for all three listeners is 0**, the handler is the function pointer.
- The handler `0x6F2F4950` (CONFIRMED bytes): `add ecx, 0x14; call 0x6F470C70; mov eax, 1; ret`.
  So the handler expects **ECX = object O** and reads the float at **T = O + 0x14**.
- `0x6F470C70 → 0x6F46E9D0 → 0x6F46E520(T)`: `now := now + *T` (LIKELY softfloat add, see 7.3).

### 8.4 Which object is O: NOT identified (UNSURE; superseded by section 9: O = the tick event E)
- The node's context is 0, so the dispatcher cannot pass `node+0x18` as ECX (it would read T = 0x14).
  The dispatcher walker that calls `node+0x14` with some ECX was **not found**. The obvious linear
  sweep of .text stops early (about 190k instructions, desync on data), and the observer walker is not
  among the decompiled `call [reg+0x14]` sites checked (those are vtable slot 5 calls, not handler
  calls).
- Candidates, checked:
  - **O = CNetObserver record 0 (CNetData+8)**: T = CNetData+0x1C. The base constructor
    `0x6F548EA0` does `this[2] += 1` and `this[5] += 1` (CONFIRMED), so T starts as the **integer 1**
    (float 1.4e-45), not a time. **Rejected unless overwritten later** (not checked).
  - O = CNetData: CNetData+0x14 is not written by the constructor `0x6F550460` (CONFIRMED absence in
    the decompile). Value unknown. UNSURE.
  - O = a sub-object from `[this+4]`/`[this+0x10]`: unknown.
- The float value is therefore **not determined**. The 0x6FAAE4A0 constant (0.1f) is used only by the
  game-start call `0x6F470C70(&0.1)` and by queue B `0x6F46FFE0`, not by the per-tick listener.

### 8.5 Is there a "0.1 s simulation step"? (CONFIRMED negative, LIKELY conclusion)
- The tick path dispatches only 0x40090092 (per tick), 0x40090093 (idle, once per driver call with
  `5 * n`) and 0x40090094 (async idle, wall-clock delta). No 100 ms event is sent by the frame driver or
  registered at `0x6F301690` (CONFIRMED by enumeration of those sites; the replay pump was not enumerated).
- The agile clock advances by the tick increment per tick, and the timer queues fire exactly at expiry
  (lockstep 7.3, game-loop-2.md). A "step" is therefore not an engine event. A 0.1 s period (timers,
  the training tick, AI pacing) is an ordinary timer on that clock (LIKELY).
- The sim step in `crates/sim/src/lockstep.rs` (`sim_steps` = game ms / 100) is an **accounting value
  for tests and reports**, not an engine concept (LIKELY). It is kept, documented as such.

### 8.6 Summary (pass 4)
| Question | Answer | Confidence |
|---|---|---|
| DAT_6F957640 | double 5.0, in the file, read-only (3 refs) | CONFIRMED |
| Speed factor +0x22AC | SPEED / 5 = 0.6 / 0.8 / 1.0 | CONFIRMED arithmetic |
| Tick length | 5 ms of game time | LIKELY (two readings agree: +0x1C70 += 5, factor = SPEED/5) |
| Per-tick increment speed-scaled? | No (the clocks would disagree) | LIKELY |
| Registered listener object | Observer record 0 (CNetObserver, CNetData+8) for the registration; the dispatcher's ECX is unknown | CONFIRMED (registration), UNSURE (dispatch ECX) |
| Context / player stored | context 0, player 0 | CONFIRMED |
| Per-tick float value | not found; T = O+0x14 with O unknown | UNSURE |
| 0.1 s "step" | no engine event; a timer period on the agile clock | LIKELY |

### 8.7 Open work (next steps)
- Find the observer walker: a function that reads `node+0x10`/`node+0x14`/`node+0x18` and calls the
  handler. Start from the bucket layout (`this+0x34 + (event & 0x3F) * 4`) and from the dispatcher
  slot-0x10 functions of the sub-objects at `CNetData+0xC` and `+0x18`. Use a proper function-boundary
  disassembly (Ghidra per-function), not a linear sweep.
- Then read the float at O+0x14 in Unicorn, with the observer populated (needs the pool at `this+0x1C`
  and the manager slot 0xD). Compare with 0.005f (`0x3BA3D70A`, present in .rdata at 8 places, none
  identified yet).

---

## 9. Pass 5 (2026-10-11): the tick event object, the handler's `this`, the per-tick float

Sources: per-function disassembly (capstone, each function from its entry), Ghidra decompile
export chunks (chunk_019, chunk_022, chunk_024), a byte-level search of the image for absolute
addresses, and Unicorn runs of the static initializer and of the slot-3 to handler hop
(`tools/emu/emu.py`, probes outside the repo). Resolves the "UNSURE" items of sections 7.3, 8.4 and 8.7.
Result in one line: **the handler `0x6F2F4950` receives the tick event object itself as ECX, and
the float it adds to the agile clock is `0.005f` (= 5 ms), written once at start-up to
`0x6FACC8CC` and copied into the event at +0x14.**

### 9.1 The chain from the frame driver to the handler (CONFIRMED)

1. **The event object E** is built on the frame driver's stack (`0x6F553470`, around 0x6F5536FD):
   - `E+0x00` = vptr `0x6F9580E4` (CNetEventGameTick);
   - `E+0x08` = event id `0x40090092` (`mov [esp+0x30], 0x40090092` at 0x6F553701);
   - `E+0x14` = the dword at **`0x6FACC8CC`** (`mov edx, [0x6facc8cc]` at 0x6F553715, stored at
     0x6F553723). This is the only place that reads that address (byte search, CONFIRMED).
   - E is built once per driver call. The tick loop (0x6F553770..0x6F5537A5) then calls the dispatcher
     `n` times with the same E (`lea edx,[esp+0x28]; push edx; mov ecx, esi+8; call 0x6F5378E0`), and
     adds 5 to `+0x1C70` before each call (CONFIRMED, lockstep 7.1).
2. **`0x6F5378E0`** (thiscall, `ret 4`, CONFIRMED): `ECX = R = CNetData+8` (observer record 0, the
   `CNetObserver` at `CNetData+8`, 0x304 bytes per player). It calls slot 4 (`+0x10`) of the vptr at
   `R+4` (`A`, `CNetData+0xC`) with `ECX = R+4` and the event, then slot 4 of the vptr at `R+0x10`
   (`B`, `CNetData+0x18`) with `ECX = R+0x10`, and returns OR of the two results.
3. **Slot 4 of A** (vtable `0x6F970780`, `0x6F629A90`, CONFIRMED): calls slot 5 (`0x6F62A7B0`) with
   the event id (`E+8`) and E as arguments.
4. **Slot 5** (`0x6F62A7B0`, CONFIRMED): `ECX = [A+8]` = the observer agent (an event hash). If it is
   non-zero, it calls the walker `0x6F62A5D0` with `ECX = agent`, `param_2 = id`, `param_3 = E`.
   The agent is created lazily by the registration notice: `0x6F548F70` calls slot 2 of A
   (`0x6F62A9A0`, which calls `0x6F629F70` to create `[A+8]` and then `0x6F62A820` to insert a listener).
5. **The walker `0x6F62A5D0`** (CONFIRMED by decompile and disassembly): takes the bucket of `id`
   (`[agent+4] + ((byte[agent+1]-1) & id) * 4`), walks its list, and for each entry whose key
   `entry[1]` equals `id`: stores `E+8 := entry[3]` (the id again, so E+8 is overwritten during the
   walk, harmless) and calls **vtable slot 3 of `entry[2]` with ECX = entry[2] and one stack argument
   = E** (`call [vptr+0xC]`). `entry[2]` is the observer node `N` from the registration (the
   `0x6F548F70` node, passed as the notice's third argument).
6. **Node slot 3** (`N`'s vtable `0x6F9582A8`, slot 3 = `0x6F5375B0`, CONFIRMED): 
   ```
   mov eax, ecx          ; N
   mov edx, [eax+0x18]   ; context (0 for player 0, CONFIRMED from registration)
   mov ecx, [esp+4]      ; the stack argument = E
   mov eax, [eax+0x14]   ; handler
   call eax              ; handler(ECX = E, EDX = context)
   ret 4
   ```
   **So the handler's ECX is the event object E.** The node's context is not used as `this`.
7. **Handler `0x6F2F4950`** (`add ecx, 0x14; call 0x6F470C70; mov eax, 1; ret`, CONFIRMED bytes):
   `T = E + 0x14` (the float `[0x6FACC8CC]` copied at step 1), passed as the pointer `p` to
   `0x6F470C70`.

**Unicorn check of steps 6-7 (CONFIRMED for this hop):** `probe_hop` ran `0x6F5375B0` with ECX = a
node (handler at +0x14 = `0x6F2F4950`, context 0) and `[esp+4]` = E, after the static initializer.
The handler was reached with **ECX = E** (`True`), and `[ECX+0x14]` read `0x3BA3D70A` (0.005f).
Not run end-to-end: steps 3-5 (the agent's bucket list) were checked by reading only; building a
populated agent in emulation needs the allocator stubs (`0x6F4C1AB0`, `0x6F629E90`) and was not done.

### 9.2 The float: `0x6FACC8CC` = 0.005f (CONFIRMED value)

- The file holds 0 at `0x6FACC8CC` (also 0 at its neighbours `0x6FACC8C8`, `0x6FACC8D0`).
- The only writer is the static initializer **`0x6F8621C0`** (CONFIRMED by its code). It is listed
  in the start-up initializer table in `.rdata` at `0x6F87438C` (byte search; CONFIRMED membership,
  LIKELY that the CRT calls it at start-up, as for the other entries).
- Its code (CONFIRMED by disassembly): `i2r(1000)` into a stack slot; `i2r(5)` into a second slot;
  then `div(ECX = 0x6FACC8CC, EDX = pointer to 5.0, stack = pointer to 1000.0)`. So the value is
  **`5 / 1000`** (operand A = 5, B = 1000, from the register flow; CONFIRMED).
- **Unicorn run** (`probe_tick`, the initializer alone): `0x6FACC8CC` goes from `0x00000000` to
  **`0x3BA3D70A`** = 0.004999999888241291 as f32 (= the f32 nearest to 0.005). CONFIRMED.
- Rust check: `crates/sim/src/lockstep.rs` `tick_increment_is_five_over_thousand_in_soft_float`
  computes `div(i2r(5), i2r(1000))` with the soft-float tables from `GAME_DLL` and gets `0x3BA3D70A`
  (passes). CONFIRMED.
- Nothing else writes the address (byte search of all sections for `CC C8 AC 6F`: two hits, the read
  in the driver and the initializer). The value is the same for every speed and every tick.

### 9.3 What the handler does with the float (LIKELY for the operator, CONFIRMED for the read)

- `0x6F470C70(p)` (fastcall, CONFIRMED decompile): if the agile manager (global `0x6FAB7368`) `+0x38`
  is 0, it calls `0x6F46E9D0(p)` (queue A), then `0x6F46F390(p)`, `0x6F46F3A0(p)`, `0x6F470740(p)`.
- `0x6F46E9D0` (tiny) → `0x6F46E520(ECX = p, EDX = queue A)` (CONFIRMED decompile): if the queue flag
  `+0x4C` bit 0 is clear, it computes `tmp = add(*(queue+0x40), *p)` (softfloat add `0x6F6EF000`; the
  operand pointer is in EDX and is hidden by the decompiler, LIKELY `queue+0x40`). If `tmp < queue+0x48`
  (the horizon), it stores `queue+0x40 := tmp` and runs the timer pump `0x6F46E370`. Otherwise it takes
  the horizon branch (`0x6F6EEEF0`, `0x6F469D40`, not analysed; UNSURE).
- So **each tick adds the float `*p = 0.005f` to the agile clock by one soft-float add**. The increment
  is the same constant on every call. (The earlier reading of `0x6FAAE4A0` = 0.1 as the per-call step
  is withdrawn for the tick path, see section 8.5.)

### 9.4 Consequences (answer)

| Question | Answer | Confidence |
|---|---|---|
| Object passed as ECX to the handler `0x6F2F4950` | the tick event object E (built on the driver's stack, `esp+0x28` at the call) | CONFIRMED (static chain + Unicorn hop) |
| Why not the observer record or the node | the walker calls node slot 3, which forwards `[esp+4]` (= E) as ECX | CONFIRMED |
| Float at `T = E+0x14` | `[0x6FACC8CC]` copied into E by the driver | CONFIRMED |
| Value of `[0x6FACC8CC]` | 0x3BA3D70A = 0.005f = `I2R(5)/I2R(1000)` (initializer `0x6F8621C0`) | CONFIRMED (Unicorn + soft-float test) |
| Initializer runs at start-up | in the initializer table `.rdata:0x6F87438C` | LIKELY |
| Speed dependence of the increment | none: a single constant; speed changes only the tick count per unit | CONFIRMED |
| Agile clock per tick | `now := add(now, 0.005f)` (softfloat), then the timer pump | LIKELY (operand order hidden; the read is CONFIRMED) |
| Tick = 5 ms of agile time | 0.005 s, the same 5 ms as `+0x1C70 += 5` | CONFIRMED value, LIKELY unit match |
| 0.1 s "step" | still no engine event (section 8.5 unchanged) | LIKELY |
| Horizon branch (`tmp >= queue+0x48`) | not analysed | UNSURE |

### 9.5 Code changes that follow (this pass)

- `crates/sim/src/lockstep.rs`: `TICK_INCREMENT_BITS = 0x3BA3_D70A` and `advance_agile_clock(clock,
  ticks)` (one soft-float add per tick; linear cost, no shortcut because each add rounds). Tests:
  the value, the soft-float division (with `GAME_DLL`), two adds, 200 ticks against a hand fold.
- `crates/sim/src/timer.rs`: `AgileClock::advance(increment_bits)`; `advance_step` now calls it with the
  0.1 s step bits (unchanged result, existing tests unchanged).
- `TurnScheduler::push_turn` is not changed: it computes ticks in integers, and the clock is advanced
  by the caller with `advance_agile_clock`, because a turn can hold up to `u64::MAX` ticks in the
  saturation test.

### 9.6 Open items

- Run the agent walk end to end in Unicorn (steps 3-5): needs the bucket list layout of `[A+8]`
  (intrusive list with sentinels; the walker's lock calls `0x6F629730/0x6F629740`) and allocator stubs.
  Expected to give the same ECX = E.
- Run `0x6F46E520` in Unicorn with a queue object (flags, `+0x40`, `+0x48`) to confirm the add operand
  order and the horizon branch.
- The idle events `0x40090093` / `0x40090094` build their float payload differently (`fild` of the
  tick total, a double constant at `0x6F932C60`); not analysed here.

---

## 10. Pass 6 (2026-10-11): the simulation driver advances one game tick at a time

Implementation: `crates/sim/src/script.rs` (`ScriptHost::step_ticks`, `ScriptHost::step` = one batch),
`crates/engine/src/lib.rs` (`Engine::step_ticks`, `Engine::step`, `Engine::run_turn`),
`crates/sim/src/lockstep.rs` (`TICK_INCREMENT_BITS`, `TICKS_PER_BATCH`). Before this pass the host added
0.1 s (`0x3DCCCCCD`) once per `step()`, which is not what the original does.

### 10.1 What the driver does per game tick (in this order)

1. **Clock.** `clock := softfloat_add(clock, 0.005f)` with `TICK_INCREMENT_BITS = 0x3BA3D70A` (section 9).
   CONFIRMED value; the operator is LIKELY (section 9.3). One add per tick, no speed scaling.
2. **Timer pump to the new clock** (`0x6F46E370`, CONFIRMED in section 7.3): pops every queue entry with
   expiry `<= clock`, in (expiry, insertion) order. During each callback the queue time (and the VM time)
   is that entry's expiry; when the pump ends it is the clock. Callbacks may queue entries that are due
   at or before the clock; those are popped in the same tick. Entry kinds: sleep (resume the thread at
   its expiry), timer expiry (handler and listening triggers), trigger event, timed life, attack timer.
3. **Batch work** only at tick multiples of `TICKS_PER_BATCH = 20` (counted from host creation):
   a. order and movement tick (`Game::tick_orders`, which includes the combat tick of units with orders);
   b. one JASS scheduler pass (`Vm::step(clock)`), every runnable thread once;
   c. (engine only) the AI host pass (`AiHost::step(clock)`), after the main host's batch work.

`ScriptHost::step()` is one batch (20 ticks = 100 ms), so existing callers keep their meaning. The
engine counts `steps` as batches and `ticks` as game ticks. A replay drives the same path:
`TurnScheduler::push_turn(...).game_ticks` is the tick count, passed to `Engine::run_turn` after the
turn's commands are applied (section 3 order).

### 10.2 Cadence of each per-step piece

| Piece | Cadence in this model | Original | Status |
|---|---|---|---|
| Clock add | every tick | every tick (driver `0x6F553470`, listener `0x6F2F4950`) | CONFIRMED value, LIKELY operator |
| Timer pump | every tick | every tick (`0x6F46E520` after the add) | LIKELY (horizon branch below) |
| Timers, sleeps, trigger events, attack timers (`0xD01AF..C1`) | exact, at the tick the clock reaches the expiry (queue timers) | queue timers, game-loop-2 4.2 | LIKELY |
| Order and movement tick (`tick_orders`) | every 20 ticks; a move is `speed * 0.1` per call (`STEP_BITS`, orders.rs) | movement message `0x8026A` (movement.md); the cadence of the mover call is not known | **UNSURE** |
| Combat tick of units with orders | every 20 ticks (inside `tick_orders`) | attack cycle is timer-driven (attack-cycle.md) | **UNSURE** |
| JASS scheduler pass (`Vm::step`) | every 20 ticks | the interpreter is only called from the wake paths (game-loop-2 4.5), so there is no polling pass | **UNSURE**, a model |
| AI host pass | every 20 ticks, after the main batch | AI dispatcher with a +100 ms next run (ai.rs notes, base UNSURE) | **UNSURE** |

### 10.3 Consequences of the 0.005 s tick (CONFIRMED arithmetic, measured in tests)

- The clock after `n` ticks is an `n`-fold soft-float add, not `n * 0.005` nor `0.1` at batch 1
  (`per_tick_clock_is_twenty_soft_adds_per_batch`). Truncating adds drift, so a value that is exactly
  reached in IEEE arithmetic may be reached one tick later.
- A 0.1 s `TimerStart` at tick 20 fires at the first tick whose clock is `>= expiry`, where
  `expiry = soft_add(clock(20), 0.1f)`. The test `point_one_second_timer_fires_on_the_first_tick_whose_clock_reaches_expiry`
  checks this tick by tick.
- A timer is observed by batch code only at the batch boundary after its tick. A 0.5 s scan armed at
  tick 20 fires in the batch after tick 120 (test `idle_unit_acquires_an_enemy_that_enters_range_at_the_next_scan`
  derives the step from the scan chain).
- A computer player's `Sleep(0.1)` wakes at the first batch boundary whose clock reaches the wake time.
  Because that clock drifts, a 0.1 s sleep takes 1 or 2 batches. The AI tests therefore run until the
  count is reached and check the interleaving (`run_until` in `tests/ai_natives.rs`). UNSURE: the original
  wakes the AI from a 100 ms timer on the tick that reaches it, so this is a known divergence of the
  batch model.

### 10.4 What is UNSURE in the driver

- **Batch length 20 ticks** for the order/movement, JASS-scheduler and AI work: no source in the spec.
  It is kept at the old 100 ms so the per-step behaviour does not change. Change `TICKS_PER_BATCH` when
  the original's cadence is known.
- **Horizon branch.** `0x6F46E520` advances only if `tmp < queue+0x48` (the limit); otherwise it takes a
  branch not analysed (section 9.3). The driver always advances: no limit is modelled.
- **Movement step.** A move step is `speed * 0.1` per `tick_orders` call. Under the 20-tick batch that is
  per 100 ms of game time; the original's per-tick movement (`0x8026A`) may move `speed * 0.005` per tick.
  Not verified (movement.md still-open list).
- **Frame-driver ticks per turn.** The driver runs `SPEED * m` ticks per 25 ms unit (section 7.1). The
  multiplier `m` is still UNSURE (section 7.2); the engine takes `game_ticks` from `TurnScheduler` as is.
- **Agile clock vs +0x1C70.** Both advance per tick; whether `+0x1C70` (5 per tick) is the same time base
  as the agile clock is LIKELY (section 8.2), not confirmed.
- **Wake order** inside one tick (timers before sleeps of the same expiry) follows the queue's (expiry, seq)
  order; the original's order for equal expiries is not in the spec (game-loop-2 4.5).

### 10.5 Verification

- `cargo test --workspace` (no `GAME_DLL`) and with `GAME_DLL` set: all pass.
- Clock tests: `per_tick_clock_is_twenty_soft_adds_per_batch` (sim `tests/timer.rs`), the lockstep unit tests
  (`agile_clock_advances_by_one_soft_float_add_per_tick`, `tick_increment_is_five_over_thousand_in_soft_float`
  with `GAME_DLL`).
- No Unicorn check of the pump after the tick add yet (needs the populated queue object, section 9.6).
