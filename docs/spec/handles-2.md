# JASS handle ids in Warcraft III 1.26a (Game.dll): assignment, reuse and release

Scope: how a JASS handle value is produced, how its id is chosen, and when it is released.
Read-only analysis of the decompile and disassembly. No code is reproduced here.
Confidence labels: CONFIRMED (seen in the disassembly or confirmed by the Unicorn run), LIKELY
(strong structural evidence, one link not fully traced), UNSURE (not settled by this pass).
Addresses are for the 1.26a binary supplied in the session.

## 0. Summary

- The JASS handle value is the id itself. GetHandleId is an identity function (CONFIRMED, body at 0x6f3b2d00 returns its argument).
- Ids are integers starting at 0x100000 (CONFIRMED). Values below 0x100000 are never valid object handles (CONFIRMED).
- A fresh id is 0x100000 plus the current size of the entry table, so ids are issued in order (CONFIRMED).
- Released ids go on a LIFO stack and are reused most-recently-freed first (CONFIRMED). Not lowest-free, not FIFO.
- An id is held by a reference count stored in its table entry. It is freed (pushed to the stack) only when the count reaches zero (CONFIRMED).
- The count starts at 1 when the object is created. It is released when the owning agent dies (LIKELY). Extra holds come from hashtable saves and other containers (CONFIRMED for hashtable saves).
- The first id a map sees is most likely 0x100001, because a table reset at game-state construction leaves a dummy entry in slot 0 (LIKELY; the emulated reset gives 0x100001, CONFIRMED).
- Emulation (Unicorn, real allocator code, stubbed externals): 21 of 21 checks passed (section 9).

## 1. Entry points

1.1 GetHandleId (CONFIRMED)
- Native registered as taking a handle and returning an integer. Body at 0x6f3b2d00 returns its first argument unchanged.
- So the number a script sees is the same value the engine uses internally. There is no mapping step in GetHandleId.

1.2 Creating natives (CONFIRMED)
- CreateTimer (body at 0x6f3d0500) creates a timer agent, then calls the id allocator at 0x6f430c80 with the agent and flag 1, and returns the id it gets back.
- The same allocator is called from many other object creators (the decompile shows about 15 call sites in the first pass alone). The allocator is shared, not per type.

1.3 Destroying natives (LIKELY for the chain, UNSURE per native)
- DestroyTimer (0x6f3c1350) looks the handle up (0x6f3bd710, which uses the id-to-agent lookup at 0x6f428050), then calls virtual slot 0x5c on the agent.
- For the CAgent base class, slot 0x5c is 0x6f472990. It calls the agent death routine 0x6f4a6970 under a type condition (UNSURE in detail).
- The death routine sets a "dead" flag at agent+0x20 and dispatches an event (0x6f4a66e0, then 0x6f4a6650). Subscribers include the handle manager (see 5.1). The final dispatch to the manager was not traced line by line (LIKELY).
- DestroyGroup, DestroyTrigger, RemoveLocation: not traced individually. They are assumed to follow the same agent-death route (UNSURE).

## 2. The handle manager object

2.1 Singleton (CONFIRMED)
- The manager is the object returned by 0x6f3a8060. It is created lazily on first use: if the pointer at the global's +0x1c is empty, it creates the object and stores it there.
- LIKELY: the manager is the game-state object. The table reset (section 8) runs from the game-state constructor at 0x6f430050 and writes the same field offsets.

2.2 Fields used by the allocator (CONFIRMED offsets, manager = ECX of 0x6f430c80)
- +0x184 to +0x190: free-id stack vector. Layout: +0x184 capacity, +0x188 size, +0x18c data pointer, +0x190 granularity.
- +0x194 to +0x1a0: entry table vector. Layout: +0x194 capacity, +0x198 size, +0x19c data pointer, +0x1a0 granularity. Reset sets granularity to 1000.
- +0x1a4: map from agent key to node (a hash map object; its first dword is a vtable pointer).
- Node layout (CONFIRMED): +0x00 agent key, +0x18 id, +0x1c agent key again.

2.3 Vector growth (CONFIRMED)
- Both vectors grow in whole multiples of their granularity. Growth reallocates and copies; old buffers are freed. Sizes never shrink when ids are released.

## 3. Id allocation (0x6f430c80, CONFIRMED)

Inputs: the manager (ECX), an agent pointer, and a flag. Returns the id, or 0 on failure (agent null, or agent type check failed).

3.1 Existing id first (CONFIRMED)
- The manager looks the agent up in the map at +0x1a4 by agent key (agent+0x10).
- If a node exists and the entry's agent pointer is still this agent, the existing id is returned. No count is changed and no new slot is taken.

3.2 Otherwise pick an id (CONFIRMED)
- If the free stack is non-empty: pop its top. The id is that popped value. The entry's count is set to 0.
- If the free stack is empty: the id is 0x100000 plus the current entry-table size (the size before the push). A new entry is pushed (count 0).
- The table index of id N is N minus 0x100000. Entry address = table data pointer + index * 12.

3.3 Entry layout (CONFIRMED)
- 12 bytes per entry.
- +0: reference count (not an id key, which corrects the earlier handles.md).
- +4: agent pointer, held through a smart-pointer assign that also adds one reference to the agent (the function at 0x6f022aa0).
- +8: flag passed by the caller (1 from creators).

3.4 After picking (CONFIRMED)
- A node is inserted into the map under the agent key, with the id stored in it.
- The agent is subscribed to event 0x80267 (via the agent's virtual subscribe slot, event 0x40190064), with the manager as the sink.
- The entry's count goes from 0 to 1. The function returns the id.

3.5 The allocation rule in one line (CONFIRMED)
- id = pop(free stack) if the stack is non-empty; otherwise id = 0x100000 + table size, and the table grows by one.

## 4. Free-id stack (CONFIRMED)

4.1 Order
- Push at the end (0x6f3a3d20: push returns a slot at the current size, and the id is written there).
- Pop from the end (inside 0x6f430c80: reads the top element, then sets size to size minus one with 0x6f3a3d70).
- So reuse is LIFO: the most recently released id is the next one handed out.

4.2 Pushed only at zero count (CONFIRMED)
- Push happens in the free routine 0x6f42d3f0, which is reached only from the release routine 0x6f42dd40 when the count drops to 0.

## 5. Reference counting and release

5.1 Release routine (CONFIRMED)
- 0x6f42dd40 (ECX = manager, one argument = id): if id is at least 0x100000, decrement the entry count. If it reaches 0, call the free routine 0x6f42d3f0.
- Ids below 0x100000 are ignored (CONFIRMED).

5.2 Free routine (0x6f42d3f0, CONFIRMED except the notify step)
- Calls the agent's slot 0x5c notify only when the entry flag is non-zero and the agent passes the type check 0x6f0418a0 (LIKELY that this is the destroy notify; UNSURE in detail).
- Releases the entry's agent reference. If that reaches zero, calls the agent's virtual delete. The entry's agent pointer is cleared.
- Removes the map node for the agent (lookup, then remove, then a virtual call on the map).
- Pushes the id onto the free stack.

5.3 AddRef (0x6f428080, CONFIRMED)
- Increments the count of an id at least 0x100000. Ids below are ignored.

5.4 Holders that keep an id alive (CONFIRMED for the code paths, LIKELY for the meaning)
- Creation gives count 1. That count is the "object is alive" reference and is released by the agent death path (5.5).
- Hashtable saves (SaveLocationHandle, and the other Save*Handle natives): they pass flag 1 down to a keyed slot store at 0x6f402d80. When the flag is 1 the store adds a reference to the new value and releases the old owned value. So a saved handle holds an extra count, and its id stays reserved after DestroyX.
- Some internal holders also add counts directly (for example at 0x6f416d50, 0x6f445050, 0x6f3efc30). They were not traced fully.
- JASS local and global variables: no code path was found in which the script VM adds or releases a count when a handle is assigned to a variable (UNSURE). The working assumption is that variables hold raw integers and rely on the creation count.

5.5 When an id is released (LIKELY)
- The agent death event 0x80267 is handled by the manager's dispatcher (case 0x80267 calls 0x6f42dd80). That routine releases the id once (the creation count) and clears the entry's agent pointer.
- So after DestroyX, the id is freed at once if no other count exists. If a hashtable still holds the id, the id stays reserved but its agent pointer is cleared (dangling). CONFIRMED for the code path, LIKELY for the timing.
- The exact gate in the destroy routine (0x6f472990) and the trace from the event dispatch to the manager were not completed (UNSURE).

## 6. Id validity

- 0x6f428050 (id to agent): returns 0 for ids below 0x100000 (CONFIRMED). So 0, and any small integer, is a null handle.
- 0x6f428080 (AddRef) and the release path ignore ids below 0x100000 (CONFIRMED).

## 7. Same object, same id (CONFIRMED)

- While an agent holds a live id, asking for an id for that agent again returns the same id and changes no count.
- Once the id is freed, a new id is issued next time (the stale map node is removed in the free routine).

## 8. Pre-assignment at map start

8.1 Table reset (CONFIRMED structure, LIKELY timing)
- The reset routine at 0x6f42d2e0 clears the agent-side entries, sets the entry table size to 0, then calls the table set-size routine with 1. This pre-creates entry 0, which corresponds to id 0x100000, with count 0 and no agent. Granularity is set to 1000.
- The reset is called from the game-state constructor 0x6f430050 (and from 0x6f42db40, role not traced).
- Consequence: the first id a map issues is 0x100001, and 0x100000 is a permanent dummy slot (LIKELY). This matches the commonly observed first handle value of 1048577 in Warcraft III.

8.2 Other pre-allocation (UNSURE)
- A second table-size sequence (set to 0, then to a count from a game-state getter) appears in the function at 0x6f4313e0. The count's source was not traced, so it is unknown whether slots are pre-created for players or other objects at map load.
- The manager's lazy creation (0x6f3a8060) also creates an agent and calls a day-hours initialiser. Whether any of this issues handle ids was not determined.

8.3 Per game (LIKELY)
- Because the reset runs in the game-state constructor, the counter restarts for each game, not for each map load. Not checked against a second game in the same session.

## 9. Unicorn check of the allocator

Harness: scratchpad/specs/handle_alloc_emu.py. It loads Game.dll with the same approach as the existing harness and runs the real code of the create, release, add-ref, free, push, pop, table-push and set-size routines. External callees are replaced by stubs:

- 0x6f03fa30: returns a fake agent-class object (tag 0x2b61676c, flag 0 at +0x20).
- 0x6f0418a0: returns 0 (skips the notify).
- 0x6f42ccf0 and 0x6f430580: a Python dictionary for the map (lookup and insert by agent key).
- 0x6f42b060 and 0x6f3a1aa0: vector resize (element size 12 for the table, 4 for the stack).
- 0x6f022aa0: smart-pointer assign plus AddRef on the agent.
- 0x6f471a70, 0x6f4287c0, 0x6f02f780: no-ops.
- Fake agent vtable slot 0 and map vtable slot 0: no-ops.

Setup: manager object at a heap address with vectors at the offsets in 2.2, granularity 1000, a map object whose first dword points to a fake vtable. Fake agents have a vtable, a count of 1 and a key at +0x10. The fs:[0] address is mapped so the prologues can run.

Results (21/21 PASS):
- Three creates give 0x100000, 0x100001, 0x100002. Table size 3. Each count is 1. Agent counts are 2.
- Releasing 0x100001 then 0x100000 gives a free stack of [0x100001, 0x100000]. The freed entry's agent pointer is cleared, and the agent count returns to 1.
- Next creates return 0x100000, then 0x100001 (LIFO). Then a fresh create gives 0x100003 (table size 3 plus the base).
- AddRef then one release keeps the id alive (count 1, not on the stack). A second release frees it.
- The same agent asked again returns the same id with no count change and no new slot.
- After the table set-size call with 1, the first id is 0x100001.

Limits of the check: stubs replace the external callees, so the agent-death route (5.5), the hashtable store and the JASS VM were not run. The stubs' calling conventions come from the first ret instruction and the decompiled signatures, which the decompiler sometimes gets wrong for thiscall (the register holding this was lost in several places). The results depend on those readings.

## 10. Corrections to docs/spec/handles.md (previous pass)

- Entry offset 0 is a reference count, not a key. Offset 4 is the agent pointer, and offset 8 is a flag.
- The base 0x100000 is found and confirmed (section 3). The previously UNSURE LIFO vs FIFO question is settled: LIFO.
- The counter is the entry-table size, not a separate counter. The 32-slot ring at 0x6f4b7610 is not part of id assignment (not traced further).
- GetHandleId is identity (CONFIRMED).
- The free-list pool at 0x6f3a3d70 and 0x6f3a3d20 is the free-id stack, not a generic agent pool.
- The agent allocator at 0x6f4a52e0 remains unexplained by this pass.

## 11. Open items

1. Trace the agent-death gate (0x6f472990, the type test and the flag at agent+0x20) and the dispatch from event 0x80267 to the manager's dispatcher.
2. Confirm the id release for DestroyGroup, DestroyTrigger and RemoveLocation individually.
3. Find the JASS VM store for handle-typed variables and check whether it adds or releases counts.
4. Trace 0x6f4313e0 to see whether any pre-sized slots are created at map load (section 8.2).
5. Confirm that the game-state constructor runs once per game and that the reset is not repeated mid-game.
