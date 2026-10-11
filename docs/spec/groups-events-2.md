# Groups and unit events in Game.dll 1.26a: pass 2

Follows groups-events.md (pass 1). Same rules: plain English, addresses, no decompiled
code, each claim tagged CONFIRMED / LIKELY / UNSURE. "Harness" means a Unicorn run of the
original Game.dll code (tools/emu/jasscomp Harness, plus Storm ordinal 401 from that harness).
Scripts and raw output are in scratchpad/pass2/ (not in the repo).

Summary of what changed from pass 1:
- Insertion order of group members is now CONFIRMED (harness plus asm).
- The range test is now CONFIRMED end to end on the emulated sub-query (900 of 900 cases).
- The Counted limit is CONFIRMED to be inert for range queries by logic plus the harness
  insert test; it is UNSURE for type and player enumeration.
- The state-event mapping and the player-unit observer owner are now mostly CONFIRMED.

---

## 1. Spatial structure behind 0x6F467570 (range and rect)

### 1.1 Entry chain (CONFIRMED)
- GroupEnumUnitsInRange (0x6F3C41A0) and InRangeOfLoc (0x6F3C41E0) reach 0x6F3E7C70, which
  clears the group, stores the parameters, and calls 0x6F467570 with the collector 0x6F3E7610.
- 0x6F467570 tests its second argument: non-zero goes to 0x6F47FB50, zero goes to
  0x6F47FAF0. Inside those, a radius of 0 or -1 sends the work to a whole-area sweep
  (0x6F47E9E0 or 0x6F47F1F0). Otherwise the sub-query is chosen by a flag in the stack
  arguments: flag zero gives 0x6F47EF40 or 0x6F47F7B0, non-zero gives 0x6F47EC80 or
  0x6F47F4C0. (The two sub-queries 0x6F47EC80 and 0x6F47F4C0 have the same gate code.)

### 1.2 Grid object (CONFIRMED structure, LIKELY identity)
- The grid is returned by 0x6F481420, which returns the pointer at [layer+0x234] (CONFIRMED
  read; the layer comes from an argument that varies by caller).
- Fields used by the sub-query (CONFIRMED from the code that reads them):
  - +0x28: cell array, one 4-byte head per cell. The low 24 bits are the index of the first
    entry in that cell. 0xFFFFFF means empty.
  - +0x3C: row stride. Cell index = stride * x + y.
  - +0x54, +0x58, +0x5C, +0x60: bounds clamp (xmin, ymin, xmax, ymax), in cells.
  - +0x68: scale float used when a box is converted to cells (the setup replicates it into
    four floats and multiplies a box by it).
  - +0x78: entry pool, 8 bytes per entry. Word 0: low 24 bits are the next entry index
    (0xFFFFFF ends the chain); high byte is the kind (0x01 is eligible, 0x02 is skipped).
    Word 1: pointer to the unit object.
  - +0xB4: stamp counter, incremented once per visited cell.
- Cell size is 32 world units. CONFIRMED: the centre conversion subtracts the origin and then
  subtracts 5 from the float exponent, which is an exact division by 32.
- World to cell: cell = (world - origin) / 32. The origin is at the world object: the x origin
  is at W+0x70 and the y origin at W+0x6C (CONFIRMED from the harness pairing, see section 2).

### 1.3 Visit order (CONFIRMED from the control flow of 0x6F47DA10)
- Bounds are the query box clipped to the grid bounds: x from max(boxXmin, G+0x54) to
  min(boxXmax, G+0x5C) exclusive; y from max(boxYmin, G+0x58) to min(boxYmax, G+0x60)
  exclusive. The loop is skipped when either range is empty.
- Outer loop over x ascending, inner loop over y ascending. Each cell index is stride * x + y.
- Each cell is visited by 0x6F47D8A0, which walks the cell's chain from its head.

### 1.4 Per-entry rules (CONFIRMED structure; some predicates UNSURE)
- Entry with kind 0x02 is skipped. Entry with kind 0x01 is considered. Other kinds are only
  stamped.
- Stamp dedupe: for a considered entry, the unit's stamp field (unit+0x38) is compared with
  the query's record stamp (the record's id, passed as the fourth argument of the cell routine) and with the
  current cell stamp. If either matches, the entry is skipped. This means a unit can enter
  the candidate list at most once per query, whichever cells it spans. CONFIRMED.
- Append test for a kind 0x01 entry: a helper 0x6F47D750 is ANDed with the record's 16-bit
  player mask (record+0x44), then ORed with bit 0x10000000 of the unit's +0x40 word. If the
  result is zero, the unit is appended to the record's candidate array (8-byte items, count at
  record+0x1C). The unit's stamp is set in both cases. The helper 0x6F47D750 is UNSURE.
- Consequence for the no-player case: the record mask is 0x14FD0000 (see section 2.4), so the
  bit-28 test is what excludes flagged units. UNSURE which unit flag bit 28 is.

### 1.5 Callback order (CONFIRMED)
- The query's candidate array is in the same order as the cell walk: cells in x-major order,
  then each cell's chain from its head.
- The circle test (section 2) and then the callback run in array order. So the order units
  enter the group is: cell order (x ascending, then y ascending), then chain order inside each
  cell. Chain order is the order entries were linked into the cell, and that linking code is
  NOT read yet (UNSURE; probably newest first, since a head pointer is the usual insert point).

### 1.6 Box construction (UNSURE)
- The box that feeds the cell walk comes from 0x6F47BA20 (floor of each of four floats, plus 1
  on the two maximum values) applied to a box produced by 0x6F47D5D0 (each float multiplied
  by the grid scale). The input box is probably the query centre plus or minus the radius,
  but the harness returned [0,0,1,1] for every input I tried, so the real input is not yet
  reproduced. The formula stays UNSURE. This is the next thing to pin down.

---

## 2. Exact range test (CONFIRMED by harness)

Source: the candidate loop of 0x6F47EC80 (and the same gate in 0x6F47F4C0). The harness calls
the real sub-query with the cell walk replaced by a supplied candidate list (see 4.1 for the
replacements). Results: 900 of 900 candidate decisions match the model below.

### 2.1 Operands
- D (squared distance) = 0x6F6EF3B0 applied to the candidate pair and the centre pair.
  - Candidate pair: unit+0x78 and unit+0x7C, read from the unit object reached through
    the candidate's first word and its +0x30 pointers (candidate is cells, not world units).
  - Centre pair: ((arg0 - W+0x6C) / 32, (arg1 - W+0x70) / 32), where arg0 and arg1 are the
    first two stack arguments of the sub-query. In the native these are the query's two
    coordinates; which of them is "x" is LIKELY but the pairing above is the verified part.
- V (threshold) = 0x6F6EEE20 (softfloat multiply) applied to s and s, where s = r / 32 and
  r is the radius argument. The division by 32 is an exact exponent change. CONFIRMED for
  r = 333.3, 1000.7 and 17.123 (bit-equal).
- Accept rule: accept if D <= V (inclusive). The x87 compare reads D and V and accepts when
  V >= D. CONFIRMED.
- Boundary check (world units, centre 0,0, radius 500): distance 500.0 accepted, 499 accepted,
  500.0001 rejected, 354-by-354 (distance 500.63) rejected. CONFIRMED.

### 2.2 Distance routine 0x6F6EF3B0 (CONFIRMED behaviour, not reproducible by composition)
- It computes a squared 2D distance of a pair difference (its own fused code, with its own
  rounding). Composing the public softfloat helpers, sub then mul then mul then add, matches it
  bit for bit in only 78 percent of 3000 random cases (differences of one or two ulp).
  Truncating the exact square sum matches 48 percent, and round-to-nearest matches less.
  CONFIRMED.
- Consequence: a bit-exact port must port 0x6F6EF3B0 itself, not the composition.

### 2.3 Other facts in the gate (CONFIRMED)
- Removed units (unit+0x30's removed flag at +0x20 non-zero) are skipped before the gate's
  mover call. A null mover pointer at +0x54 skips the unit too.
- After the gate: the mover's virtual slot at +0x1C is called, then predicate 0x6F471840 with
  the stack argument at [ebp+0x14], then the callback. Any zero return from the callback stops
  the walk.
- Dead (life 0) units are not excluded by the gate.

### 2.4 Mask in the record (CONFIRMED)
- The query stores the stack argument at [ebp+0x10] into record+0x44. For GroupEnumUnitsInRange
  the native passes 0x14FD0000 OR a 16-bit mask. The mask is ~(1 << player) & 0xFFFF for a
  player filter and 0 for "any", so the value stored is 0x14FD0000 with the mask bits added.
  The bit pattern 0x14FD0000 is a fixed constant (CONFIRMED value).

---

## 3. Counted limit (CONFIRMED inert for range queries; UNSURE for type and player)

### 3.1 Insert return values (CONFIRMED by harness)
- Harness setup: a CUnitSet built exactly as its constructor builds it (sentinel at set+8
  pointing to itself, head at set+0xC holding the bitwise NOT of the sentinel address, count
  zero), units with a valid vtable whose slot 7 returns the tag 0x2B773375.
- Results: first insert returns 1, the same unit again (checkDuplicates = 0) returns 0, a second
  distinct unit returns 1. Count after those three calls is 2. CONFIRMED.

### 3.2 Collector counter (CONFIRMED, asm at 0x6F3E7707 to 0x6F3E770B)
- The collector increments the enum counter when Insert returns 0, not when it returns 1.
  So the counter counts failed inserts (duplicates or NULL).
- The limit test: if the limit is non-zero and the counter is at least the limit, the query
  stops. CONFIRMED.

### 3.3 Can a duplicate reach the collector?
- Range and rect queries: NO. Section 1.4 shows a unit is appended at most once per query
  because of the stamp dedupe, and each collector call comes from one appended candidate.
  So Insert never returns 0, the counter stays at 0, and the limit never stops the walk.
  LIKELY-high from the logic; the end-to-end run of the native was not done (see section 6).
- Type and player enumerations (0x6F46D0B0 world walk): UNSURE. The walk visits a world list
  and then a second cell-based recursion. If the same unit can appear in both, duplicates would
  reach the collector and the counter would count them. Not resolved.
- Practical conclusion for a port: GroupEnumUnitsInRangeCounted and InRectCounted should not
  stop early, unless the type and player walks are shown to produce duplicates.

---

## 4. Method notes and limits of the harness runs

### 4.1 Replacements used (not in the original)
- 0x6F47DAD0 (candidate collection) replaced by a stub that uses a supplied candidate list.
- 0x6F481420 (grid accessor) replaced by a stub returning the test grid.
- 0x6F471840 (predicate) replaced by a stub returning 1.
- 0x6F46AC70 (free) and 0x6F46A950 (grow) replaced by no-ops.
- Callback replaced by a stub that records calls.
The distance, threshold and gate arithmetic and the loop itself are the original code.

### 4.2 Scripts (scratchpad/pass2/)
- range_gate.py, range_model.py: sub-query harness and the 900-case model check.
- range_thresh.py: threshold check against softfloat multiply.
- range_dist2.py, range_search.py, range_origin.py, range_pairs.py: operand pairing checks.
- range_box.py: box check (inconclusive, see 1.6).
- insert_check2.py, order_check.py: insert return values and order.
- dist_check.py, dist_check2.py: distance routine versus softfloat composition.

---

## 5. Insertion order for groups (CONFIRMED)

- The sorted branch of insert compares a new unit with each existing member of the same type
  id. Before comparing, it looks up the existing member's +0xC word through the handle table
  (0x6F03FA30, with the member's +0x10 as the second argument). For a unit, +0xC holds the
  agent kind tag 0x2B61676C. That value is far outside the handle-table size, so the lookup
  returns 0, the member is treated as invalid, and the insert appends at the tail.
- Result: group order equals insertion order for units. ForGroup order and FirstOfGroup order
  are therefore the order in which units were added to the group, and for enumerations that is
  the candidate order in section 1.5.
- Harness: two units of the same type inserted in order A then B always iterate A then B,
  including when their +0xC words differ (order_check.py). CONFIRMED.

---

## 6. Open items (for pass 3)

1. Box input for the cell walk (section 1.6): trace where the centre and radius enter the
   box and reproduce the [0,0,1,1] mismatch with real inputs.
2. Entry linking order inside a cell (section 1.5): who calls the linker that puts an entry at
   the head of a cell chain.
3. Append mask helper 0x6F47D750 and unit flag bit 0x10000000 (section 1.4).
4. End-to-end run of GroupEnumUnitsInRangeCounted with a limit on a real cell world (to confirm
   that the limit never stops it).
5. Type and player walk duplicates (section 3.3).
6. Whether the sub-query's mover call (+0x54, slot +0x1C) changes state that affects later
   candidates.

---

## 7. State events and player-unit observers

### 7.1 Unit state events (CONFIRMED structure, LIKELY mapping)
- TriggerRegisterUnitStateEvent (0x6F3D21B0): state index must be less than 4, and the raw
  operator value must be less than 0x80106 (signed compare). CONFIRMED.
- The helper 0x6F435C20 receives: the trigger as this, the unit, the state, the operator and
  the value. It stores the registration key at trigger+0x34 as operator + 0x80101 and the same
  key at trigger+0x48. CONFIRMED.
- State dispatch on the state value (CONFIRMED structure):
  - state 0 (life) picks comparator 0x6F0E1BA0; state 2 (mana) picks comparator 0x6F02DCC0.
  - Operator switch on key (operator + 0x80101): keys 0x80101 and 0x80102 call the comparator
    once; key 0x80103 calls it twice (with different arguments); keys 0x80104 and 0x80105
    call it once with a different argument shape; any other key skips to the notify.
  - states 1 and 3 (max life, max mana): the helper returns before registering and before the
    notify key. So these events register nothing. CONFIRMED. This matches unit-core.md, where
    SetUnitState ignores states 1 and 3.
- Notify key 0x8023B is fired on the unit at the end of registration for states 0 and 2,
  including the default operator branch (no comparator). CONFIRMED.
- Operator mapping: raw operator 0..4 gives keys 0x80101..0x80105. Operator 5 (NOT_EQUAL in
  my recollection of common.j) gives key 0x80106, which matches no case, so it notifies and
  registers no comparator. The JASS name-to-number map is not in the repo (common.j is not in
  the upload), so the mapping from LESS_THAN and friends to 0..5 is LIKELY, not confirmed.

### 7.2 Player-unit observer owner (CONFIRMED target, LIKELY firing player)
- TriggerRegisterPlayerUnitEvent (0x6F3D1DD0) calls 0x6F437000 with the trigger as this, the
  player object, the key (event + 0x80200) and the boolexpr. CONFIRMED.
- 0x6F437000 stores the key at trigger+0x34, stores the boolexpr in the trigger's filter slot
  at trigger+0x48 (through 0x6F3B55C0), then calls the PLAYER object's observer add (vtable slot 2
  with key, key, trigger). So the observer lives in the player's table, with the trigger as
  owner and the key as payload. CONFIRMED.
- Firing: the unit death sequence (0x6F41CA40) fires key 0x80214 through the event helper
  0x6F416BA0, which is a CPlayerWar3 event that pushes the player into slot 3 (GetTriggerPlayer)
  before running. So player-unit death is fired on a player object. LIKELY that it is the owner
  of the dying unit (the call that gets the player comes from the unit's own data; the exact
  source is UNSURE).
- Filter: the boolexpr at trigger+0x48 is evaluated when the event runs (LIKELY, from the layout;
  the evaluation site is not read).

---

## 8. Summary of confidence changes versus pass 1
| item | pass 1 | pass 2 |
|---|---|---|
| Group insertion order for units | LIKELY | CONFIRMED |
| Range test formula | UNSURE | CONFIRMED (inclusive circle, cell units, threshold s*s) |
| Distance routine | not known | CONFIRMED (fused, port the routine) |
| Counted limit, range and rect | LIKELY | CONFIRMED inert (logic plus insert harness), end-to-end run still LIKELY |
| Counted limit, type and player | not known | UNSURE |
| Spatial cell order | UNSURE | CONFIRMED (x-major, ascending), chain order UNSURE |
| Cell size and origin | not known | CONFIRMED 32 units, origin W+0x70 / W+0x6C |
| State-event mapping | UNSURE | LIKELY (states 1 and 3 register nothing: CONFIRMED) |
| Player-unit observer owner | UNSURE | CONFIRMED target is the player object, LIKELY firing player |


## Addendum (lead, 2026-10-11)
The claim in section 2.2 that composing the public helpers matches 0x6F6EF3B0 in only 78% of cases was an operand-order error: `sub(bx, ax)`, `mul`, `mul`, `add` matches the fused routine on all 12,000 recorded Unicorn cases (crates/softfloat `arith::dist2`, tools/emu/softfloat_vectors.py case `dist2`).
