# Game loop, JASS wait/timer path, and agile time (Game.dll 1.26a), second pass

Scope: bottom-up from the JASS interpreter 0x6F45E9D0. Sources: decompile chunks 014, 016, 017, 019, 026, natives.c, raw bytes via capstone/pefile, scratchpad vfunc_names.tsv and vtables.tsv.
Labels: CONFIRMED = read directly from decompile or disassembly bytes. LIKELY = strong inference from names and structure. UNSURE = not resolved.
No decompiled code is pasted; addresses point to the functions.

Corrections to the first pass (game-loop.md):
- The interpreter is NOT driven by a fixed per-step loop. Resumption of sleeping JASS threads is event-driven through a timer priority queue (see sections 3 and 4).
- TriggerSleepAction does not store the wait as an integer in +0x38. It stores the raw real bits in +0x40 and zeroes +0x38 (CONFIRMED, see section 2).
- The vtable entry called "CAgentTimer" at 0x6F2FBE80 is a vector of 20-byte records (first pass). The timer class itself is CAgentTimer (RTTI), and its start/stop methods are at 0x6F477FF0 and 0x6F477D70 (see section 3).
- 0x6F44B400 is still not a scheduler. It is a state serializer (no change from first pass).

---

## 1. Callers of the JASS interpreter 0x6F45E9D0 (CONFIRMED, full binary scan)

Direct call sites found by scanning every executable section for call and jump rel32 to 0x6F45E9D0. There are exactly four. No data-section pointer (vtable or function table) holds 0x6F45E9D0.

| Call site | Containing function | Budget argument (3rd arg) |
|---|---|---|
| 0x6F45F985 | FUN_6F45F8B0 (inside a loop over list at this+0x2880) | 300000 (constant) |
| 0x6F45FA20 | FUN_6F45F8B0 (single run of handle at this+0x18) | caller supplied |
| 0x6F45FBFE | FUN_6F45FA30 (thread start/resume with node cleanup) | caller supplied |
| 0x6F45FD0D | FUN_6F45FD00 (run current thread at this+0x20) | 300000 (constant) |

The interpreter returns a status: 1 done, 2 budget exhausted, 3 sleep (suspended), 4 yield, 5 unknown function, 6 abort path, 7 divide by zero (first-pass table, consistent with the code paths read here).

Who calls the four wrappers (CONFIRMED by scan, containing functions named where indexed):
- FUN_6F45FD00 is called only from FUN_6F44B640 (site 0x6F44B657).
- FUN_6F45F8B0 is called only from FUN_6F44B640 (site 0x6F44B672).
- FUN_6F45FA30 is called only from FUN_6F44B6A0 (site 0x6F44B6CF).
- FUN_6F44B640 is called from FUN_6F2E0B70 (frame resume), FUN_6F3B5470 (TriggerExecute-style, passes 1,1,300000), FUN_6F3B54B0 (TriggerExecuteWait-style, passes 0,0,300000), and an unindexed site at 0x6F3B53D0 (inside a function not present in the index).
- FUN_6F44B6A0 is called from FUN_6F2DE1D0 (returns a result from a 300000 run), FUN_6F2E0B70 (the non-current path), and FUN_6F3A8340 (script call with a "Script function timeout!" string, budget 300000).
- FUN_6F2E0B70 (frame resume) is called from FUN_6F2E3320 (message handler, see section 4) and FUN_6F2E3840 (event start, which is reached from FUN_6F2DA590 and others).

Conclusion (LIKELY): the interpreter is never run by a per-step loop of its own. It runs only when something starts, resumes, or calls a script. The periodic driver is the agile timer pump (section 4).

Status handling in FUN_6F2E0B70 (CONFIRMED): status 2 posts a constant message (budget, to be resumed later); status 3 posts a message carrying the frame's +0x28 value (sleep, see section 3).

---

## 2. TriggerSleepAction, Sleep, TriggerWaitForSound (CONFIRMED bytes)

- The native TriggerSleepAction at 0x6F3B2DB0 is three instructions: load the first stack argument (a pointer to the real argument) into ECX, zero EDX, and jump to 0x6F44B2F0.
- 0x6F44B2F0 (fastcall, ECX = pointer to the wait argument, EDX = wake value): gets the thread manager from 0x6F4C34D0, takes the current thread as the top entry of the manager's stack array (index count - 1, array at manager+0xC, count at manager+0x14), then:
  - sets thread +0x34 = 1 (sleep flag),
  - sets thread +0x38 = EDX (0 for TriggerSleepAction),
  - sets thread +0x40 = the 32-bit value at [ECX] (the JASS real, stored as its raw IEEE binary32 bit pattern; see jass-vm.md section 2.5).
  The wait value is therefore stored as seconds in binary32, with no conversion at this point. CONFIRMED.
- TriggerWaitForSound (0x6F3C3F20) jumps to the same 0x6F44B2F0 with the same convention. CONFIRMED.
- A second accessor 0x6F44B340 does the same as 0x6F44B2F0 but sets +0x38 = 0 and copies the value from its pointer argument. Its callers are at 0x6F446C4E (LIKELY a wait-type native).
- The native Sleep (R)V shares its code with the function at 0x6F71D9D0, which just returns 0. Sleep therefore does nothing in this build (CONFIRMED folding, LIKELY no-op).
- The executor (0x6F45E9D0) checks these flags after every native call (case 0x15): if +0x34 is nonzero it returns status 3; if +0x3C (yield) is nonzero it returns status 4. CONFIRMED.

Minimum wait: no minimum is applied on this path. The timer path (section 3) clamps negative times to 0.0 (LIKELY, see below).

---

## 3. Timers and sleep wake-up (CONFIRMED structure, LIKELY meaning)

### 3.1 TimerStart native
- 0x6F3C1370 (TimerStart): resolves the handle with 0x6F3BD710; if found, calls 0x6F3F6890 with the timeout (real, bit pattern), the periodic flag (bool), and the code handle.
- 0x6F3F6890 (CONFIRMED): if the timer's +0x40 is zero it creates the timer's agent through 0x6F001EC0. It then runs the callback registration 0x6F3B1480 / 0x6F3B14D0, makes a virtual call on the timer's object at +0x44 (vtable slot 1, timeout, 1), and dispatches message 0x80204 through 0x6F477A80.
- 0x6F477A80 (CONFIRMED): a generic message dispatcher. It calls vtable slot +8 (that is, slot 2) of the target object with the message and payload. For a CAgentTimer, slot 2 is 0x6F477FF0 (Start). So 0x80204 means "start timer".

### 3.2 CAgentTimer virtual table (RTTI, CONFIRMED)
Slots as listed in vtables.tsv for class CAgentTimer (0x6F8784AC): slot 0 0x6F00C060 (base), slot 1 0x6F041940, slot 2 0x6F477FF0 (Start), slot 3 0x6F477D70 (Stop/cancel), slot 4 0x6F4776B0 and slot 5 0x6F4776E0 (get timeout value), slot 6 0x6F4779F0 (get something from the request), slot 7 0x6F4775D0 (get elapsed), slot 8 0x6F477A30 (save), slot 9 0x6F478380 (load / restore). CAgentTimerExtended has the same layout with different targets.

### 3.3 CAgentTimer::Start (0x6F477FF0), CONFIRMED
1. If the timer already has a request at +0xC, cancel it (0x6F477D30 marks the old request with flag 0x10000 and drops it).
2. Choose the dispatch queue: if the float argument is 0.0, use DAT_6FAB7368 +0x40 (queue A) and do not set timer flag 4; otherwise set timer flag bit 0x4 and use +0x44 (queue B, "(l)").
3. Read the timeout as a binary32 float from the pointer argument.
4. If the timeout is at or above the queue's +0x48 limit (float), replace it with 0.0 (DAT_6FAAE47C, which is 0.0 in the file image). The exact role of +0x48 is UNSURE (see section 4.3).
5. Call 0x6F477F80, which clamps the timeout to a minimum of DAT_6FAAE478 (0.0 in the file image, but **1e-4 = 0x38D1B717 at runtime**, written by the start-up stub 0x6F84DC90; correction 2026-10-11: any timeout strictly below 1e-4, including 0 and negatives, becomes 1e-4; NaN keeps the keep-branch), increments a sequence counter at the timer's +0x50, computes an absolute expiry, and builds a request node (0x6F477EF0). The node is pushed onto the queue with 0x6F46A030. Store the request at timer +0xC.
6. If the periodic flag is set, set timer flag bit 0x2 and request flag bit 0x1.

Time unit: the timeout is a JASS real in seconds, and the absolute expiry is in seconds (float). There is no millisecond conversion anywhere on this path. LIKELY (the unit follows from the JASS real and the float clock below).

### 3.4 Heap node and ordering (CONFIRMED from 0x6F46A030 and its duplicate 0x6F46AEE0)
- The queue is a binary min-heap stored in an array (root at index 1, parent = index / 2). It is the template instance CPriorityQ of CSiRequest of CAgentTimer with unsigned long.
- Sort key 1: the node's float expiry at node+4, ascending.
- Sort key 2 (tie break): the node's sequence number at node+0x14, ascending. Equal expiry times therefore fire in insertion order (the lower sequence number is nearer the root).
- Caveat (LIKELY): the sequence counter comes from the timer object's own +0x50 field (the object passed as "this" to Start and to the node builder). If that is per timer and not global, ties between different timers are broken by each timer's own start count, not by one global order. UNSURE until the owner of +0x50 is confirmed as a single global agent.

---

## 4. The agile timer pump (the real scheduler), CONFIRMED structure

### 4.1 Dispatch objects
- DAT_6FAB7368 is the agile manager (a global object). Its +0x40 holds queue A (dispatch object "Agile/Timer Dispatch/Queue"), and its +0x44 holds queue B ("Agile/Timer Dispatch/Queue (l)"). Both are created in 0x6F46DA10 (source file Agile.cpp, lines 0x182 and 0x188), together with the "Signals" names. CONFIRMED.
- The dispatch object is a CSiDispatch of CAgentTimer with unsigned long, constructed in 0x6F46C590 (capacity 0x80 in the call) and 0x6F46E3D0 (vtable CSiDispatch). Its fields used here: +0x40 is a float "now" (the dispatch time; initial 0.0), +0x48 is a float "horizon" limit read from a 0.0 file-image constant (value at runtime UNSURE), +0x4C is a flag word (bit 0 = inactive).
- +0x3C and +0x38 of the manager are flags checked at the top of the frame pump (0x6F46FFE0 checks +0x3C; 0x6F470C70 checks +0x38). Their meaning is UNSURE (likely "paused" or "replay/net active").

### 4.2 Pump (0x6F46E370), CONFIRMED
Input: the dispatch object's +0x40 holds the target time T on entry (the caller writes it first).
Loop: while the heap has more than one node and the top node's expiry is at or below T:
1. Pop the top node (0x6F46C9C0).
2. Set +0x40 := the popped node's expiry. During the callback, "now" is the event time, so timers started from a callback are relative to the event time.
3. Clear flag 0x20000 on the node (queued flag).
4. Fire the node's signal through 0x6F46D520 (the signal delivery; the message type is the request's stored code, which is 0xD01BF for sleep wake-ups, see 4.4).
After the loop: +0x40 := T (the target is restored).

### 4.3 Advance wrapper (0x6F46E520), CONFIRMED structure
Given a target float T and the dispatch object: if T is below the object's +0x48 limit, set +0x40 := T and run the pump. Otherwise run a "horizon" branch: set +0x40 to the limit, pump, call 0x6F469D40 (a rebase or wrap helper, UNSURE), set +0x40 to 0.0, and pump again. The exact effect of the horizon branch is UNSURE.

### 4.4 Per-frame driver (CONFIRMED call chain, LIKELY frequency)
- 0x6F3A2EC0 is a large per-frame update. It calls 0x6F470C70 and then 0x6F46FFE0 with a pointer to the global float at 0x6FAAE4A0 (passed in ECX). Its callers are 0x6F3A3AD0 and 0x6F3A49E0 (not traced further). The frequency of the loop is UNSURE; it is not a fixed step count here.
- 0x6F470C70 (if manager+0x38 is zero): pumps queue A (manager+0x40) to the target float at 0x6FAAE4A0, then calls 0x6F46F390, 0x6F46F3A0 and 0x6F470740 on three other sub-objects of the agile manager (LIKELY other agile queues).
- 0x6F46FFE0 (if manager+0x3C is zero): reads the target T from 0x6FAAE4A0. While the global float at 0x6FAB7380 is below T, it runs the same pump sequence against 0x6FAB7380 (which advances that global to each event time), then runs a final pass to T. So 0x6FAB7380 is a second agile time cursor. LIKELY (it is set to each popped expiry by the pump).
- The 0x6FAAE4A0 float is passed as a snapshot to many messages (for example 0xD01C1, 0xD01BA, 0xD01B4, 0xD01C4). Its writers were not identified. It is multiplied by a rate in one place (near 0x6F6EEE20 use in chunk 003), which suggests it is a scaled game-time value, but this is UNSURE.

### 4.5 Sleep wake-up path (LIKELY)
1. A JASS thread calls a sleep native; the executor sets +0x34 and returns status 3.
2. The frame resume code (0x6F2E0B70, status 3 branch) posts message 0xD01BF with a pointer to the frame's +0x28 value (the wait real) through 0x6F2DC6C0 and 0x6F477A80. Slot 2 of the target object is Start, so this creates a timer with the wait time as its timeout.
3. When the timer expires, the pump (4.2) fires the node. Message 0xD01BF is delivered to the handler at 0x6F2E3320 (the handler checks that the message type equals 0xD01BF and that the agent global at DAT_6FAB65F4 +0x3E0 is set). It has no direct callers, so it is reached through a registered function table (UNSURE which table).
4. The handler calls 0x6F2E0B70, which resumes the thread through 0x6F44B640 or 0x6F44B6A0 with budget 300000, and the interpreter continues from where it stopped.

So sleeping threads are woken by the expiry of a timer in the agile queue, not by a loop that compares a thread's wake time with a clock. The pump runs whenever the per-frame update runs.

Ordering among threads that wake on the same step: by expiry, then by the sequence number in section 3.4 (LIKELY). Threads with the same expiry resume in insertion order within one queue.

---

## 5. Clocks

- 0x6F6C4E80 (CONFIRMED): returns GetTickCount() converted to float seconds (multiplied by the double constant 0.001 at 0x6F932C60, with a correction of +2^32 when the 32-bit result is negative, the constant at 0x6F875AB8 is 4294967296.0). It is a wall clock with a 49.7-day wrap. It has about 15 callers, mostly UI and network code. It is NOT called from the agile pump (CONFIRMED by caller scan).
- 0x6F6C4E10 (CONFIRMED): a Storm-style millisecond clock. If a high-resolution flag at 0x6FAD1418 is set it returns QueryPerformanceCounter scaled by a float (0x6FAD140C); otherwise GetTickCount. It has one unindexed caller at 0x6F553906.
- 0x6F6C4CD0 (CONFIRMED): calibrates the QPC frequency once (sleeps about 500 ms and measures) into the globals at 0x6FAD1428.
- DAT_6FAB65F8 is set to GetTickCount() inside 0x6F3A2EC0 when its second argument is nonzero, and is later reduced by 62 (0x3E) (LIKELY a frame-time stamp for UI or replay latency; not game time).
- Game time for the JASS timers is the agile float (section 4), in seconds, and it does NOT use wall-clock time. This is LIKELY: the agile pump never calls the wall-clock functions, and the wall-clock callers are UI and network.

---

## 6. Global game-time variables (summary)

| Name | Type | Unit | Written by | Label |
|---|---|---|---|---|
| Queue A "now" | float at dispatch+0x40 | seconds | pump 0x6F46E370 and advance 0x6F46E520 | CONFIRMED (location), LIKELY (unit) |
| Queue horizon | float at dispatch+0x48 | seconds | runtime value UNSURE | UNSURE |
| Agile cursor | float global 0x6FAB7380 | seconds | pump via 0x6F46FFE0 loop | LIKELY |
| Frame target | float global 0x6FAAE4A0 | seconds (LIKELY) | writers not found; read by 0x6F470C70 and 0x6F46FFE0 | UNSURE for writers |
| Zero constant | float 0x6FAAE470 (also 0x6FAAE47C, 0x6FAAE594) | none | file image is 0.0 | CONFIRMED (file image); **0x6FAAE478 is 1e-4 at runtime** (the timer minimum) |
| Net step counter | int at CNetData+0x628 (from first pass) | steps (unit UNSURE) | net bracket 0x6F65C7D0 / 0x6F65C840 | CONFIRMED (from first pass) |
| Wall clock | GetTickCount (import slot 0x6F86D230) | milliseconds | system | CONFIRMED |

Relation between the net step counter and the agile seconds: NOT FOUND. No function was found that converts +0x628 to seconds, or that writes 0x6FAAE4A0 from the net turn. The per-turn mapping (how many agile seconds a lockstep turn represents) is therefore UNSURE. The first pass said "1 bracket = 1 unit" with the unit unknown; this pass does not change that.

---

## 7. Constants

| Name | Value | Where | Label |
|---|---|---|---|
| Interpreter budget used by callers | 300000 | 0x6F45F985, 0x6F45FD0D call sites and wrappers | CONFIRMED (value); meaning LIKELY: instruction count per run, not milliseconds |
| Message: start timer | 0x80204 | 0x6F3F6890 | CONFIRMED |
| Message: wake sleeping thread | 0xD01BF | 0x6F2DC6C0, 0x6F2E3320 | CONFIRMED |
| Other messages | 0xD01C1, 0xD01BA, 0xD01B4, 0xD01C4, 0xD01E6 | payload uses of 0x6FAAE4A0 | CONFIRMED (value) |
| Queue flag: queued | 0x20000 on node | 0x6F46E370, 0x6F477EF0 | CONFIRMED |
| Queue flag: cancelled | 0x10000 on request | 0x6F477D30 | CONFIRMED |
| Timer flag: periodic | 0x2 on timer | 0x6F477FF0 | CONFIRMED |
| Timer flag: queue B | 0x4 on timer | 0x6F477FF0 | CONFIRMED |
| Clamp minimum | 0.0 (float) | 0x6F477F80 | CONFIRMED (file image) |
| Clamp limit replacement | 0.0 (float) | 0x6F477FF0 | CONFIRMED (file image) |
| Milliseconds to seconds | 0.001 (double) | 0x6F6C4E80 | CONFIRMED |
| 2^32 | 4294967296.0 (float) | 0x6F6C4E80 | CONFIRMED |
| Thread sleep flag | 1 at +0x34 | 0x6F44B2F0 | CONFIRMED |
| Net states | 2 replaying, 3 playing | first pass | CONFIRMED (first pass) |

---

## 8. Open questions (ordered by value)
1. Find the writer of the frame target float at 0x6FAAE4A0. Most likely the net turn clock or a per-frame timer; check the function containing the multiply at chunk 003 (the one with the call to 0x6F6EEE20 on that address), and the unindexed caller of 0x6F6C4E10 at 0x6F553906.
2. Decode 0x6F469D40 and the horizon value at dispatch+0x48 (is it set in 0x6F46C590 or 0x6F46B120 at runtime?). This decides whether the rebase branch can fire in normal play.
3. Find the callers of 0x6F3A2EC0 (0x6F3A3AD0 and 0x6F3A49E0) and the main loop that calls them, to get the frame rate and the number of pump calls per turn.
4. Confirm whether the timer object's +0x50 sequence counter is per timer or global (the owner of the request pool at +0x38).
5. Identify the table that holds the handler for message 0xD01BF (the handler at 0x6F2E3320 has no direct callers). This confirms the wake path in 4.5.
6. Verify, with a replay that has a Wait, that a sleeping thread resumes at the expected net turn (a byte-level check of the net step counter against the agile seconds).

## 9. Method notes (for reproduction)
- Direct caller scan: all executable sections, capstone 32-bit, call and jump rel32 targets, with the containing function taken from the index or, when the index has no entry, from a padding search backwards.
- Data-pointer scan: raw 32-bit little-endian search through all sections for the interpreter address (no hits).
- Disassembly of sleep and timer natives: byte-level via capstone (see 0x6F3B2DB0, 0x6F44B2F0, 0x6F3C1370, 0x6F6C4E80, 0x6F470C70).
- Index coverage: the function index has about 70,500 entries but large gaps (about 65,000 gaps between consecutive starts); unindexed callers are marked as such above.
