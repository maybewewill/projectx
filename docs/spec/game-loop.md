# Game loop, replay turns, and sync checksum (Game.dll 1.26a)

Status: analysis from decompiled pseudo-C only (chunks 014, 016, 018, 019, 022, 025, natives.c). No raw-byte or emulator checks were done.
Labels: CONFIRMED = read directly in the decompile. LIKELY = strong inference. UNSURE = not resolved.

Summary of corrections to AGENTS.md section 4.3d: 0x6F44B400 is NOT established as the thread scheduler (see section 2). The per-turn simulation driver is NOT the function that calls it.

---

## 1. Lockstep turns and replay time slots

### 1.1 Replay block dispatcher (CONFIRMED)
- Function 0x6F54EB00 (chunk_022, ~lines 17440 to 17700). It is the CNetData replay-turn pump. It reads one block-id byte at a time from the replay ring buffer (readers 0x6F4C2C10 for 1 byte, 0x6F4C2C70 for 16-bit, 0x6F4C2FD0 for N bytes, 0x6F4C2B30 for the bounds check).
- It runs only when the net state at CNetData +0x614 equals 2 or 3 (playing or replaying).
- Loop control: it keeps reading blocks while the replay cursor is not ahead of the "turn due" time. It exits the loop after a 0x1F block is consumed (the local flag is set to 1 only when the block id is 0x1F). A 0x1E block does not end the loop.
- Block ids handled (names are unknown; mappings are LIKELY from the public W3G format): 0x10, 0x16, 0x17, 0x19, 0x1A, 0x1B, 0x1C, 0x1E/0x1F, 0x20, 0x21, 0x22, 0x23, 0x2F, 0x30, 0x31. Any other id hits the error "CNetData::EnqueueReplayTurn: Bad replay event %u" and then calls the disconnect path (0x6F54C7E0).
- Most non-time blocks follow the same pattern: call a parser, check that the turn-due time is not ahead of now, store the block's time into +0xAC8, then call an enqueue helper (0x6F54DC60, 0x6F54DE00, 0x6F54DB90, 0x6F54DD30, 0x6F54DED0, 0x6F54DFA0, 0x6F54E070, 0x6F54E3B0, 0x6F54E2E0, 0x6F54E140, 0x6F54E210, 0x6F54E480, 0x6F54E550, 0x6F54E620).
- Blocks 0x1E/0x1F: read one 16-bit word, copy that many bytes into a buffer (0x6F4C2FD0), store the time to +0xAC8, then enqueue through 0x6F54CAD0 with the block id (capped at 0xFF for ids over 0x3F). The 0x1F case also sets the loop-exit flag.
- Open inconsistency (UNSURE): the 16-bit word is used BOTH as a comparison value (the time check "current minus turn-due is less than word, then turn-due = current + 1") AND as the byte count of the payload copy. A time increment is not visibly read anywhere in this path. Either (a) the word is the payload size and the time increment is read in a helper such as 0x6F650D40 or 0x6F6516C0 that the decompile did not resolve, or (b) the decompile mis-typed a field. Resolve by reading 0x6F650D40, 0x6F6516C0, 0x6F5491B0 and by checking one real replay byte-by-byte under a debugger or the emulator.

### 1.2 Time fields on CNetData (CONFIRMED by access pattern, meanings partly LIKELY)
- +0x628: current game time (int). Read and written by the replay pump and by the step bracket (see 1.4).
- +0x62C: turn-due time for the next replay block. Set to 0xFFFFFFFF when no turn is pending (CONFIRMED in several places).
- +0xAC8: time of the last block that was enqueued (LIKELY "last applied turn time").
- +0xACC: running byte count or offset of the replay stream (LIKELY). When the replay-state flag at +0x58C is nonzero, +0x628 is set from +0xACC at the top of the pump (CONFIRMED).
- +0x614: net state (2 = replaying/loaded, 3 = playing, others not). Decremented on certain events (CONFIRMED).
- +0xAD0: set to 1 by the pump when it starts a pass (CONFIRMED). Reset elsewhere.
- +0xAD4: decremented on each OnReplayData call when state is 2 (CONFIRMED).
- +0x610 and +0x60C: head and tail of the queued-event linked list (CONFIRMED by the traversal code).

### 1.3 Net-event insertion and turn application (CONFIRMED code, LIKELY semantics)
- The NetProvider (class Net::NetProvider, `FUN_6F65D9E0` is its destructor; source file NetProvider.cpp) keeps a circular list of queued net events. Each record is about 0x104 bytes (allocation 0x108 with a 0x2E-dword payload copied from the input).
- New records get a GetTickCount() timestamp at +0x0C (CONFIRMED). This is wall-clock time for latency, NOT simulation time.
- Player index is checked against a cap of 16: an index above 16 rejects the event (CONFIRMED).
- Records are appended at the tail (insertion order). Application walks head to tail (LIKELY first-in first-out within a turn).
- Records carry flags in the high bits of +0x10: bit 0x20000000 = "queued for application" (routed through 0x6F65B750), bit 0x40000000 = routed through 0x6F65AB90, sign bit = routed through 0x6F65ABC0 and then removed from the list (CONFIRMED).
- An "apply pending" test is done only when the event hasn't been deferred (+0x6D0 style flag at NetProvider +0x18A). If that flag is set the record is marked 0x20000000 and deferred (CONFIRMED).

### 1.4 Per-step bracket (CONFIRMED code)
- Two helpers form a bracket around each simulation step:
  - 0x6F65C7D0 (pre-step): increments the time counter at +0x628 by one. If the old value was 0 (outermost entry), it walks the event list and sets the sign bit on records and decrements the pending count at +0x614. (The sign bit is the "not yet executed" marker.)
  - 0x6F65C840 (post-step): decrements +0x628. When it reaches 0 (outermost exit), it walks the event list and applies the records flagged as above, in list order. Records flagged for removal are unlinked (0x6F65C570).
  - 0x6F65CD70 = pre then post (a full step).
  - 0x6F65CE60 = "advance and dispatch": pre-step, then walks the list calling 0x6F65C900 for each record, then post-step.
- Callers of the full bracket (CONFIRMED): 0x6F65D9E0 (NetProvider teardown, chunk_025 ~line 32387), 0x6F65DDD0 (~32576), 0x6F661A20 (~35471), 0x6F6637C0 (~36759). The last one is the replay/stream feeder: it reads a 1-byte and a 2-byte block header (0x6F4C2C10, 0x6F4C2C70) and then calls 0x6F663700 with the payload. So step execution is driven by net and replay data ARRIVAL, not by a fixed local frame timer. This matches a lockstep design (LIKELY).
- Step size: each bracket advances the counter by exactly 1 unit (CONFIRMED). Whether 1 unit = 1 ms, or one turn, is UNSURE. Nothing in these functions converts to milliseconds.

### 1.5 Players and command order within a turn
- Player commands are applied in the order they were queued (LIKELY). Turn application is not sorted by player id in this path. Player ids are bounded at 16 (CONFIRMED).
- A "lockstep mode" message exists in the router (0x6F5C25C0, string "Router is running in lockstep mode"). That function is large (634 bytes) and was not fully read. It is the place to look for the number of simulation steps per turn.
- Steps per turn: UNSURE. The bracket is entered once per arriving turn packet in the feeder (0x6F6637C0). The count of steps per turn is probably the value carried in the time-slot word (see 1.1), but this is unverified.

---

## 2. Per-step update order

### 2.1 The function at 0x6F44B400 (CONFIRMED structure, meaning UNSURE, AGENTS claim NOT SUPPORTED)
- Size 529 bytes, chunk_018. It takes one argument (a serializer or the thread manager object) and returns 1.
- It gets the global thread-manager state with 0x6F4C34D0, which returns an array slot from 0x6F6D98E0 (CONFIRMED).
- Its primitives are 0x6F4C2360 (append a 32-bit value to a growable byte buffer), 0x6F4C22C0 (same, for pointer-sized values), 0x6F4C1FB0 (write a 32-bit value at an earlier offset, back-patching a length prefix), and 0x6F4C2760/0x6F4C2BA0 (append blocks). Together this is a length-prefixed serializer pattern (CONFIRMED primitives, LIKELY meaning).
- It loops over list A (object +0x44 count, array +0x48), list B (+0x8C count, array +0x90), and list C (+0x14 count, array +0xC of records whose field at +0x2850 is written as a 32-bit value).
- The records it writes include thread objects (0x6F44B220 writes a per-thread count and records, and 0x6F45CF00 writes per-entry data).
- Conclusion: LIKELY a state serializer for sync or save (for example, a snapshot of thread and timer state), not the per-tick scheduler. Its only static call site is in 0x6F3AF8D0 (chunk_016, ~line 51395), which is the game-start sequence (it contains no loop), so it does not run per step.
- Action: rename in the spec to "thread-state snapshot (LIKELY)". Verify by checking whether its output reaches the CRC routine in section 3.

### 2.2 Thread sleep (CONFIRMED partially)
- The TriggerSleepAction native (0x6F3B2DB0) calls 0x6F44B2F0. That function finds the current thread (through the thread-manager array at +0xC indexed by +0x14), sets its state flag at +0x34 to 1, stores the wake value at +0x38 and a saved value at +0x40 (the first argument's pointer contents).
- The native's wait argument is lost in this decompile (the native calls the helper with no arguments). So the wait value's type and units are UNSURE. The JASS side is real (type R, seconds), so a conversion to game steps must exist somewhere. Not found yet.
- TimerStart (0x6F3C1370) calls 0x6F3F6890, which takes the timer object, the timeout, and a periodic flag. It builds a request record with flag 0x80204 (CONFIRMED) and dispatches through a templated request class (CSiRequest of CAgentTimer). The fields are not decoded.
- CAgentTimer (0x6F2FBE80, 0x6F2F7830): this symbol is actually a resizable vector of 20-byte records (vtable + refcount + handle + fields). It is NOT the timer tick routine (CONFIRMED). The AGENTS note about "agent timer granularity" cannot be taken from this symbol.

### 2.3 Order of the other systems (UNSURE)
- Threads, timers, unit updates, missiles (for example CMissileStampede::Tick at 0x6F1DF5C0), pathing, fog (checksum string at 0x6F406DF0), and triggers are all present as functions, but no single driver function that calls them in order was found.
- Ordering among objects is by list (vector index or linked list). LIKELY insertion-order lists, with the thread manager using index arrays. Priority queues keyed by (time, id) were NOT found. Treat that claim as UNSURE until the driver is found.

---

## 3. Sync checksum

### 3.1 CRC-32 (CONFIRMED)
- A table of 256 entries is built at object offset +0x8E0, with a ready flag at +0xCE0 (builder 0x6F649360). The table is the reflected CRC-32 with polynomial 0xEDB88320 (CONFIRMED, from the constant).
- Update routine 0x6F649410: for each byte, crc = (crc >> 8) XOR table[(byte XOR crc) and 0xFF]. No pre-inversion or post-inversion inside it (CONFIRMED).
- Finalize routine 0x6F649460: returns the bitwise complement of the update result (CONFIRMED). Initial value is not visible here; standard CRC-32 uses 0xFFFFFFFF (LIKELY).
- Helper 0x6F649480 reads 4 bytes and packs them big-endian (CONFIRMED).

### 3.2 Where the CRC is checked (CONFIRMED)
- Function 0x6F63E160 (chunk_025, ~lines 12809 to 12960) reads a block with a trailing 4-byte CRC. It computes the finalized CRC over the block, reads the stored value with the big-endian helper, and returns error 0x403 on mismatch. This is a stream-integrity check, not clearly the per-turn sim checksum.
- Strings that show sync checksums: "[CSyncData] (%s) csum[%08X], count[%u]" (0x6F536C30), "Net:\nsyncNetEventChecksum = %08x" (0x6F537E30), "[PrPa] - csum[0x%08X]" (0x6F49CB70 area), "Last fog checksum was %d" (0x6F406DF0), "net sync events" (0x6F550460 area, which is in the CNetData constructor).
- CSyncData objects expose their checksum through a virtual method at vtable +4 (CONFIRMED from 0x6F536C30). CNetData's sync-event checksum object is at +0x2278 (CONFIRMED).

### 3.3 What is hashed and when (UNSURE)
- The inputs to the per-turn checksum (which fields, which order) were NOT identified. The CRC table is used in only one place in this decompile (the footer reader in 3.2), so the sync checksum either uses a separate copy of the table (not found by the offset search), or uses the Storm CRC through an import (not checked).
- Not verified: whether the checksum is computed per turn, per step, or per replay block. Candidate: each CSyncData object's checksum is computed over its own state and combined in CNetData +0x2278.
- Next steps: find the vtable entries for CSyncData subclasses (the vtables near 0x6F006F10 and the CNetData vtable), and the caller of the object at +0x2278.

---

## 4. Game time representation

- Game time in the net layer is a 32-bit signed integer (CONFIRMED: +0x628, +0x62C, +0xAC8 are int or uint32; 0xFFFFFFFF is the "none" value). Units not confirmed.
- Wall-clock uses GetTickCount (32-bit ms, CONFIRMED at 0x6F65C900). Do not confuse it with sim time.
- JASS wait values: TriggerSleepAction and Sleep take a real (seconds). Conversion to integer game time is UNSURE. The JASS reals are software float (AGENTS 4.3g). A conversion, if any, must be done through the software float helpers, but the code was not found.
- The Sleep native (0x6F71D9D0) shares code with other natives. Not decoded.

---

## 5. Constants

| Name | Value | Source | Label |
|---|---|---|---|
| CRC-32 polynomial | 0xEDB88320 | 0x6F649360 | CONFIRMED |
| CRC table offset in object | +0x8E0 | 0x6F649360, 0x6F649410 | CONFIRMED |
| CRC table ready flag | +0xCE0 | 0x6F649360 | CONFIRMED |
| Max player index | 16 | 0x6F65C900 | CONFIRMED |
| Net state: replaying | 2 | 0x6F54EB00 | CONFIRMED |
| Net state: playing | 3 | 0x6F54EB00 | CONFIRMED |
| "No turn pending" | 0xFFFFFFFF | 0x6F54EB00 | CONFIRMED |
| Replay time-slot block id | 0x1F (0x1E shares handler) | 0x6F54EB00 | CONFIRMED |
| Block id cap for enqueue | 0x3F (larger ids become 0xFF) | 0x6F54CAD0 | CONFIRMED |
| Thread sleep state | 1 at +0x34 | 0x6F44B2F0 | CONFIRMED |
| Timer request flag | 0x80204 | 0x6F3F6890 | CONFIRMED |
| Script-op budget | 300000 | 0x6F45E9D0 callers | CONFIRMED (from AGENTS) |
| Event flag: queued | 0x20000000 | 0x6F65C840 | CONFIRMED |
| Event flag: other route | 0x40000000 | 0x6F65C840 | CONFIRMED |
| Steps per turn | unknown | - | UNSURE |
| Sim time unit | unknown (1 per bracket call) | 0x6F65C7D0 | UNSURE |
| Sleep wait conversion | unknown | 0x6F44B2F0 | UNSURE |

---

## 6. Open questions (ordered by value)
1. Find the true per-frame driver: the caller of the bracket at 0x6F6637C0 (receive path), and the caller of 0x6F5C25C0 (lockstep router). Then read the path that calls per-step updates.
2. Resolve the 0x1E/0x1F word inconsistency (section 1.1) by reading 0x6F650D40, 0x6F6516C0 and 0x6F5491B0, and by checking one real replay.
3. Decode the sleep native's argument (the JASS stack) and find the conversion to game steps.
4. Identify what the per-turn checksum hashes: the CSyncData subclass vtables and the caller of +0x2278.
5. Confirm the CRC init value (0xFFFFFFFF expected) and the byte order of the stored footer with a known-good replay file.
