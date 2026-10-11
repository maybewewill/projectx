# Groups and unit events in Game.dll 1.26a (behavioural spec, clean-room)

Scope: JASS groups (CreateGroup and the group natives), group enumeration, unit and
player-unit events, region and state events, and the event-response getters.
Source: static reading of the decompiled chunks and the disassembly of Game.dll
(SHA1 in AGENTS.md). Decompiled code is not reproduced here; only addresses and
behaviour are given. Every claim is tagged:

- CONFIRMED: read directly from the control flow or the instructions, with the
  vtable or constant checked.
- LIKELY: strong evidence (names, constants, consistent call pattern) but not
  checked by running the code.
- UNSURE: not resolved, or the decode is ambiguous.

Method note: the decompiler's `__thiscall` parameter mapping is unreliable in
several places here, so argument roles were checked against the disassembly.
Where that was not possible, the claim is marked UNSURE.

---

## 0. Summary of the most important findings

1. A group is a `CGroup` object. Its embedded base `CUnitSet` (vtable 0x6F94C9F4)
   is a doubly linked list of unit references at group+0x24. Iteration order is
   insertion order for unit objects (LIKELY, see section 2.3).
2. ForGroup takes a snapshot of the list first, then runs the action on each entry in
   list order. Units added during the action are not visited. A unit whose removed flag
   is set when the loop reaches it is skipped; dead (life 0) units are not skipped
   (CONFIRMED for the snapshot and skip rule; section 4.2).
3. Every GroupEnum* native (except the selection variant, which does its own snapshot)
   first clears the group, then stores the filter and query parameters inside the
   group object, then runs a query whose collector is 0x6F3E7610.
4. The collector computes the filter-unit slot, evaluates the boolexpr, and adds the
   unit to the group. The "Counted" limit counter is incremented only when the group
   insert FAILS (CONFIRMED from the instructions). For normal units the insert
   succeeds, so the counted limit very likely never stops the enumeration (LIKELY,
   needs a harness run to confirm; this would be a notable behaviour to reproduce).
5. Unit events are stored in a per-agent observer hash table (agent+8). Each
   registration is a node keyed by `event id + 0x80200`. Dispatch walks the key's
   bucket list from oldest to newest and calls every matching listener, without
   early exit (CONFIRMED structure, LIKELY for trigger semantics).
6. Event-response natives (GetTriggerUnit, GetDyingUnit, GetEnteringUnit,
   GetFilterUnit, GetEnumUnit, ...) read a per-slot value stack. Slot numbers are
   listed in section 6 (CONFIRMED for the numbers).

---

## 1. Object model

### 1.1 Handles
- Each group, unit, trigger, boolexpr, player, region, rect and location is a JASS
  handle resolved by a per-type resolver. The unit resolver is 0x6F3BDCB0. The group
  resolver is 0x6F3BEA30. The trigger resolver is 0x6F3BDEF0. The boolexpr resolver
  is 0x6F3BD950, the player resolver 0x6F3BD4D0, the region resolver 0x6F3BDA70, the
  rect resolver 0x6F3BDB90 and the location resolver 0x6F3BD3B0. CONFIRMED for the
  addresses, and each resolver returns NULL for an invalid handle.
- A unit resolver returns NULL when the agent's removed flag (+0x20) is non-zero.
  CONFIRMED.
- Handle ids for objects are created by 0x6F430C80 (object to handle id). The
  object's id is at +0x10. CONFIRMED.
- Group handle tag (the value returned by the group creation tag function
  0x6F3DAE70) is 0x2B677270. LIKELY it is the group kind tag.
- Unit kind tag at agent+0x0C is 0x2B61676C (CONFIRMED value; see unit-core.md).

### 1.2 CGroup and CUnitSet
- CGroup vtable: 0x6F94CA0C (30 slots). CUnitSet vtable: 0x6F94C9F4 (4 slots).
  CONFIRMED from the RTTI tables.
- The CUnitSet base sits at group+0x24. The vtable pointer is at group+0x24. The
  thunks at 0x6F3DAEC0, 0x6F3DAEE0, 0x6F3DAEF0 and 0x6F3DAF00 add 0x24 to `this`
  before calling into the set. CONFIRMED.
- CUnitSet slots: slot 0 destructor 0x6F3F4B60; slot 1 insert 0x6F421690; slot 2
  remove 0x6F421920; slot 3 clear 0x6F421980. CONFIRMED.
- Non-virtual set methods: contains 0x6F4219B0 (linear search by pointer), indexed
  get 0x6F421A20 (returns the unit at index n from the head), next-node step 0x6F41FF40.
  CONFIRMED.
- Snapshot into a temporary array (used by ForGroup and the selection enum) is
  0x6F421A80. CONFIRMED.

### 1.3 Set layout (offsets relative to the set base, i.e. group+0x24 for groups)
| set offset | group offset | meaning | confidence |
|---|---|---|---|
| +0x00 | +0x24 | vtable pointer | CONFIRMED |
| +0x08 | +0x2C | sentinel link (the "tail" end of the ring) | CONFIRMED (insert appends before it) |
| +0x0C | +0x30 | head pointer (first node) | CONFIRMED (walks start here) |
| +0x10 | +0x34 | element count | CONFIRMED (++ on insert, -- on remove, 0 on clear) |
| +0x14 | +0x38 | current filter boolexpr (set by the enum prep call) | LIKELY |
| +0x18 | +0x3C | ForGroup action code slot | CONFIRMED (ForGroup stores the code here) |
| +0x1C | +0x40 | enum parameter A: unit type id, or -1 for any | CONFIRMED (collector compares it with unit+0x30) |
| +0x20 | +0x44 | enum parameter B: player index, or -1 for any | CONFIRMED (collector compares it with the owner index) |
| +0x24 | +0x48 | enum limit (0 or -1 means no limit in practice; see 3.4) | CONFIRMED read; LIKELY meaning |
| +0x28 | +0x4C | enum counter | CONFIRMED |

Caution: the offsets +0x38 to +0x4C lie inside the CGroup object, after the CUnitSet
fields. The enum prep call writes them, so the group keeps its query state between
calls.

### 1.4 List node
- A node is allocated from a pool (or fall-back allocator) with a 12-byte logical
  layout: +0x00 previous link, +0x04 next link, +0x08 element reference.
- The element reference is a smart pointer to the unit agent. The agent's refcount
  is at +0x04 of the agent object. CONFIRMED: insert and snapshot increment it and
  remove decrements it.
- The list is walked by following the next link from the head, and a walk ends when
  the link is zero or negative (as a signed value). CONFIRMED.
- Link values use an encoded form (a negative value means "points to the head slot").
  The exact encoding is UNSURE, but the traversal order is not affected.

---

## 2. Group storage

### 2.1 CreateGroup (0x6F3D2900)
- Creates a CGroup object, allocates a handle id for it, and returns the id.
  CONFIRMED that it returns a handle. The exact allocation calls are not all named.
- An empty group has count 0 and no head. CONFIRMED.

### 2.2 DestroyGroup (0x6F3C3FA0)
- Resolves the group and calls the group object's vtable slot +0x5C. The group is
  released through that slot (LIKELY: destruction plus handle release). The group
  handle is then invalid. UNSURE whether the slot clears the members (it would not
  need to, since members are reference counted).

### 2.3 Insertion (CUnitSet slot 1, 0x6F421690)
Insert(set, unit, checkDuplicates, sortedFlag):
1. A NULL unit returns 0. CONFIRMED.
2. If checkDuplicates is 0 (this is how GroupAddUnit and the enumerator call it), the
   list is scanned and an existing entry returns 0 (not added). CONFIRMED.
3. If sortedFlag is non-zero (GroupAddUnit and the enumerator both pass 1), the
   list is scanned from the head. A new node is placed before the first existing node
   where both (a) the two units have the same value at +0x30 (unit type id) and (b)
   the new unit's value at +0x0C is less than the existing node's value at +0x0C
   (unsigned compare). CONFIRMED structure.
4. If no position was found, the node is appended before the sentinel (the tail).
   CONFIRMED.
5. A new insert increments the count and returns 1. CONFIRMED.
6. Units use the kind tag 0x2B61676C at +0x0C. Because it is the same value for every
   unit, the comparison in step 3 is always false for units, so the sorted path never
   reorders them. LIKELY. Consequence: for unit groups the iteration order is the
   order of insertion (LIKELY).
7. A special branch exists when the unit's type id has an uppercase high byte (0x41 to
   0x5A). It allocates a second node and skips the final append. Its effect on the
   list is UNSURE, and it is not reached for ordinary type ids.

### 2.4 GroupAddUnit (0x6F3C3FC0) and GroupRemoveUnit (0x6F3C3FF0)
- Both resolve the group and the unit (the unit resolver rejects removed units). If
  the unit resolves, add calls Insert(unit, 0, 1) and remove calls the set's remove
  slot. CONFIRMED.
- Adding a removed unit is a no-op (the resolver returns NULL). CONFIRMED.
- Remove (slot 2, 0x6F421920): walks from the head, compares element pointers,
  unlinks the node on a match, decrements the count and releases the reference.
  Returns 1 if removed, 0 if not found. CONFIRMED.

### 2.5 GroupClear (0x6F3C4020) and clear slot (0x6F421980)
- Clear removes every node in order (head first), releases references, and sets the
  count to 0. CONFIRMED.

### 2.6 IsUnitInGroup (0x6F3C7A00)
- Resolves the unit (removed units give false) and the group, then does a pointer
  search through the list (0x6F4219B0). Returns 1 or 0. CONFIRMED.

### 2.7 FirstOfGroup (0x6F3C4460)
- Returns the unit at index 0 (the head) converted to a handle id (0 if the group is
  empty). The conversion (0x6F430C80) returns the existing handle, or creates one.
  CONFIRMED.
- FirstOfGroup does NOT check the removed flag. A removed unit that is still at the
  head is returned as a handle. LIKELY. Dead units (life 0) are returned too.

---

## 3. Enumeration

### 3.1 Common prologue (every GroupEnum* except Selected)
Shared prep (0x6F3E01C0) does, in this order:
1. Clear the group (group clear slot). CONFIRMED.
2. Store the filter boolexpr object into the group (via 0x6F3B55C0; LIKELY into +0x38).
   A null boolexpr means no filter. LIKELY.
3. Store the limit (+0x48), zero the counter (+0x4C), and store the two parameters
   (+0x40, +0x44). CONFIRMED.
4. Start the query and run the collector (0x6F3E7610) over the candidates. CONFIRMED.

The group is cleared only when the native's validity checks pass, which differ by
native (CONFIRMED from the control flow):
- GroupEnumUnitsOfType: group valid AND the type name resolves to a valid type id
  (not -1). Otherwise the group is left untouched.
- GroupEnumUnitsOfPlayer: group valid AND player valid. Otherwise untouched.
- GroupEnumUnitsInRange, ...InRangeOfLoc, ...InRect: group valid (rect or location
  must also resolve for those two). Otherwise untouched.
- GroupEnumUnitsSelected: group valid AND player valid, then the group is cleared.

### 3.2 Parameter assignment per native
| native | +0x40 (type) | +0x44 (player) | query engine | candidate source |
|---|---|---|---|---|
| GroupEnumUnitsOfType (0x6F3C4040) | type id | -1 | 0x6F46D1B0 (global) | world unit list (see 3.5) |
| GroupEnumUnitsOfPlayer (0x6F3C42C0) | -1 | player index (player+0x30 byte) | 0x6F46D1B0 (global) | world unit list |
| GroupEnumUnitsInRange (0x6F3C41A0) and InRangeOfLoc (0x6F3C41E0) | -1 | -1 | 0x6F467570 (spatial, radius mode) | spatial cells |
| GroupEnumUnitsInRect (0x6F3C40E0) | -1 | -1 | 0x6F3E7CD0 path, box mode | spatial cells (box) |
| GroupEnumUnitsSelected (0x6F3CE590) | - | - | none (own loop, see 3.6) | player selection snapshot |
Counted variants (0x6F3C4090, 0x6F3C4140, 0x6F3C4230, 0x6F3C4270) differ only in
passing the limit argument instead of -1. CONFIRMED.

### 3.3 Collector (0x6F3E7610, called for each candidate unit)
Order of checks, per candidate:
1. If the limit is non-zero and the counter is at least the limit, return 0 (stop
   the query). CONFIRMED.
2. If parameter A (+0x40) is not -1 and differs from the unit's type id (+0x30), skip
   the type test. If parameter B (+0x44) is not -1 and differs from the unit's owner
   index (virtual slot +0xEC of the unit, returns the owner index), skip. CONFIRMED.
   Both pass means the unit is a candidate.
3. Set the filter-unit slot (slot 7) to this unit, and run the filter boolexpr if one
   is set. The filter runs after the type/player test and after the filter-unit slot
   is set. CONFIRMED.
4. If the filter passes (or is null), call Insert(unit, 0, 1). CONFIRMED. Then the
   counter is incremented if and only if Insert returned 0 (CONFIRMED, asm at
   0x6F3E7707 to 0x6F3E770B).
5. The filter-unit slot is popped and the collector returns 1 (continue).
   CONFIRMED.

What this means for the Counted natives: the counter counts insert FAILURES, not
inserts. A fresh unit never fails to insert (the group was just cleared and the same
unit is not visited twice). So the counter stays at 0 and the limit never fires.
LIKELY, the most important open point: confirm with the Unicorn harness that
GroupEnumUnitsInRangeCounted with limit N returns more than N units when more than N
match. If confirmed, reproduce the behaviour exactly (it is a quirk of the original).

Note: the removed-flag check and the life check are NOT done in the collector. Dead
units (life 0) are added if they match. Removed units are skipped at the candidate
level (see 3.5 and 3.7).

### 3.4 Limit values
- A limit of -1 (0xFFFFFFFF, unsigned large) is never reached in practice, since the
  test is `limit != 0 && limit <= counter`. CONFIRMED.
- A limit of 0 means no limit. CONFIRMED.

### 3.5 Global (type and player) enumeration
- Query engine: a call to 0x6F26C1C0 returns the constant tag 0x2B773375 (CONFIRMED).
  That tag is used as the root key for 0x6F46D1B0, which resolves the world root and
  calls 0x6F46D0B0 with the collector.
- 0x6F46D0B0 first walks a linked list of unit objects at world+0x58 (next link at +8,
  removed flag at +0x20 is skipped, the tag check at +0x0C must match the unit tag).
  Each valid unit is passed to the collector. A collector return of 0 stops the
  walk. CONFIRMED structure.
- When that list is exhausted, the function walks a second structure (an array of
  sub-cells at +0x48 with count at +0x50, each cell with a cached handle at +0x1C) and
  recurses. CONFIRMED structure; the exact cell semantics are UNSURE.
- Order: the world list order first, then the cell walk. LIKELY that the world list
  is in creation order (UNSURE how it is maintained).

### 3.6 Selection enumeration (GroupEnumUnitsSelected, 0x6F3CE590)
- Clears the group, then snapshots the player's selection set (the set at player+0x34
  is checked for non-zero count first). CONFIRMED.
- For each selected unit, in snapshot order: skip NULL, skip if the tag does not match
  or the removed flag (+0x20) is non-zero. Then set the filter-unit slot (slot 7), run
  the filter if present (the filter is the native's own argument), add with Insert(0,1),
  and pop the slot. CONFIRMED.
- There is no type or owner test here. Owner filtering comes only from the selection
  set itself. CONFIRMED.

### 3.7 Spatial enumeration (InRange, InRangeOfLoc, InRect)
- InRange and InRangeOfLoc call the shared path 0x6F3E7C70 (InRangeOfLoc first
  converts the location to x and y). The rect variant is 0x6F3E7CD0 and copies the four
  rect bounds. CONFIRMED.
- Both paths call 0x6F467570 with the collector. The shared dispatcher chooses by its
  second argument:
  - non-zero: 0x6F47FB50 (sub-dispatch by radius and a second flag: 0x6F47F4C0 or
    0x6F47F7B0; zero or -1 radius: 0x6F47F1F0 whole-area sweep). LIKELY.
  - zero: 0x6F47FAF0 (sub-dispatch: 0x6F47EC80 or 0x6F47EF40 for a radius; 0x6F47E9E0
    sweep for zero or -1). LIKELY.
  The exact meaning of the second argument (rect versus circle) is UNSURE.
- The radius sub-queries (0x6F47EC80 and 0x6F47F4C0) do the following (CONFIRMED
  structure, UNSURE formula):
  1. Allocate a query record from a pool (the pool is at a global block, index
     counter at +0x64, record array at +0x60) and mark it in use.
  2. Compute a bounding box for the query from the centre and radius using softfloat
     add (0x6F6EF000), subtract (0x6F6EEEF0) and multiply (0x6F6EEE20) helpers. The
     float results are then compared using the float compare helpers. UNSURE which
     compare is used.
  3. Collect candidate cells (0x6F47BA20 and 0x6F47D5D0 build the list; the cells are
     stored in the record's array at +0x0C with count at +0x1C). Order is the order
     the collection routine produces; UNSURE.
  4. For each candidate cell, read the unit reference at cell+0x30. Skip the candidate if
     the unit's removed flag (+0x20) is set, or if the unit's position component
     (+0x54) is NULL. Then call a per-unit virtual (slot +0x1C of that component), run
     a predicate (0x6F471840), and call the collector. A collector return of 0 stops the
     walk (the function breaks out of the loop).
  5. The record is released (the in-use counter is decremented).
- The distance test: the candidate's position is compared with the query centre and the
  radius using softfloat operations. The comparison that gates each candidate is a
  float less-or-equal. Whether this is squared distance against squared radius, or the
  distance itself, is UNSURE. Do not reproduce the formula before the harness check.
- Not in the softfloat table: 0x6F6EF3B0 (one float argument, used in the candidate
  loop). Its role is UNSURE.

### 3.8 Range test: summary
- Gate: a float compare of a computed value against the radius (or squared radius).
  Operations: softfloat add, subtract, multiply. Compare helper: float less-or-equal.
  Formula: UNSURE (the arithmetic order matters for bit-exact reproduction).
- Dead units are NOT excluded by the range test. Only the removed flag is checked.
  LIKELY.

---

## 4. ForGroup, removed units and ghost entries

### 4.1 ForGroup (0x6F3C44A0, body 0x6F3F5620)
Order of work:
1. Resolve the group. If the group is NULL, do nothing. CONFIRMED.
2. Save the current thread context (0x6F3A8060). Resolve the action code handle to an
   action object (0x6F4309A0, 0x6F3B54D0). If the code handle is 0, use the default
   action slot. CONFIRMED.
3. Store the saved values in a small local record (context, group, action).
4. Call the iterator 0x6F3EFCC0 with the action callback 0x6F3E7D20 and limit -1
   (all). CONFIRMED.

### 4.2 Iterator and callback
- The iterator takes a snapshot of the group (0x6F421A80). The snapshot holds the
  elements in list order, head first, and takes a reference on each. The count is
  taken from the set's count at snapshot time. CONFIRMED.
- For each snapshot entry (in order): skip NULL. Then the tag and removed-flag checks
  run (tag must equal 0x2B61676C and the removed flag +0x20 must be zero). If they fail,
  the entry is skipped silently and the loop continues. CONFIRMED.
- The callback sets the enum-unit slot (slot 10) to the unit, runs the action code if
  it is non-null (through the action invoker), and pops slot 10. The callback always
  returns 1, so the iteration never stops early. CONFIRMED.
- Consequence: units that are added to the group during the callback are NOT visited
  (the snapshot is fixed). Units removed during the callback are still in the
  snapshot, but they are skipped if their removed flag is set when the loop reaches
  them. Dead (life 0) units are NOT skipped. CONFIRMED structure, LIKELY for the dead
  case.
- After the iterator, the saved context is restored. Nested ForGroup calls work
  because the saved values are restored on return. LIKELY.

### 4.3 Ghost entries
- Nothing removes a unit from groups when the unit is removed with RemoveUnit. The
  group keeps its reference, so the unit stays in the list and in the count. LIKELY.
  This means ghost entries exist.
- Visible effects of a ghost (removed unit still in a group):
  - ForGroup skips it (removed flag check). CONFIRMED.
  - FirstOfGroup can return it (no check). LIKELY.
  - IsUnitInGroup returns false for it (resolver rejects removed units). CONFIRMED.
  - GroupAddUnit and GroupRemoveUnit: adding is a no-op (resolver); removing a ghost
    by its unit handle also fails at the resolver, so the ghost stays. LIKELY.
  - The count (group+0x34) still includes ghosts. LIKELY.
  - Group enumerations clear the group first, so ghosts disappear at the next enum.
    CONFIRMED.

---

## 5. Unit and player-unit events

### 5.1 Event id ranges (JASS constants are passed directly as the numbers)
TriggerRegisterUnitEvent (0x6F3D23B0):
- Accepted: 0x34 to 0x58 and 0x119 to 0x129 (inclusive). CONFIRMED by the range checks
  (the first set plus the 0x3C-0x58 and 0x11E-0x129 sets combined).
- Anything else returns 0 with no side effects. CONFIRMED.
- Also requires the unit and trigger handles to resolve. CONFIRMED.

TriggerRegisterPlayerUnitEvent (0x6F3D1DD0):
- Accepted: 0x12 to 0x33 and 0x10D to 0x118 (inclusive). CONFIRMED.
- Requires the trigger, the player and the boolexpr to be valid (the player must
  resolve; the boolexpr may be null). LIKELY.

### 5.2 Registration key
- Key = event id + 0x80200 for both unit and player-unit events. CONFIRMED (the
  registration calls add 0x80200).

### 5.3 Where the listener is stored
- Unit event: the unit agent's observer table (agent+8, created on demand by
  0x6F629F70 through 0x6F629E90). The trigger also records the key at trigger+0x34.
  CONFIRMED for the table and the key storage; the trigger+0x34 role is LIKELY.
- Player-unit event: registered on the player object (0x6F437000 takes the player and
  a boolexpr). CONFIRMED for the call; LIKELY that the player holds its own observer
  table. How the unit death handler reaches it is UNSURE.

### 5.4 Death events (CONFIRMED key values from the firing sites)
- Player-unit death uses key 0x80214 (event 0x14). It fires from the unit death
  sequence inside 0x6F41CA40, which is called from 0x6F297520 right after the kill side
  effects (0x6F2834E0). The fire is guarded by "observers exist for this key".
- Unit death uses key 0x80235 (event 0x35). It fires from 0x6F297520 after the
  player-unit death call. Its payload pushes the dying unit into slot 0 (LIKELY) and
  the event object is fired through 0x6F285E10.
- Order inside the death sequence: player-unit death (0x80214) first, then unit death
  (0x80235). CONFIRMED by code order.
- Also CONFIRMED: the kill event 0xD01E6 (CEvent, see unit-core.md) is a different
  mechanism. It is not the JASS death key.

### 5.5 Other keys seen in the firing sites (names UNSURE)
0x80230, 0x80231, 0x80232, 0x80233, 0x80234, 0x80236 to 0x80239, 0x8023B, 0x8023D.
Notes:
- 0x8023B is fired at the end of unit state-event registration. CONFIRMED.
- 0x8023D is the "unit moved into a region" key used by the enter test. LIKELY.

---

## 6. Region and state events

### 6.1 Region enter and leave (0x6F3D2700 and 0x6F3D2800)
- TriggerRegisterEnterRegion uses key 0x80205 (event 5). CONFIRMED.
- TriggerRegisterLeaveRegion uses key 0x80206 (event 6). CONFIRMED.
- Both require the trigger, the region and the boolexpr to resolve. Enter passes the
  boolexpr through; leave does the same. CONFIRMED for the calls.
- Event 0x3D (61) also counts as an "enter" for GetEnteringUnit (key 0x8023D). LIKELY.

### 6.2 State events (0x6F3D21B0)
- Requires state index < 4 and an operator code < 0x80106. CONFIRMED.
- Registers an operator-specific key from the range 0x80101 to 0x80105. LIKELY (from
  the operator switch in 0x6F435C20). The exact mapping of the JASS operator constants
  to those keys is UNSURE.
- Ends by firing the notification key 0x8023B on the unit. CONFIRMED.

---

## 7. Observer table (agent-side listeners)

### 7.1 Layout (CONFIRMED from the table code, 0x6F62A820 and 0x6F62A5D0)
- Table object (created by 0x6F629E90, stored at agent+8):
  - byte +0: flag (non-zero while the table is being walked; enables removal of dead
    entries). CONFIRMED.
  - byte +1: bucket count (the bucket index is key AND (count - 1)). CONFIRMED.
  - word +2: number of entries. CONFIRMED.
  - pointer +4: bucket array (one head pointer per bucket). CONFIRMED.
- Node (allocated by the node pool, 0x6F4C1AB0):
  - +0x00: next node link (circular within the bucket). CONFIRMED.
  - +0x04: key (the event key, e.g. 0x80235). CONFIRMED.
  - +0x08: owner (a referenced listener object, e.g. the trigger). CONFIRMED.
  - +0x0C: payload (a value copied into the event object at +0x08 when dispatching).
    CONFIRMED.

### 7.2 Insertion and dedupe (0x6F62A820)
- The bucket is a circular list. The bucket head pointer always points to the newest
  node. Its next link points to the oldest. CONFIRMED.
- A new node is linked after the newest and becomes the newest. CONFIRMED.
- Duplicate rule: if a node already has the same key AND the same owner, the existing
  node's payload is overwritten and no node is added. The position in the bucket is
  kept. CONFIRMED.
- Dead nodes (owner NULL) met during the scan are unlinked and freed. CONFIRMED.

### 7.3 Dispatch (0x6F62A5D0, reached through agent slots 4 and 5)
- Agent slot 4 (0x6F629A90) forwards to slot 5. Slot 5 (0x6F62A7B0) calls the dispatch
  only if the table exists (non-null). CONFIRMED.
- Walk: start at the bucket's oldest node (the newest's next), go to the next link,
  stop after the newest. CONFIRMED.
- For each node with a matching key: write the node's payload into the event object
  at +0x08, then call the owner's vtable slot 3 with the event object. If the call
  returns non-zero, the result flag is set. The walk does NOT stop early. CONFIRMED.
- The dispatch returns the OR of all results. CONFIRMED.
- Consequence for several triggers on the same event: they run in registration order
  (oldest first), each one in turn, all of them. LIKELY (the owner's slot 3 is the
  trigger's execute entry; not checked directly).
- Re-registering the same trigger on the same event keeps its original position (the
  dedupe rule above). CONFIRMED.

### 7.4 Slot 2 (AddObserver wrapper, 0x6F62A9A0)
- Pushes the constant 1 for "create table" and calls the table getter (0x6F629F70),
  then jumps into the insert. Its arguments are (key, key, owner): the payload is the
  key itself, and the owner is the trigger. CONFIRMED for the argument order, LIKELY for
  the meaning of the owner.
- So after dispatch the event object's +0x08 holds the key (the event id + 0x80200).
  LIKELY.

---

## 8. Event-response getters

Each getter first reads the current event key through GetTriggerEventId (slot 6),
and returns the null handle if the key does not match. Values below are the
event ids (key minus 0x80200).

| getter | address | accepted event ids | value read | confidence |
|---|---|---|---|---|
| GetTriggerUnit | 0x6F3BB240 | any | slot 0 | CONFIRMED |
| GetDyingUnit | 0x6F3BB410 | 0x14, 0x35 | slot 0 | CONFIRMED |
| GetKillingUnit | 0x6F3C23F0 | 0x14 only | killer through 0x6F40B6C0 (after 0x6F3C1D60 check) | CONFIRMED for the id test; killer lookup UNSURE |
| GetEnteringUnit | 0x6F3BB6D0 | 0x05, 0x3D | slot 0 | CONFIRMED |
| GetLeavingUnit | 0x6F3BB6F0 | 0x06 | slot 0 | CONFIRMED |
| GetFilterUnit | 0x6F3BB2E0 | any | slot 7 | CONFIRMED |
| GetEnumUnit | 0x6F3BB300 | any | slot 10 | CONFIRMED |
| GetTriggerEventId | 0x6F3BB2C0 | any | slot 6 | CONFIRMED |
| GetTriggeringTrigger | 0x6F3BB2A0 | any | slot 2 | CONFIRMED |
| GetTriggerPlayer | 0x6F3BB280 | any | slot 3 | CONFIRMED |

Interpretation of the event ids (CONFIRMED from the getter checks):
- 0x14 is the player-unit death event (key 0x80214).
- 0x35 is the unit death event (key 0x80235).
- 5 and 6 are region enter and leave (keys 0x80205, 0x80206).
- 0x3D is the alternative enter (key 0x8023D).

Slot storage (CONFIRMED read; push/pop semantics LIKELY):
- The slot stack for slot N is at (this + 0x94 + 0x10 N). Its fields: a count at +0x04,
  an array base at +0x08. The read returns the top value (or 0 if empty).
- Push: 0x6F430DB0 (converts a handle or object to an id, then pushes).
- Pop: 0x6F42DEE0 (pops, returns the value, and releases it if the flag is set).
- Filter-unit slot (7) and enum-unit slot (10) are pushed and popped around each
  callback. CONFIRMED.

---

## 9. Event firing order summary (unit death)
1. Kill side effects (0x6F2834E0).
2. Player-unit death key 0x80214 (0x6F41CA40), only if observers exist.
3. Unit death key 0x80235 (inside 0x6F297520), only if observers exist. Push the dying
   unit into slot 0 first.
4. Observers for each key are dispatched in registration order (section 7.3).

UNSURE: whether the death event is fired from 0x6F297520 only when life reaches 0, or
from every kill path. The existing spec (unit-core.md) says KillUnit fires 0xD01E6,
which is a different path.

---

## 10. Open questions and how to close them
1. Counted limit (section 3.3). Run GroupEnumUnitsInRangeCounted with a small limit on
   N > limit matching units in the Unicorn harness (tools/emu) and count the result.
   If it returns all N, the "counter counts insert failures" reading is confirmed.
2. Range formula (section 3.7). Read the float compare and the softfloat call order in
   0x6F47EC80 and 0x6F47F4C0 with the arguments. Then check candidates in the harness
   with random centres and radii. Also resolve 0x6F6EF3B0 and 0x6F471840.
3. Spatial cell order (section 3.5 and 3.7). Identify the cell array build in
   0x6F47BA20 and 0x6F47D5D0. Order matters for the callback order.
4. Global unit list order (section 3.5). Find where the world list at world+0x58 is
   appended to on unit creation.
5. Selection set (section 3.6). Identify the player selection set at player+0x34 and
   its insertion order.
6. Sorted-insert special branch (section 2.3 step 7). Find 0x6F4207E0 and check the
   path for uppercase type ids.
7. Trigger execute entry (owner slot 3). Confirm that the trigger's slot 3 runs the
   action list and returns its result.
8. Player-unit event storage (section 5.3). Find where the player observer table is
   created and who fires it for death.
9. State event operator mapping (section 6.2). Map JASS LESS_THAN and friends to keys
   0x80101 to 0x80105 from the switch in 0x6F435C20.
10. Death trigger path (section 9). Check whether 0x6F297520 runs from every kill.

Suggested harness work: each check above is a single native or helper call with
known arguments, so the existing Unicorn harness (tools/emu) can run them. Do not write
harness output into the repo; keep it in scratchpad.


## Addendum (lead, 2026-10-11)
Event 0x3D is TriggerRegisterUnitInRange (FUN_6F3D25F0, key 0x8023D), not an alternative region-enter event (found while implementing regions). The world-to-cell origin is world+0x6C for x and +0x70 for y (Unicorn-checked in the region box helper 0x6F47BA20), which also corrects groups-events-2.md section 1.2.
