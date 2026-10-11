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
