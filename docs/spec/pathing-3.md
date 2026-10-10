# Pathing regions: how they are labelled, shaped, invalidated (Game.dll 1.26a)

Scope: clean-room interoperability notes for a reimplementation. Plain English, no code.
Addresses are VAs in Game.dll (image base 0x6F000000). Labels:
CONFIRMED = read in the disassembly (capstone, 32-bit x86) and/or reproduced by Unicorn emulation (section 12).
LIKELY = strong inference from the code shape, not emulated. UNSURE = not established.

Read first: pathing.md and pathing-2.md. This file corrects several claims in pathing-2.md (listed in section 13).

---

## 0. Summary of findings

1. There is NO flood fill and NO connected-component labelling in the pyramid path. Region ids are assigned
   lazily, one pyramid record at a time, the first time the A* search asks about a cell (CONFIRMED).
2. A region is an aligned square block of 1, 2, 4 or 8 cells on a side (level 0 to level 3). It is not a
   connected component: passable cells on opposite sides of a full wall share one id (CONFIRMED, emulated).
3. The region id is the index of the entry appended to the search's node list at the moment of labelling.
   The node list and the region list are the same array (CONFIRMED).
4. The list is cleared at the start of every path request, and the record stamps are invalidated by bumping a
   generation counter whenever start and goal cells differ. Region ids therefore restart at 0 per request (CONFIRMED).
5. The pyramid states are not updated in place by any code found in the 0x6F49xxxx to 0x6F4Axxxx range. They
   are read in from stored data when the pathing data is loaded (CONFIRMED for the writers found; LIKELY that
   building and tree changes never touch these records).
6. The lookup returns the id of the record ONE LEVEL BELOW the first non-mixed coarser record, not of that
   coarser record (CONFIRMED, emulated). The result is that a uniform coarse block does not return "blocked"
   for its cells when the cell itself is mixed.
7. The lookup's blocked codes are: -1 for state 01 at any level, -1 for state 10 at level 0, -2 for state 10 at
   level greater than 0 (CONFIRMED, emulated). The block-level scanners use -2 to request a finer look.
8. Region ids are stored in 16 bits and there is no cap in the pyramid path, so more than 65536 labels in one
   request wrap (LIKELY). The second region system (0x6F4A1800) caps at 0x7FFF labels (CONFIRMED).

---

## 1. Objects and offsets

### 1.1 Pyramid/search object (called S here)

S is the pathing search object. The lookup routine 0x6F498A60 takes it as ECX (CONFIRMED: the probe wrapper
0x6F4995E0 loads its own ECX into the lookup call). Offsets used by the region code (CONFIRMED unless marked):

| Offset | Meaning |
|---|---|
| +0x1C, +0x20, +0x24, +0x28 | pointers to pyramid levels 0, 1, 2, 3 (level 0 = finest) |
| +0x2C | generation stamp (32-bit). Bumped by the request setup (section 7) |
| +0x3C | pointer to a 12-byte link table (LIKELY, used by the cell expander when its flag is set) |
| +0x50 | start of the region/node list struct W (see below) |
| +0x5C | W base pointer = node array base. The A* loop reads this same field as its node array |
| +0x64 | W grow step (count of entries to add at minimum) |
| +0x68 | W capacity (entries) |
| +0x6C | W count = number of labelled regions = number of nodes in this request. Also the "next region id" |
| +0x8C, +0x98, +0x9C | heap size, expansion budget, pop counter (pathing-2). +0xA0 is reset to 0 per request (meaning UNSURE) |
| +0x90 | mode: 1 << (the size class argument). Value 2 = two-cell footprint mode (LIKELY) |
| +0xB4, +0xB8 | start cell x, y (CONFIRMED from the request setup) |
| +0xBC, +0xC0 | goal cell x, y (CONFIRMED) |
| +0xC4 | start region id |
| +0xC8 (= +200) | goal region id (-1 if the goal is blocked) |
| +0xCC, +0xD0 | squared distance start to goal, and the start region in the cross-region case |
| +0xD4 | path type T. Used as a bit shift on the level records (section 2) |
| +0xD8 | flag: when nonzero, the labeller copies the record's flag byte into the node |

The W struct (CONFIRMED): the labeller calls the append helper with ECX = S + 0x50, so W base is at S+0x5C,
grow at S+0x64, capacity at S+0x68, count at S+0x6C.

### 1.2 Level object (one per pyramid level, CONFIRMED)

| Offset | Meaning |
|---|---|
| +0x28 | cell record array base |
| +0x3C | width in cells (x extent). Index = width * y + x |
| +0x40 | height in cells (y extent) |
| +0x30 | width * height (count) |

Level objects are built by 0x6F4A03F0 (writes +0x1C, +0x28, +0x3C, +0x40). Its callers include 0x6F4A0440 (LIKELY
the level constructor). Level dimensions for levels 1 to 3 are not confirmed; the test harness used halved
sizes (UNSURE whether the real game uses ceiling or floor).

### 1.3 Cell record (8 bytes, CONFIRMED)

| Bytes | Meaning |
|---|---|
| +0 (32-bit) | stamp. Equal to S.gen means "labelled in this generation". Init value 0 |
| +4 (16-bit, low half of word 1) | region id (valid only when the stamp matches) |
| +6 (16-bit, high half of word 1) | per-path-type state field area (bits 16 to 31 of word 1) |

The 2-bit state for path type T is the pair of bits at positions (30-T, 31-T) of word 1. The lookup computes
(word1 << T) and tests bits 30 and 31 (mask 0xC0000000). T is S+0xD4 and is even (0, 2, ...). The 16-bit
high half therefore holds 2-bit fields for up to eight type slots (LIKELY the game uses slots 0 to 3 for
T = 0, 2, 4, 6, as pathing-2 says; the code itself allows T up to 14, CONFIRMED by the mask arithmetic).

The byte at +6 is the low byte of the high half. The labeller copies it into the node flag when S+0xD8 != 0
(CONFIRMED).

---

## 2. The lookup routine 0x6F498A60 (CONFIRMED)

Signature in plain terms: lookup(S, level L, x, y) returning a signed 32-bit value. The cell probe 0x6F4995E0
and the request setup 0x6F49A640 pass L = 0 (CONFIRMED). The block scanners 0x6F4998F0 and 0x6F499CF0 pass L = k
and decrement the level after a -2 (CONFIRMED).

Algorithm, in order:

1. Shift x and y right by L. If the shifted x is >= width of level L, or shifted y >= height of level L, or the
   record pointer is null, return -1.
2. Read the state of record R_L = level L record at (x>>L, y>>L):
   - state 01 (word1 << T has top bits 0x40000000): return -1 (no labelling, no walk). CONFIRMED, emulated.
   - state 10 (0x80000000): return -1 if L == 0, else return -2 (0xFFFFFFFE). CONFIRMED, emulated.
   - state 00 or 11: continue to step 3.
3. Walk upward. For k = L+1, L+2, ... up to 3, take R_k = level k record at (x>>k, y>>k).
   - If R_k's state (same shift, same mask) is nonzero (01, 10 or 11), stop the walk. The record to use is
     R_(k-1), the record just below the stop (for the first step, that is R_L itself). CONFIRMED, emulated.
   - If the walk reaches level 3 without a stop, use R_3. CONFIRMED, emulated.
4. If the chosen record's stamp differs from S.gen, label it (section 3). CONFIRMED.
5. Return the chosen record's region id as an unsigned 16-bit value (zero-extended). CONFIRMED.

Consequence (CONFIRMED by the code and by emulation): a level-0 query of a mixed cell that sits inside a
uniform coarse block returns the id of the mixed record below the uniform block. The uniform block's own state
is never returned. Correctness of "blocked" therefore depends on the stored data (section 9, item U1).

Return-value map for the scanners (CONFIRMED from the callers): -1 = blocked and stop; -2 = blocked at a coarse
level, descend one level and test again; 0 or more = a region id, usable as a node.

---

## 3. Lazy labelling 0x6F498A00 and the append helpers (CONFIRMED)

The labeller is called only from the lookup, when the chosen record's stamp differs from S.gen. It:

1. Writes the record stamp := S.gen (store at 0x6F498A07).
2. Writes the record's low 16 bits := S.count (store at 0x6F498A0D). The stored value is the count BEFORE the
   append. The first label in a request gets id 0.
3. Fills a 36-byte STAGING buffer at the global 0x6FA84FD0:
   - +0 := x (the query cell x at full resolution, NOT the block origin)
   - +4 := y (the query cell y)
   - +0x20 := the record's byte at +6 when S+0xD8 != 0, else 0
   - +0x22 := the level of the labelled record (the k-1 from section 2, or 3 at the top)
   The staging buffer's other bytes are not written by the labeller. They keep whatever value was there before.
4. Appends one copy of the staging entry to W via the append helper (0x6F4984E0 with ECX = S+0x50, count 1).
   The copy routine 0x6F498010 copies 36 bytes per entry.

Append helper rules (CONFIRMED):
- Count 0 requests are a no-op that return success.
- If count + n <= capacity, copy into the existing array.
- Otherwise the new capacity = old capacity + max(grow, count + n - capacity), reallocated through 0x6F4B39C0.
  If grow is 0, the helper fails and returns 0 without changing the count.
- The labeller does not check the helper's result (LIKELY from the decompile, the tail of 0x6F498A00). A
  failed append leaves the record stamped with an id that the next label will reuse.

Label record summary: an entry is (seed x, seed y, flag byte at +0x20, level byte at +0x22). The seed is the
query cell, so it is the cell that first touched the record, not the aligned block's corner (CONFIRMED,
emulated: the entry for a level-1 record labelled from query (2,0) stores x=2, y=0, level=1).

---

## 4. Write set of the pyramid arrays (CONFIRMED scan)

Scan method: every instruction of every indexed function was decoded (2,145,516 instructions, function by function
from the index). Stores were matched against the record offsets (+0, +4, +6) and the S and W offsets listed
in section 1. Results:

| Address | Function | Store | Effect |
|---|---|---|---|
| 0x6F4989D0 | 0x6F498960 (level init) | dword [record] := 0 | stamp := 0 (once per level load) |
| 0x6F4989E1 | 0x6F498960 | dword [record+4] := (16-bit stream value << 16) | state field loaded, region id := 0 |
| 0x6F498A07 | 0x6F498A00 (labeller) | dword [record] := S.gen | stamp set |
| 0x6F498A0D | 0x6F498A00 | word [record+4] := S.count | region id set |
| 0x6F498A15 .. 0x6F498A46 | 0x6F498A00 | staging stores, then call to append with ECX = S+0x50 | node append |
| 0x6F4984E0 | append helper | count and capacity updates (S+0x5C..0x6C) | region list growth |
| 0x6F498010 | copy helper | 36-byte entry copies | region list fill |
| 0x6F498710 | erase helper | removes entries from the front of W | used at request start (section 7) |
| 0x6F4972xx, 0x6F497080 | W init / grow helpers | base, grow, capacity, count | W lifecycle (LIKELY constructor) |
| 0x6F49A3F0 | request reset | calls the erase helper with (0, count) | region list cleared |
| 0x6F49A640 | request setup | dword [S+0x2C] += 1 (at 0x6F49A75B) | generation bump |
| 0x6F4A03F0 | level constructor | +0x1C, +0x28, +0x3C, +0x40 | level dims and base (LIKELY) |

Pyramid record stores not found in the 0x6F49xxxx to 0x6F4Axxxx code outside the rows above: no in-place state
update (no store into record+6 or a read-modify-write of record+4 from a region-update routine). Apart from the labeller's
own store at 0x6F498A0D, the 16-bit stores into +4 and +6 in that range belong to the second region system (section 10).

Read-only consumers of the records (for completeness): 0x6F498890 (passable test), 0x6F498E80, 0x6F498ED0,
0x6F498F30, 0x6F498F90, 0x6F498FE0, 0x6F499050, 0x6F4990C0, 0x6F499110 through 0x6F499590 (neighbour tests),
and 0x6F4988F0 (save: reads the high halfword of every record and writes it to the save stream).

Node writes that share the same array (CONFIRMED): 0x6F498B80 writes node visit counters at node+8, 0x6F498BD0
writes closed-list links, 0x6F498C70 writes the heuristic and parent fields. These are A* fields. The region
list and the node list are one array, so a port must keep both views consistent.

---

## 5. Region shapes (CONFIRMED by emulation)

Test results (all on a synthetic 8x8 level-0 map; scratch harness in scratchpad/work4):

| Case | Result |
|---|---|
| Level-0 cell state 01 | lookup returns -1, no label |
| Level-0 cell state 10 | lookup returns -1, no label |
| Level-0 cell 00, all coarser 00 | walk to level 3, one label. Cells (0,0) and (5,5) both get id 0 (the whole 8x8 block) |
| Level-0 00, level-1 block at (0,0) = 01 | cells (0,0) and (1,0) get ids 0 and 1: each cell is its own region |
| Level-0 00, level-2 block at (0,0) = 01 | cells (0,0), (1,0) share id 0; cells (2,0), (3,0) share id 1: regions are 2x2 blocks (level-1 records) |
| Level-0 00, level-1 block = 10 | cell (0,0) gets id 0. Other cells of that block were not run; one cell per region is LIKELY |
| Level-0 11 | walk as for 00 |
| Level-1 query, record 10 | -2 |
| Level-1 query, record 01 | -1 |
| Level-2 query, record 10 | -2 |
| Level-0 cell 10, query at level 1 | level-1 record is consulted, not the cell: walk, label |
| Full wall (state 10) at x = 3 and x = 4, all rows | cells (0,0), (7,0), (0,7), (7,7), (2,4) all get id 0 |

Shape rules derived from these results:
- A region is an aligned square of 2^k cells per side, k in {0, 1, 2, 3}. The largest is 8x8 cells, which is
  the level-3 record. No region can exceed the level-3 block (CONFIRMED by the walk limit).
- The square is determined by the highest uniform coarser record, not by connectivity (CONFIRMED).
- Passable cells that are separated by blocked cells can share one id (CONFIRMED, wall test). This is only
  safe if the search never assumes that every cell of a region is reachable from every other cell. See section 8
  for where the search does assume it.

---

## 6. Traversal order for labelling (CONFIRMED sequence)

There is no queue, no stack and no neighbour scan that labels regions. Labelling happens only when the lookup is
called, and the lookup is called only from:
- the request setup (start cell, then goal cell), and
- the cell expander and the block expander (section 8), once per neighbour probe.

So the discovery order of region ids is the order of these calls, which is the order of the A* pops and of the
expander's probe sequence. The first label of a request is the start cell's region (id 0 if the start is
passable, see section 7).

---

## 7. Request lifecycle (CONFIRMED sequence; ordering of the reset LIKELY)

Request entry 0x6F49A640 (path type in its second argument, flag in the stack argument, mode as 1 << argument, start and goal world positions converted to cells):

1. Reset via 0x6F49A3F0 (called with 0, so the cell-stream helpers are skipped): clears S+0xA0, S+0x9C, sets S+0x94
   to -3 (sentinel), erases W from the front by count (removes all region entries, so W count becomes 0; LIKELY
   from the erase routine's arithmetic, not emulated), clears the waypoint list at S+0x8C (only if nonzero), and
   calls 0x6F4865B0 with (0, 1).
2. Store path type (S+0xD4), flag (S+0xD8), mode (S+0x90), source and destination arrays (S+0xA4..0xB0).
3. Convert start and goal to cells (S+0xB4, S+0xB8, S+0xBC, S+0xC0).
4. If the start cell is inside the level-0 bounds and its record is not null:
   a. If start cell != goal cell: bump S+0x2C (generation). This invalidates every record for this request.
      Then label start (lookup level 0). S+0xC4 := start id. If start is blocked (-1), set S+0xC8 := -1 and return 1
      without labelling the goal. Otherwise label goal and set S+0xC8 := goal id.
      If start id != goal id: S+0xD0 := start id, S+0xCC := squared start-to-goal distance, S+0x98 := budget.
      Return 2 (run the region search). Otherwise return 1.
   b. If start cell == goal cell: return 1 (direct move, no search, no bump).
5. If the start is out of bounds, return 0.

Region ids under this order (LIKELY): the start region is id 0 when the start is passable (it is the first label
after the reset), the goal region is id 1 unless it is the same record as the start.

Note: a request whose start and goal share a cell does NOT bump the generation. Its direct move never looks at
the records (CONFIRMED from the branch structure).

---

## 8. The region search: expansion order (CONFIRMED for the cell expander; block expander partly UNSURE)

The search loop 0x6F49A170 pops the lowest key from the open list (binary heap, pathing-2 section 5.3: ties go to
the right child; equal keys sift a newer element above older ones), skips stale entries, returns when the goal
id is popped, and otherwise expands the node. Expansion takes one of two routines based on the node's level byte
(+0x22):
- level 0 (byte 0): the cell expander 0x6F499E90, which uses the node's seed cell and flag.
- level >= 1: the block expander 0x6F499FA0, which uses the node's aligned block and the level as the block size.

### 8.1 Cell expander (0x6F499E90), CONFIRMED order

It probes the four orthogonal neighbours of the seed in this fixed order, and each probe is a lookup followed by
a relax (the probe wrappers call the relax helper 0x6F498C70 when the neighbour is passable):
1. North: (x, y-1)
2. East: (x+1, y)
3. South: (x, y+1)
4. West: (x-1, y)

Then the four diagonals, each gated by its two orthogonal neighbours (no corner cutting), in this order:
5. North-east (x+1, y-1), only if North and East were passable.
6. South-east (x+1, y+1), only if East and South were passable.
7. South-west (x-1, y+1), only if South and West were passable.
8. North-west (x-1, y-1), only if West and North were passable.

Mode 2 (S+0x90 == 2) adds extra checks in the probe wrappers: the east neighbour's level-0 state must be 00
(0x6F498E80), and for odd x with a set node flag an extra test runs (0x6F498FE0). The exact footprint meaning is
UNSURE, probably a two-cell footprint for the larger unit class.

The cell expander also uses the node's flag byte as an INDEX into the 12-byte table at S+0x3C. If the byte is
nonzero and that entry's bit 0 is set and its (x, y) differs from the seed, it adds one edge to that (x, y) through
0x6F4998C0 (LIKELY: a ramp or jump link; table layout from the decompile, not emulated).

### 8.2 Block expander (0x6F499FA0), partly UNSURE

For a node of level k the block size is 2^k. The block origin is the seed shifted right by k and left by k.
It scans the four edges of the block's perimeter (top, right, bottom, left) with the edge scanners at
0x6F4998F0, 0x6F4999F0, 0x6F499AF0, 0x6F499BF0. Then the four corners run through 0x6F499CF0, 0x6F499D60,
0x6F499DD0, 0x6F499E40 (corners gated by the edge results, the same no-corner-cut rule as above).

The edge scanner calls the lookup at the node's level and handles three cases:
- a region id (0 or more): leave the loop and continue to the code after it (the relax step; exact tail UNSURE);
- -2: recurse one level down into the two halves of the edge (CONFIRMED, from the decrement in the loop);
- -1: return at once (blocked at level 0).
The exact order of the two halves and the exact segment arithmetic are UNSURE (not emulated).

---

## 9. Unresolved items and open questions

- U1 (important): the walk never returns a coarser record's own state. Whether a stored block marked blocked
  (state 10 or 01) can have passable-looking children (state 00) is decided by the data builder, which was not
  found in the game code. LIKELY the builder never leaves state 00 below a blocked coarse record, but this is
  unverified. A port must match whatever the stored data uses.
- U2: the meaning of coarse state 11 and of the 2-bit field values in general. The code treats 01 and 10 as
  "blocked" at level 0. At coarser levels it treats both as "stop the walk". LIKELY 01 = fully blocked and
  10 = blocked but refinable, based on the -2 return.
- U3: the trigger for the pyramid reload. The reload entry 0x6F471460 is called by 0x6F3AFE90, which has no direct
  call sites in the call table. It is probably reached through a function pointer or a tail jump. The save entry
  0x6F46DB30 is called by 0x6F3AF8D0, which is called by 0x6F43EF70 (LIKELY the save-game writer). The reload
  therefore probably runs on map or save load, not on each building change. UNSURE for tree cuts.
- U4: no code was found that rebuilds coarse levels from level 0 at run time. Levels are read as four stored
  sections (0x6F481340 calls the level init four times).
- U5: the constructor of the W struct (0x6F497080 and 0x6F497260 callers) and the initial capacity and grow
  values. Not confirmed.
- U6: the start region id. Section 7 says it is 0 when passable. This follows from the call order and was not run
  through the full request.

---

## 10. Second region system: layer labels (CONFIRMED for the labeller, LIKELY for the purpose)

A second labeller, 0x6F4A1800, works on a different object (call it M). It uses the cell words of M's level-0
grid instead of pyramid records:
- The labeller 0x6F4A1800 reads width, height and cell words from the object that M+0x1C points to (its +0x3C,
  +0x40 and +0x28). The occupant helper 0x6F49F6C0 reads M+0x3C and M+0x28 directly. LIKELY the same grid is
  mirrored in both places (UNSURE).
- Cell word low 24 bits: index of the head occupant record in M's pool at M+0x78 (8-byte records), or 0xFFFFFF for
  none. Bitmask at M+0x98: one bit per cell (set when a cell is visited by the helper 0x6F49F6C0).
- Occupant record: word 0 top byte 0x02 = region record, low 24 bits = next index. Word 1: 16-bit layer key at +4
  (compared with M+0x20), 16-bit region id at +6.
- Region list: W' = M+0x24 with base M+0x30, grow M+0x38, capacity M+0x3C, count M+0x40. The count is the region
  id source. Cap: if count > 0x7FFF, the labeller returns -1 (CONFIRMED).
- Behaviour (CONFIRMED from the decompile): it looks only at the HEAD occupant of the cell. If the head is a region
  record with the same layer key, it returns that record's id. If the head is a region record with a different
  key, it is re-keyed to the current layer and given the new id (overwrites the other layer's record). If there is
  no head, it allocates a new occupant record through 0x6F49F6C0 and appends the seed to W'.
- No generation stamp exists in this system. Invalidation would have to come from the occupant pool (free list
  head at M+0xAC, pool count at M+0x88), which was not traced (UNSURE).

The 8-neighbour expander 0x6F4A2750 (and its variants 0x6F4A2880, 0x6F4A29C0, 0x6F4A2B00) calls the labeller for
the eight neighbours in this output order: NW, N, NE, W, E, SW, S, SE. Each is gated by an 8-bit blocked-ring mask
from 0x6F4A1080, where bits 0 to 7 are NW, N, NE, E, SE, S, SW, W (CONFIRMED from the mask tests). An orthogonal
probe is skipped when its own bit is set. A diagonal probe is skipped when its own bit or either adjacent
orthogonal bit is set (for example NE uses bits 1, 2 and 3). These variants are called from 0x6F4A3AD0 and friends (LIKELY the
per-layer region-graph expansion).

---

## 11. Limits and quirks (for a clean port)

- Region id width: 16 bits (lookup returns zero-extended 16 bits; the labeller stores the low 16 bits). The region
  list is 32-bit. A request with more than 65535 labels wraps (LIKELY, not emulated). The labeller has no cap.
- Maximum region size: 8x8 cells (level 3), with 1x1, 2x2 and 4x4 below it (CONFIRMED).
- Maximum region count in the second system: 0x8000 (CONFIRMED).
- Append failure: the labeller ignores the failure and leaves the record stamped (LIKELY from the decompile).
- The staging buffer keeps stale bytes in offsets 8 to 0x1F (CONFIRMED: the labeller writes only +0, +4, +0x20,
  +0x22). A port should zero these or the A* node init must set them (LIKELY the A* init does).
- The -2 return only occurs for a query whose own record is 10 at a level above 0 (CONFIRMED).
- Level-0 queries return only -1 for blocked, never -2. The cell probe 0x6F4995E0 tests only for -1, so a -2 from a
  higher-level query would be taken as a region id by any caller that does not handle it. The block scanners do
  handle it (CONFIRMED).

---

## 12. Emulation evidence (Unicorn harness, scratchpad/work4)

Scripts: lookup_test.py (sections 2, 3, 5 tests A to G), lookup_test2.py (level-1 and level-2 return codes),
lookup_test3.py (wall test). Each builds a synthetic S object with the level layout of section 1, sets
S.gen = 1, path type 0, flag 0, mode 1, and calls 0x6F498A60 through the harness call helper. The outputs are
the ones quoted in section 5 and in section 2. Entries were read from W after each call.

---

## 13. Corrections to pathing-2.md

- The state 0x40000000 (01) returns -1, it does not mean "open" (CONFIRMED, emulated). The state 0x80000000 (10)
  returns -1 at level 0 and -2 above level 0.
- The lookup does NOT return the coarse record's region id when a coarse level is non-mixed. It returns the record
  one level below that coarse level (CONFIRMED, emulated).
- "The flood fill was not located" is now answered: there is no flood fill. The labeller assigns one id per
  record on first query (CONFIRMED).
- The work list (pathing-2 section 3) and the node list are one array: the W struct at S+0x50 (base S+0x5C,
  count S+0x6C). The count is also the next region id.
- The +6 field of a record is the high 16-bit half of word 1 (bits 16 to 31), not a separate byte flag. The byte
  at +6 is its low byte.
- The region id is the W index at the time of labelling (not a separate counter). The first id of each request is 0.

---

## 14. Confidence per item

| Item | Label |
|---|---|
| Lookup algorithm and return codes | CONFIRMED (disasm + emulation) |
| Lazy labelling, id = count, seed = query cell, level and flag bytes | CONFIRMED (disasm + emulation) |
| Region shapes are aligned blocks, not components | CONFIRMED (emulation) |
| No flood fill in the pyramid path | CONFIRMED (absence of stores and of any loop over regions in 0x6F49xxxx) |
| Write set of the pyramid arrays (section 4) | CONFIRMED (instruction scan) |
| Generation bump and reset per request | CONFIRMED (disasm of 0x6F49A640 and 0x6F49A3F0); the erase arithmetic LIKELY |
| Region ids restart at 0 per request | LIKELY (call order) |
| No in-place pyramid state update in the game | CONFIRMED for the range scanned; LIKELY overall |
| Reload on map or save load | LIKELY (call chain), trigger UNSURE |
| Cell expander probe order and diagonal gating | CONFIRMED (decompile of 0x6F499E90) |
| Block expander details | UNSURE |
| Second system (M) behaviour | CONFIRMED (decompile); purpose LIKELY |
| 16-bit id wrap | LIKELY |
| Meaning of coarse state values | UNSURE (section 9) |
