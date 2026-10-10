# Handle ids in Warcraft III 1.26a (Game.dll) - behavioural spec

Scope: what the decompiled pseudo-C in `scratchpad/decomp/chunk_*.c` shows about JASS handle ids,
agent objects, and the containers that hold them. No code is reproduced here.
Confidence labels: CONFIRMED (seen directly in the decompile or RTTI), LIKELY (strong structural
evidence, not fully traced), UNSURE (not found in this pass, or contradicted by the evidence).

## 0. Summary of what was and was not found

| Item | Status | Where |
|---|---|---|
| RTTI for agent classes (CAgent, CAgentWar3, CAgentTimer, CAgentTimerExtended, CAgentPtr<...> for CUnit, CEffectImage, CAgent, CRallyPoint, CMineShaft) | CONFIRMED | chunk_000, chunk_001, chunk_003, chunk_004 |
| Handle-mapping containers: Agent2HandleReg, Handle2AgentReg, JassHandle, String2HandleReg, FuncAddr2HandleReg, Script2HandleReg | CONFIRMED (names and container types) | chunk_016 to chunk_019 |
| Source path "agent_.cpp" | CONFIRMED (string present, one function) | chunk_020, function at 6f4a52e0 |
| Source path "AgentRef.cpp" | NOT FOUND | - |
| Native signature for GetHandleId, which takes a handle and returns an integer | CONFIRMED (signature string) | chunk_017 / chunk_027, native-type registration |
| Native body of GetHandleId | UNSURE (not located; natives are registered from data tables) | - |
| Base id 0x100000 as a literal | NOT FOUND in the decompile | - |
| Global id counter or free list for handle ids | NOT FOUND | - |
| Pre-allocation order of players and other objects at map start | UNSURE | - |
| Relationship of selection-action object ids to handle ids | UNSURE | - |

Consequence: the commonly stated base of 0x100000 and the LIFO/FIFO question cannot be settled from
this decompile. Those statements below stay UNSURE until a function that writes a fresh id to a new
object is found (a constructor that stores into an agent field and reads a counter).

## 1. Containers that map handles to agents

1.1 Agent-to-handle table (CONFIRMED)
- A hash table template instance keyed by "no key" type (HASHKEY_NONE) holds entries of type Agent2HandleReg.
- It is used once with a 256-slot allocator variant and once as a plain hash table.
- Each entry is a small node with an intrusive link and a value; the node is freed through the standard
  delete path (the "delete" string shows the allocator is the usual one with a free-list pool).

1.2 Handle-to-agent table (CONFIRMED structure, LIKELY role)
- Stored as a flat array of 12-byte entries (TSExplicitList-style). Each entry holds:
  - offset 0: a key (the value a script sees, most likely the handle id, LIKELY)
  - offset 4: a pointer to a reference-counted object (an agent, LIKELY)
  - offset 8: a third word (linkage or sentinel, UNSURE)
- Growing the array (resize routine at 6f42b060) moves entries into a new buffer and releases the
  old one. Entries whose pointer is non-null are released on resize.
- Destruction (6f42a750) walks every entry, decrements the referenced object's count at offset 4 of
  the object, and calls the object's virtual destructor (slot 0) when the count reaches zero.
  CONFIRMED: the container owns references, not raw pointers.

1.3 JassHandle registry (CONFIRMED names, LIKELY NOT the object-id table)
- A string-keyed table (HASHKEY_CONSTSTR) holds JassHandle entries, with a 128-slot allocator variant.
- Companion tables: String2HandleReg (string-keyed), FuncAddr2HandleReg (function-address-keyed),
  Script2HandleReg (script-keyed). These look like the JASS symbol and type registry (the "handle"
  type name, the "(Hhandle;)I" signature, the "handle" symbol in the type table) rather than the
  per-object id map. LIKELY.

1.4 Agent allocator (agent_.cpp) (LIKELY, partly traced)
- The function at 6f4a52e0 requests a 0x20-byte object from the file ".\agent_.cpp" line 256
  (0x100), then runs a loop whose count is stored at offset 0x74 of the owning object, calling a
  per-slot initializer for each of those entries. Its role is a fixed pool of agent slots, not
  a handle-id generator. UNSURE beyond that.

## 2. Id assignment (UNSURE overall)

2.1 Base value
- The literal 0x100000 and the decimal 1048576 do not appear as a constant in any assignment in the
  decompile. The bit 0x100000 does appear, but only as a flag test on agent flags at offset 0x20
  or 0x5c (for example the bit tests in chunk_001 and chunk_024). Those are flags, not ids.
- Verdict: the base id 0x100000 is UNSURE. It cannot be confirmed or refuted from these chunks.

2.2 Counter and growth
- The closest counter is at 6f4b7610: a 32-slot ring (index masked by 0x1f) that returns a pointer
  into a 0x40-byte-per-slot table. This is the kind of wrap-around pool an object allocator might
  use, but it returns a pointer, not an integer, and it is not connected to GetHandleId in this pass.
  UNSURE whether it is the handle id source.
- Growth of the handle tables is by resize (entries are moved and the old buffer is freed), which
  matches the TSExplicitList pattern (CONFIRMED for the arrays; the id values are not traced).

2.3 Free list order
- No free-list code that hands out ids was identified. The intrusive-list code in 1.2 manipulates
  link words but does not show whether the newest or oldest freed id is reused.
- Verdict: LIFO vs FIFO is UNSURE.

## 3. Reuse and lifetime

3.1 Reference counting (CONFIRMED for the containers, LIKELY for JASS locals)
- Every container that stores an agent holds a counted reference (the resize and destructor code
  increments and decrements the count on the object).
- Therefore an object referenced by a handle-table entry is kept alive while the entry exists.
- Whether a JASS local holds a reference in the VM sense (so the id stays reserved while a variable
  points to the object) was not traced; LIKELY, by analogy with the container behaviour.

3.2 When an id is freed (UNSURE)
- Options that cannot be distinguished from this pass: (a) immediately on destruction, (b) when the
  count drops to zero, or (c) deferred to a sweep. The destructor path (6f42a750 and its neighbours)
  releases counts synchronously, so (b) at least applies to the container's own reference.
- Verdict: the exact moment an id returns to a pool is UNSURE.

## 4. Which object kinds get handle ids

- Agent classes present in RTTI: CAgent (base), CAgentWar3, CAgentTimer, CAgentTimerExtended, and
  CAgentPtr<T> smart pointers for CUnit, CEffectImage, CAgent, CRallyPoint and CMineShaft.
  CONFIRMED that these classes exist; LIKELY that units, items, timers, effects and similar game
  objects are the agents that receive handle ids. UNSURE for groups, locations, players and triggers.
- The CHandleObject class is a wrapper for operating-system HANDLE values (file, thread, event);
  it is NOT the JASS handle id system. CONFIRMED.
- Pre-allocation at map start: not determined. UNSURE. Players are most likely created from the map
  data in a fixed order, but no code showing this was located.

## 5. Agent ids, handle ids, and replay object ids

- Agent identity: each agent is a C++ object referenced through CAgentPtr smart pointers. Its handle
  id is the key by which the handle-to-agent container finds it. LIKELY.
- Replay selection: the replay system has TReplayUnit containers and a replay state machine
  (REPLAY_STATE_NONE, LOADED, STREAMINGIN, STREAMINGOUT, STOPPED). CONFIRMED for the state names.
- Selection actions in replays: no decode of the two 32-bit object ids per unit was found. The
  relationship to handle ids is UNSURE. A fair guess (UNSURE) is that the first id is the agent's
  internal object id and the second is a generation or sequence value; this is not supported by the
  decompile yet.

## 6. Open items to finish this spec

1. Find the constructor that initialises a new agent and stores its id (look for a write to an agent
   field next to a call into the Handle2Agent insert routine; the insert is around 6f42e040).
2. Trace the native body for GetHandleId by following the native registration tables (data-driven;
   the name-to-function map is in the data section, not the pseudo-C).
3. Trace the replay selection decoder (TReplayUnit and the selection command types in chunk_022)
   to read the two object-id words.
4. Confirm or refute the 0x100000 base by finding the counter write, not by searching for the literal.
