# Pathing search and unit mover, pass 2 (Game.dll 1.26a, Warcraft III)

Scope: clean-room interoperability notes for a reimplementation. Plain English, no code blocks.
Addresses are VAs (image base 0x6F000000). Labels:
CONFIRMED = read directly in the disassembly (capstone, 32-bit) or in the file bytes.
LIKELY = strong inference from the disassembly and call shapes, not yet checked by emulation.
UNSURE = not established.

Previous pass: scratchpad/specs/pathing.md. This pass located the A* loop, the heap, the cost and
heuristic, the neighbour order, the corner rule, the search budget and the top-level request flow. The
per-step mover is only partly located. Unit-unit separation and path smoothing are NOT located.

Method: a direct-call graph of all 70,526 indexed functions was built with capstone (151,738 edges; indirect
calls are NOT in it, so vtable-dispatched calls are missed). Function bodies were read from the decompile
and then re-checked in the disassembly for every register-passed argument that matters.

---

## 1. The search system objects

- CLrPathingSys (vtable 0x6F95359C, constructor FUN_6F4A1B00, pool accessor FUN_6F482880). It is the
  per-game long-range system. CONFIRMED.
- CLrPath (vtable 0x6F95336C, constructor FUN_6F49CA10, pool accessor FUN_6F48A990). One request and its
  state. CONFIRMED.
- The open list is a heap object embedded at CLrPathingSys+0x70 (its element array pointer at +0x7C, its
  size counter at +0x8C, its capacity fields at +0x14 and +0x18). CONFIRMED from the thiscall in the search
  loop (ECX = system + 0x70 for the pop).
- The node array is a dynamic array of 36-byte records (stride 0x24) whose pointer is at system+0x5C.
  CONFIRMED from the strides in the loop and in the relax routine.
- The path waypoint list is an array of 8-byte point records (x, y, both 32-bit). CONFIRMED (append and erase
  helpers FUN_6F498770 and FUN_6F4987F0 use stride 8).
- The destructor body is FUN_6F4A1CF0. It is the only place that sets the vtable of the embedded heap
  to CPriorityQ<CLrOpenNode>. CONFIRMED.

### 1.1 Node record (36 bytes, one per region seen by the current search). CONFIRMED offsets, LIKELY meanings

| Offset | Meaning (LIKELY unless noted) |
|---|---|
| +0x00 | seed cell x (node position, used in distance math; CONFIRMED that relax reads it) |
| +0x04 | seed cell y (CONFIRMED read) |
| +0x08 | stamp. Incremented on every push and on every close. An open-list entry is valid only if its stored stamp equals this field. CONFIRMED |
| +0x0C | closed-list back link, or -2 (0xFFFFFFFE) while in the open list, or -1 if never touched. CONFIRMED values |
| +0x10 | closed-list forward link, -1 at push. CONFIRMED values |
| +0x14 | g cost (accumulated path cost). CONFIRMED that relax stores the new g here |
| +0x18 | h cost (heuristic to goal). CONFIRMED that relax stores it here |
| +0x1C | parent node index (the node we came from). CONFIRMED store |
| +0x20 | byte flag copied to the expansion routine as its fifth argument (LIKELY: per-region attribute) |
| +0x22 | byte: if nonzero, the expansion routine FUN_6F499FA0 is used instead of FUN_6F499E90 (LIKELY: a second region kind) |
| +0x23 | byte cleared on relax (LIKELY: "closed or reopened" marker) |

### 1.2 System fields used by the search (CONFIRMED offsets, LIKELY meanings)

| Offset | Meaning |
|---|---|
| +0x1C, +0x20, +0x24, +0x28 | four pyramid level maps (level 0 = finest). Each has width at +0x3C, height at +0x40, cell array at +0x28. CONFIRMED |
| +0x2C | search generation stamp (incremented once per request in the start-setup routine). CONFIRMED |
| +0x40 | (on the object used by the region-id helper FUN_6F4A1800, not verified to be this system) region count, capped at 0x7FFF. CONFIRMED check, owner UNSURE |
| +0x5C | node array pointer (see 1.1) |
| +0x6C | next-region counter used by lazy labelling (see section 3). CONFIRMED read |
| +0x70 | open-list heap object (see above) |
| +0x8C | open-list size (sentinel included) |
| +0x90 | level shift: 1 << (level class >> 1), so 1 (class 0 or 1) or 2 (class 2 or 3). CONFIRMED value set, see 4.2 |
| +0x94 | closed-list head (node index, -3 when empty). CONFIRMED |
| +0x98 | expansion budget for this call (set from the caller's limit). CONFIRMED |
| +0x9C | expansions done so far in this call. CONFIRMED |
| +0xA4, +0xA8 | start point (float x, float y, copied from the request) CONFIRMED copy |
| +0xAC, +0xB0 | second point (float x, float y) CONFIRMED copy |
| +0xB4, +0xB8 | start cell (x, then y after conversion; the order is UNSURE) |
| +0xBC, +0xC0 | goal cell x and y (used in every heuristic). CONFIRMED that these are the goal coordinates |
| +0xC4 | start region id. CONFIRMED |
| +0xC8 | goal region id (or -1 if the goal is blocked). CONFIRMED |
| +0xCC | best squared distance seen to the goal (for the closest-node fallback). CONFIRMED |
| +0xD0 | node index of that closest node, or the start region in the cross-region case. CONFIRMED |
| +0xD4 | path type 0,2,4,6 (see 4.1). CONFIRMED |
| +0xD8 | flag byte from the request (a debug or fallback switch; UNSURE) |

### 1.3 CLrPath fields used (CONFIRMED offsets and constants)

- +0x54: embedded sub-structure passed to the dispatcher as the "state" object (its +0x1C is cleared first).
- +0x60: pointer to the request's start point (two floats).
- +0x68 and +0x48: initial 128 (0x80). CONFIRMED
- +0x84 = 700 and +0x86 = 400 (16-bit). CONFIRMED initial values. +0x86 is the expansion budget passed into
  the search (see 5.2). LIKELY.
- +0x88 = flags. CONFIRMED bits used: 0x200000 (run the region search rather than a direct move), 0x2000000
  (a precomputed region bound exists), 0x1000000 (set when the large-unit flag applies), 0x20000000 (the
  destination was adjusted), 0x100000 (the step may skip movement this tick), and bits 30 and 31 (the path
  type, see 4.1).
- +0x80: cleared to zero when the path length is under 32 units (0x20). CONFIRMED.

---

## 2. Grid and the level pyramid (CONFIRMED structure)

- The level-0 grid is the cell grid of 32 world units per cell (already CONFIRMED in pathing.md).
- Above it there are three coarser level maps. A lookup at level L uses the coordinates shifted right by L
  (x >> L, y >> L). CONFIRMED from the shift in the lookup routine (FUN_6F498A60).
- Each level cell is an 8-byte record. Word 0 is the stamp (see 3). Word 1 holds, for each path type, a
  2-bit state in the top bits, and the low 16 bits hold the region id.
- The 2-bit state for path type T is read as (word1 << T) and the top two bits (mask 0xC0000000).
  Type T is 0, 2, 4 or 6, so the four path types use 2-bit fields at bits (30,31), (28,29), (26,27) and
  (24,25). CONFIRMED shift and mask. LIKELY that the field means "per movement class".
- State values (CONFIRMED from the lookup routine): 0x40000000 means open at this level (return its region
  id). 0x80000000 means blocked (return -1). Any other value means mixed, so the lookup walks up to coarser
  levels 1 to 3 until it finds a non-mixed record, then returns that record's region id (LIKELY: a coarse
  block is open or blocked for the whole cell).

---

## 3. Lazy region labelling (partly CONFIRMED, flood-fill details UNSURE)

- A lookup whose record stamp differs from the system generation (+0x2C) is "stale". The routine then writes
  the current generation and the next-region counter (+0x6C) into the record and appends the seed cell to a
  global work list (CONFIRMED stores; the list growth helper is FUN_6F4984E0, stride 0x24 CONFIRMED).
- A second labelling helper (FUN_6F4A1800) keeps a per-cell region id in the cell word: the low 24 bits of a
  cell word point to an occupant record of 8 bytes at system+0x78, whose top byte tag 0x02 marks a region
  record, and the 16-bit field at +6 holds the region id. Records are keyed by a 16-bit layer key at
  system+0x20. CONFIRMED layout, LIKELY purpose.
- The actual flood fill that spreads a region id over neighbouring cells was NOT located (UNSURE). Expected
  to run from the work list. This is the main open item for exact region shapes.

---

## 4. Request flow

### 4.1 Entry and path type

- FUN_6F49B9C0 is the request driver (thiscall on CLrPathingSys, path object as the argument). It calls the
  dispatcher FUN_6F49A800. CONFIRMED.
- The path type passed down is (flags >> 30) * 2, so 0, 2, 4 or 6. CONFIRMED from the disassembly-level call
  shape (decompile and index agree).
- The caller FUN_6F49DAC0 (the per-step update of a CLrPath) runs the driver when flag 0x200000 is set on the
  path. Otherwise it appends a straight waypoint with FUN_6F498770. CONFIRMED branch.

### 4.2 Size class to level (CONFIRMED function, constants partly confirmed)

- FUN_6F38ADA0 maps a float (a unit size or radius) to a class 0 to 3 with three thresholds:
  below 0.5 (0x6FAAE4D4 = 0.5, CONFIRMED) gives 0; below the value at 0x6FAAE4F4 (the value is not recovered,
  UNSURE; it is probably 1.0) gives 1; below 1.5 (0x6FAAE500 = 1.5, CONFIRMED) gives 2; otherwise 3.
- The driver passes (class >> 1) as the level byte. So the system level shift at +0x90 is 1 for classes 0 and 1,
  and 2 for classes 2 and 3. LIKELY. The value 2 switches on the extra footprint tests in section 5.4.

### 4.3 Dispatcher (FUN_6F49A800, CONFIRMED structure)

1. Clear the previous waypoint list of the path if it has one.
2. Run the start-setup routine (FUN_6F49A640). Its result:
   - 0: start or goal cell is out of bounds or blocked (failure, goes to step 6).
   - 1: start and goal are the same cell, or they are in the same region. Append a straight two-point path
     (FUN_6F49A5E0). Return success.
   - 2: different regions. Run the region search (step 3).
3. Region search: FUN_6F49A240 stores an h value for the start node (an integer square root call, argument
   not recovered from the decompile, LIKELY the start-to-goal distance), pushes the start node, then calls the
   search loop FUN_6F49A170. If it returns a goal, FUN_6F49A470 walks the parent chain
   from the goal back to the start and appends waypoints. Return success.
4. If the search does not reach the goal, but the closest node (system+0xD0) differs from the start, the
   path is extended to that closest node (partial path) and the call returns "not reached". LIKELY.
5. If the closest node is the start, a straight line to the target is appended and the call returns "not
   reached".
6. Failure: straight line, return 0.

### 4.4 Start setup (FUN_6F49A640, CONFIRMED values)

- It clears the scratch (FUN_6F49A3F0), stores the level shift (1 << level), copies both points, converts
  each float point to a cell with floor then truncate (the same pattern as the 1/32 conversion in
  pathing.md), and stores the cells.
- It bumps the generation stamp (+0x2C) when start and goal differ, looks up both regions, and in the
  cross-region case stores the squared cell distance from start to goal at +0xCC and the start region at
  +0xD0. It also copies the budget into +0x98.

---

## 5. The A* loop (CONFIRMED logic, from the disassembly)

### 5.1 Costs and heuristic

- All costs are integers. The square root is an integer routine FUN_6F4BABB0 (Newton iteration, see 5.1a),
  NOT a float sqrt. CONFIRMED.
- Step cost from the parent node P to the neighbour node N: g(N) = g(P) + isqrt(576 * D), where
  D = (P.x - N.x)^2 + (P.y - N.y)^2 using the seed coordinates of the two nodes. CONFIRMED: the 576 is built as
  (24*dx)^2 + (24*dy)^2, so the scale factor is 24 on each axis. So the step cost is floor(24 * Euclidean
  distance between seeds). CONFIRMED.
- Heuristic: h(N) = isqrt(576 * ((N.x - goalX)^2 + (N.y - goalY)^2)), i.e. floor(24 * Euclidean distance to the
  goal cell). CONFIRMED.
- Open-list key: f = g + h. The key is an unsigned 32-bit number. CONFIRMED (the push routine is called with
  h + g and the comparisons are unsigned).
- Diagonal cost: the same formula as for orthogonal moves, because the cost uses the seed distance and not the
  step size. So there is NO separate diagonal weight. CONFIRMED (no separate weight in the relax routine).
- Relaxation only accepts a strictly smaller g (a "jae" compare). CONFIRMED.

5.1a Integer square root (CONFIRMED, from the disassembly of FUN_6F4BABB0, argument in ECX, result in EAX):
- Initial guess u0: for n up to 0xFF it is n/12 + 1; for n up to 0xFFFF it is n/200 + 0x15; above that it
  is ((n - hi) >> 1 + hi) >> 14 + 0x1BC, where hi = high 32 bits of n * 0x39ACC69D.
- Iteration: q = n / u (unsigned); d = u - q (signed); next u = (q + u) / 2 (signed, truncating); loop while
  d / 2 (truncating) is not zero. The return value is the last computed u. This is an APPROXIMATE floor
  square root, not exact. A port must reproduce the stopping rule exactly. CONFIRMED logic, LIKELY that it is
  exact to the bit for all inputs (not tested).

### 5.2 Budget and cap (CONFIRMED wiring)

- The expansion budget is the path's 16-bit field at +0x86, which is 400 by default. The dispatcher passes it
  down, and the start-setup routine stores it at system+0x98 in the cross-region case (CONFIRMED store site).
  The counter at +0x9C is incremented once per loop iteration, before the stale check. The loop stops when the
  counter reaches the budget. CONFIRMED. The reset of +0x9C at the start of each call was not checked (LIKELY).
- The loop also stops when the open-list size is 1 or less (empty). CONFIRMED.
- Stop on goal: the loop returns the node index as soon as a valid (non-stale) pop equals the goal region
  id (+0xC8). CONFIRMED.

### 5.3 Open list (CONFIRMED)

- Binary min-heap, 1-based array with a sentinel at index 0. Elements are 12 bytes: key (unsigned 32-bit),
  node index, stamp. The root is index 1. The parent of k is k >> 1, the children are 2k and 2k+1.
  CONFIRMED.
- Push: append at the end, then move parent elements down while parent.key >= new key (so an equal key
  moves the parent down, which means a newer equal-key element sits above older ones). CONFIRMED.
- Pop: take the root; take the last element; sift down from the root. At each level choose the right child
  when right.key <= left.key (ties go RIGHT), but only if the right child exists. Stop when last.key <= the
  chosen child's key (ties stop the sift, so the last element stays). CONFIRMED.
- Stale entries: an entry whose stored stamp differs from the node's current stamp is skipped. CONFIRMED.
- Closed-list bookkeeping: closing a node moves it onto a doubly linked list headed at +0x94. CONFIRMED.

### 5.4 Neighbour expansion (CONFIRMED order and corner rule)

Expansion of the node being popped uses its seed (x, y). The routine that expands a normal node (FUN_6F499E90)
probes the eight cells around the seed. Each probe calls the lookup for the neighbour cell's region, and if the
lookup gives a region, relaxes that region with the node as parent.

Order of probes, which is also the order of pushes:
1. North: cell (x, y-1)  (routine FUN_6F4995E0)
2. East: cell (x+1, y)   (routine FUN_6F499660)
3. South: cell (x, y+1)  (routine FUN_6F4996C0)
4. West: cell (x-1, y)   (routine FUN_6F499720)
5. Northeast: cell (x+1, y-1) (FUN_6F4997A0), only if North and East are both passable
6. Southeast: cell (x+1, y+1) (FUN_6F4997F0), only if East and South are both passable
7. Southwest: cell (x-1, y+1) (FUN_6F499840), only if South and West are both passable
8. Northwest: cell (x-1, y-1) (FUN_6F499890), only if West and North are both passable

- Corner rule: a diagonal is taken only when both orthogonal cells it cuts past are passable. CONFIRMED from
  the control flow (the branches test the four orthogonal results before each diagonal).
- The orthogonal probes relax their neighbour even when a later diagonal is not allowed. CONFIRMED.
- "Passable" for a probe means the lookup returned a region (not -1), CONFIRMED.
- At level shift 2 (large units, section 4.2) each orthogonal probe also calls one extra check helper:
  FUN_6F498E80 (north probe), FUN_6F498ED0 (east), FUN_6F498F30 (south), FUN_6F498F90 (west). The west probe
  additionally checks the x parity bit and the node's +0x22 byte, and calls FUN_6F499050 in that case. CONFIRMED
  that these calls exist and what they test (one neighbouring cell's 2-bit state, plus a parity test). The
  exact footprint shape they encode is UNSURE.
- At level shift 2, the diagonal probes also call a check helper: NE calls FUN_6F4990C0, SE calls FUN_6F499110,
  SW calls FUN_6F499170, NW has none. CONFIRMED call, details UNSURE.
- Nodes with +0x22 nonzero are expanded by FUN_6F499FA0 instead (a second neighbour rule, details UNSURE).

Because the node is a region and its neighbours are read from the seed's eight neighbours, the search is a
graph search over regions whose edges come from adjacency of their seed cells. It is NOT a plain cell-by-cell
A*. LIKELY, from the structure, but the seed choice is the first cell that created the region (LIKELY).

### 5.5 Path extraction

- FUN_6F49A470 walks the parent chain from the goal node back to the start (following +0x1C until -1).
  CONFIRMED loop. It appends one point per node to the path list with FUN_6F498770. CONFIRMED that the output
  is a list of 8-byte point records. The final write to the path's two end points is CONFIRMED to exist
  (two stores after the loop) but their meaning is UNSURE.
- Waypoint coordinates are seed cells converted with the integer-to-float helper and offset by a constant
  (0x6FAAE4B8 = 0.25, CONFIRMED value). The exact offset formula is UNSURE (cell centre or cell corner).
- No path smoothing or string-pulling step was located. The per-step consumer (section 6) pops waypoints.
  Smoothing is UNSURE (probably none in the search itself).

---

## 6. Per-step mover (partly located)

### 6.1 Call chain found (CONFIRMED edges, LIKELY roles)

- FUN_6F493410 calls FUN_6F490FC0 (a batch over point records). CONFIRMED edge.
- FUN_6F490FC0 walks an array of 44-byte records (stride 0x2C; count at its +0x38, array pointer at its +0x28).
  Each record's flag word at +0x28 bit 0x200000 means "skip" and bit 0x100000 means "moving". The moving bit
  is set when the record's speed field (+0x20) is greater than 0.01 (constant 0x6FAAE480 = 0.01, CONFIRMED
  value). LIKELY that this is the per-tick speed threshold. Records are processed in groups of four (CONFIRMED
  unrolled loop) and then singly.
- FUN_6F490FC0 calls FUN_6F4907F0 once per record. CONFIRMED edge.
- FUN_6F4907F0 (the per-record update, LIKELY the point motion step):
  - it reads a target vector from the record and computes its length with the softfloat square root
    (FUN_6F6EF750, CONFIRMED edge), using softfloat sub, add and mul;
  - it normalises the heading and sets the record's heading or speed fields;
  - it calls FUN_6F495320 for the step itself. After that call it clamps the record's speed to the owner
    object's maximum at owner+0x88 (CONFIRMED clamp: if the max is less than the speed, the speed is set to
    the max). CONFIRMED edges and clamp; the owner is reached through the record's pointer at +0x14.
- FUN_6F495320 (LIKELY the single step along the path): it asks the path system for the next step with
  FUN_6F49E020, which calls FUN_6F49DAC0 (the CLrPath step) and FUN_6F49DF00 as the fallback. CONFIRMED edges.
  It then integrates the position with FUN_6F493FA0 (UNSURE details).
- FUN_6F49E020: if flag 0x100000 on the path is clear, it counts down a wait counter at +0x94 (returns 1
  while it is nonzero), otherwise it runs the step. CONFIRMED.

### 6.2 Constants and rules found

- Movement tick: the 0.1-second step constant is 0x6FAAE4A0 = 0.1 (CONFIRMED value). It is used in 68 functions
  (pathing.md). The mover's use of it is LIKELY per tick.
- Turn rate: the setter FUN_6F474B30 clamps the turn value from below at 0.001 (0x6FAAE47C, CONFIRMED value).
  The per-tick turn update was NOT located (UNSURE).
- Propulsion window: setter FUN_6F474BA0 stores without a clamp (CONFIRMED). Its per-tick use is UNSURE.
- Flying height default 0.01 (0x6FAAE480, CONFIRMED), used by the setter as a minimum.
- Order dispatch (CAbilityMove slot 3, FUN_6F206EE0): command 0xD0012 goes to FUN_6F203D60 with flags
  (0,0), and commands 0xD000F and 0xD0014 go to FUN_6F203D60 with (1,0). CONFIRMED mapping. LIKELY: 0xD0012 is
  the move order, 0xD000F attack and 0xD0014 patrol, matching the standard WC3 order ids.
- The CUnit virtual at slot 3 (FUN_6F2A7E60, 616 bytes) and slot 15 (FUN_6F290850, 3886 bytes) are the largest
  unit virtuals. They are NOT yet checked as the mover (UNSURE).

### 6.3 Not located

- The per-tick code that changes the unit's facing toward its heading with the turn rate.
- The acceleration rule and the way speed reaches the maximum.
- Collision and push-apart between units. CPoSeparate, CProximityMap, CPoPosCl and the cluster classes exist,
  and their vtables are listed in pathing.md. No separation update was found. The batch in 6.1 contains
  pairwise checks that may belong to this (UNSURE).

---

## 7. Determinism summary (what a port must match)

1. Node cost: floor(24 * distance) via the approximate integer square root (section 5.1a).
2. Keys: unsigned f = g + h. Ties broken by the exact heap rules of section 5.3.
3. Expansion order: N, E, S, W, then NE, SE, SW, NW, with the corner rule (section 5.4).
4. Budget: 400 pops per call by default (the path's +0x86 value), with the counter reset at the start of a call
   (LIKELY; the reset in the start-setup routine was not checked in full).
5. Unreached goal: the best node is the one with the smallest squared cell distance to the goal (ties: the
   first one found, since the comparison is strict). CONFIRMED.
6. Path output: the list of seed points from the goal back to the start, reversed for use (the reversal was
   not located; UNSURE).

---

## 8. Open items, in order

1. Find the flood fill that assigns region ids from the work list (section 3). This decides the region
   shapes and so the path shapes.
2. Find the code that copies the start point into the first node (the start-setup routine's full body), and
   confirm the start cell order (+0xB4 versus +0xB8).
3. Read FUN_6F493FA0, FUN_6F493EA0, FUN_6F493E80 and FUN_6F493D90 for the per-step integration.
4. Find the per-tick facing update and the acceleration rule (turn and prop window).
5. Find the separation or push-apart step (CPoSeparate, CProximityMap). Start from the pairwise checks in
   FUN_6F490FC0.
6. Read FUN_6F499FA0, FUN_6F4998C0 and the level-2 footprint helpers (FUN_6F499110, FUN_6F499170,
   FUN_6F4990C0, FUN_6F499050, FUN_6F498F30, FUN_6F498F90, FUN_6F498ED0) to pin the large-unit rule.
7. Confirm the value of the constant at 0x6FAAE4F4 (used as the level-1 threshold and as the segment step in
   the segment walk FUN_6F49BD10; probably 1.0).

Key addresses, quick reference:
- Search loop FUN_6F49A170 (node pop, stale check, goal check, expansion, close).
- Heap pop FUN_6F4985F0, heap push FUN_6F498580, heap grow FUN_6F4865B0, heap erase FUN_6F498060.
- Neighbour expansion FUN_6F499E90 (default), FUN_6F499FA0 (second kind).
- Orthogonal probes FUN_6F4995E0, FUN_6F499660, FUN_6F4996C0, FUN_6F499720; diagonal probes FUN_6F4997A0,
  FUN_6F4997F0, FUN_6F499840, FUN_6F499890.
- Relax FUN_6F498C70 (g, h, parent, push); push-node FUN_6F498B80; close FUN_6F498BD0; unlink FUN_6F498C20.
- Integer square root FUN_6F4BABB0.
- Level lookup FUN_6F498A60 (4 levels); lazy label FUN_6F498A00; region id per cell FUN_6F4A1800.
- Dispatcher FUN_6F49A800; start setup FUN_6F49A640; region search start FUN_6F49A240; path extraction
  FUN_6F49A470; request driver FUN_6F49B9C0; per-step CLrPath update FUN_6F49DAC0.
- Size class FUN_6F38ADA0.
- Segment (ray) walk FUN_6F49BD10 with its cell predicates FUN_6F4A1230, FUN_6F4A1350, FUN_6F4A14D0,
  FUN_6F4A1660.
- Per-step mover FUN_6F490FC0 -> FUN_6F4907F0 -> FUN_6F495320 -> FUN_6F49E020 -> FUN_6F49DAC0.
- CAbilityMove dispatcher FUN_6F206EE0 (CAbilityMove vtable 0x6F92A054, slot 3).
