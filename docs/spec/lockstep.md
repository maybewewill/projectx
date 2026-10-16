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

- The file holds 0 at `0x6FACC8CC` (also 0 at its neighbour `0x6FACC8D0`). **Correction 2026-10-11 (runtime-constants audit):** the other neighbour `0x6FACC8C8` is not zero at start-up: it holds 180/pi (0x42652EE1).
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
- **Horizon branch (resolved in section 14, 2026-10-11).** `0x6F46E520` advances only if `tmp < queue+0x48` (the limit); otherwise it takes a
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

---

## 11. Pass 7 (2026-10-11): the real cadence of the per-tick work

Question: how often do unit movement, the AI dispatcher and the JASS scheduler run, now that one game
tick is 0.005 s of agile clock (section 9) and the tick pumps the timer queues (section 10).
Sources: disassembly, decompile chunks, call-site scans, and Unicorn runs of the start-up initialisers
(`tools/emu/runtime_consts.py`). Addresses are in the original image.

### 11.1 The tick hook and the four timer receivers (CONFIRMED structure)

`0x6F470C70` is the per-tick body, called by the listener `0x6F2F4950` with p = `&E+0x14` (section 9):

```
mgr = [0x6FAB7368]                       ; agile manager
if mgr.+0x38 == 0:
    6F46E9D0(p)  ECX = mgr.+0x40          ; queue A   -> 6F46E520
    6F46F390(p)  ECX = world.+0x14        ; queue G1  -> 6F46E9E0
    6F46F3A0(p)  ECX = world.+0xBC        ; queue G2  -> 6F46EAB0
    6F470740(p)  ECX = world.+0x164       ; queue G3  -> 6F46F3B0
world = [0x6FAB73D8]
```

The four receivers share one template (disassembled in 6F46E9E0, 6F46EAB0, 6F46F3B0, 6F46E520):
`if (obj.flags+0x4C & 1) == 0: tmp = softadd(obj.clock(+0x40), *p); if tmp < obj.horizon(+0x48):
obj.clock = tmp; pump(obj)`; otherwise the horizon branch (not analysed; UNSURE). Each object is a 1-based
min-heap of timers with head/count at +0x10/+0x20. An element has expiry +4, flags +0x10 (bit 0 = repeat,
bit 16 = cancelled), owner +0x18 and period +0x8. Re-arm (`6F46D570`, `6F46D5F0`, `6F46CC80`):
`expiry := softadd(clock, elem+8)`.

| receiver | pump | fire (per due element) | Confidence |
|---|---|---|---|
| queue A `mgr+0x40` (6F46E520) | 6F46E370 | owner vtable slot 3 (`6F46D520`) | CONFIRMED |
| G1 `world+0x14` (6F46E9E0) | 6F46E910 | owner vtable slot 18 (`6F46E480`) | CONFIRMED |
| G2 `world+0xBC` (6F46EAB0) | 6F46EB80 | `6F46E5F0` → `6F49F1E0` (clears a bit set at +0x98, calls `6F49F030` per bit) | CONFIRMED (fire); LIKELY (what is queued) |
| G3 `world+0x164` (6F46F3B0) | 6F46EB80 | as G2 | LIKELY (same pump) |

### 11.2 Message 0x8026A: who posts it and at which period

- The only post of 0x8026A is `0x6F426BF0(flag=1)`, called once from the map-start routine `0x6F3B0750`
  (call at `0x6F3B0AB8`). `0x6F426BF0(0)` (from `0x6F3A2EE8` in the game-start initialiser `0x6F3A2EC0`)
  cancels it. CONFIRMED (byte scan and the two call sites).
- `0x6F426BF0` pushes `(0x6FAB7370, 0x8026A, obj, 1, 0)` and calls `0x6F4778F0` on the holder `obj+0x294`.
  That calls the holder's vtable slot 2 with `(agent, timeout*, msg, repeat=1, queue-select=0.0)`. The
  target is `CAgentTimer::vf2 0x6F477FF0`: arg2 is a `float*` read as the timeout, a nonzero arg3 means
  repeat, and arg5 == 0.0 selects queue A (`mgr+0x40`); otherwise queue B (`mgr+0x44`) with flag 4.
  CONFIRMED for the argument order (disassembly); LIKELY for the slot-2 identity (the vtable is built
  at run time).
- **The timeout is the f32 at `0x6FAB7370` = 0.03 (`0x3CF5C290`).** The static initialiser `0x6F85F650`
  calls `S2R` (`jmp 0x6F6F0520`) on the literal at `0x6F942DD8`, which is the text `"0.03"`. Unicorn run of
  the initialisers: `0x6FAB7370 = 0x3CF5C290`. CONFIRMED.
- The post is a repeating timer: element flag bit 0 is set, and `6F46E480` re-arms it with
  `expiry := softadd(clock at fire, period)`. The period is the same 0.03 (the element's +8 is the
  timeout; LIKELY, the element constructor `6F477F80` is not decoded).
- Dispatch: element fire → queue A pump → owner slot 3 → message switch `6F42FC80` (case 0x8026A) →
  `6F426C30` → `6F4675C0` → `jmp 0x6F483300` (the mover walk). CONFIRMED.

**Answer 1a.** The movement walk runs from a repeating agent timer on queue A with period **0.03 s of
agile clock** (6 game ticks). It is not every tick and not from G1-G3. The first fire is 0.03 after the
timer is armed at map start (clock 0; LIKELY).

**Correction (2026-10-11, vision pass 2, `vision.md` section 11.4).** The vision refresh `0x6F40A8F0` is not called
from the movement handler. It is case `0x80269` of the handler `0x6F42FC80` (`0x6F4299E0`), driven by its own
repeating timer with period 0.4 s (`0x6FAB6F00`, posted by `0x6F426C50`). The movement walk stays on 0x8026A.

### 11.3 The mover walk and the per-call step

- `0x6F483300` (CONFIRMED): increments the walk counter `world.+0x538`; calls `0x6F49AC40`; walks the
  cluster list `world.+0x38C` (`0x6F48E4C0`); walks the mover list `world.+0x3B8` calling `0x6F493410`
  per unit (`0x6F490FC0` → `0x6F4907F0` per 44-byte record); walks the turn list `world.+0x440`
  (`0x6F494200`); toggles `world.+0x53C` and walks `world.+0x51C` on every second call (`0x6F4961C0`).
- `0x6F4907F0` (the per-record step; structure CONFIRMED): r = sqrt(dx² + dy²) of the delta vector
  `unit+0x80/+0x84` (softfloat mul/add/sqrt). `record.+0x20 := r` if |r| ≥ `DAT_6FAAE618`, else 0.0.
  Facing `record.+0x24 := heading` (`0x6F4B2DE0`), or `unit+0x8C` when there is no step. Then
  **`record.+0x20 := min(record.+0x20, unit+0x88)`**. So the per-call distance is capped by `unit+0x88`.
  Nothing in the chain multiplies by an elapsed time; the cap is the only time-dependent input.
- The writer of `unit+0x88` is **not identified**. The store scan finds about 190 candidates and none is a
  speed setter with a constant multiply. The speed chain (`0x6F26DE00` → `0x6F202F30` → `0x6F201D50` →
  vtable 0xB8/0x18 of an unidentified class) does not reach it in the decompile. **UNSURE**: if the cap
  is `speed × period`, the per-call step is `speed × 0.03`. The earlier `speed × 0.1`
  (`orders.rs` `STEP_BITS`) is therefore withdrawn for movement.
- Arrival (`|r| ≤ 0.2`, section 4.3k) and the 0.49 path margin are unchanged.

**Answer 1b.** Per movement call the step is `min(distance, unit+0x88)`. The elapsed time of the call is
the 0.03 period (CONFIRMED value); the cap formula is UNSURE. No multiply by 0.005 is in the mover chain.

### 11.4 What 0x6F46F390, 0x6F46F3A0, 0x6F470740 do

- `0x6F46F390`: advances the clock of G1 (`world+0x14`) by p and pumps its due timers (`6F46E910`); each
  fire calls the owner's vtable slot 18 (CONFIRMED).
- `0x6F46F3A0`: advances the clock of G2 (`world+0xBC`) by p and pumps its due entries (`6F46EB80`); each
  fire clears a bit set on the pathing side (`6F49F1E0`) (CONFIRMED).
- `0x6F470740`: thunk to `0x6F46F3B0`; advances the clock of G3 (`world+0x164`) and pumps it as G2 (LIKELY).

### 11.5 The AI dispatcher cadence (CONFIRMED values, UNSURE mapping)

- The agent switch `0x6F2DBEE0` dispatches 0xD01C2 to the AI dispatcher `0x6F2DB4B0`, and 0xD01C1 to
  `0x6F2DB230` (CONFIRMED).
- 0xD01C2 is posted through the agent timer with two timeout constants (CONFIRMED values, Unicorn):
  `0x6FAAE484 = 0.02` (`0x3CA3D70A`), one-shot (repeat 0), from `0x6F044340`; and `0x6FAAECA8 = 1.0`
  (`0x3F800000`), repeating, from a site in chunk_001 near line 12708 (guarded by a unit-type check).
  0xD01C1 uses `0x6FAAF0A0 = 1.0`, repeating, from `0x6F04C579`, `0x6F070CB7` and `0x6F070DC1`.
- Inside `0x6F2DB4B0`: a budget `local_20 = 5000` (ms-like). For each candidate, `next = (object+0x9c) + 100`
  with the object from `0x6F481460` (`[ecx+0x250]`), and `local_20 = min(local_20, next)`. When the list is
  empty it clears `0x2EC` and cancels its timer (`0x6F477D30`). So "+100" is a deadline in the object's own
  time units, **not a 100 ms agile-clock period**. The 0.1 s batch of AiHost is not supported by the
  original. **UNSURE** what the dispatcher's real period is; the periodic 1.0 s timers are the candidates.

### 11.6 The JASS scheduler (LIKELY: no tick-driven pass)

- Interpreter entry points: `0x6F44B640` (flags, budget 300000 or 0), `0x6F44B6A0` (budget 300000), and the
  interpreter `0x6F45F8B0` / `0x6F45FA30` / `0x6F45FD00`. Callers (CONFIRMED by call-site scan):
  `0x6F3A8340` (trigger execute path), `0x6F3B5470` and `0x6F3B54B0` (map/game natives), `0x6F2DE1D0` (AI
  thread start, `0x6F44B6A0(0, ..., 300000, 0, 0)`), `0x6F2E0B70` (AI command run).
- Sleeps: `TriggerSleepAction` (`0x6F44B2F0`) stores status 3; the wake is a timer (game-loop-2 4.5). Timer
  expiries and trigger events run their callbacks inside the queue pump (section 11.1 fire path).
- **No function found runs the interpreter on a fixed tick or batch cadence**: every caller is an event, a
  native or a timer callback. The original runs a thread when it is started or woken, inside the tick's pump
  or the native call. Our `batch()` pass every 20 ticks (`Vm::step`) has no original counterpart. It is kept
  (UNSURE) only because budget-exhausted threads need a continuation rule that the original's op-limit path
  does not show in this analysis.

### 11.7 Summary (what the code does after this pass)

| Piece | Cadence | Confidence |
|---|---|---|
| Agile clock | +0.005 per tick, pump after each add | CONFIRMED value (section 9), LIKELY operator |
| Movement walk (0x8026A → 0x6F483300) | repeating timer, period 0.03 s, fires at the tick whose clock reaches the soft-added expiry | CONFIRMED value and path; period LIKELY |
| Movement step | `min(distance, speed × 0.03)` per call | UNSURE (cap writer not found) |
| Sleeps, timer expiries, trigger events | exact tick of their expiry | LIKELY |
| Combat tick of units with orders | every 20 ticks (kept) | UNSURE |
| JASS scheduler pass | every 20 ticks (kept as a model) | LIKELY not in the original (11.6) |
| AI host pass | every 20 ticks (kept) | UNSURE (the original's values are 0.02 s one-shot and 1.0 s periodic 0xD01C2 timers) |

### 11.8 Open

- `unit+0x88`: the writer of the per-call cap and its value per second. A dynamic run of the original (watch
  stores to `unit+0x88` while a unit walks) would settle it.
- The element constructor `0x6F477F80` (to confirm element+8 = 0.03) and the horizon `mgr+0x48` value
  (0.03 is far below any horizon, so no clamp is expected).
- Which object owns the 1.0 s 0xD01C2 timer (the AI dispatcher's real owner) and the period of `6F2DB4B0`'s rearm.
- The horizon branch of the four templates (`tmp >= horizon`).

---

## 12. Pass 8 (2026-10-11): JASS start timing and the batch pass

Question: when does a script thread run after it is started, and is the 20-tick batch pass (`Vm::step`,
section 10.1 step 3b, section 11.6) in the original? Implementation: `crates/sim/src/script.rs`
(`ScriptHost::start`, `ScriptHost::step_ticks`, `ScriptHost::continue_threads`), `crates/jass/src/vm.rs`
(`Vm::start_now`, `Vm::continue_runnable`, `Vm::has_runnable`), `crates/engine/src/lib.rs` (`start_main`).
This section supersedes section 10.1 step 3b and the "JASS scheduler pass" rows of 10.2 and 11.7.

### 12.1 A started thread runs at once (CONFIRMED callers, LIKELY timing)

The interpreter entry points are `0x6F44B640` (flags, budget 300000 or 0) and `0x6F44B6A0` (budget 300000); the
interpreter itself is `0x6F45F8B0` / `0x6F45FA30` / `0x6F45FD00` (section 11.6). Their callers, found by a
call-site scan, are all events or natives:

| Caller | What starts the script | Confidence |
|---|---|---|
| `0x6F3A8340` | trigger execution | CONFIRMED call; LIKELY immediate |
| `0x6F3B5470`, `0x6F3B54B0` | map and game natives that run script code | CONFIRMED call |
| `0x6F2DE1D0` | AI thread start, `0x6F44B6A0(0, ..., 300000, 0, 0)` | CONFIRMED call |
| `0x6F2E0B70` | AI command run | CONFIRMED call |

None of these callers stores a request for a later pass. So a started script runs from the calling site, before
the caller continues. Model: `ScriptHost::start` calls `Vm::start_now` with the current agile clock. The thread
runs until its first stop (sleep, yield, end, or exhausted budget) before `start` returns. Threads it starts run
after it in the same call. The engine's `main` is queued by `Engine::boot` and started at the first `step_ticks`
(`Engine::start_main`), so the host is set up first. UNSURE: the real game's start of `main` at map load is not
traced.

Before this pass `start` only queued the thread, and the first batch pass ran it up to 100 ms later.
Section 12.5 lists the consequences for the tests.

### 12.2 Sleeps, timers and trigger events resume in the pump at their expiry (LIKELY)

The pump (`ScriptHost::pump`, section 10.1 step 2) pops every queue entry with expiry `<= clock`. A sleep calls
`Vm::resume` for its own thread only (`Payload::Sleep`). A timer expiry runs its handler
(`natives::expire_timer`, through `Vm::run_chain`), and a trigger event runs its trigger
(`natives::fire_trigger`), both as immediate chains. This agrees with game-loop-2 4.5 (CONFIRMED: no polling of
sleeping threads; TriggerSleepAction stores status 3 and a timer wakes the thread).

### 12.3 Budget-exhausted and yielded threads: continuation (UNSURE, a model)

A thread that stops on its budget (300000 instructions) or yields has no continuation rule in the original that
this analysis found (section 11.6). The model keeps one rule and makes it cheap:

- `ScriptHost::step_ticks` calls `continue_threads` after the pump of every tick. It runs
  `Vm::continue_runnable` only when `Vm::has_runnable` is true (some thread is runnable, which means budget-exhausted
  or yielded). Sleeping threads are never run here: their wake-up is `Vm::resume` from the pump.
- Before this pass the same threads ran once per 20 ticks inside `Vm::step`, which also ran every due sleeper. That
  pass is removed. Its only remaining job was continuation.
- Cost: a thread that exhausts its budget on every run (an endless loop) runs 300000 instructions on each tick. The
  original's behaviour here is unknown, so the cadence is UNSURE.

### 12.4 What still runs on the batch cadence (UNSURE)

Only `ScriptHost::batch`: the combat tick of units with orders (`Game::tick_combat`), every `TICKS_PER_BATCH` = 20
ticks counted from host creation. Movement is not on this cadence (section 11.2: its own repeating timer). The AI
host still runs at the batch boundaries from the engine (`crates/engine`), with the same UNSURE status as in
section 11.7.

### 12.5 Expectations changed by this pass (and why)

Each change is a test whose model used the batch (tick 20) as the start time of a script. With immediate starts the
script's first action happens at tick 0 (clock 0.0), so the timer, sleep and event times move earlier by one batch.
Each value was re-derived from the agile clock (`clock_at_tick`, soft-float adds), not copied from the new output,
except where noted.

| Test file | Test | Change and reason |
|---|---|---|
| `sim/tests/timer.rs` | `trigger_sleep_resumes_at_the_expected_step` | Start at tick 0. The sleep wakes at the first tick whose clock reaches the soft-float `0 + 0.25`. Checked tick by tick. |
| `sim/tests/timer.rs` | `point_one_second_timer_fires_on_the_first_tick_whose_clock_reaches_expiry` | Expiry `0 + 0.1`. Fires at tick 20 or 21 (soft-float drift), not at tick 41 (the old start at tick 20 plus a batch). The test source also gained the `type timer` line the original had. |
| `sim/tests/timer_natives.rs` | `start()` helper | Clock 0.0 instead of tick 20. The other timer tests keep their meaning. |
| `sim/tests/timer_natives.rs` | `negative_timeout_is_clamped_to_zero_and_fires_at_once` | The zero-length timer fires in the first pump (tick 1), not in the second step. |
| `sim/tests/ability_natives.rs` | timed-life expiry, pause, zero-duration (3 tests) | The buff is applied at clock 0.0 (start), so the expiry base is 0.0 (the old tests read the queue time after the first step). The zero-duration unit dies at tick 1, so the test steps one tick. |
| `sim/tests/trigger_natives.rs` | `start()` helper | Clock 0.0; the gate and expiries are derived from it. |
| `sim/tests/ai_natives.rs` | `unit_and_gold_queries_read_the_game` | The main script starts inside `rig`, before the test inserted the unit type it reads. A `rig_with` setup hook now inserts the type before start. The start is immediate, so the setup must come first. |
| `sim/tests/combat_natives.rs` | `mutual_attack_hits_at_pinned_steps` | `duel` is ordered at clock 0.1 (the end of the setup step), one step earlier than before. Hits move from steps 7, 17, 27, 37 to 6, 16, 26, 36. The 0.6 s from the order to the first hit is unchanged. |
| `sim/tests/combat_natives.rs` | `first_acquire_step` (idle acquisition) | The scan timer is armed at setup, which now runs at tick 0, so the model's base is clock(0). The new step value (5) comes from the same model. |
| `sim/tests/combat_natives.rs` | `reaction_delay_draws_from_stream_nine` | The reaction is armed at the order's game time. The test now reads the queue time before the step; it used to read it after the step (one batch later). |
| `sim/tests/order_natives.rs` | `straight_moves_arrive_with_model_positions` | The order is issued at tick 0, so the walks of batch 1 (ticks 6, 12, 18) happen before the first recorded step. Recorded positions are now 48.6, 72.9 and 97.2 (3 walks per batch, since the soft-float sum of the 0.03 period drifts). The old first entry is gone. |
| `engine/src/lib.rs` | `Engine::boot` | No change in the tests: `main` is still queued by boot and starts at the first `step_ticks`, now at once (so it sees the unit types the caller registered after boot; starting it inside boot broke `engine/tests/pathing.rs`). The status text is "started as thread N" after the start. |

### 12.6 Verification

- `cargo test --workspace` without `GAME_DLL`: 732 passed, 0 failed. With `GAME_DLL`: 732 passed, 0 failed (the
  table-dependent tests in `order_natives`, `combat_natives`, `timer` and `engine/tests/pathing.rs` included).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- Not checked against the original: the immediate-start timing. The evidence is the call-site scan in 12.1 (the
  callers are CONFIRMED) and the absence of a deferred request in those callers. A dynamic check (break on
  `0x6F44B6A0` during a trigger execution and compare the agile clock before and after) would settle it.

### 12.7 Summary

| Piece | Model after this pass | Confidence |
|---|---|---|
| Start of a script (trigger, AI, map natives) | runs at once, at the calling time | CONFIRMED callers, LIKELY timing |
| Sleep, timer expiry, trigger event | run in the pump at their expiry | LIKELY (game-loop-2 4.5) |
| Budget-exhausted or yielded thread | continues once per tick, only when runnable | UNSURE |
| Combat tick of units with orders | every 20 ticks | UNSURE |
| AI dispatch | batch boundaries (engine) | UNSURE (section 11.5) |

## 13. Pass 9 (2026-10-11): the 0x22 sync record, its content and its sender (replay oracle)

> **Correction (2026-10-12, section 15):** the value chain in 13.3 and 13.5 is wrong. It is `rol(rol(rol(rol(T ^ c, 3) ^ X, 3) ^ Y, 3) ^ R, 3)`: `X`
> once, and a fourth term `R` = the byte-hash sum of the global generator state, which 13.3 called dead. Registry slot 2 is `CNetData` (a running checksum
> of the received blocks); the 13.7 participants are the custom-data kinds. Use section 15.

Labels: CONFIRMED = read in the Game.dll machine code (addresses given) and checked on the real 1.26a replay
where a file check is possible; LIKELY = strong structure, not run; UNSURE = open.

### 13.1 Record layout (CONFIRMED on the real file, `crates/w3g` `Block::Checksum`)

- Block id `0x22`, then `u8 n` (= 4 in all 89,259 records of the real file), then `u32` little endian.
  Total 6 bytes. Parsed as `w3g::Block::Checksum { csum }`; any other length is refused (`BadBlockLength`).
- One record follows **every** time slot (`0x1E`/`0x1F`): 89,259 time slots, 89,259 records, each record
  directly after its slot (`crates/w3g/tests/checksums.rs`, env `W3G_REPLAY`). The record has no time increment.
- Receiver (CONFIRMED): block handler `0x6F551D80`, case `0x22`. It reads the record, checks that all its bytes
  were consumed (length), and when the local player is not a computer/replay mode it compares the 4 bytes with
  `CNetData+0x1C64` (the local value, section 13.3) with a 4-byte loop, when `CNetData+0x610 != 0` and the flags
  `+0x2270` and `+0x226C` are both zero (other cases only log). A mismatch takes the desync path (`0x6F5378A0`,
  `0x6F2C8A10`); the log line `csum[%08X]` comes from the sync object printer `0x6F536C30`. So the record is the
  word the original computed for this slot. The record's own count and the `count` field printed in
  the log (`CNetData+0x1C68`, incremented by `0x6F5516E0` for player 0) are not in the record.

### 13.2 The sync object and the record writer (CONFIRMED structure)

- `CSyncData` (`NTempest`, vtable `0x6F9581A4`, RTTI from `tools/analysis/rtti_vtables.py`): embedded in
  `CNetData` at `+0x2278` (`0x6F55062F`). Slot 1 (`vtable+4`, `0x6F536BE0`) returns the dword at `this+4`, slot 2
  (`0x6F536C10`) clears `this+4` and `this+8`, slot 4 (`0x6F536C30`) prints `csum` through slot 1 of `this+8`.
  The CSyncData methods are not the checksum computation; the value itself is computed in 13.3.
- The record is written by `0x6F537820` (called in the time applier `0x6F5516E0` right after 13.3, for the local
  player, CONFIRMED call order) with `&CNetData+0x1C64` as its value argument (the record body is its
  serialisation; the exact bytes it emits are the `u8 4` + `u32` of 13.1, CONFIRMED by the real file).
  Replay reading: block `0x22` → `0x6F54E140` → `0x6F54CC10` → `0x6F54CAD0` (CONFIRMED path, which does not add
  time for `0x22`; only `0x1E/0x1F` do).

### 13.3 The value (CONFIRMED arithmetic, `FUN_6f545840`, `crates/w3g/src/sync.rs`)

Called only from the time applier `FUN_6f5516E0` (`0x6F551818`) for player 0 when the applied amount is nonzero
and the replay/online conditions hold. Writes `CNetData+0x1C64`:

```
A  = FUN_6f005e20(FUN_6f4c34d0(13))   fold over the sync participants of registry 13:
                                      for i = n-1 .. 0: v = vtable[0](participant_i); acc = rol(v ^ acc, 3)
c  = rol(A ^ 0x17AB, 3)
T  = CNetData+0x1C70   (the tick clock; +5 per tick, section 11), or FUN_6f543d40(world) when CNetData+0x2F8 != 0
X  = world+0x40, or world+0x1C when that is -1          (world = dword at 0x6FAB778C, set by 0x6F46D910)
Y  = world+0x50
v  = T ^ c
v  = rol(v,3) ^ X ;  v = rol(v,3) ^ X ;  v = rol(v,3) ^ Y
value = rol(v,3)                                          -> CNetData+0x1C64
```

Facts checked on the disassembly: the RNG-state hash `FUN_6f280BE0` of `[0x6FAB73D8]` is computed in
`FUN_6f545840` but its accumulator is never read (dead for the value). The whole-state variant `FUN_6f545930`
(which also folds X, Y and the RNG bytes through `FUN_6f4AB660` seeding) is **not** the 0x22 value; its callers
are `0x6F26CA98`, `0x6F2751E2`, `0x6F2852EC` (not the sync path). `FUN_6f4AB660` is the RNG seeding of section 4.3f.

Consequences (CONFIRMED by the algebra, tested in `sync.rs`):
- For fixed A, X, Y the value is GF(2)-affine in T: `value(T) ^ value(T') = rol(T ^ T', 12)`.
- A, X, Y change during a game (the real file's consecutive values are not affine in the millisecond clock: the
  test of `T` = the prefix ms sums gives 1 match in 89,259). So the record covers game state through A, X, Y.
- Verifying the value needs the game state at each slot. It cannot be checked from the file alone; the
  record stream is the oracle for the engine once A, X, Y and T are computed by the simulation.

### 13.4 Open (UNSURE)

- Which objects register in registry 13 (`0x6F6D98E0` is the per-thread table accessor; the registering call is
  not found) and what their vtable slot 0 returns. This is the main gap for A.
- The producers of world+0x1C, +0x40, +0x50 (world = the dword at `0x6FAB778C`, set by `0x6F46D910`). The other
  global used by the same function, `0x6FAB73D8`, is only read there (its first 8 bytes are hashed into a dead
  accumulator).
- Whether T is the tick clock before or after the slot's own ticks (the applier runs before the ticks; LIKELY
  before). The GF(2) test above assumed the prefix sums; with the state-dependent A, X, Y it cannot decide.
- `CNetData+0x2F8` (alternative tick source) and the flag that gates the computation (`+0x2288` tag, `+0x610`).

### 13.5 Pass 10 (2026-10-11): the components, re-read from the machine code

Labels as above. Addresses are Game.dll 1.26a. The value chain was re-read from the **machine code** (capstone
over the whole `.text`, plus an independent disassembly from `0x6F545840`), not only from the Ghidra decompile.

- **Correction of the decompile reading (CONFIRMED)**: the Ghidra output of `FUN_6f545840` shows only two XOR
  steps with the world words. The machine code has **three**: `rol3 ^X`, `rol3 ^X`, `rol3 ^Y`, then a final
  `rol3` (`0x6F5458E0`..`0x6F5458F9`). Section 13.3 is correct; `crates/w3g/src/sync.rs` and the new
  `crates/sim/src/sync.rs` implement the machine-code order (tests compare against the step-by-step version).
  Rule for agents: when a decompile and the disassembly disagree on a value chain, trust the disassembly.
- The registry fold (`0x6F005E20`, CONFIRMED): starts at acc = 0; walks slots from index 19 down to 0 (count at
  object+0x0C = 20); skips null slots; each non-null participant `p` contributes `v = vtable[0](p)` (the call goes
  through the object's first dword, the vtable) and `acc = rol(v ^ acc, 3)`. Implemented as
  `sim::sync::registry_fold(&[Option<u32>; 20])`.
- The registry object (CONFIRMED layout): `FUN_6f007d90` builds a 20-byte struct: +0 and +4 are two ref-counted
  pointers (cleared and released by `0x6F009B80`), +0x0C = 20 (slot count), +0x10 = data pointer to 20 zeroed
  dword slots (`0x6F006F10` reserve + `memset`). Created lazily by `0x6F008420` when the thread table entry 13 is
  null (`0x6F0084B2`), stored into table entry 13 at `0x6F0084E3`. The table is per thread: `FUN_6f4c34d0(id)` =
  `TlsGetValue`-based table lookup (`0x6F6D98E0`), `FUN_6f4c34f0(id, value)` = store. Writers of entry 13: `0x6F007AA6`
  (read), `0x6F007AE0` (clears it to 0 in the start-up sequence of `0x6F007A50`), `0x6F0084E3` (creates it). About
  33 functions read entry 13 (`0x6F002698` ... `0x6F06AA46`, list from the disassembly scan).
- **Open for A**: the code that writes a participant into a slot of the 20-slot array was not found. The scan for
  `[reg + idx*4 + 0x10]` stores is too broad (many containers). Which objects are participants, and what their
  vtable slot 0 returns, is still UNSURE.
- The tick clock T = `CNetData+0x1C70` (CONFIRMED): reset to 0 in the CNetData init (`0x6F5505D6`, and `0x6F54C5ED`
  in another init). Per game tick the frame driver loop (`0x6F553470`) runs `add [esi+0x1C70], ebp` at `0x6F553770`
  **before** it dispatches the tick (`0x6F5378E0`), and `add [esi+0x22C0], ebp` (wrap at 250) after. `ebp` =
  `[CNetData+0x139C]` (`0x6F553516`); the value 5 is LIKELY (110 ms per slot at normal speed = 22 ticks of 5 ms), the
  writer of +0x139C was not found (no direct store; UNSURE).
- The `+0x2F8 != 0` branch (CONFIRMED arithmetic): T = `FUN_6f543d40(world)` (`0x6F543D40`), a walk over 8-byte
  entries at world+0x0C, count at world+0x1C, that calls vtable slot 0x3C of entries tagged -2 and returns the
  value written by that call. Whether +0x2F8 is set in the replay checked here is UNSURE.
- The world words (`crates/sim/src/sync.rs::WorldWords`):
  - `world = [0x6FAB778C]`, allocated in `0x6F46D940` (0xDC bytes, constructor `0x6F46B6B0`).
  - The constructor's init `0x6F46B120` (CONFIRMED): `world+0x40 = -1`, `+0x44 = -1`, `+0x48 = 0`, `+0x4C = 0`,
    `+0x50 = 0`, `+0x54 = 0`, `+0x58 = 0` (the `or eax, 0xFFFFFFFF` at `0x6F46B169` makes the -1). So at creation
    X = world+0x1C (since +0x40 = -1) and Y = world+0x50 = 0.
  - `world+0x1C` is not written by the constructor; its writers are not found. UNSURE what it holds at start.
  - `world+0x50` has more writers (e.g. `0x6F46C616`, inside a different constructor with vtable `0x6F951858` that
    also writes +0x40 = `[0x6FAAE470]` = 0.0). The writers that change world+0x40/+0x50 during a game were not
    identified in the world's own code (UNSURE).
- The record stream itself (CONFIRMED on the real 1.26a replay, 89,259 records, tool `tools/analysis/sync_stream.py`):
  - **No consecutive pair shows the constant-state signature**: for two records with the same A, X, Y the XOR
    `rotr(csum_k ^ csum_k+1, 12)` equals `T_k ^ T_k+1`, which is below 2^20 for slots about 110 ms apart. The
    smallest bit length over all 89,258 pairs is 20, the typical one is 31 or 32. So A, X or Y changes between
    **every** pair of slots, from the first slot on (the test does not depend on the T model, only on T being
    small and close between slots).
  - One-variable tests fail too: "only Y changes" (`rol(dY,23)` pattern) gives large implied dY in every pair;
    "only A changes" gives a random-looking constant in every pair.
  - The 89,259 values are distinct except for one pair (consistent with random 32-bit values).
  - **First record**: with A = 0 (no participants yet), Y = 0 (world init) and T = 0 (CNetData init), the first
    value `cef28d12` is **not reachable** for any X (solved as a GF(2)-affine system in X: rank 31, 998/2000 random
    targets are reachable, so the test is about 50% weak per record, but it rejects this start state). So at the first
    slot either a participant is registered, or Y/T is not the start value assumed here. This is the first concrete
    hypothesis test and it fails.
- Verification counts (this pass): `crates/sim/src/sync.rs` 7 tests; `crates/w3g/tests/sync_state.rs` 3 tests (the real-file
  one under `W3G_REPLAY`); workspace tests 1037 passed without and with `GAME_DLL` + `W3G_REPLAY`; clippy clean.

### 13.6 Status of the oracle (what can and cannot be checked now)

- Checkable now: the arithmetic from (T, A, X, Y) to the record (sim and w3g agree with the machine code).
- Not checkable yet: any record of the real file, because A, X and Y are not produced by the simulation. To reproduce
  the first N records one needs (a) the start participants of registry 13 (their vtable slot 0 values), (b) the
  producers of world+0x1C and world+0x50 during map init (DotA's init script runs before slot 0), and (c) the
  per-slot changes. Without (a) and (b) the hidden state is not determined; the GF(2) solve shows that the
  simplest start state is wrong.
- Next steps: find the `[reg + idx*4 + 0x10]` store used by the participant registration (the functions that take
  the table entry 13 and the object to add: probably the sync-participant registration on object creation); trace
  the writes to world+0x1C and world+0x50 with a Unicorn run of the world constructor and the first map-init
  calls (the `tools/emu/emu.py` harness runs initializers); get the value of `[CNetData+0x139C]` from a running
  constructor.

### 13.7 Pass 11 (2026-10-11): registry 13 objects, world writers, the tick step, start-state test

Labels as above. Addresses are Game.dll 1.26a. Tools: `scratchpad` disassembly scans (not committed),
`tools/analysis/sync_stream.py` (new `implied_fold`), `crates/sim/src/sync.rs` (new producers, 11 tests).

**Goal 1: what registry 13 holds and how it is filled (partly CONFIRMED)**

- The table entry 13 (lookup `0x6F4C34D0`, ECX = 0xD) points to a registry object: data pointer at `+0x10`, 20 dword
  slots, each slot an owned object pointer or 0. Slot accessors (CONFIRMED, decompile + disassembly):
  - `0x6F0026B0` **get-or-create** (ECX = slot index): `0x6F6EB5B2` allocates 0xC bytes (Data.h line 0x74; 0xC =
    size of a `CMiscCustom`), `0x6F0020A0` builds it, and **the store is at `0x6F0026DE`: `mov [ecx+esi*4], eax`**
    (ecx = registry data pointer, esi = slot index). On allocation failure the slot is set to 0.
  - `0x6F002700` **clear**: if the slot is non-null, calls `vtable[2](1)` (deleting destructor) and sets the slot to 0.
  - About 19 further pairs of the same shape exist (`0x6F25AA20`..`0x6F25B040`, `0x6F2806E0`, `0x6F2808E0`, ...;
    list in the scan output, 38 functions with the `+0x10 + idx*4` pattern). Only the `CMiscCustom` pair is decoded
    so far. No direct `call`, `jmp` or data reference to these accessors exists in the file (they are reached some
    other way, e.g. a computed dispatch); **the callers and so the slot indices are UNSURE**.
- The participant classes (RTTI from `tools/analysis/rtti_vtables.py`, CONFIRMED names):
  - `CMiscCustom` (vtable `0x6F8750A4`): slot 0 = `0x6F0020D0`, slot 1 = `0x6F002000`, slot 2 = `0x6F002760`.
  - `CSkinCustom` (vtable `0x6F937B38`, ctor `0x6F309800` calls the `CMiscCustom` base ctor): slot 0 and 1 the
    same functions, slot 2 = `0x6F30ECC0`, slot 3 = `0x6F309820`.
  - So the participants that were decoded are **custom game-data objects** (Misc/skin overrides). A depends on the
    map's custom data, which the sim does not load.
- Slot 0 of `CMiscCustom` (`0x6F0020D0`, CONFIRMED arithmetic): with children `a = [obj+4]`, `b = [obj+8]`:
  `s = a ? rol(H(a),3) : 0`, `H = b ? rol(H(b) ^ s, 3) : s`. `H(child)` is the walk `0x6F6FA990` → `0x6F4D0570`
  with callback `0x6F6FA970` (not reproduced). Implemented as `sim::sync::custom_node_hash` (the child hashes are
  inputs). An empty object hashes to 0.
- The registry fold itself (section 13.5) is unchanged: `A` = fold over the non-null slots, the walk runs from
  slot 19 down to 0, each `acc = rol(v ^ acc, 3)`.

**Goal 2: writers of world+0x1C and world+0x50 (CONFIRMED for the constructor; the rest NOT FOUND)**

- `world = [0x6FAB778C]`. The world object is allocated in `0x6F46D940` with flag 8 (`Ordinal_401(0xDC, "...tempest_thread.h", 0x66, 8)`);
  its constructor `0x6F46B6B0` calls `0x6F46B120` (CONFIRMED stores: `+0x40 = -1`, `+0x44 = -1`, `+0x48..+0x58 = 0`,
  so `+0x50 = 0` at start). The constructor's own stores to `+0x1C` are into sub-objects at `+0x5C`, `+0x7C`,
  `+0x9C`, `+0xBC` (`ebx+0x1C`, not `world+0x1C`). The constructor also sets `[0x6FAB7788] = world`.
- So at start X = `world+0x1C` (because `+0x40 = -1`) and Y = 0. `+0x1C` is 0 if the allocation is zero-filled (flag 8;
  LIKELY, Storm not checked). Implemented as `sim::sync::WorldWords::at_start()`.
- Functions that read the world global (`[0x6FAB778C]`, 22 machine loads, 18 functions, 59 with direct callees,
  scan of stores with displacement 0x1C or 0x50): the only stores at those displacements are stack slots
  (`[esp+0x1C]`), sub-objects, or `[eax+0x50]` in `0x6F484C00` (a serializer-like function, its base is not the world).
  **No direct store to world+0x1C or world+0x50 was found** in the world code. Writers must go through an alias
  (world pointer passed as `this`/argument to another class) → **UNSURE, still open**.
- Other world-global writers: `0x6F46FCC0` (`mov [0x6FAB778C], ebx`), `0x6F46D9E0` (clear), `0x6F46D910`/`0x6F46D940`
  (create/replace, the `0x6F46B790`/`Ordinal_403` destroy path).

**Goal 3: the tick step `[CNetData+0x139C]` (CONFIRMED: the value is 5 per tick; the field has no writer)**

- `[CNetData+0x139C]` is read once (`mov ebp, [esi+0x139c]`, `0x6F553516`) as one term of a pending-count sum.
  No instruction in `.text` stores to `+0x139C` (no `mov/add/lea` with that displacement, no `0x139C` in the
  decompile except that read). So its value is whatever the object holds (0 unless a block copy writes it).
- On the path that runs ticks, `ebp` is **overwritten** before the tick adds: `mov eax, 0x19` (`0x6F553665`),
  `cmp [esi+0x2250], eax; jb 0x6F553869` (the tick block is entered only when the accumulator is at least 25 ms;
  the `jb` target does not reach the tick block), then `lea ebp, [eax-0x14]` (`0x6F553676`) = **5**. The tick adds
  (`0x6F553770`, `0x6F553785`) use this `ebp`; the only jump into `0x6F553770` comes from the loop at `0x6F5537A5`,
  which skips the `lea`, so `ebp` stays 5. Implemented as `sim::sync::TICK_CLOCK_STEP` = `TICK_UNIT_MS (0x19) -
  0x14` (test `tick_step_is_the_25ms_unit_minus_20`).
- Consequence: the tick step is a constant 5 ms, and the 139C field is irrelevant to the clock.

**Goal 4: start-state test on the real replay (LIKELY model, result: rejected or incomplete)**

- Real records (`w3g checksums --tsv`, 89,259 slots): slot 0 `cef28d12`; time before slot k = 110·k ms (22 ticks of 5 ms).
- Under the simplest start state T = clock before the slot's ticks (110·k), X = 0, Y = 0, the record implies one
  registry fold per slot (`sync_stream.py`, `implied_registry_fold` is the exact inverse of the value chain):
  record 0 → A = `1A258A4E`, record 1 → `5C7C8201`, record 2 → `5D432204`. **A = 0 (empty registry) is impossible
  for every record** (0 of 89,259 are 0), and 89,257 of the 89,259 implied folds are distinct. So at least one
  participant of registry 13 is non-empty from slot 0 (a custom-data object with unknown content) **or** X/Y are not
  constant zero. A fold that changes on every slot needs changing participants or world words.
- The simpler test "A = 0, Y = 0, T = 0, X free" is unreachable (`sync_stream.py`, unchanged result).
- Conclusion: **the sync value cannot be reproduced yet**. The missing inputs are (1) the contents (H values) of the
  `CMiscCustom`/`CSkinCustom` registry objects of the map (requires the map's custom Misc/skin data and the child walk
  `0x6F4D0570`), (2) the writer of world+0x1C/+0x50 (alias path), and (3) whether T is the clock before or after the
  slot's own ticks (110·k assumed). Next: a Unicorn run of the map-start path with a real `CMiscCustom` tree, and a
  watch-point on `0x6F0026DE` to see which slot index the first map start uses.

**Verification (this pass)**: `crates/sim/src/sync.rs` 11 unit tests pass (`cargo test -p sim --lib sync`); the real
replay export (89,259 records) and `sync_stream.py` run without error (values above).

---

## 14. Pass 8 (2026-10-11): the agile clock restart at 300 s

Question: how does the original keep agile time bounded, and what does the restart do to pending timers, sleeps,
`TimerGetElapsed/Remaining` and the mover's `(t - snapshot)`. Sources: disassembly of `0x6F46E520`, `0x6F46E370`,
`0x6F469D40`, `0x6F46CC80`, `0x6F477FF0`, `0x6F477F80`, `0x6F477EF0`, `0x6F4779F0`, `0x6F4775D0`, `0x6F46A030`,
`0x6F46C9C0`; the real routines run in Unicorn (`tools/emu/agile_clock_emu.py`). All addresses are in the original image.

### 14.1 The queue object (CONFIRMED)

A timer queue ("Agile/Timer Dispatch/Queue", built by `0x6F46C590`, called from `0x6F46DA10` for queue A at
`manager+0x40` and the long-timer queue "(l)" at `manager+0x44`; the world queues G1/G2/G3 use the same constructor
and template, section 11.1):

| Offset | Field |
|---|---|
| `+0x10` / `+0x1C` / `+0x20` | heap array / capacity / count (1-based, the count includes the unused slot 0) |
| `+0x38` / `+0x3C` | free list of element blocks (block = 4-byte link + element) / blocks in use |
| `+0x40` | the **clock**, an f32 (seconds within the current row) |
| `+0x44` | the **row**, a u32 (number of restarts), starts at 0 |
| `+0x48` | the **horizon**, a copy of the float at `0x6FAAE594` = **300.0** (set by the start-up initialisers) |
| `+0x4C` | flags: bit 0 = the clock does not advance (`0x6F46E520` returns at once) |
| `+0x50` | sequence counter, incremented by every timer start |

A heap element: `+4` expiry (f32), `+8` the clamped timeout (also the period), `+0xC` the queue, `+0x10` flags (bit 0
repeat, bit 16 cancelled, `0x20000` queued), `+0x14` the sequence number, `+0x18` the owner (called through its
vtable slot 3). The heap orders by expiry (an x87 float compare, not the soft float), then by the sequence number.

### 14.2 The advance and the restart (`0x6F46E520`, CONFIRMED; Unicorn-checked)

```
tmp = softadd(clock, inc)                        ; inc = the tick float 0.005 (section 9)
if tmp < horizon:                                ; fcompp, NaN or equal goes to the restart branch
    clock = tmp ; pump(queue)                    ; 0x6F46E370
else:
    excess = softsub(tmp, horizon)
    if |softsub(excess, 0.0)| < 8e-7 (0x6FAAE61C): excess = 0      ; unreachable in practice: floats near 300 are 3e-5 apart
    clock = horizon ; pump(queue)                ; everything due up to 300.0 fires first, at its own expiry
    for every heap element: expiry = softsub(expiry, horizon)     ; 0x6F469D40, array order
    row = row + 1
    clock = excess ; pump(queue)
```

* The pump (`0x6F46E370`) pops while `clock >= top.expiry`; during each callback the clock is the popped expiry; at the
  end the clock is restored to what it was on entry. A periodic element is re-armed after its callback with
  `expiry = softadd(clock, elem+8)` (`0x6F46CC80`) and keeps its sequence number (the sim assigned a new one before;
  fixed). Tie order among equal expiries is therefore the order of the original starts.
* Every pending expiry is above the horizon at the rebase (the pump to 300.0 popped the rest) and below twice of it
  (a start cuts the timeout, 14.3), so the subtraction is exact (Sterbenz) and the heap order stays valid.
* The rebase is the only place that subtracts anything. **Nothing else is rebased**: elements store absolute times in the
  queue's frame, a callback that starts a timer adds to the clock as it stands (the popped expiry), and the clock of a
  fresh row starts at the excess. Which frame a stored time belongs to is told by the row.
* The restart is the same for all queues that use the template (A, G1, G2, G3). They are advanced by the same float
  from the same start, so their clocks and rows agree (LIKELY, not run together).
* Game time is therefore `300 * row + clock`. Each row loses precision again from zero (the truncating add
  loses at most one unit in the last place per tick, and the unit grows with the clock), so 120,000 ticks of 0.005 s no
  longer read 597.39 s: the sim now reads **598.91 s** (the previous reading was one 600 s row without a restart).
  The remaining 1.09 s is the original's own truncation inside a row (the adds are the real routine).

### 14.3 Timer start (`0x6F477FF0` -> `0x6F477F80` -> `0x6F477EF0`, CONFIRMED; Unicorn-checked)

TimerStart and the sleep wake-up (`0x6F2DC6C0`) both pass a zero long-timer argument, so both use queue A. The timeout is:

1. **at or above the horizon** (300.0, infinity included; NaN is kept): replaced by `softsub(horizon, 0.001)` =
   **299.999** (`0x6FAAE47C`). Probe results on the real routine: timeouts 300, 600, 1e6 all start with expiry
   `clock + 299.999`, period 299.999.
2. **below 1e-4** (`0x6FAAE478`, zero and negatives included): replaced by exactly 1e-4.

then `expiry = softadd(clock, timeout)`, the sequence number is `++queue+0x50`, `elem+8` stores the clamped timeout.
This was not in the sim (it only clamped below). A sleep of 1000 s, or a 1000 s TimerStart, wakes after 299.999 s at
this level. Whether the JASS timer object above it restarts the remainder is **UNSURE**: its message handler
`0x6F3F6970` runs `0x6F3E8E70` for 0x80204 and restarts the timer for the stored timeout on 0x80272 (`0x6F3F6940`, only
the periodic flag read), and the sleep handler `0x6F2E3320` just resumes the thread (or re-posts 1.0 s when a flag at
`+0x1D8` of an agent is set). No code that continues a cut timeout was found. The sim follows the engine level.

### 14.4 Remaining and elapsed (CONFIRMED; Unicorn-checked, 2,385 queries in one run, plus 1,158 in the vectors)

* `TimerGetRemaining` (`0x6F4779F0`): `softsub(elem.expiry, elem.queue.clock)`, both in the queue's current frame, so a
  restart changes neither (the expiry lost 300.0 and so did the clock). Result is bit-equal to the sim's formula.
* `TimerGetElapsed` (`0x6F4775D0`): `softsub(elem+8, remaining)` with the **clamped** timeout of 14.3. The sim used the raw
  timeout (clamped to 0 below); fixed in `natives/timer.rs`. `TimerGetTimeout` is the raw stored value (unchanged).
* A timer whose element is gone (expired one-shot, paused) answers from the JASS object (paused value); unchanged.

### 14.5 What the restart does to the other holders of time

* **Movers** (movement-6.md section 1): a mover stores `(snap_t, snap_row)`; elapsed time is
  `snap(T - T0) + horizon * (row - row0)` with the layer's current time `T` and row, so a unit that started walking
  before a restart keeps its position. The sim now passes the clock row to the layer (`Game::game_row`,
  `set_game_clock`); before, it used row 0 and `T` jumped back by 300 s.
* **JASS sleeps**: the VM keeps each sleeping thread's wake time; at a restart it loses the horizon with the same soft
  sub as the paired timer expiry (`Vm::rebase_sleeps`), so `wake == expiry` bit for bit. The AI VMs follow when the next
  AI step runs (`AiHost::rebase_sleeps`).
* **Timers, attack/point-damage timers, timed life, vision refresh, movement timer**: they live in the queue and shift
  with it. Attack cooldowns are timers (relative). Nothing in the sim kept an absolute time except the stand-in
  production model (training/building/upgrade completion times, the harvest cycle), which now gets
  `AgileClock::absolute()` = `300 * row + clock` as an f32 (monotone; it is a model, not the original's clock).
* The engine report and `w3boot` print `300 * row + clock`.

### 14.6 Verification

* `tools/emu/agile_clock_emu.py` runs the real advance, pump, rebase, re-arm, start and remaining/elapsed routines on a
  synthetic queue (element pool as a prefilled free list; the owner callback and the base tail `0x6F022AA0` stubbed) and a
  Python model of the control flow (add/sub are the real routines): `check 200000 1` = 200,000 ticks, **6 restarts, 1,376,927
  fires, 2,385 remaining/elapsed queries, every start (expiry, period, flags, sequence), the state every tick and the full fire
  log: 0 mismatches**; `check 20000 1`: 0 mismatches. The scenario mixes one-shot and periodic timers (0.02 s to 2,000 s,
  timeouts of 0, 5e-5, 1e-4, 299.999, 300, 600, 1800) and increments of 0.005 s with occasional 0.001 to 20 s steps.
* `crates/sim/tests/agile_clock_vectors.txt` (85 kB, written by `vectors`, no game bytes) holds three scenarios (150,000
  plain ticks with 2 natural restarts, 90,000 and 150,000 ticks with 2 and 4 restarts; 1,428,691 fires in all);
  `crates/sim/tests/agile_clock.rs` replays them on `AgileClock` + `TimerQueue` (checkpoints, queries, the first 100 fires,
  a hash of all fires): 0 mismatches. `tests/timer.rs`: a thread sleeping 100 s keeps waking across two restarts (7 wakes in
  150,000 ticks, no misfires); a 1000 s sleep wakes at 299.999.
* `w3boot --computer 1:orc --steps 6000` on (2)Circumvention: 120,000 ticks, **game time 598.91 s**, 82,585 timer/sleep
  entries, the orc AI plays the same economy as before (14 peons, 6 grunts, 3 head hunters, a blademaster, 4,580 gold
  deposited by 600 s), 0 aborted AI threads.

### 14.7 Still UNSURE

* The long-timer queue "(l)" (`manager+0x44`, chosen by a non-zero last argument of the start routine) is never pumped by `0x6F470C70` as far as read;
  nothing in the sim uses it.
* Whether the world queues' rows move exactly with queue A (same adds from the same start: LIKELY).
* The flag bit 0 of `+0x4C` (clock paused): never set in the sim.
* The cut of long timeouts at the JASS level (14.3).
* `GetGameTime`-style readers: the native `TimerGetElapsed(bj_gameTimer)` style use is the same getter; no separate game clock.

---

## 15. Pass 12 (2026-10-12): the 0x22 value decoded (net checksum, registry slots, tags) and the replay oracle

Labels as in section 13. Addresses are Game.dll 1.26a. Tools: IDA (database `game126a`), `tools/emu/net_checksum_emu.py`
(Unicorn), the real replay `Replay\2856263.w3g` (DotA v6.83se, 9 players, 142,647 time slots, seed `0xd8917f0a`; the user's file,
never committed), `w3boot --replay FILE --check-sync`, code in `crates/sim/src/sync.rs` and `crates/engine/src/sync_check.rs`.

### 15.0 Corrections to section 13 (read this first)

The Ghidra decompile of `0x6F545840` that sections 13.3 and 13.5 were built on dropped an output argument. From the machine code
(`0x6F5458B8..0x6F5458F9`, and the Unicorn run below):

```text
c      = rol(A ^ 0x17AB, 3)                     A = registry fold (13.5)
v      = T ^ c                                  T = [CNetData+0x1C70]
v      = rol(v,3) ^ X                           X = [world+0x40], or [world+0x1C] when that is -1
v      = rol(v,3) ^ Y                           Y = [world+0x50]
v      = rol(v,3) ^ R                           R = the value 0x6F280BE0 stores through its edx argument
sync   = rol(v,3)                               -> [CNetData+0x1C64]
```

* **`X` is used once, not twice**, and there is a **fourth term `R`**. The accumulator that 13.3 called "dead" is `var_C`, the
  output of `0x6F280BE0([0x6FAB73D8], &var_C)`: it is XORed in the third step. `0x6F280BE0` (CONFIRMED) sums the byte hash
  `h(b) = b ^ (b * 0x7A2D)` (32-bit) over the 8 bytes at `[0x6FAB73D8]` and `[0x6FAB73D8]+4`, i.e. over the global generator state
  (A, B). So `R` is the hash of the generator state used by `GetRandomInt`.
* The value is linear over GF(2): `sync = rol12(T) ^ rol12(c) ^ rol9(X) ^ rol6(Y) ^ rol3(R)`.
* CONFIRMED by running the original `0x6F545840` in Unicorn on 400 synthetic states (a fake CNetData, world object, generator object and
  a registry whose slots are `CCheatData` objects): 0 mismatches (`tools/emu/net_checksum_emu.py`; 40 of the vectors are embedded in
  `crates/sim/tests/sync_vectors.rs`). The 13.5 "start-state test" (A = 0, Y = 0, T = 0 unreachable) and the 13.7 numbers
  (`implied fold`) used the wrong chain and are void.
* Section 13.7 named `CMiscCustom` as the registry-13 participant. It is one of the **custom-data** kinds (slots 12..19, 15.2); the
  participants that vary during a game are different objects.

### 15.1 The net checksum (registry slot 2, `CNetData+0x227C`): CONFIRMED

Registry 13 slot 2 is the `CNetData` object itself (vtable `0x6F9590D0`; many `CNetData` methods fetch it with
`mov ecx,0Dh; call 0x6F4C34D0; mov eax,[eax+10h]; mov eax,[eax+8]`). Its vtable slot 0 `0x6F537E10` jumps to slot 1 of the embedded
`CSyncData` (`0x6F536BE0`), which returns the dword at `this+4`, i.e. **`CNetData+0x227C`** (written by the constructor with 0,
`0x6F550639`). That field is a running checksum of the received **blocks**; it is the part of the sync value that changes in every slot.

The block pump is `0x6F5530D0` (thiscall, one stack argument `a2`). For every pending block node (node+0x14 = block id, +0x15 = flag,
+8 = payload pointer, +0xC = payload length) it computes, when the block is applied (`a2 != 0`), the node's term and then runs the block
handler (`0x6F551D80`; for a time slot that is the time applier `0x6F5516E0`, which computes the sync value). So the slot's own term is
**already in the checksum when the slot's record is computed**.

Conditions for hashing a node (`0x6F55315A..0x6F553191`, LIKELY meaning): `(node+0x15 != 0) == ([CNetData+0x610] == 0)` and
`[CNetData+0x278] >= 4`. In replay playback the replay pump enqueues with flag 0 and `+0x610` is nonzero; the replay oracle below works
with exactly these conditions true. Block ids that are hashed: `0x1E`, `0x1F` (time slots), `0x17` (leave), `0x21`, `0x2F`; every other id is only applied.

Terms (machine code `0x6F553307..0x6F5533BA`; `h4(x)` = sum of the four byte hashes of `x`, i.e. `0x6F280B90` into a zero accumulator):

| block | `v` | checksum update |
|---|---|---|
| `0x1E` / `0x1F` | `id + h4(P)` where `P = 0x6F39E5C0(payload, len)` | `N += h4(v)` |
| `0x17` leave | `0x17 + h4(reason) + h4(player) + h4(result)` (fields of `w3g::Block::LeaveGame`; the 4th field is not hashed) | `N += h4(v)` |
| `0x2F` game end | `0x2F + h4(mode) + h4(countdown)` | `N += h4(v)` |
| `0x21` | two bytes of the block, `h4` each (no `w3g` block) | not implemented |

`0x6F39E5C0(ptr, len)` (CONFIRMED): `acc = 0`; for each little-endian dword `w`: `acc = rol(w ^ acc, 3)`; then for each remaining byte `b`:
`acc = rol(b ^ acc, 3)`. The payload of a time slot is the block body **after its u16 length**: the u16 time increment, then for each player
`u8 id, u16 len, actions` (exactly what `w3g::Block::time_slot_payload` rebuilds; the replay stores the network bytes unchanged).

Unicorn: the real `0x6F5530D0` on a one-node queue (all other calls stubbed) agrees with `sim::sync::NetChecksum` on 300 random
time-slot nodes and 60 leave + 60 game-end nodes, 0 mismatches. The primitives `0x6F39E5C0`, `0x6F280B90`, `0x6F280BE0`, `0x6F280C80`,
`0x6F3834D0` also match (900 + 80 cases).

### 15.2 The twenty slots of registry 13: CONFIRMED indices, partial contents

The dump function `0x6F5455A0` (it prints "build 6059", "next presence tag", "next birth tag" and the checksums below) reads slots 0..19 in
order and calls vtable slot 0 of each non-null one:

| slot | class (vtable) | vtable slot 0 | notes |
|---|---|---|---|
| 0 | `CCheatData` `0x6F940A90` | `[this+4] & 0xFFFFEFFF` (`0x6F3834D0`) | printed as "cheat checksum"; 82 readers |
| 1 | combat (printed "combat checksum") | | **null**: no code besides the dump reads or creates it |
| 2 | `CNetData` `0x6F9590D0` | the net checksum, 15.1 | printed "net checksum" |
| 3 | `CRandData` `0x6F92FE18` | `0x6F280C80`: sum of `0x6F280BE0` over the **45 streams** (the object holds the 45 8-byte states at +4) | printed "rand checksum"; accessed by `0x6F285320` (stream draws) |
| 4 | unit data ("unit checksum") | | null (no other reader) |
| 5 | `CAbilityDatabase` `0x6F92E0A8` | `0x6F005D60`: `[this+0xC]` once mixed with a file hash (`0x6F4C9050`) | created lazily; static per game |
| 6 | `CGameWar3` `0x6F941B10` | `0x6F3A1690`: `acc = 12`; for the 12 player objects `acc = rol(acc ^ H(player), 3)`, `H = 0x6F40FEB0` | printed "game checksum"; player state hash not decoded |
| 7, 8, 9 | data databases (slot 7 created by `0x6F2B6860`, a 0x118-byte object; slot 8 cleared at `0x6F26B6D2`; slot 9 created by `0x6F25F0B0`) | not decoded | static per game |
| 10, 11, 15 | | | no creator/reader: null |
| 12..19 | `CCustom*Data` (CMiscCustom `0x6F8750A4`, CSkinCustom ...) | map-custom text tables (string hash `0x6F39E5C0` of key/value pairs) | created on demand (get/create/clear triples `0x6F25AA20..`, `0x6F25B420..`); static during a game |

The walk and fold (13.5) are unchanged: slots 19 down to 0, null slots skipped, `acc = rol(v ^ acc, 3)`. The rotation with which slot `s` reaches the
sync value is `12 + 3 + 3 * (number of non-null slots with index <= s)` (`sim::sync::participant_rotation`). With slots 0 and 2 non-null and
slot 1 null the net checksum enters as `rol21(N)`. **The real replay confirms rotation 21 and no other** (15.3), so slot 1 is null and slot 0
is present for the whole game.

### 15.3 The record stream of the real replay: what is reproduced

* One record per computed slot: the time applier computes the value only when the slot's **increment is nonzero**; on a zero-increment slot the
  file repeats the previous value, and the first slot (increment 0) carries 0. 142,647 slots, 142,616 records (31 slots at the very end have none),
  71,308 slots with a nonzero increment.
* **Rotation scan**: for consecutive computed records the residual `v_a ^ v_b ^ rol(N_a ^ N_b, s)` is small (its `rotr12` is below 4096) in 367 of the
  first 5,000 pairs for `s = 21` and in **no** pair for any other `s` in 0..31. The net checksum is therefore right, bit for bit, wherever the
  hidden words did not change.
* **Clock model**: `T = 25 * floor(elapsed_before / 25)` (elapsed = sum of the earlier increments): the frame driver runs 5 ms ticks in 25 ms units
  (13.5) and the slot is applied once the earlier slots' ticks are done. Pairs with exactly zero residual under this model: **516** (of 71,307) with
  `T = 25*floor(c/25)`; shifting the model by +-1 ms leaves 444 / 461, +-2 ms 410 / 413, and so on down; `T = c` exactly gives 6 and
  `T = 5*floor(c/5)` gives 0. So the 25 ms granularity is real.
  The leave blocks occur mid-game in this file (the first one before pair 16,359, ten in all): without their terms 493 pairs are exact and none after the
  first leave; with the `0x17` term hashed as in 15.1, **23 more exact pairs appear after the leaves**, which confirms that term (516 total).
* `w3boot --replay FILE.w3g --check-sync` output on this file:

```text
  time slots 142647  records 142616  computed 71308 (nonzero increment)
  net checksum rotation 21  final net checksum 0xa849e6c4  leave blocks 10
  absolute: 0 of 71308 computed records equal the start-state prediction; first mismatch: slot 1
  differential: 71307 pairs, 516 exact (clock + net explain everything), 16268 birth-tag-like, 54523 other
  first inexact pair: 0  first exact pair: 3350  longest exact run: 2
  exact pairs per 5000 pairs: 333 141 14 5 3 2 1 1 2 1 2 1 5 4 1
```

* **How far the match goes**: records are **not** reproducible in absolute terms. The first record (slot 1, 38 ms, value `0xED219BF0`) depends on
  hidden words fixed by the DotA map script before the first slot (the number of pathing-side objects created: Y, the presence count X, the
  generator state after the map's own draws: R, and the static data slots 5..9 and 12..19). What is reproduced from the replay stream alone is every
  term that depends on the stream: the clock and the net checksum, exactly, which is proved by the 516 zero-residual pairs and by 16,268 further pairs
  whose residual is a pure `rol6` change of at most 16 bits (see 15.4). **No** pair of consecutive records is constant-state from the start: the birth tag
  `Y` changes in almost every slot.

### 15.4 `X` and `Y`: what the tags are (CONFIRMED meaning, producers partly open)

* The dump prints `[world+0x40]` (or `[world+0x1C]` when that is -1) as "next presence tag" and `[world+0x50]` as "next birth tag". `world` is the
  0xDC-byte object at `[0x6FAB778C]` (also `[0x6FAB7788]`; allocated in `tempest_thread.h`): its first two members are `NTempest::CDynTable<CPrRgEntry>`
  containers (vtable `0x6F9517D8`, data pointer +0x0C, element count +0x1C, 8-byte entries `(key, object)`).
* `0x6F4AEFF0` is the registration of a `CPrRgEntry`-derived object: it takes a slot in the table (`0x6F4BC310`); the object's tag is
  `world+0x50` (then `world+0x50 += 1`, **birth tag**), or, when no slot could be allocated, `world+0x54`. `X` (+0x1C) is the number of table entries
  (a high-water mark that never shrinks), used while `+0x40 == -1`. The `CPr*` classes that register (callers of `0x6F4AEFF0`: `0x6F483080`, `0x6F487ED0`,
  `0x6F48ED30`, `0x6F48F0D0`, `0x6F4924D0` = the movement cluster of 12 unit records (movement-3.md), `0x6F49A330`, `0x6F49B680`, `0x6F49F5D0`,
  `0x6F4A0710`, `0x6F4A6070`, `0x6F4AA340`) are the pathing/movement objects: every path-request or mover object creation advances `Y`.
* On the real file `Y` advances in almost every slot (the residual is a `rol6` change in 16,268 of the 70,791 inexact pairs, and the rest are mixed
  changes of Y with X, R or the unit/data slots). Reproducing Y needs the pathing and movement object lifetimes of the whole game, i.e. a
  simulation of every unit order of every player and creep: out of reach until movement and pathing are complete, and independent of the map-script gap.

### 15.5 Code and tests

* `sim::sync` (`crates/sim/src/sync.rs`): `sync_value(t, fold, x, y, r)`, `registry_fold`, `participant_rotation`, `byte_hash`, `word_hash`,
  `payload_hash`, `NetChecksum` (time slot, leave, game end), `rng_state_hash`, `rand_data_hash`, `cheat_data_hash`, `game_data_hash`,
  `tick_clock_at_slot`, `SyncState` (feeds blocks, applies the zero-increment rule, keeps the hidden words as inputs), `residual`.
* `engine::sync_check` (`check_replay`, report as above), `Engine::sync_time_slot` (the per-slot hook: net checksum, `R` and the rand slot from the host's
  generator, hidden words settable through `Engine::sync_mut`), `w3boot --replay FILE.w3g --check-sync`.
* `w3g`: `Block::TimeSlot` now keeps the block id (`id`), `Block::time_slot_payload`, example `slotdump`; `w3g::sync::sync_value` has the corrected 5-input form.
* Tests: 20 unit tests in `sim::sync`, `crates/sim/tests/sync_vectors.rs` (40 original-code vectors), 4 in `engine::sync_check`, `engine/tests/sync_real.rs`
  (`W3G_REPLAY`: at least one exact pair, rotation 21, absolute mismatch), `w3g/tests/sync_state.rs`.

### 15.6 Open / next

1. The static hash of slots 5, 7, 8, 9 and 12..19 (data tables; custom text from the map) and the player hash `0x6F40FEB0` (slot 6): needed for an absolute
   match once Y is known. Slot 6 and the generator hash are the pieces the sim can already supply (`R`, slot 3).
2. `X`/`Y` producers: the `CPr*` object lifetimes (path requests, mover clusters). A way to calibrate without the full game: choose the registry constant by
   fitting one record, then check the `Y` sequence is a non-decreasing counter (the XOR structure of the residuals allows this); not done.
3. `0x21` block: its 2-byte reader `0x6F6516F0` and where the replay shows it (none in the DotA file).
4. Whether the pump conditions hold (`+0x610`, `+0x15`) for a live game: irrelevant for replays.

## 16. Pass 13 (2026-10-12): the world words X and Y, their writers, and what the replay can say about them

Labels as in section 13. Tools: `tools/emu/pr_registry_emu.py` (Unicorn), `crates/engine/examples/syncdump.rs` (one TSV row per
slot: increment, clock, net checksum, record, actions), `sim::sync::PrRegistry`, `w3boot --replay FILE --check-sync`. Real replay
`2856263.w3g` (not committed).

### 16.1 The registry (CONFIRMED, Unicorn)

`0x6F4AEFF0` (thiscall on the object, one optional descriptor argument) registers a `CPrRgEntry`-derived object; `0x6F4AEF00`
unregisters it (frees the slot, sets slot and tag to -1, then calls virtual slot 1). The world object `[0x6FAB7788]` holds two
8-byte-entry tables (`world+0x00` and `world+0x20`; data +0xC, count +0x1C, free-list head `world+0x40` / `+0x44`, live count
`+0x48` / `+0x4C`). Table allocator `0x6F4BC2B0` / `0x6F4BC240`: take the free-list head if any (the freed entry stores the next
head), else append. Free `0x6F4BC1D0` pushes the entry as the new head. Descriptor `+0x24 == -1` or no descriptor: table 0 and the
**birth tag** `Y = world+0x50++` (`0x6F4AEFC0`; `world+0x54++` for slots with bit 31 = table 1). `+0x24 == -2`: table 1 (tag from
`+0x54`). Otherwise the descriptor gives the slot and tag.
`X` printed by the dump (`0x6F5455A0`) = free-list head when not -1, else the table length. `Y` never decreases.
Verification: 400 random sequences, 24,598 operations (register with and without descriptors, flagged, explicit slot, unregister),
comparing counters, both tables and every object's slot/tag after every operation: **0 mismatches**; 10 short sequences embedded in
`crates/sim/tests/pr_registry_vectors.rs`. The Rust `PrRegistry` follows it exactly.

### 16.2 Who registers (CONFIRMED callers, meaning of each partly LIKELY)

Callers of `0x6F4AEFF0` (12 sites) and their classes by RTTI (all in the slot-3 virtual of the class): `0x6F483080` (class not in a
vtable), `0x6F487ED0` = `CBaseMap<CPaCell>` / `CAcceleratorMap`, `0x6F48ED30` = `CPrClusterGroup`, `0x6F48F0D0` = `CPrClusterGen`,
`0x6F4924D0` = `CPrCluster` (the movement cluster of 12 unit records), `0x6F49A330` = `CLrPathingAcc`, `0x6F49B680` = `CLrPath`
(a path request object), `0x6F49E0C0` = `CPmRegion`, `0x6F49F5D0` = `CBaseMap<CPmCell>`, `0x6F4A0710` = `CLrPathingSys`,
`0x6F4A6070` = `CPrRelation`, `0x6F4AA340` = `CPrBehavior` / `CBhPoBehavior`. Unregister callers: `0x6F487FAB, 0x6F48ED97,
0x6F48F137, 0x6F49318F, 0x6F49A630, 0x6F49DE82, 0x6F49E0D0, 0x6F49F6AB, 0x6F4A3978, 0x6F4A695E, 0x6F4AA55A`.
Y advances for every cluster, path request, region, relation and behaviour object created (behaviour objects and relations are
per-unit / per-pair and probably dominate). Only clusters and path requests are hooked in the sim (`Game::pr_unit_created`: one
registration per 12 units, LIKELY; `Game::pr_path_request`: register + release, UNSURE lifetime). Creation sites of CPrBehavior,
CPrRelation and CPmRegion objects are NOT located, so Y in the sim is a lower bound. `Engine::sync_time_slot` now takes X and Y from
`Game::pr`.

### 16.3 Replay evidence (numbers exact)

Computed records: 71,308 (nonzero increment); pairs: 71,307. With clock and net checksum removed, per-pair residual `D`:

| class | pairs |
|---|---|
| zero (nothing hidden changed) | 516 |
| `rotr6(D) < 2^16` (birth-tag-like) | 16,268 |
| `rotr9(D) < 2^16` (X-like) | 1,504 |
| `rotr3(D) < 2^16` (R-like, small) | 19 |
| other (several words) | 53,000 |

Most frequent `rotr6(D)`: 2 (319), 5 (238), 3 (225), 7 (198), 6 (174), 1 (161). Minimal birth-tag step per tag-like pair (smallest d
with `y ^ (y+d) = r` for some y; 3,764 pairs have one with d <= 300): d=1 786, d=2 700, d=3 504, d=4 57, d=5 197, d=6 86, then a long
tail. Lower bound: the true step is only known modulo the unknown start.

**dY vs actions** (3,764 compatible pairs; orders = actions 0x10..0x15 between the two records): 0 orders in the window: 3,337 pairs,
mean minimal step 36.0; 1 order: 329, mean 105.0; 2: 82, mean 114.2; 3: 16, mean 123.8. Orders raise dY (about +70 for the first
order) but the idle baseline (units walking, creeps, behaviours) is far larger, so a model dY = f(actions) explains **no pair
exactly**. Exactly explained pairs: 516 before and 516 after this pass.

**Chain test** (new in `--check-sync`): a run of consecutive birth-tag-only pairs must come from one monotone counter: start
`y0 < 2^16`, `y_{i+1} = y_i ^ r_i`, increase <= 4096. Result: 2,511 runs of >= 2 pairs (cut at 12), 468 consistent (1,031 pairs);
a random control of the same lengths gives 220 of 2,511. The birth-tag reading holds for a subset, weakly overall: many tag-like
pairs are coincidences with X, R or data-slot changes.

### 16.4 Status

* CONFIRMED: registry algorithm and the X/Y definitions (Unicorn); the residual structure.
* LIKELY: clusters register once per 12 units.
* UNSURE: path-request lifetime; creation of behaviour/relation/region objects; absolute Y at slot 1 (the map script's units).
* Explained pairs: 516 exact (unchanged); 1,031 further pairs are only chain-consistent, not exact.

### 16.5 Open

1. Locate creation/release of `CPrBehavior` and `CPrRelation` (they dominate Y), e.g. by Unicorn runs of the order start path.
2. Start values of X and Y after the DotA map script (needs the full unit-creation order).
3. R, data slots 5..9 and 12..19 remain unknown; an absolute match needs them all.

## 17. Pass 14 (2026-10-12): who creates and releases the pathing objects (the birth tag Y), and what the replay says

Tools: `tools/emu/births_emu.py` (Unicorn, real code of Game.dll on a fake registry world and fake pools), `crates/engine/examples/ybirth.rs`
(replay analysis), `sim::unit_position::PrObjects` + the `pr_*` hooks. Labels as in section 13. Replay: `2856263.w3g`
(71,308 computed records; found in the game's `Replay` folder). All addresses are Game.dll 1.26a.

### 17.1 How a registration happens (CONFIRMED structure)

`0x6F4AEFF0` is **virtual slot 3** of `CPresenceTagged` (the base of every pathing object) and `0x6F4AEF00` is **slot 4**. Creators are
small wrappers of the shape "get the pool from the movement world `[0x6FAB73D8]`; allocate (`CDataAllocator` free list
`0x6F4C1AB0`); construct; store the pointer; if the third argument is non-zero call `[vtable+0xC]` (= slot 3, the registration)".
So a registration is a separate step after construction and several creation paths build without it (`a3 = 0`).
The pools are in the movement world: generators `+0x638` (ctor `0x6F4916D0`), clusters `+0x678` (ctor `0x6F486A90`), paths `+0x958`
(ctor `0x6F49CA10`). Attach overrides (slot 3) that nest a second registration: `CPrCluster` `0x6F4924D0` calls the base **and**
`0x6F48BFD0(1)` (creates and attaches a `CLrPath`, stored at cluster+0x3C).

### 17.2 The table (the 12 register callers, with their creators and triggers)

| class (slot-3 override) | creator / constructor | trigger | births | lifetime / release |
|---|---|---|---|---|
| `CPrCluster` `0x6F4924D0` | `0x6F48DF00` (pool `+0x678`) | the flush `0x6F493590` of a generator makes one per alive member (`0x6F4934E0`); also world build | 2 (itself + its `CLrPath`) | the walk `0x6F493410` calls slot 4 `0x6F493130` when no member is alive: unregisters the path, then itself (2) |
| `CLrPath` `0x6F49B680` (no own slot 4: base `0x6F4AEF00`) | `0x6F48BFD0` -> `0x6F48ACC0` -> ctor `0x6F49CA10` (pool `+0x958`) | (a) cluster attach, (b) the unit's own path: `0x6F474C10` -> `0x6F495960` creates it when unit+0xA8 is empty; `0x6F474C10` is called by `CUnit::vf107` `0x6F2A0E30` (unit creation) and by 5 missile paths (`0x6F2BF3E0`, `0x6F2C1360`, `0x6F2C3DC0`, `0x6F2C3E90`, `0x6F2EBAB0`) | 1 | with its owner |
| `CPrClusterGen` `0x6F48F0D0` | `0x6F474220` (pool `+0x638`, ctor `0x6F4916D0`); `0x6F47B270` (creates without the flush, group moves) | a mover start: `0x6F474D40` (move to a point: `CAbilityMove` start from `0x6F205890` at 4 sites, missiles `CMissileLine` / `CArtilleryLine` / `CBulletPath` / `CMissilePath` and `0x6F2EC310`), `0x6F475030` (turn towards: `CAbilityMove` `0x6F206960`, missiles), `0x6F475230` (**`SetUnitFacingTimed`** `0x6F3C5B30` and `CAbilityMove` `0x6F204750`), `0x6F493620` via `0x6F4761E0` (stop / clear movement: buffs Ensnare, Web, EntanglingRoots, Purge, `0x6F202FF0`, `0x6F12A1D0`) | 1 | released inside the same call by the flush (1 unregister) |
| `CPrClusterGroup` `0x6F48ED30` | `0x6F47B7E0` (wrapper `0x6F47B810`), ctors `0x6F47B1B0`, `0x6F489750` | missile code (`0x6F2EA810` <- `0x6F2EAD00`), world build | 1 | the walk `0x6F48E4C0` releases an empty one (slot 4) |
| `CPmRegion` `0x6F49E0C0` | `0x6F48ACF0`, `0x6F48C000`; chain `0x6F48D410` <- `0x6F48DA90` <- `0x6F4A03F0` <- `0x6F4A0440` <- `0x6F47C1A0`, `CTriggerRegion` vf24 `0x6F47D210` | JASS regions (UNSURE) and world build | 1 per region | region destroy (`0x6F49DE40`) |
| `CPrRelation` `0x6F4A6070` and its 11 callers (`CRlAgent`, `CRlAgentDef`, `CRlProperty`, 8 `CPo*` modifiers / listeners) | ctors `0x6F4801E0`, `0x6F4803B0`, `0x6F487390`, `0x6F488C50`, `0x6F488DD0`, `0x6F489E70`, `0x6F48AE90` ... `0x6F48C6E0` | all reachable only from the one-time world build `0x6F48E490` (called from `0x6F4830D0`) and from static tables; no per-tick creator | per map | map end |
| `CPrBehavior` / `CBhPoBehavior` / `CBhPoProjectile` `0x6F4AA340`, `0x6F496A40` | ctors `0x6F479D10`, `0x6F479E40` have no caller; `0x6F479F90` only from the dead pool allocator `0x6F47A9C0` and `0x6F47AA10` <- `0x6F47AA70` | world build, function table entry `0x6F47AAA0` (`.rdata` `0x6F877F34`); **no gameplay trigger found** | per map | UNSURE |
| `CBaseMap<CPaCell>` / `CAcceleratorMap` `0x6F487ED0`, `CBaseMap<CPmCell>` `0x6F49F5D0`, `CProximityMap` `0x6F4A04D0`, `CLrPathingAcc` `0x6F49A330`, `CLrPathingSys` `0x6F4A0710`, `0x6F483080` | ctors `0x6F4845C0`, `0x6F4891F0`, `0x6F49F2A0`, `0x6F49FAA0`, `0x6F4889E0`, `0x6F4A1B00` | map load | one each | map end (`0x6F487F70`, `0x6F49A600`, `0x6F49F670`, `0x6F4A3960`) |

Unregister callers (11): `0x6F487F70` (maps), `0x6F48ED60` (group), `0x6F48F130` (generator), `0x6F493130` (cluster), `0x6F49A600` (pathing
accelerator), `0x6F49DE40` / `0x6F49E0D0` (region), `0x6F49F670`, `0x6F4A3960`, `0x6F4A6920` (relation), `0x6F4AA550` (behaviour). The
`CLrPath` uses the base routine directly. Triggers: the movement walk `0x6F483300` (empty groups `0x6F48E4C0`, finished clusters
`0x6F493410`), the end of every flush (the generator), unit teardown (the unit's path), map end.

So in play **Y moves because of `CLrPath`, `CPrClusterGen`, `CPrCluster` and `CPrClusterGroup` objects**; relations, behaviours and
regions are map-load objects and do not move Y during play (the map script's `Region` natives are the only gameplay exception).
This contradicts the section-16 guess that behaviours and relations dominate.

### 17.3 Unicorn results (`births_emu.py`, 300 cases each, real code, fake pools)

* C1 `0x6F48DF00(1)` creates a cluster: **2** registrations (classes `0x6F95230C` cluster, `0x6F95336C` path), 0 unregistrations; without
  the attach flag 0. C2 `0x6F474220(1)` (generator): **1**. C3 `0x6F48BFD0(1)` (path): **1**. All 300/300 identical.
* C5 the flush `0x6F493590` of a generator with k valid members (k = 0..12, random): **2k** registrations and exactly **1**
  unregistration (the generator) for every k: 300/300. The non-creating callees (`0x6F4927A0`, `0x6F4927F0`, `0x6F4932D0`,
  `0x6F491410`, `0x6F48E5F0`, `0x6F48E600`, `0x6F48F650`) are stubbed; that none of them registers or unregisters is by reading only
  (`0x6F4927F0` is large and recursive: **UNSURE**, it detaches the member from its old cluster).
* C6 `0x6F474C10` on a fresh mover: first call **1** registration, later calls **0**: 300/300. Hence a created unit is **+1** (its own path).
* C7 releasing a cluster through its vtable slot 4: **2** unregistrations (path first, then the cluster): 300/300.
* C8 the mover start `0x6F474D40` (stubs for its pure movement callees, real flush): live unit **3** registrations + 1
  unregistration; unit whose pair no longer resolves **1** + 1: 300 cases, 0 deviations.
  `0x6F475030` and `0x6F475230` have the same generator + member + flush shape by reading (LIKELY 3), not run.

CONFIRMED: the counts of these entry points as listed. LIKELY: that the gameplay events listed in the table reach them as described
(static caller trees; virtual calls hide some edges, e.g. who calls the `CAbilityMove` handler and how often).

### 17.4 The sim hooks (`sim::unit_position`, `PrObjects`)

Replaced the section-16 hooks (a cluster per 12 units, a register+release per path request: both unsupported; a path request creates no
registered object: no creator is reachable from the dispatcher `0x6F49A800`).
* unit created (`place_new_unit`): +1 (`pr_unit_created`; C6).
* route start or chase repath (`issue_route_motion`): mover start, Y + 3, the generator slot is the free-list head again (`pr_mover_start`; C1, C2, C5, C8).
* waypoint reached (`step_unit`): a new mover start per leg (UNSURE: that the original restarts the mover per leg).
* unit stops after moving, unit removed: its cluster pair is released at the next walk, path then cluster (`pr_mover_idle`, `pr_walk_release`; C7);
  removal also unregisters the unit's path (`pr_unit_removed`).
* missile launch: one mover start, released at impact in launch order (`pr_missile_launch`, `pr_missile_impact`; UNSURE: the missile's own objects).
Not modelled: stop / face / SetUnitFacingTimed starts, group moves (`0x6F47B270`), `CPrClusterGroup`, region natives, the map script's objects.
Tests: `unit_position::tests::registrations_follow_the_movement_object_counts`, `missiles_release_oldest_first`.

### 17.5 What the replay says (numbers exact; `w3boot --replay ... --check-sync`, `ybirth`)

Unchanged by the hooks (the check needs no map and does not run the sim): 71,308 computed records, 71,307 pairs, **516 exact**, 16,268
birth-tag-like, 54,523 other, 468 chain-consistent runs (1,031 pairs). The map of this replay (DotA Imba v6.63z10) is not on this
machine, so the sim cannot be run on the replay and **no absolute match or sim-vs-replay ΔY is available**.
What the replay's own order stream shows (`crates/engine/examples/ybirth.rs`; tag-like pairs with a feasible step up to 1500):
* windows in which no player did anything: median minimal step **3** (3,419 pairs <= 400, mean 48.7).
* windows with a movement order (move, smart, attack, attack-move, patrol): median **140**, mean 158.2 (387 pairs); other orders: median 212 (29 pairs).
* the cost is not proportional to the selection: one selected unit 151.5 (354 pairs), 4-6 predicted births 175.9, 7-12 165.1, i.e. not `3 n`.
* idle windows by the number of movement orders in the previous 20 records: 0 orders: median 3 (2,705 pairs); 1 order: 256; 3: 257; 6: 497; 7: 514; 8 or more:
  411 (mean 408 to 597 against 143.6). **Births follow the units that are walking, after the order**, which is what the per-leg mover start predicts qualitatively.
* the model predicts `3 n` births for the order alone; the feasible step set of the pair contains exactly that number in 12 of 4,044 pairs, which is chance level
  (the hit rate of a fixed step in these sets is about 1 to 2 percent): **no pair is explained exactly**, and the exact-pair count stays at 516.
The window attribution is coarse (a pair spans one computed record, the births of an order spread over the following seconds).

### 17.6 Open

1. How many legs a path has and when the original starts each leg (whether `CAbilityMove` restarts the mover per waypoint): the idle-after-order excess (median
   about 250 to 500 per 20 records) says the rate is high; a Unicorn run of the `CAbilityMove` handler `0x6F206EE0` with a built path is needed.
2. The member detach `0x6F4927F0` and the old cluster of a unit that starts again: whether the release is the next walk (modelled) or something else.
3. Group moves (`0x6F47B270` with up to 12 members, `0x6F203520`) and `CPrClusterGroup` creation (missile formations?).
4. Absolute Y after the map script (units, regions, relations at load): needs the map and the full creation order.
5. `SetUnitFacingTimed` and stop starts are not hooked.

## 18. Pass 15 (2026-10-17): the full-engine 0x22 oracle on a second real replay (`--sim-sync`), the pause rule, event classes

Labels as in section 13. Replay: `tools/replays/echoisles_ai_1.w3g` (not committed): 1.26a build 6059, `Maps\FrozenThrone\(2)EchoIsles.w3x`, seed
`0x030b2473`, flags `0x00800002` (speed bits 2 = fast), one human (the host) against one computer, both slots with the random race (`0x60`); the first
orders show the human is Human (`hpea` is trained) and the sim's melee start gives the AI Orc. 5,543 time slots, 5,543 records, 5,504 of them computed
(the zero-increment slots repeat the previous value), 524 s of game time, five pauses (`0x01` at slots 57, 262, 280, 430, 798; `0x02` at 97, 275, 361,
480, 932; the command slots carry increment 0).

### 18.1 The tool

`w3boot --replay FILE.w3g --sim-sync [--sync-trace] [--sync-slots N] [--sync-after] [--sync-defer-main] [--pr-off MASK] --dll Game.dll --mpq ... MAP.w3x`
boots the map with the replay's seed and slots (`Engine::boot_from_replay`), runs `main` (as the game does while loading; before this pass the first step
did), and then per time slot: decodes the pause/resume actions into `SyncState`, folds the slot into the net checksum, computes the record from the
simulated state (`Engine::sync_time_slot`: clock `T`, net checksum, X and Y from `Game::pr`, `R` and the rand sum from the host's generators), applies the
commands, runs the 25 ms units the sync tracker released (5 ticks each at fast speed) and compares the record with the file (`engine::sim_sync`).
The report gives, per run:

* **absolute** exact records and the first mismatching slot, with the difference `D = file ^ sim` explained by shape (`explain`);
* **calibrated**: the same with the modal difference taken as the static constant `K` (the registry slots 5 to 9 and 12 to 19 and the cheat word are
  not simulated; `K` absorbs them), with the exact prefix and the first mismatch;
* **pairs** (no constant needed): consecutive computed records whose differences `D` agree, i.e. the simulated hidden-word changes equal the recorded
  ones; the first mismatching pair; and the confusion matrix real quiet/active against sim quiet/active;
* **event timelines**: every recorded hidden-word change is classified by its rotation signature (`EventClass`) and compared with the simulated input
  that changed, matched within one slot. Signatures (CONFIRMED structure, thresholds empirical): a generator change is `rol3` of a hash difference
  below `2^26`, so its low three bits are zero and it is below `2^29` (691 events); a tag change is `rol6(dY) ^ rol9(dX)` with small differences,
  at most 22 bits (1,180); a rand-sum change has bits 20 to 23 clear and a full-width top byte (210); the rest is the game-hash slot, a participant that
  appears, or several inputs at once (646).

`--sync-trace` prints one line per computed slot (file, sim, `D`, the hidden words of both sides, the change classes, the simulated X, Y, R, rand and which
generators changed). `--pr-off MASK` switches the pathing-object hooks of the sim off one by one (`sim::unit_position::pr_off`).

### 18.2 What was fixed, with numbers (EchoIsles, 5,503 consecutive computed pairs)

| step | stream-only exact pairs | sim-vs-file exact pairs |
|---|---|---|
| start of this pass (`--check-sync`, clock `25*floor(E/25)`) | 127, longest run 6, first exact pair 3 | n/a |
| pause stops the clock (frozen `T`, increments of the paused slots not added) | 406 | n/a |
| + the first run of the engine (`--sim-sync`, `main` deferred to the first step) | | 1,199 |
| + pause with a buffer: increments pile up, whole units dropped at the resume, one extra unit at the resume | 2,766 | |
| + the extra unit belongs to the pause (taken by the first slot with time to spend after the command, then the clock freezes) | 2,776, longest run 134 | |
| + `main` runs before slot 0; creep camps no longer attack the neutral-passive units | | 1,302 |

* **Pause (CONFIRMED on five pauses).** Every pair of records inside a pause has a zero residual with `T` frozen. The records around the pauses fit this
  rule: the pause command arms a pause; the next slot with time to spend releases exactly one 25 ms unit (`T += 25`) and the clock freezes; slots keep
  arriving and their increments pile up in the buffer; the resume drops the whole 25 ms units and keeps the remainder (`buffer %= 25`). It was found by
  fitting the leftover `T` anomalies, which are `rol12(T ^ (T + 25))` at the first slot after each pause and after each resume (`0xf9 = 5775 ^ 5750` at
  both ends of the first pause). Alternatives fitted on the same file: the extra unit at the resume gives 2,766 exact pairs (10 fewer), an extra unit after zero to three further slots is worse than after one in each of the five pauses, and without the drop at the resume the count falls to 127 to 406.
  `sim::sync::SyncState::{pause, resume, clock, last_units}`; `sync_check` and `sim_sync` decode the actions.
* **Neutral players.** The sim treated players 12 to 15 as enemies of each other; the creep camps attacked the neutral-passive units from the first second
  and drew the reaction delay (stream 9) and the combat stream every slot. The recorded game has no stream activity there. `Game::is_enemy` now returns
  false between the neutral players (the real rule is probably an alliance default of the neutral players, UNSURE).

### 18.3 Where the first mismatch is, and why it cannot move yet

* Absolute: 0 of 5,504 records match; the first mismatch is slot 0. The first record needs the static registry slots (5 to 9, 12 to 19), the presence and
  birth tags after the map's load, the generator and rand sums after the game start (including the random-race draws of the two slots, not modelled) and
  the game-hash slot. None of them is known; `K` cannot be derived from the sim.
* Calibrated: the modal difference is carried by the 133 records of the long pause (slots 800 to 933), where both sides are frozen. Prefix 0.
* Pairs: the first mismatching pair is slot 1. The file has a tag-sized change at 0 to 1 (`0x000b7c00`) and a generator-sized change at 1 to 2
  (`0x07de1e40`); the sim draws the AI's first three `GetRandomInt` calls ((1,4), (1,3), (1,2)) during slot 0's ticks (so the change is visible at 1), the
  recorded game one slot later. The AI cadence (when the dispatcher first runs a thread) is the cause (11.5 is UNSURE).
* Timelines (final run, `real matched` against the number expected if the simulated events were spread at random): generator events real 691 / sim 257,
  matched 98 (chance 92); tag events real 1,180 slots / sim 3,094, matched 1,037 (chance 1,081); rand-sum events real 210 / sim 580, matched 81 (chance 60).
  **The simulated hidden-word events are uncorrelated with the recorded ones** (the sim registers about 2.6 times too often: 5,221 mover starts, 1,678 of
  them leg restarts, 132 unit creations; switching the hook classes off one by one moves the exact-pair count between 1,302 and 2,461 without ever
  predicting an event, so the pair count alone is not a score; the AI cadence knobs `--ai-delay/--ai-period` move the generator matches between 4 and 24
  of 103 in the first 150 s, against about 8 by chance: a 1.0 s period with phase 0 or 1 is the best of the ones tried, not proof).
* Facts from the file worth keeping: (a) ~600 of the recorded changes are pure generator draws by signature but none matches the generator sequence of the
  record seed within 40 draws at any start (searched n < 300,000 for every event; the false-positive rate is high because hash differences are not
  uniform, so this is "not confirmed", not "refuted"; the seed link and the start state stay unchecked); (b) a human train order shows up as a change of
  `0x20000000` and then `0x60000000` (a counter 0 to 1 to 2 in a high rotation) plus a tag change, i.e. the game-hash slot's player data; the player
  hash `0x6F40FEB0` folds two float fields (+0x29c, +0x2a4, both 0.0 at construction), a list hash of +0x2d0, the dead-hero list and a hash of the object at
  +0x34 (strings at 0x6F94D488: "Dead hero %d: age[%d] tags[%u %u]"); the game-hash slot is `acc = rol(acc ^ H(player), 3)` over the 12 players
  (0x6F3A1690), so human orders and AI production change it; (c) the first tag changes of the real game are in the thousands of bits (`dY` 1861 at slot 11),
  so the birth tag after the map load is in the thousands (trees and other map objects register), against 107 in the sim.

### 18.3b Replay object ids and the human's orders (new data, UNSURE meaning)

`cargo run -p engine --example objids -- FILE.w3g` prints the object pairs of the selection and order actions. On EchoIsles every unit pair has two equal
words: the hall of the human is `0x3a3c` (14908; the train orders at slots 137 and 140 follow its selection), the peasants are `0x3a71`, `0x3a9d`,
`0x3ab3` (14961, 15005, 15027; selected for the move orders), a harvest target (`0xD0032` at slot 243) is `0x1929` (6441). The peasants are 22 apart
(`0x3a71 + 22k`, k = 0..4: 22 agents per peasant) and the hall is 53 below the first peasant. So the ids are **agent ids from one counter that every agent
draws from** (trees and doodads first, about 15,000 of them, then the start units with their abilities), not the handle ids (`0x100000` up) and not the
sim's `(handle, 0)` model: without help **no order of the replay reaches a unit in the simulation**. `sim_sync::register_start_units` (`--sync-register`)
registers the human's pairs by a heuristic (ids in ascending order are the player's start units in creation order, spaced by the smallest gap); it is
off by default because it makes the comparison worse (exact pairs 1,302 to 373): with the human's peasants moving, the sim registers pathing objects in
nearly every slot (8,608 mover starts, `Game::chase_route` replans a point order every batch), while the recorded game shows a tag change only in the
one or two pairs after each human order (slots 122, 166, 174, 189, 212, 230, 243, 254: tag-only changes in two consecutive pairs, then quiet) and none
while the peasant walks. That is direct evidence for the registration model: **a mover start and a stop register; walking does not**, and a point order
must not replan every batch. The per-order Y step looks small (the first change after the order at slot 122, `0x2040`, can be read as `rol6(1) ^ rol9(16)`, a single birth plus a
free-list head change, not three; the other reading is a 12-bit Y step); the agent-id counter and the first-order step are the two numbers to decode next.

### 18.4 Open

1. The static constant `K`: hashes of the registry slots 5, 7, 8, 9 and 12 to 19 from the game data (ability database `0x6F005D60`, custom data kinds).
2. Birth and presence tags after the map load (destructables, regions, relations): needs the creation/registration counts of a whole map load.
3. The AI cadence in the first slots (first draw one slot later than the sim), and the production events (the game-hash slot's player data).
4. The generator link: verify the seed with an isolated draw (a quiet pair whose only change is a draw count), or decode the random-race draw at the game start.
5. The agent-id counter (14,908 for the human's hall): per-type agent counts of a unit (22 for a peasant) and the order of map-load creation, to resolve the replay's object pairs and let the human's orders act.
6. Why the pause takes exactly one extra unit (probably the frame driver finishes the unit in progress); the rule is fitted on one replay.
