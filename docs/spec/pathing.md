# Pathing and unit movement: reverse-engineering spec (Game.dll 1.26a)

Scope: clean-room interoperability notes for a reimplementation. Plain English only, no code.
Addresses are VAs in Game.dll (image base 0x6F000000; file offset = VA - 0x6F000000 for .rdata/.data strings).
Labels: CONFIRMED = checked in the disassembly or in data bytes of the binary. LIKELY = strong inference from
names, constants and call shapes, not yet checked in emulation. UNSURE = not established.

Status summary: the grid resolution, the cell word layout, the pathing-type bit masks, the flag names, the
unit and terrain setter paths and several default constants are CONFIRMED. The A* loop, its neighbour order,
heuristic and diagonal costs, the per-step mover, and unit-unit separation are NOT yet located. Those sections
below list what is known about them and where to look next.

---

## 0. Sources and method

- Decompiled chunks (scratchpad/decomp/chunk_*.c, natives.c) were used for names and call graphs. Several
  decompiled bodies lose register arguments (`__thiscall` and `__fastcall` ECX/EDX), so every claim about
  argument order was re-checked in the disassembly (capstone, 32-bit x86) where it matters.
- Cross-references to string and vtable addresses were found with a raw scan of .text and .rdata
  (scratchpad/work3/xref.py). Data words were read from the file bytes (rd.py). Constant stores were found by
  scanning .text for `mov dword ptr [imm], imm` (stores.py).
- Software-float helper names come from the repo spec (docs/spec/softfloat.md, crates/softfloat), as cited.

---

## 1. Pathing map

### 1.1 Class layout (RTTI, CONFIRMED)

Namespace NIpse. Pathing-related classes and their RTTI and vtable addresses:

| Class | Vtable | Role (LIKELY from name and use) |
|---|---|---|
| CBaseMap<CPaCell> | 0x6F952224 | grid of pathing cells (static / terrain side) |
| CBaseMap<CPmCell> | 0x6F953484 | grid of pathing-map cells used by regions |
| CPaCell / CPmCell | (CPaCell memblock 0x6F933164, CDynTable 0x6F933210; CPmCell memblock/CDynTable not separately listed) | cell payloads |
| CPmRegion | 0x6F9529C4 | region: a set of cells, owns a map pointer at +0x20 |
| CLrPathingSys | 0x6F95359C | long-range pathing system (one per game) |
| CLrPathingAcc | 0x6F95273C | long-range accessor / scratch buffer |
| CLrPath | 0x6F95336C | one long-range path request and its state |
| CLrCellNode, CLrOpenNode | 0x6F952134 / 0x6F952154 (memblocks) | search node and open-list node |
| CPriorityQ<CLrOpenNode> | 0x6F9525BC | binary heap of open nodes |
| CPmQuery<CPoQu>, CPmQuery<CSeQu> | 0x6F9525C4 / 0x6F9525CC | point and segment queries against the pathing map |
| CPoQuery, CSeQuery | 0x6F952844 / 0x6F95284C | query objects |
| CProximityMap | 0x6F9534CC | spatial index for proximity lookups (units / objects) |
| CPrCluster, CPrClusterGroup, CPrClusterGen | 0x6F95230C, 0x6F951EC4, (gen) | clustering of proximity entries |
| CPoPos, CPoPosCl, CPoPosBh | 0x6F9524A4, 0x6F952554, (Bh) | point-position records, cluster and behavior variants |
| CPoSeparate | 0x6F9521B4 | separation records (candidate for unit push-apart) |
| CPrRelation, CPrBehavior, CBhPoBehavior | 0x6F952004, 0x6F951D44, 0x6F951DA4 | relation and behavior records |
| CPmQueryRec | (memblock 0x6F95177C) | query record pool |
| CRlAgent, CRlAgentDef | 0x6F95207C, 0x6F952794 | per-agent records for the proximity/relation system |
| CPathingFootprint, CPathingFootprintReg | (see 3.4) | unit / building footprints (source: CPathingFootprint.cpp) |

### 1.2 Grid resolution: 32 world units per cell (CONFIRMED)

- The native RegionAddCell (0x6F3C1580) calls FUN_6F47CFB0, which converts the world coordinate to a cell index.
- The conversion scales a float by 2^-5 using integer arithmetic on the float bit pattern: it adds 0xFD800000 to
  the bits, which subtracts 5 from the exponent, i.e. divides by 32. A guard flushes values with bits below
  0x03000000 (about 2^-121 in magnitude, and zero) to zero. The scale is exact for normal floats and is not a
  softfloat call. Inside the same region code, the cell-test function FUN_6F468E40 does the same scale.
- Consequence: one cell = 32 x 32 world units. Together with the usual terrain tile of 128 units (external
  knowledge, LIKELY), this is 4 x 4 cells per tile.

### 1.3 Cell coordinates and origin (CONFIRMED for the arithmetic, LIKELY for names)

- World-to-cell conversion, in order: subtract a map origin (x uses the float at global+0x6C, y uses the float at
  global+0x70, where global = the pointer stored at 0x6FAB7368), then scale by 1/32, then floor
  (softfloat floor 0x6F6EF170, FUN_6F6EF170) and truncate to integer (softfloat f2i 0x6F6EEC20, FUN_6F6EEC20).
  Both RegionAddCell (FUN_6F47CFB0) and the terrain test (FUN_6F468E40) subtract these origins; the subtract is
  the softfloat sub 0x6F6EEEF0 (FUN_6F6EEEF0; convention: out = x - y, per the repo spec).
- Which of x and y uses +0x6C versus +0x70 is LIKELY (x with +0x6C), not separately proven. The origin floats are read from a global object whose
pointer is stored at 0x6FAB7368. That object is created at runtime, so the origin values are not known from the
file (UNSURE: they are probably the map's playable-area minimum corner, set during map load).
- The cell index is row * width + column (CONFIRMED in FUN_6F49FCD0: stride is the map's width field at +0x3C,
  column and row come from the converted coordinates). The map keeps width at +0x3C, height at +0x40 and cell
  count (width * height) at +0x30 (CONFIRMED, constructor FUN_6F487ED0 for CBaseMap<CPaCell>).
- The constructor converts width and height to floats with the softfloat i2r (0x6F6EF350, FUN_6F6EF350; takes
  the integer in EDX and an output pointer in ECX; CONFIRMED call shape) and stores them at +0x4C and +0x50. The
  floats at +0x44 and +0x48 are 0.0. So the map's float extent is (0,0) to (width, height) in cells.
- Cell storage: a block of width * height 32-bit words is allocated by FUN_6F484810 (CONFIRMED call, size details
  not traced).

### 1.4 Cell word layout (CONFIRMED for the bit layout, LIKELY for names)

Each cell is one 32-bit word, pointed to by the map's cell-array pointer at +0x28:

- Bits 24 to 31 (top byte): the pathing flags, one bit per pathing type (see 1.5).
- Bits 0 to 23: a 24-bit index of the head of a list of occupants of that cell (see 1.7). The value 0xFFFFFF
  means "no occupant record" (the code tests for it explicitly, CONFIRMED).

Updates (FUN_6F49FCD0, the cell set/clear primitive):
- The low 24 bits are replaced with the new head index; the top byte is preserved (CONFIRMED).
- When the flag argument is zero, a separate 1-bit-per-cell "dirty" bitmap (pointer at map+0x98, word index =
  cell index >> 5, bit = cell index & 31) is set (CONFIRMED). The dirty bitmap most likely records cells whose
  long-range data must be rebuilt (LIKELY).
- A counter at map+0xB0 is incremented on every update (CONFIRMED). Likely a change stamp (LIKELY).

Flag edits on the top byte (FUN_6F468DF0): set = OR the mask into the top byte; clear = AND-NOT the mask
(CONFIRMED by the bit operations).

### 1.5 Pathing-type masks and flag names (CONFIRMED)

Native pathing types map to a bit mask by a switch (FUN_6F3B4280):

| Type index | Mask | Flag name it matches (see below) | Likely JASS name (LIKELY, not in the DLL) |
|---|---|---|---|
| 0 | 0xFF (all eight bits) | (none) | ANY |
| 1 | 0x02 | unwalkable | WALKABILITY |
| 2 | 0x04 | unflyable | FLOATABILITY |
| 3 | 0x08 | unbuildable | BUILDABILITY |
| 4 | 0x10 | (no name in the table) | PEON HARVEST (guess) |
| 5 | 0x20 | blighted | BLIGHT |
| 6 | 0x40 | unfloat | WATER (guess) |
| 7 | 0x80 | unamph | AMPHIBIOUS |
| other | 0 | | |

The flag-name table (8-byte entries at 0x6FA70FB0, read by FUN_6F26DFF0) is CONFIRMED:
- blighted = 0x20; unbuildable = 0x08; unflyable = 0x04; unwalkable = 0x02; unfloat = 0x40; unamph = 0x80.
- The seventh entry has the placeholder name "_" with flag 0. The eighth entry, "nightelf", is outside the loop
  (the loop runs over seven entries) and is not a pathing flag in this code.
- Bits 0x01 and 0x10 have no name in this table.
- FUN_6F26DFF0 tokenizes a delimited string, compares each token case-insensitively against the table names, and
  ORs the matching flag bytes into a result. Its caller and the exact field it parses are UNSURE (the doodad and
  destructable loader FUN_6F260E30 is the likely caller; not traced).

Meaning of the bits for the type 0 test: the mask 0xFF covers all eight bits, so "ANY" is LIKELY a test that the
cell is blocked if any bit is set.

Blocked semantics: a set bit means blocked. The terrain setter passes the inverse of the pathable argument
(pathable == 0 becomes "set"), so SetTerrainPathable(false) sets bits (CONFIRMED by the disassembly of
0x6F3B42F0 and 0x6F468DF0).

### 1.6 Terrain and object writers (partly CONFIRMED)

- Native IsTerrainPathable (0x6F3B42D0): computes the type mask, then calls the cell test (FUN_6F468F90 ->
  FUN_6F468E40 -> FUN_6F4A3790, which floors and converts the scaled coordinates to a cell). The returned
  boolean is taken from the test helper's return value. Its sign convention is LIKELY "true = pathable = no
  blocking bit in the mask" (UNSURE in detail).
- Native SetTerrainPathable (0x6F3B42F0): flag = (pathable == 0); FUN_6F3B4280 gives the mask; FUN_6F46BD10 writes
  the flag into the cell's top byte (CONFIRMED).
- Native RegionAddCell (0x6F3C1580) passes flag 1 to FUN_6F47CFB0; that flag becomes 0x1000000 for FUN_6F49FCD0,
  which changes its path (the dirty bit is skipped for that value). Exact meaning UNSURE.
- Map load: FUN_6F01DE80 builds the name "war3map" plus the extension at 0x6F876A24 (".wpm", the pathing map)
  and checks for the file (CONFIRMED string use). The file format and the cell decoder are not traced (UNSURE).
  The terrain file (.w3e, string at 0x6F876A2C) is referenced nearby, not decoded.
- Cliffs and water are present (strings "cliffSet", "cliffModelDir", "Naval Pathing Display", "DeepLevelPathing").
  Which cell bits cliffs and water set is UNSURE.

### 1.7 Occupant (dynamic blocking) records (CONFIRMED for layout)

- The occupant pool is an array of 8-byte records at map+0x78: each record holds a next index (24-bit) and an
  object pointer. A free-list head is kept at map+0xAC (sentinel 0xFFFFFF). A record count is kept at map+0xB0.
- When a record is added, the object's field at +0x3C is incremented (CONFIRMED). Meaning of that field LIKELY
  reference count.
- Units register their footprint (SetUnitPathing, see 4.1); what fills the footprint is in CPathingFootprint
  (section 3.4). The collision radius itself is not traced (UNSURE).

---

## 2. Search structures and the long-range path system

### 2.1 Objects and their constants (CONFIRMED for the values)

- CLrPath constructor (FUN_6F49CA10) and init (FUN_6F49B680):
  - Two 16-bit limits: 700 at +0x84 and 400 at +0x86. The debug dump names them "lim" (see 2.4).
  - 0x80 (128) at +0x48 and +0x68.
  - Flags 0x200000 at +0x88.
  - Initial sentinels: -1 (0xFFFFFFFF) at +0x74, +0x78 and the position fields; 0.0 at +0xB4.
  - An embedded array of two 32-byte elements (constructed with FUN_6F49C530; the two-element array is LIKELY
    the open and closed scratch lists).
  - Meaning of the limits: UNSURE (candidates: maximum expansions per phase, or maximum search extent).
- CLrPathingAcc init (FUN_6F49A330): reserves a buffer of 0x100 = 256 entries (CONFIRMED). Teardown FUN_6F49A600.
- CPriorityQ<CLrOpenNode> is a member of the long-range system (its vtable is set in FUN_6F487E60, FUN_6F488BE0
  and FUN_6F4A1CF0). The ordering comparator is UNSURE.

### 2.2 Algorithm: A*-like? (LIKELY from names; not confirmed)

- The presence of CLrCellNode, CLrOpenNode, a priority queue of open nodes and a long-range path object strongly
  suggests an A* style search over cells, with a hierarchy (regions and clusters) to shorten it. LIKELY.
- NOT FOUND: the main search loop. Searched: functions that reference the open-node or cell-node types (only six
  small constructors and wrappers); CLrPath virtual slots 0 to 16 (the slot-4 method FUN_6F49DE40 is a state
  reset and FUN_6F49D450 is a teardown); CLrPathingAcc slots (init and teardown only).
- NOT FOUND: the 8-direction neighbour offset table. A scan of the whole file for the eight (dx, dy) pairs
  as 32-bit, 16-bit or 8-bit values found no table, so neighbour expansion is inline or computed.
- Therefore neighbour order, tie-breaking, heuristic (octile / Euclid / Chebyshev), diagonal cost and corner
  cutting are UNSURE. These matter for determinism; they must be derived from the search loop before the
  reimplementation can match paths.

### 2.3 Search limits and sync checks

- The long-range state hash (FUN_6F49CDE0, a vtable method) accumulates a checksum over the state fields. For
  each byte b of a field it adds (b * 0x7A2D) XOR b, and folds in the bytes in a fixed order (CONFIRMED). This is
  a state checksum for desync detection (LIKELY).
- The save/load method (FUN_6F49D5F0) gates a block of fields on a version value 0x178A (6026): if the version is
  below it, two fields are copied from the first generation; otherwise two extra reads run (CONFIRMED values,
  meaning LIKELY: a save-format version change).

### 2.4 Debug dump strings (CONFIRMED text, meaning partly UNSURE)

The state dump (FUN_6F49CB70) prints these labels: "[PrTg] t[%u] b[%u]", "[PrPa] - csum[0x%08X]",
"pos[%u] lim[%u]", "gol x[%s] y[%s] - x[%s] y[%s] - x[%s] y[%s]", "opt[0x%08X] rcr[%u] tcn[%u]",
"ifr[%s] pfm[0x%08X]". Labels suggest: target (t), bounds (b), checksum (csum), position and limit pairs, goal
(gol), options (opt), recursion count (rcr), a counter (tcn), a flag (ifr) and a path-flag mask (pfm). Meanings
of rcr, tcn, ifr and pfm are UNSURE.

### 2.5 Proximity and separation classes (names only)

- CProximityMap, CPrCluster, CPrClusterGroup and CPrRelation form a spatial index with clustering.
- CPoSeparate, CPoPosBh and CPrBehavior hold per-object separation parameters. CPoPosBh's constructor (FUN_6F4AB390)
  stores the step constant 0.1 (0x6FAAE4A0) and a 1.2 constant (0x6FAAE4FC = 0x3F99999A), among others. Their
  update code was not located (UNSURE).

---

## 3. Units, footprints and collision

### 3.1 Movement fields in unit data (CONFIRMED that the names exist)

The unit database loader (FUN_6F263850) reads these movement and pathing names: turnRate, propWin,
orientInterp, moveFloor, moveHeight, movetp, pathTex, canSleep, fatLOS, collision (and collision shape data). The
destructable/doodad loader (FUN_6F260E30) reads pathTexDeath, maxPitch, maxRoll, numVar, targType and others.
Their semantic units are LIKELY standard WC3 meanings; the conversion to internal units is UNSURE.

### 3.2 Turn speed (CONFIRMED path and clamp)

- Native SetUnitTurnSpeed (0x6F3C5CB0) calls unit virtual slot 0xC0 (CUnit::vf48, 0x6F28B0F0). That thunk adds 0x164
  to the unit pointer and jumps to FUN_6F474B30 (the movement sub-object).
- FUN_6F474B30 clamps the value from below at the constant 0.001 (stored at 0x6FAAE47C = 0x3A83126F, CONFIRMED
  from the initializer store at 0x6F84DCA0), then stores it through FUN_6F4937C0.

### 3.3 Prop window and move speed (CONFIRMED path, meaning UNSURE)

- Native SetUnitPropWindow (0x6F3C5CD0) calls unit slot 0xC4 (0x6F28B100), which adds 0x164 and jumps to FUN_6F474BA0.
  That stores through FUN_6F4937E0 with no clamp. Units of the prop window (degrees or radians) are UNSURE.
- Native SetUnitMoveSpeed (0x6F3C5C40) calls FUN_6F26DE00: if the unit field at +0x1EC is set, it calls FUN_6F202F30,
  which calls a virtual method of the object at +0x6C with (speed, 1), then FUN_6F201D50 (CONFIRMED path; the
  virtual method is UNSURE).
- Native SetUnitFlyHeight (0x6F3C5C60): if the value is above zero it is used; otherwise the default 0.01 (stored at
  0x6FAAE480 = 0x3C23D70A, CONFIRMED from the initializer at 0x6F84DCB0) is used. Clamp direction LIKELY a minimum
  height rule.

### 3.4 Footprints (partly CONFIRMED)

- RegisterPathingFootprint and the keys "pPathingFootprint" and "pPathingFootprintAlt" are data-table keys (0x6F32CF20
  and neighbours). CPathingFootprint and CPathingFootprintReg (source path "...\Game\CPathingFootprint.cpp") are
  built by FUN_6F405540 and FUN_6F405660 and stored in a hash table keyed by name. Internal format and how a
  footprint is stamped into cells are UNSURE.
- SetUnitPathing (0x6F3C5FA0): the enable path calls unit slots 0x164, 0x160 and 0x15C (thunk slots, not traced); the
  disable path calls slot 0xB8 and then 0x15C. Semantics UNSURE, but this is the add and remove of a unit's
  footprint in the map.

### 3.5 Collision radius and unit-unit push (NOT FOUND)

- No collision radius formula was located. The "collision" and "CollisionShape" data are loaded (see 3.1), and
  there is a collision event class (CCollisionEvent) with separate code (not traced).
- The separation and proximity classes (2.5) are the likely candidates for pushing units apart. Their update
  code is not yet located.

---

## 4. Per-step movement (NOT LOCATED)

- The 100 ms simulation step (AGENTS.md 4.3h) is supported by the constant 0.1 (0x6FAAE4A0), which is referenced from
  68 decompiled functions (CONFIRMED count; the number of uses inside each is not counted). Using it per unit per step is LIKELY but not confirmed.
- FUN_6F26A830 (258 lines) was examined. It is a path-request or order helper: it takes a start point, a target
  and a size, clamps a float parameter, uses softfloat mul (0x6F6EEE20, FUN_6F6EEE20; name from the repo spec) and
  calls several order-state helpers. It is NOT the per-step mover.
- Candidates not yet read: the movement sub-object methods reached through +0x164 and +0x1EC, and the functions that
  call the speed and turn setters.
- Turn rate, the propulsion window (prop window), the speed and the acceleration rules: UNSURE.
- Re-path triggers (blocked, target moved, footprint change, timeout): UNSURE. The cell update counter (1.4) and the
  dirty bitmap are the probable inputs.

---

## 5. Software-float helpers used (addresses from docs/spec/softfloat.md and AGENTS.md 4.3g)

| Helper | Address | Seen in | Status |
|---|---|---|---|
| sub | 0x6F6EEEF0 (FUN_6F6EEEF0) | cell conversion (1.3), RegionAddCell, terrain test | CONFIRMED use |
| floor | 0x6F6EF170 (FUN_6F6EF170) | cell conversion (1.3) | CONFIRMED use (repo spec gives it as floor) |
| f2i (truncate) | 0x6F6EEC20 (FUN_6F6EEC20) | cell conversion (1.3) | CONFIRMED use |
| i2r | 0x6F6EF350 (FUN_6F6EF350) | CBaseMap constructor (1.3) | CONFIRMED use |
| mul | 0x6F6EEE20 (FUN_6F6EEE20) | FUN_6F26A830 | CONFIRMED call; operands UNSURE |
| R2I | 0x6F6EEC70 | not seen in these paths | not used here |

- The 1/32 scale in 1.2 is NOT a softfloat call. It is integer arithmetic on the float bits (add 0xFD800000 with a
  zero-flush guard), so a port must reproduce the flush rule (values with bits below 0x03000000 become 0).
- Pathing movement math does not show the x87 path. Its results must be taken from the softfloat helpers listed.

---

## 6. Open items (what to do next, in order)

1. Find the A* loop: look for the function that pops the CPriorityQ<CLrOpenNode> heap and the code that expands
   eight neighbours. Read its comparator and the heuristic. This sets neighbour order and diagonal cost.
2. Identify the movement sub-object (+0x164 of CUnit) and its update method; read it for speed, turn and propulsion
   window logic, and for the per-step timing.
3. Decode the .wpm loader (FUN_6F46B850 and the reader it calls) for the header and the cell byte format, and the
   terrain writers (cliffs, water, deep water, buildability).
4. Decode CPathingFootprint (FUN_6F405540 / FUN_6F405660) to get the footprint cell pattern and the collision
   radius.
5. Decode the separation update (CPoSeparate and CPoPosBh methods) for unit push-apart.
6. Check the CLrPath limits (700, 400, 128) and the flag 0x200000 against the search code once it is found.

Key addresses, quick reference: cell primitive FUN_6F49FCD0; cell test FUN_6F468E40; cell write FUN_6F468DF0 and
FUN_6F46BD10; region add FUN_6F47CFB0 (native 0x6F3C1580); terrain natives 0x6F3B42D0 and 0x6F3B42F0; pathing-type
mask FUN_6F3B4280; flag-name parser FUN_6F26DFF0 (table 0x6FA70FB0); map constructor FUN_6F487ED0; CLrPath
constructor FUN_6F49CA10 and init FUN_6F49B680; CLrPath checksum FUN_6F49CDE0; turn speed FUN_6F474B30; prop window
FUN_6F474BA0; speed FUN_6F26DE00; fly height native 0x6F3C5C60; map load FUN_6F01DE80.
