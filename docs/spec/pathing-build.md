# How Game.dll 1.26a builds its pathing data at load time

Scope: clean-room interoperability notes. Plain English, no code. Addresses are VAs in Game.dll (image base 0x6F000000).
Labels: CONFIRMED = read in the disassembly or decompile and, where stated, checked by Unicorn against a Python model.
LIKELY = strong inference from call shapes and constants, not emulated. UNSURE = not established.

Read with: pathing.md, pathing-2.md, pathing-3.md. This file supersedes three claims there (section 9).

Method: decompile bodies (scratchpad/decomp), capstone disassembly of every register-sensitive call site, a direct-call
scan of .text (all E8/E9 targets), and Unicorn runs of the two builders on random inputs (scratchpad/emu_copy/pyr_check.py,
9,600 level-0 calls and 9,600 upper-level calls, 0 mismatches against the models in section 4).

---

## 0. Summary

1. war3map.wpm is read by 0x6F46B850. The file is: 4 bytes magic "MP3W", one u32 that the game never checks, then width
   (u32), then height (u32), then width x height bytes row by row. Each byte ORs bits into the top byte of a 32-bit cell word.
2. The bit mapping is a plain OR with no inversion. A set file bit means blocked. Cell top byte bits: 0x02 unwalkable,
   0x04 unflyable, 0x08 unbuildable, 0x20 blighted, 0x40 unfloatable, 0x80 unamphibious, plus a summary bit 0x01 (and 0x10
   for the unwalkable case). File bits 0x01 and 0x10 do nothing.
3. Map load then builds four pyramid levels from the cell words (0x6F468C70 with ECX = 0, which calls 0x6F488330). Level A
   (at G+0x23C) is built from 2x2 blocks of cells with a per-type test; levels B, C, D (G+0x240, +0x244, +0x248) are built
   from 2x2 blocks of the level below. The builder rules are CONFIRMED in Unicorn.
4. Each pyramid record is 8 bytes: word 0 is a stamp (0 at load), word 1 holds four 2-bit type states in its top byte
   (types 0, 2, 4, 6 at bits 30-31, 28-29, 26-27, 24-25) and a tag byte in bits 16-23. States: 00 open, 01 all blocked,
   10 mixed (or blocked, refine), 11 never written by the builder.
5. Doodads and destructables are placed in a separate world init (0x6F3A2920) that runs before a second full rebuild.
   Whether they change cells is UNSURE.
6. A saved game does not rebuild the pyramid. The reader (0x6F471460, from 0x6F3AFE90) releases the levels and reads the
   stored 16-bit states per record (0x6F481340, 0x6F498960). The saved pyramid overrides the fresh build.
7. Runtime: unit creation (CreateUnit, CreateCorpse, ReviveHero, CreateBlightedGoldmine, PlaceRandomUnit) rebuilds the
   footprint rect twice (erase, then recompute) around a placement query (0x6F475560).
8. CreateDestructable, RemoveDestructable, SetUnitPathing and SetTerrainPathable do not reach any pyramid rebuild in the
   direct-call graph. LIKELY the pyramid is stale after those until a later rebuild.
9. Open: the level dimensions. Level A is indexed at 2x cell coordinates, so it is probably half the cell grid. The search
   reads its own level-0 object (S+0x1C, one cell per record) and I did not find where that object is built.

---

## 1. The wpm reader (FUN_6F46B850)

### 1.1 Entry and file name (CONFIRMED call, LIKELY name)
- Called from the map-load routine FUN_6F01DE80 (call at 0x6F01E100). Arguments: the playable-bounds rect global at
  0x6FADB3F8 and a name pointer at 0x6FAB7370. **Correction 2026-10-11 (runtime-constants audit):** 0x6FAB7370 is not a name pointer; its start-up value is the float 0.03 (the movement walk timer, movement-4/5). 0x6FADB3F8 holds the float 32.0 at start-up, not a bounds rect, so the source of the bounds argument is still open (UNSURE). The name is "war3map" plus the extension string ".wpm" (string at 0x6F876A24,
  from pathing.md). The reader first stores the bounds floats into the world object: 0x6FAB7368 +0x6C gets bounds[1],
  +0x70 gets bounds[0], +0x74 gets bounds[3], +0x78 gets bounds[2] (CONFIRMED stores; which of x and y the origin
  subtraction uses is UNSURE, see 5.3).
- The pathing world object is G = the pointer stored at 0x6FAB73D8. The reader and the rebuild both use this pointer
  (CONFIRMED: 0x6F46B9A1 loads it, and 0x6F468C70 loads the same slot).

### 1.2 File layout (CONFIRMED from the compare and read order)
- Offset 0, u32 little endian: must equal 0x5733504D (bytes "MP3W"). Otherwise the file is handled as an image (1.5).
- Offset 4, u32: read, stored in a local, never compared. (wpm.rs requires 0; the game does not check it.)
- Offset 8, u32: WIDTH (columns).
- Offset 12, u32: HEIGHT (rows).
- Offset 16: width x height bytes, row 0 first, column 0 first within a row. The reader reads exactly width x height bytes
  and ignores anything after them (CONFIRMED read size).
- Note the field order in the code: the column loop limit is the second header value and the row loop limit is the third.
  wpm.rs names them the same way.

### 1.3 Per-byte mapping (CONFIRMED from the disassembly at 0x6F46BAD7 to 0x6F46BB22)
Each file byte b ORs a constant into the top byte of its cell word (bits 24-31). Lower 24 bits are never written:
- bit 0x02 set: OR 0x13 (bits 0x10, 0x02, 0x01)
- bit 0x04 set: OR 0x05 (bits 0x04, 0x01)
- bit 0x08 set: OR 0x09 (bits 0x08, 0x01)
- bit 0x20 set: OR 0x20 (bit 0x20 only, no 0x01)
- bit 0x40 set: OR 0x41 (bits 0x40, 0x01)
- bit 0x80 set, or both 0x02 and 0x40 set: OR 0x81 (bits 0x80, 0x01)
- bits 0x01 and 0x10 of b: no effect.
A cell word is therefore blocked for a type when the matching top bit is set (section 3). There is no inversion.

Interpretation (LIKELY, from the type masks in 3.2): the summary bit 0x01 is set by every blocking flag except blight,
so it is probably "some movement is blocked here". Its exact meaning is UNSURE.

### 1.4 Write target and bounds (CONFIRMED)
- Target word index = row x map_width + column, where map_width is the cell map's width field (+0x3C of C, see 2.1).
- The reader checks column < C.width and row < C.height. If the check fails, the target pointer is set to 0 and the OR
  writes to address 0. The reader does no clipping, so the wpm dimensions must equal the cell-grid dimensions. A port
  should reject a mismatch.
- Before the loop the reader allocates a staging buffer of width x height bytes (0x6F46A9B0). The allocation of the cell
  words themselves was not traced. Initial cell word values are UNSURE. LIKELY the top byte is 0
  and the low 24 bits are 0xFFFFFF ("no occupant"), because the cell test treats 0xFFFFFF as "no occupant" (3.1).

### 1.5 Image fallback (CONFIRMED structure, channel order LIKELY)
If the first u32 is not "MP3W", the same routine opens the file as a 24-bit image (targa-style header, bit depth byte 0x18
checked at 0x6F46BC1C). Per pixel, four bytes in file order B, G, R, A (LIKELY) map to top-byte values:
- byte 0 (B): OR 0x09
- byte 1 (G): OR 0x05
- byte 2 (R): OR 0x03
- byte 3 (A): OR 0x20
Rows are reversed when bit 5 of the image descriptor is set (CONFIRMED test, LIKELY meaning). The branch is taken whenever
the magic does not match (CONFIRMED). Which file names reach it is LIKELY the "Path.tga" case (string nearby in 0x6F01DE80).

### 1.6 Tail (CONFIRMED)
After the fill, the reader calls 0x6F468C70 with ECX = 0. That is the full pyramid build (section 4). This means the
pyramid is built as soon as the wpm is read, before the map's doodads and units are placed.

---

## 2. Pathing objects

### 2.1 Cell map C and query Q (CONFIRMED)
- G+0x238 (read by getter 0x6F481430) is the cell map C. C+0x3C = width, C+0x40 = height, C+0x28 = array of 32-bit cell
  words (index row x width + column). C+0x78 = occupant pool (8-byte records), used only for occupied cells.
- G+0x24C is the query object Q (read by the builders). Q+0x1C = C. The cell test is the function at 0x6F4A0E80.
- C+0x54, +0x58, +0x5C, +0x60 = a rect (x0, y0, x1, y1) in cells. The full rebuild uses it (section 4). Where it is set is
  UNSURE (probably the playable area).
- The other pathing slots: G+0x234 and +0x250 (objects, released by 0x6F4814F0), G+0x23C..+0x248 (the four pyramid
  levels, section 3).

### 2.2 Level objects (CONFIRMED for slots and reads, UNSURE for dimensions)
- Each level object has width at +0x3C, height at +0x40, record array at +0x28 (8-byte records), and a count at +0x30.
- The constructor 0x6F4A03F0 zeroes width, height and count (CONFIRMED). Earlier pathing-3 text said it sets them; that is
  wrong. Dimensions are set elsewhere and not located (UNSURE).

---

## 3. Cell test, record layout and the per-type masks

### 3.1 Cell test 0x6F4A0E80 (CONFIRMED logic, from the decompile and disassembly)
Inputs: a mask in the query object (Q+0xA4, copied by 0x6F4A38C0 from a pointer argument) and a cell (x, y).
- Out of range (x >= C.width or y >= C.height, or null word pointer): result 0 (not passable).
- Cell word w: if (w & mask & 0xFF000000) != 0, result 0 (blocked for this type).
- Otherwise, low 24 bits of w = occupant head index. 0xFFFFFF means no occupants: result 1 (passable).
- Otherwise the occupant list is walked (8-byte records at C+0x78; tag byte 0x01 marks an object record, 0x02 a region
  record). An object record blocks the cell when the object's flags (+0x40) have bit 0x10000000 set, have no other bits
  from 0x8FFFFFFF set, have bit 0x01000000 set in +0x34, and (+0x34 & mask & 0xFFFFFF) is nonzero: result 0. This walk is
  CONFIRMED from the decompile but NOT emulated.
- The walk also writes an iteration stamp into the object's +0x38 and a counter into C+0xB4 (side effects).

### 3.2 Per-type masks (CONFIRMED values in the file image at 0x6FA84E84..0x6FA84E90; LIKELY the same at run time)
The level-0 builder uses one mask per state field:

| State field | Type T | Mask (dword) | Top-byte bits that block | Low-24 bits (occupant test) |
|---|---|---|---|---|
| bits 30-31 | 0 | 0x06000006 | 0x02 (unwalkable) or 0x04 (unflyable) | 0x000006 |
| bits 28-29 | 2 | 0x80000080 | 0x80 (unamphibious) | 0x000080 |
| bits 26-27 | 4 | 0x40000040 | 0x40 (unfloatable) | 0x000040 |
| bits 24-25 | 6 | 0x04000004 | 0x04 (unflyable) | 0x000004 |

The masks were read from the emulator's mapped image and matched the model in Unicorn. The names are LIKELY from the flag
names in pathing.md. Type 0 blocking on 0x04 is unexpected for a walker and may be a naming error. UNSURE, but the masks
themselves are CONFIRMED.

### 3.3 Record layout (CONFIRMED)
- Word 0: stamp (0 at load; the search overwrites it).
- Word 1, bits 24-31 (byte +7): the 2-bit states for types 0, 2, 4, 6 at bits (30-T, 31-T). For T = 0 the state 01 is the
  word value 0x40000000 and 10 is 0x80000000 (the same pattern shifted right by T for other types).
- Word 1, bits 16-23 (byte +6): a tag byte. Nonzero means "force mixed" for the next level up (3.5). Only the rect-tag fill
  (0x6F4852C0, value passed in by 0x6F488420) writes it. The loader reads it from the stream (section 6).
- Word 1, bits 0-15: the region id used by the search (pathing-3, LIKELY), 0 at load.
- Stored form (save): the 16-bit value of bits 16-31 (low byte = tag, high byte = states). The loader does
  word 1 = value << 16 (CONFIRMED in 0x6F498960).

### 3.4 Level-0 rule (CONFIRMED in Unicorn: 9,600 cases, 0 mismatches)
For a level-A record at (x, y), and type T:
- Look at the four cells (2x, 2y), (2x+1, 2y), (2x+1, 2y+1), (2x, 2y+1). Each counts as blocked when the cell test (3.1)
  returns 0. Out-of-range cells count as blocked.
- 4 blocked: state 01. 1 to 3 blocked: state 10. None blocked: no store (the field stays 00 because the caller cleared
  byte +7 first).
- The record's stamp is untouched.
So a level-A record covers a 2x2 block of cells (LIKELY from the 2x2 indexing and the (2x, 2y) mapping, see section 9).

### 3.5 Upper-level rule (CONFIRMED in Unicorn: 9,600 cases, 0 mismatches)
For a level k record at (x, y), built from the four level k-1 records at (2x, 2y), (2x+1, 2y), (2x+1, 2y+1), (2x, 2y+1):
- Child classification for type T:
  - BAD: out of range, or the child's state is 01.
  - BLOCK: the child's state is 10, or its tag byte is nonzero (and its state is not 01).
  - GOOD: state 00 or 11, tag byte zero.
- Any BLOCK child gives state 10 immediately, whatever the other children are.
- Otherwise count the BAD children: 0 gives no store (00), 4 gives 01, 1 to 3 gives 10.
- The tag byte of the parent is not written by the builder.

---

## 4. Build order at map load

### 4.1 Full rebuild (CONFIRMED call chain)
1. 0x6F468C70 with ECX = 0 loads G from 0x6FAB73D8 and calls 0x6F488330(G, rect = 0, flag = 0).
2. 0x6F488330 takes the rect from C+0x54..0x60 (the rect is read as x0, y0, x1, y1).
3. It calls 0x6F485150(G, rect, level A, flag 0). That routine clears byte +7 of every level-A record in the rect and then,
   for flag 0, runs the level-0 rule (3.4) for each type T = 0, 2, 4, 6 with the masks in 3.2.
4. It calls 0x6F4881C0 three times: A to B (0x240), B to C (0x244), C to D (0x248). Each call applies the upper-level rule
   (3.5) on the same rect, clearing byte +7 of the parent first.
Rect conversion inside 0x6F485150 and 0x6F4881C0 goes through 0x6F47D5D0 and 0x6F483C40. Those multiply by a per-level value
at +0x68 and convert floats to integers with floor and truncate. The end coordinate is the floor of the maximum plus 1, so
the rect is half-open (LIKELY). The per-level scale is UNSURE.

### 4.2 New game (CONFIRMED call sequence)
- 0x6F5C4030 (no saved data) calls 0x6F3B0750, which calls 0x6F01DE80(1, 1) (the wpm read and first build, section 1).
- Later 0x6F3B0750 calls 0x6F3A2920 (world init, at 0x6F3B0AD7). That routine calls 0x6F2CF910 (not traced), 0x6F468C50
  (which resets G via 0x6F4814D0, not traced), and then 0x6F468C70 with ECX = 0 (the full rebuild again).
- Therefore the pyramid is built twice on a new game. Whether the second build sees doodad and destructable changes to
  cells is UNSURE (it depends on what 0x6F2CF910 writes).
- The reset 0x6F4814D0 may release the level objects, in which case the second build creates them again. UNSURE.

### 4.3 Saved game (CONFIRMED call sequence)
- 0x6F5C0EE0 calls 0x6F3AFE90 (the load path). That routine calls 0x6F01DE80 (the same wpm read and first build), then
  0x6F471460 (pathing reader), with a callback. Then 0x6F3A2920 (world init) runs.
- 0x6F471460 calls 0x6F4814F0 (releases G+0x234..0x250, zeroing the slots), then reads the header and the object entries,
  then 0x6F485790, which reads and checks each section by its byte count.
- 0x6F485790 calls 0x6F481340, which reads the four levels from the stream: each record gets stamp 0 and a 16-bit value
  (CONFIRMED in 0x6F498960). The saved pyramid therefore overrides the fresh build from cells. No level-0 or upper rule
  runs on load.
- The matching writer is 0x6F46DB30, called from 0x6F3AF8D0 (the save driver is 0x6F43EF70). It writes each record's high
  halfword (LIKELY, from pathing-3).

---

## 5. Runtime updates

### 5.1 Rect rebuild entry (CONFIRMED mechanics)
0x6F488330(G, rect, flag):
- flag 0: normal rebuild of the rect (4.1 steps 3 and 4).
- flag 1: level A's byte +7 is cleared in the rect and the upper levels are rebuilt from that. This erases the states in
  the rect. The level-0 test is skipped (0x6F485150 gates the writes on flag = 0). Purpose LIKELY "remove before a change".
0x6F468C70 with a nonzero ECX (pointer to a world-space rect) converts the rect to cells and calls 0x6F4883D0, then
0x6F488330 with flag 0 (CONFIRMED structure). Its callers are 0x6F2AD0C0, 0x6F2AD300, 0x6F2AD400 and 0x6F2AE690 (the form of each call was not verified; these are not traced
to a native name; UNSURE which natives they serve).

### 5.2 Unit footprint update (CONFIRMED structure, LIKELY purpose)
0x6F475560 is called from 0x6F4758D0, which the reverse call graph ties to these natives: CreateUnit, CreateCorpse,
ReviveHero, CreateBlightedGoldmine and PlaceRandomUnit (via 0x6F2D1560 and related). The steps:
1. Phase 1: for each footprint object in the list, call 0x6F488330 with the object's rect and flag 1 (erase).
2. Call 0x6F49A8F0, a path-request-style placement query with a default radius of 0x80 (128 units). It returns -1 when no
   free spot is found. LIKELY used to place the unit.
3. Phase 2: for each footprint object, call 0x6F488330 with the same rect and flag 0 (recompute).
4. If a placement result is needed and the query returned -1, return -1.
The step that stamps the new footprint into the cell words is not located (UNSURE). It is not in 0x6F475560 itself. The
likely candidate is 0x6F47C1A0, which writes object flags into +0x34 from a cell-word array (its callers were not traced).

### 5.3 Natives that do not reach a rebuild (LIKELY)
A forward walk of direct calls (depth 8; reached-set sizes 1 to 931 nodes per root) from these native implementations does not reach
0x6F488330, 0x6F468C70, 0x6F475560, 0x6F4881C0, 0x6F485150 or 0x6F4852C0:
- SetTerrainPathable (0x6F3B42F0; it writes cell top bits through 0x6F46BD10 and 0x6F468DF0)
- SetUnitPathing (0x6F3C5FA0)
- CreateDestructable (0x6F3BB790), RemoveDestructable (0x6F3C4730)
So LIKELY these change cell words only, and the pyramid is stale for the search until a later rebuild. Indirect calls
(vtables) are invisible to the scan, so this is UNSURE in the strict sense.

### 5.4 Other runtime writers (CONFIRMED that they exist)
- 0x6F4852C0 fills the tag byte (+6) of every record in a rect with a value (no rebuild).
- 0x6F488420 does a tag fill followed by a rebuild, with the rect clamped to C+0x54..0x60. Its only caller is 0x6F4884D0
  (not traced).
- No function was found that updates a single pyramid record in place outside these rect routines.

---

## 6. Stored layout (save and load)
- Per level: the stream gives one 16-bit value per record in row-major order. Stamp is 0. Value bits 0-7 = tag (word 1
  bits 16-23), bits 8-15 = states (word 1 bits 24-31). CONFIRMED in 0x6F498960 and LIKELY for the writer (0x6F4988F0).
- Section sizes are length-prefixed (0x6F485790 checks each section against the consumed bytes). CONFIRMED.

---

## 7. Unicorn confirmation (scratchpad/emu_copy/pyr_check.py)
- Level-0 builder 0x6F483160 with the real mask globals (0x6FA84E84..90), 400 random grids of 1 to 7 cells per side with
  random top bytes, random child origins including out-of-range ones: 9,600 calls, 0 mismatches against the rule in 3.4.
- Upper-level builder 0x6F488010, 400 random child levels of 1 to 7 cells per side with random state fields and random
  tags, random origins including out-of-range ones: 9,600 calls, 0 mismatches against the rule in 3.5.
- Not emulated: 0x6F4881C0 and 0x6F485150 (softfloat rect code), 0x6F46B850 (file I/O), 0x6F498960 (stream),
  0x6F475560 (placement query), and the occupant walk in 0x6F4A0E80.

---

## 8. Constants and addresses (quick reference)
- wpm magic 0x5733504D; header order: magic, u32, width, height; data at offset 16.
- Type OR values for file bytes: 0x02 to 0x13, 0x04 to 0x05, 0x08 to 0x09, 0x20 to 0x20, 0x40 to 0x41, 0x80 or 0x42 to 0x81.
- Per-type masks: 0x06000006, 0x80000080, 0x40000040, 0x04000004 at 0x6FA84E84, 0x6FA84E88, 0x6FA84E8C, 0x6FA84E90.
- Pathing world pointer slot 0x6FAB73D8; 0x6FADB3F8 (float 32.0 at start-up, not a bounds rect; correction 2026-10-11); 0x6FAB7370 = float 0.03 (correction 2026-10-11, was called "name pointer"); origin slot 0x6FAB7368.
- Functions: wpm reader 0x6F46B850; map load 0x6F01DE80; new game 0x6F3B0750; world init 0x6F3A2920; full rebuild entry
  0x6F468C70; rect rebuild 0x6F488330; level-0 builder 0x6F485150 and its cell step 0x6F483160; upper builder 0x6F4881C0 and
  its child step 0x6F488010; cell test 0x6F4A0E80 (wrapper 0x6F4A38C0); tag fill 0x6F4852C0; tag fill with rebuild 0x6F488420;
  footprint update 0x6F475560 (caller 0x6F4758D0); placement query 0x6F49A8F0; load path 0x6F3AFE90; pathing reader
  0x6F471460; release 0x6F4814F0; section loader 0x6F485790; level init from stream 0x6F481340 and 0x6F498960; save writer
  0x6F46DB30; save driver 0x6F3AF8D0.

---

## 9. Corrections and open items

Corrections to earlier specs:
- pathing-3 section 9 U4 ("no code rebuilds coarse levels from level 0") is wrong. The rect builders 0x6F488330, 0x6F4881C0
  and 0x6F485150 build all levels from the cell words (CONFIRMED).
- pathing-3 section 1.2 says 0x6F4A03F0 writes level width and height. It writes zeros (CONFIRMED).
- pathing-3 section 5 table lists the level-0 state 10 as "lookup returns -1". That is the lookup's behaviour and is not
  changed here. The builder never produces 10 for a single cell, only for 2x2 blocks.

Open items, in order:
1. Level dimensions and the link to the search's level-0 object (S+0x1C). Level A is indexed at (2x, 2y) of the cell grid,
   so LIKELY it covers 2x2 cells. The search (pathing-3) uses one cell per level-0 record. The object that provides
   one-cell records is not located. This is the most important open item.
2. Initial values of the cell words (top byte and low 24 bits) before the wpm fill.
3. The step that stamps footprint cells during unit creation (between the phases of 0x6F475560).
4. Which native reaches 0x6F4883D0 with a nonzero rect (0x6F2AD0C0 and relatives), and the caller of 0x6F4884D0.
5. Meaning of the top-byte bits 0x01 and 0x10.
6. Per-level scale used in the rect conversion (the +0x68 field) and the meaning of the bounds stores at 0x6FAB7368+0x6C.
7. Run-time values of the masks after the start-up initialisers (the file image was used, not the initialised image).
