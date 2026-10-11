# Unit type records (Game.dll 1.26a): the per-type data and how it is loaded

Status: reverse engineering for a clean-room port (interoperability), 2026-10-11. Image base 0x6F000000. Written
from the decompile export and disassembly of `Game.dll` (SHA1 c5d8624b...), not committed. Only descriptions are
here: column names, record offsets, rules and addresses. No decompiled text is copied.

Confidence tags: **CONFIRMED** = read directly from decompile or disassembly, or checked in Unicorn; **LIKELY** =
consistent with the code, role inferred; **UNSURE** = not resolved.

Code: `crates/gamedata/src/unittype.rs` (reader and load-time rules), tests in `crates/gamedata/tests/unittype.rs`,
Unicorn check `tools/emu/unittype_int_emu.py`.

---

## 0. Answers in short

1. **+0x210 of the type record is `impactZ`** (the impact height of the unit's weapon, column of `UnitWeapons.slk`).
   It is **not** a move speed and not a per-walk cap. CONFIRMED (the builder stores the value read from the weapon
   row's `impactZ` column at record offset 0x210; the accessor `0x6F29C830` returns that offset).
   This retires the reading in `movement-4.md` that the creation copy into `U+0x88` is a step cap (section 6 below).
2. **Builder**: `0x6F2A6750` (one call per unit). It is driven by `0x6F2A7C30`, which walks the rows of
   `Units\UnitUI.slk`. **Lookup**: `0x6F29A8C0` in the hash table at `0x6FAB445C` (key = raw code, hash `0x6F4C8520`).
   **Column binding**: `0x6F263850`. **Table setup**: `0x6F70FC90` (CUnitDB constructor, vftable at `0x6F97CC8C`).
3. **Speed fields** (`spd`, `minSpd`, `maxSpd`) are copied from `UnitBalance.slk` with no arithmetic and no clamping
   to Misc at load time (CONFIRMED). Their use (per-tick movement) is not found here.
4. **Real numbers in cells** are parsed by an imported text-to-double routine (Storm ordinal 574) and rounded to
   single precision. **Not** the S2R soft-float path used for JASS literals. LIKELY (the import's exact semantics
   are not checkable without `Storm.dll`).
5. **Integer cells**: C `strtol(text, NULL, 0)`, or a four-character raw code in apostrophes. CONFIRMED, and
   checked in Unicorn: 25 of 25 cases, 0 mismatches.
6. **The only load-time arithmetic** is one comparison: `acquire < 1.0` becomes `sight`. CONFIRMED. No division by
   a period, no scaling, no clamp to Misc in the builder.

---

## 1. The record and the table that holds it

- The type table is a chained hash table owned by the CUnitDB object (global at `0x6FAB445C`). Each node is the
  type record. The builder writes fields up to `+0x2B8`; the node allocator is `0x6F2A6190`, its size is not read.
  CONFIRMED that the node is the record.
- Node fields used by the lookup: `+0x00` = hash of the key (the builder stores the hash returned by `0x6F4C8520`
  there), `+0x14` = the raw code (the key). CONFIRMED (the builder writes `puVar7[0]` and `puVar7[5]`).
- Hash `0x6F4C8520`: a rolling hash over the four bytes of the raw code. Each step uses a 16-entry dword table at
  `0x6F9548D0`, indexed by the high and the low nibble of the byte. CONFIRMED structure (disassembly). The bucket
  index is `hash & mask` (mask at table `+0x24`, bucket array at `+0x1C`, 12-byte buckets). CONFIRMED.
  Chains link by relative pointers.
- Accessors: one small function per field, for example `0x6F29C830` returns `+0x210` (`impactZ` of the weapon row
  of the unit type; it is the function called by the creation code `0x6F29F3E0`). Other accessors, for example
  `0x6F29C790` (returns `+0x200..+0x208`, the launch vector) and `0x6F29C7F0` (returns `+0x20C`, launch Z under
  water). CONFIRMED.
- Missing type: the accessors return 0.0 (`DAT_6FAAE470`). CONFIRMED.

## 2. Who builds the records (CONFIRMED unless marked)

1. `0x6F70FC90` (CUnitDB constructor): opens `Units\UnitUI` with key column `unitUIID` (through `0x6F70EC20`, a
   custom database object), and `Units\UnitData`, `Units\UnitBalance`, `Units\UnitAbilities`, `Units\UnitWeapons`
   (through `0x6F700FC0`, which only registers the path and sets a flag). Then 66 further database files are
   registered in a loop (`0x6F701000`, not read; the 66 entries are not part of the unit record).
2. `0x6F263850`: binds the columns. Each binding is `(table, descriptor offset in the object, column name)`. The
   tables are: `Data` at object `+0x10`, `UI` at `+0x18`, `Abilities` at `+0x20`, `Weapons` at `+0x28`,
   `Balance` at `+0x30`. A descriptor is 12 bytes. The weapon columns are bound in loops with the names
   `<prefix>1` and `<prefix>2` (two slots). The list is complete for the record as far as the builder reads it:
   107 single bindings and 27 two-slot loops (LIKELY: every bound column is read; checked by the mapping script).
3. `0x6F2A7C30`: after binding, it counts the rows of `UnitUI` and, for each row, reads the `unitUIID` text and calls
   the builder `0x6F2A6750` with the object as `this`. CONFIRMED (the row walk and the `unitUIID` read). LIKELY that
   the text becomes the raw code in a routine not read (`0x6F4C3F50`). So the list of unit types is the list of
   `UnitUI` rows.
4. `0x6F2A6750` (builder), per unit:
   - looks up the record by raw code (`0x6F29A8C0`); if it exists, nothing is done;
   - checks that the unit exists in `UnitData` (gate); if so, checks the `UnitUI` row, and the other rows
     (Abilities, Weapons, Balance). A missing `UnitUI` row makes the unit invalid ("missing required UI data - not
     valid in game"). Missing `Balance`, `Weapons` or `Abilities` rows are reported as warnings and the affected
     fields get defaults (section 5);
   - allocates the record (`0x6F2A6190`, zero-filled), stores the hash and the raw code, and fills the fields
     (section 4);
   - the gate `0x6F0098F0` (a flag table indexed by a layer number) also decides whether a row is used. Its role is
     UNSURE.

## 3. How a cell is read (accessors and cell store)

Three reader families are used. They are chosen per column by the accessor (CONFIRMED from the call targets):

| Family | Accessor shape (example) | Reader | Result |
|---|---|---|---|
| integer | `0x6F25FA80`-style, one argument (row) | `0x6F25F640` -> `0x6F70C490` | `strtol` / raw code (section 4.4) |
| real, one argument | `0x6F260460`-style | `0x6F25F6F0` -> `0x6F70C4F0` | real from the cell |
| real, output pointer | `0x6F25FFC0`-style, output buffer and row | `0x6F25F7A0` -> `0x6F25B340` -> `0x6F70C550` | real from the cell (the default of the column when the cell is missing) |
| text | `0x6F2601C0`-style, one argument | `0x6F25F870` -> `0x6F708040` | text of the cell |

- Text cells are stored in the cell itself (`cell+0x10`, the text pointer returned by `0x6F708040`). Real values are
  stored at `cell+0x410` (the value returned by `0x6F70C550`). CONFIRMED for the accessors; the cell layout by
  column type is UNSURE beyond these two places.
- Cell lookup walks a layer chain (`0x6F25BBD0` then `0x6F25F640` on the parent) when the row is missing in the
  first layer. LIKELY that the map's layer overrides the base table (not checked).

## 4. The field map

Record offsets are byte offsets from the record start (dword index x 4). "Read as" is the reader family of section 3.
Columns are in the table named in the first column. A `(see note)` row is a weapon column whose name is read from a
string table (the three enum columns; see section 4.2).

### 4.1 Unit-level fields

| Record | Table | Column | Read as | Accessor |
|---|---|---|---|---|
| +0x018 | Balance | `bldtm` | int | FUN_6f260080 |
| +0x01c | Balance | `reptm` | int | FUN_6f2600a0 |
| +0x020 | Balance | `goldcost` | int | FUN_6f25fd20 |
| +0x024 | Balance | `lumbercost` | int | FUN_6f25fd40 |
| +0x028 | Balance | `goldRep` | int | FUN_6f25fd60 |
| +0x02c | Balance | `lumberRep` | int | FUN_6f25fd80 |
| +0x030 | Balance | `bountydice` | int | FUN_6f25fe40 |
| +0x034 | Balance | `bountysides` | int | FUN_6f25fe60 |
| +0x038 | Balance | `bountyplus` | int | FUN_6f25fe80 |
| +0x03c | Balance | `lumberbountydice` | int | FUN_6f25fea0 |
| +0x040 | Balance | `lumberbountysides` | int | FUN_6f25fec0 |
| +0x044 | Balance | `lumberbountyplus` | int | FUN_6f25fee0 |
| +0x048 | Balance | `stockMax` | int | FUN_6f25fde0 |
| +0x04c | Balance | `stockRegen` | real | FUN_6f25fe00 |
| +0x050 | Balance | `stockStart` | real | FUN_6f25fe20 |
| +0x054 | Data | `canSleep` | int | FUN_6f25fa80 |
| +0x058 | Data | `canFlee` | int | FUN_6f25fc40 |
| +0x05c | Balance | `fused` | int | FUN_6f25fdc0 |
| +0x060 | Balance | `fmade` | int | FUN_6f25fda0 |
| +0x064 | Data | `cargoSize` | int | FUN_6f25faa0 |
| +0x068 | Balance | `level` | int | FUN_6f25fce0 |
| +0x06c | Weapons | `castpt` | real | FUN_6f260880 |
| +0x070 | Weapons | `castbsw` | real | FUN_6f2608a0 |
| +0x074 | Data | `death` | real | FUN_6f25fa40 |
| +0x078 | Balance | `regenType` | text | FUN_6f25ff40 |
| +0x07c | Balance | `regenHP` | real | FUN_6f25ff20 |
| +0x080 | Balance | `HP` | real | FUN_6f25ff00 |
| +0x084 | Balance | `mana0` | real | FUN_6f25ff80 |
| +0x088 | Balance | `manaN` | real | FUN_6f25ff60 |
| +0x08c | Balance | `regenMana` | real | FUN_6f25ffa0 |
| +0x090 | Balance | `def` | real | FUN_6f260020 |
| +0x094 | Balance | `defUp` | real | FUN_6f260040 |
| +0x09c | Weapons | `weapsOn` | int | FUN_6f260820 |
| +0x170 | Balance | `STR` | int | FUN_6f260100 |
| +0x174 | Balance | `AGI` | int | FUN_6f260120 |
| +0x178 | Balance | `INT` | int | FUN_6f260140 |
| +0x17c | Balance | `Primary` | text | FUN_6f2601c0 |
| +0x180 | Balance | `STRplus` | real | FUN_6f260160 |
| +0x184 | Balance | `AGIplus` | real | FUN_6f260180 |
| +0x188 | Balance | `INTplus` | real | FUN_6f2601a0 |
| +0x18c | Balance | `sight` | real | FUN_6f2600c0 |
| +0x190 | Balance | `nsight` | real | FUN_6f2600e0 |
| +0x194 | Weapons | `acquire` | real | FUN_6f260840 |
| +0x198 | Weapons | `minRange` | real | FUN_6f260860 |
| +0x19c | Balance | `collision` | real | FUN_6f260320 |
| +0x1a0 | UI | `fogRad` | real | FUN_6f260460 |
| +0x1a4 | UI | `scaleBull` | int | FUN_6f260380 |
| +0x1a8 | Data | `movetp` | text | FUN_6f25fae0 |
| +0x1bc | Data | `buffRadius` | real | FUN_6f25fbe0 |
| +0x1c4 | Data | `deathType` | int | FUN_6f25f980 |
| +0x1cc | Data | `prio` | real | FUN_6f25f9a0 |
| +0x1d0 | Data | `points` | int | FUN_6f25fb80 |
| +0x1d4 | Balance | `spd` | real | FUN_6f25ffc0 |
| +0x1d8 | Balance | `minSpd` | real | FUN_6f25ffe0 |
| +0x1dc | Balance | `maxSpd` | real | FUN_6f260000 |
| +0x1e0 | Data | `turnRate` | real | FUN_6f25f9c0 |
| +0x1e4 | Data | `propWin` | real | FUN_6f25f9e0 |
| +0x1e8 | Data | `orientInterp` | int | FUN_6f25fa00 |
| +0x1ec | Data | `formation` | int | FUN_6f25fa20 |
| +0x1f0 | UI | `occH` | real | FUN_6f260700 |
| +0x1f4 | Data | `fatLOS` | int | FUN_6f25fb60 |
| +0x1f8 | Data | `moveHeight` | real | FUN_6f25fb00 |
| +0x1fc | Data | `moveFloor` | real | FUN_6f25fb20 |
| +0x200 | Weapons | `launchX` | real | FUN_6f2608c0 |
| +0x20c | Weapons | `launchSwimZ` | real | FUN_6f260920 |
| +0x210 | Weapons | `impactZ` | real | FUN_6f260940 |
| +0x214 | Weapons | `impactSwimZ` | real | FUN_6f260960 |
| +0x218 | UI | `blend` | real | FUN_6f2603a0 |
| +0x21c | UI | `walk` | real | FUN_6f260480 |
| +0x220 | UI | `run` | real | FUN_6f2604a0 |
| +0x224 | Balance | `repulse` | int | FUN_6f260240 |
| +0x228 | Balance | `repulseParam` | int | FUN_6f260260 |
| +0x22c | Balance | `repulseGroup` | int | FUN_6f260280 |
| +0x230 | Balance | `repulsePrio` | int | FUN_6f2602a0 |
| +0x25c | Data | `isBuildOn` | int | FUN_6f25fc80 |
| +0x260 | Data | `canBuildOn` | int | FUN_6f25fca0 |
| +0x26c | Data | `nameCount` | int | FUN_6f25fc00 |
| +0x28c | Balance | `isbldg` | int | FUN_6f260300 |
| +0x290 | UI | `special` | int | FUN_6f260720 |
| +0x294 | UI | `hostilePal` | int | FUN_6f260740 |
| +0x298 | Balance | `nbrandom` | int | FUN_6f260220 |
| +0x2b8 | Data | `requireWaterRadius` | real | FUN_6f25fc60 |

Notes on the table above:
- Read but not stored in the record (the builder passes them to other routines): `sort`
  (`UnitData`), `type` and `upgrades` (`UnitBalance`; `upgrades` is also split into a list of ids). CONFIRMED that no
  record store follows the reads.
- Pass 3 (2026-10-11, natives-pending.md section 10): `race` is stored at record `+0x1C0` as the enum of
  `0x6F26E210` (`UnitTypeData::race_id`), and the `tilesets` text sits in the object at `+0x29C` (set through
  `0x6F4C5CF0`; `UnitTypeData::tilesets`, table UNSURE).
- Text fields that the builder turns into enum values through name tables (`regenType`, `movetp`, `defType`,
  `buffType`, `Primary`): the record holds the enum, not the text. The name tables and the enum values are UNSURE
  (not decoded). The defaults used when the cell is missing are UNSURE.
- `targs` and `splashTargs` are text parsed by a target-flag reader (`0x6F255250`, with the message
  "Unknown Target Flag"). The flag values are not decoded here. The unit-level `targType` is parsed the same way and
  masked to `0x1E` (the mask check reports `0xFFFFFFE1`).
- `scaleBull` is read with the integer reader, although its name suggests a real (CONFIRMED by the call target).

### 4.2 Weapon slots

Each unit type has two weapon slots. The slot-2 field is at the slot-1 offset plus 4 (the two slots are interleaved
in dword order). Column names are the prefix with the slot number (`dice1`, `dice2`, and so on).

The table gives the slot-1 offset of each field. The slot-2 offset is that value plus 4. (The builder writes through
a pointer that starts at record `+0xA8` and advances one dword per slot; the even-dword stores give the 8-byte step
between fields.)

| Offset (slot 1; slot 2 is +4) | Column (slot 1 is `<name>1`, slot 2 is `<name>2`) | Read as | Accessor |
|---|---|---|---|
| +0x0a0 | `targs` | text | FUN_6f2609c0 |
| +0x0a8 | `dmgUp` | int | FUN_6f260b70 |
| +0x0b0 | `dice` | int | FUN_6f260b10 |
| +0x0b8 | `sides` | int | FUN_6f260b30 |
| +0x0c0 | `dmgplus` | int | FUN_6f260b50 |
| +0x0c8 | `targCount` | int | FUN_6f260c80 |
| +0x0d0 | `damageLoss` | real | FUN_6f260ca0 |
| +0x0d8 | `spillDist` | real | FUN_6f260cd0 |
| +0x0e0 | `spillRadius` | real | FUN_6f260d00 |
| +0x0e8 | `(see note)` | text | FUN_6f262b00 |
| +0x0f0 | `(see note)` | text | FUN_6f262a90 |
| +0x0f8 | `(see note)` | text | FUN_6f262d30 |
| +0x100 | `rangeN` | real | FUN_6f260a20 |
| +0x108 | `RngBuff` | real | FUN_6f260ae0 |
| +0x110 | `cool` | real | FUN_6f260a50 |
| +0x118 | `dmgpt` | real | FUN_6f260a80 |
| +0x120 | `backSw` | real | FUN_6f260ab0 |
| +0x128 | `splashTargs` | text | FUN_6f2609e0 |
| +0x130 | `Farea` | real | FUN_6f260b90 |
| +0x138 | `Harea` | real | FUN_6f260bc0 |
| +0x140 | `Qarea` | real | FUN_6f260bf0 |
| +0x148 | `Hfact` | real | FUN_6f260c20 |
| +0x150 | `Qfact` | real | FUN_6f260c50 |
| +0x158 | `mindmg` | real | FUN_6f260d30 |
| +0x160 | `maxdmg` | real | FUN_6f260d60 |
| +0x168 | `showUI` | int | FUN_6f260d90 |

### 4.3 Column-to-field summary for the most used fields

| Engine meaning | Table and column | Record offset | Read as |
|---|---|---|---|
| move speed (per second) | `UnitBalance` `spd` | +0x1D4 | real |
| minimum move speed | `UnitBalance` `minSpd` | +0x1D8 | real |
| maximum move speed | `UnitBalance` `maxSpd` | +0x1DC | real |
| turn rate | `UnitData` `turnRate` | +0x1E0 | real |
| collision size | `UnitBalance` `collision` | +0x19C | real |
| acquire range | `UnitWeapons` `acquire` (after the sight rule) | +0x194 | real |
| sight / night sight | `UnitBalance` `sight` / `nsight` | +0x18C / +0x190 | real |
| hit points, regen | `UnitBalance` `HP`, `regenHP` | +0x080, +0x07C | real |
| mana, max mana, regen | `UnitBalance` `mana0`, `manaN`, `regenMana` | +0x084, +0x088, +0x08C | real |
| armor, armor upgrade, armor type | `UnitBalance` `def`, `defUp`, `defType` | +0x090, +0x094, +0x098 | real, real, enum |
| food made / used | `UnitBalance` `fmade` / `fused` | +0x060 / +0x05C | integer |
| gold / lumber cost | `UnitBalance` `goldcost` / `lumbercost` | +0x020 / +0x024 | integer |
| build time / repair time | `UnitBalance` `bldtm` / `reptm` | +0x018 / +0x01C | integer |
| impact height (the `+0x210` value) | `UnitWeapons` `impactZ` | +0x210 | real |
| launch offset x, y, z | `UnitWeapons` `launchX`, `launchY`, `launchZ` | +0x200, +0x204, +0x208 | real |
| weapon 1 dice / sides / flat bonus | `UnitWeapons` `dice1`, `sides1`, `dmgplus1` | +0x0B0, +0x0B8, +0x0C0 | integer |
| weapon 1 cooldown / damage point / backswing | `UnitWeapons` `cool1`, `dmgpt1`, `backSw1` | +0x110, +0x118, +0x120 | real |
| weapon 1 range | `UnitWeapons` `rangeN1` | +0x100 | real |
| weapon 1 minimum / maximum damage | `UnitWeapons` `mindmg1`, `maxdmg1` | +0x158, +0x160 | real |
| weapon 2 (same columns with `2`) | | the slot-1 offset + 4 | |

### 4.4 Number formats of the cells

- Integer cell (`0x6F70C490` -> `0x6F4CE050`): if the text starts with an apostrophe, it is a raw code of up to four
  characters, packed big-endian (`'hfoo'` = `0x68666F6F`); the closing apostrophe or the end of the text stops it.
  Otherwise the text is passed to `strtol(text, NULL, 0)`: leading spaces, optional sign, `0x` hex, leading `0`
  octal, decimal; parsing stops at the first character outside the base; overflow clamps to the 32-bit limits.
  CONFIRMED (the code is short and calls `strtol` directly through its import slot `0x6F86D3DC`, MSVCR80).
  **Checked in Unicorn**: `tools/emu/unittype_int_emu.py`, 25 cases (signs, spaces, hex, octal, bad digits,
  overflow, raw codes, unterminated and empty quotes), **0 mismatches**.
- Real cell: the text reaches `0x6F4D2000`, which turns the text into a string pointer (`0x6F4D1EB0`; a missing text
  gives 0.0) and calls import ordinal 574 of `Storm.dll` (import slot `0x6F86D610`), reading the result from the
  x87 stack and storing it as a single-precision value (`fstp dword`). CONFIRMED structure; the import is a
  double-returning text routine (LIKELY an `atof`-like conversion). UNSURE: its handling of text that is not a number
  (the implementation here takes the leading decimal number, which is what `atof` does), its handling of hexadecimal
  and special values, and whether it rounds the decimal correctly. Storm.dll is not available, so this part is not
  checked.
  **Implemented model (2026-10-11, LIKELY)**: C `atof` of the text (strtod prefix in the C locale: leading white space,
  optional sign, digits, optional fraction, optional exponent with digits; the rest is ignored; no number gives 0.0),
  correctly rounded to f64, then rounded to f32 (`gamedata::real::atof_f32`). Hexadecimal floats, `inf` and `nan` are
  not accepted (UNSURE); an overflowing prefix gives infinity (UNSURE). Both `gamedata::unittype` and
  `gamedata::combat` (through `unittype`) use it. Misc keeps S2R.
- The S2R soft-float routine (`0x6F6F0520`, `docs/spec/softfloat.md`) is used by other paths (JASS literals, some
  natives). It is **not** on the cell path. (The gamedata crate's `real.rs` and `combat.rs` use S2R; this table
  says the unit cells do not. The difference matters for the last bit of some values.)

## 5. Load-time rules (the whole list)

1. **Acquire**: when the balance row is missing the acquire value is set to 1.0 (the builder does this after reading
   the weapon row). Then, if the value is below the constant 1.0 (`0x6FAAE4F4`), it becomes the sight value
   (`+0x18C`). CONFIRMED (the decompile compares the `+0x194` float with the constant and copies `+0x18C`).
2. **Missing weapon row**: the unit-level weapon fields are zero, acquire becomes 0.0 before rule 1 (so it becomes
   sight), and both weapon slots get `dmgpt = 1.0` and `showUI = 1`. CONFIRMED.
3. **Missing balance row**: the balance fields are zero and acquire is 1.0. CONFIRMED.
4. **No clamp or scale** of speed, turn rate, acquire or range at load. No call to the Misc reader. No division by a
   period. CONFIRMED: the builder contains no arithmetic instruction on these fields except the comparison in rule 1.
   The clamp to `MinUnitSpeed`/`MaxUnitSpeed` and any turn scaling, if they exist, happen where the values are used.
5. **Units**: the list of units is the list of `UnitUI` rows. A unit without a `UnitUI` row is invalid. A unit
   without a `UnitData` row is not built. CONFIRMED.

## 6. Movement consequence (see `movement-4.md`, section 9)

- `+0x210` is `impactZ`. The creation code copies `+0x200..+0x210` into the unit object (launch vector to `+0x7C`,
  `+0x80`, `+0x84`; impact Z to `+0x88`). So `unit+0x88` is the impact height, not a per-walk cap. CONFIRMED.
- The per-walk step cap in `movement-4.md` (section 3) is therefore not `+0x210`. Which per-type value is the cap, if
  any, is not found. The per-type speed inputs are `spd`, `minSpd` and `maxSpd` (`+0x1D4`, `+0x1D8`, `+0x1DC`), read
  with no change. How they reach the mover is open.

## 7. Open items

1. Storm ordinal 574 (the real text routine): identify it with `Storm.dll` (not in the container).
2. The enum name tables for `movetp`, `defType`, `buffType`, `regenType`, `Primary`, `atkType`, `weapType`,
   `weapTp` and the target flags: names and values.
3. The cell layout by column type and the defaults of missing cells (the default object of each column is read
   through the column descriptor; its values are runtime data).
4. The gate `0x6F0098F0` and the layer chain `0x6F25BBD0`: which layer wins (base or map).
5. The record allocator size and the hash-table growth rules (not needed for the fields).
6. Creation (`0x6F29F3E0`): which getter feeds which unit field (about 60 getters; only `0x6F29C790` (launch vector),
   `0x6F29C830` (impact Z) and `0x6F29C7F0` (launch Z under water) are identified here).
7. Dynamic check: run the builder on the user's `UnitUI`/`UnitData`/`UnitBalance`/`UnitWeapons` under Wine and dump
   the record at `+0x210` for a known unit (expected: its `impactZ`).

## 8. Verification

- Column binding: 107 single bindings and 27 two-slot loops read from the binder decompile. The mapping scripts (kept in
  the analysis scratchpad, not committed) read each accessor's descriptor offset by disassembly. Every accessor call in the builder
  that reads a column was resolved to a descriptor or a fixed base (the three weapon readers that call through a
  generic reader were resolved from that reader's base constant). The other calls are existence tests and releases.
- Accessor-to-field pairs for the fields in sections 4.1 and 4.2 are read from the builder's stores (decompile) and
  the accessor displacement (disassembly, for example `0x6F260940` uses `lea eax, [ecx+0x644]` then calls
  `0x6F25F7A0`).
- Unicorn: the integer cell reader (25 cases, 0 mismatches). The full builder was not run (it needs the whole
  CUnitDB state; the cell reader and the Storm import are stubbed for that case).
- Rust: `crates/gamedata/tests/unittype.rs` (10 tests, synthetic tables): the strtol and raw-code cases from the
  Unicorn run, the real-cell rule, the field map on a synthetic record, the acquire rule, the missing-row defaults and
  errors, and the ordering of `load_all`.
