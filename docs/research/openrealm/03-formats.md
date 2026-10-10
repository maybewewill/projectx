# OpenRealm: Warcraft III file-format survey

Repo: `/home/user/corepunch/open-realm` (read-only; nothing in the repo or `/home/user/projectx` was modified).
Line numbers are from the checked-out tree.

## 1. MPQ archive reader

**Location:** `common/mpq.c` (2331 lines), API in `common/mpq.h`. In-tree, no StormLib.
Public surface: `SFileOpenArchive` / `SFileOpenArchiveFromMemory`, `SFileOpenFileEx`, `SFileReadFile`, `SFileFindFirstFile`, plus a writer (`SFileCreateArchive`, `SFileAddFile*`, zlib-compressed sectors only). Hash lookup is a cached path lookup with a retail-style probe (`FindBlockIndex`, ~603).

**Compression masks** (defined at `common/mpq.c:47-53`):

| Mask | Algorithm | Status in code |
|------|-----------|----------------|
| 0x02 | zlib | Yes. `TryInflateSector` (1089) and `Mpq_DecompressSector` (265-270). |
| 0x08 | PKWARE DCL (blast) | Yes, but only when the byte is exactly `0x08`. `blast()` is called at 269 with bounded output callbacks (251-259). |
| 0x40 | Blizzard ADPCM mono | Yes, exact match only (276-277), `Mpq_AdpcmDecode` (215-249). |
| 0x80 | Blizzard ADPCM stereo | Yes, exact match only (276-277). |
| 0x01 | Adaptive Huffman | Only in the combined form `0x41` / `0x81` (278-285), i.e. Huffman then ADPCM. A standalone `0x01` sector is rejected. |

- **Huffman** (`Mpq_HuffBuild` 129, `Mpq_HuffDecode` 186): the code only accepts stream profile byte `7` (`if (profile != 7) return false;` at 130). The initial tree comes from the hardcoded `mpq_huff_profile7` table (81-85). Adaptive updates are in `Mpq_HuffInsert` (167) and `Mpq_HuffRebalance` (152). The implementation is a full FGK-style adaptive tree, not a stub.
- **ADPCM** (215-249): the step table (92-98) and index table (87-90) are present, and the 0x80/0x81 escape codes are handled.
- **Unsupported masks** (e.g. 0x10 LZMA, 0x20 sparse, or combined masks such as 0x09) print `MPQ: unsupported sector compression mask` and fail (286-289).
- **Legacy file flag `MPQ_FILE_IMPLODE` (0x100)** is defined at 40 but never tested anywhere in `mpq.c`. Per-sector method bytes are what matter for WC3 archives.

**Where each codec is used:**

- Multi-sector files: `SFileReadFile` (about 1800-1845) tries `TryInflateSector` first, then falls back to `Mpq_DecompressSector` (1830-1834). A failed decode with `compressed >= uncompressed` is silently copied raw.
- Single-unit files (`MPQ_FILE_SINGLE_UNIT`, 1732-1765): only `TryInflateSector` (zlib) is tried, then raw copy when the block is big enough. **Single-unit PKWARE, Huffman and ADPCM payloads are not decoded by this path.** This is from reading the code; I did not run a sample.

**Vendored code and licenses:**

- `vendor/blast/blast.c` and `blast.h`: verbatim Mark Adler `contrib/blast` from zlib tag v1.3.1 (blast 1.3). zlib license, retained in the header. Documented in `docs/vendored-dependencies.md`. Built into `libshared` and into `COMMON_GAME_SRCS` (`Makefile:182`).
- zlib itself is **not vendored**. It is linked as system `-lz` (`Makefile:244`, `263`; `common/mpq.c:12` includes `<zlib.h>`).
- Huffman and ADPCM are **in-tree code**, not from a library.

## 2. Format parsers (one line each)

**MDX / MDL (models).** `games/warcraft-3/renderer/mdx/r_mdx_load.c` (plus `r_mdx_*.c` in the same directory, about 3450 lines total). Entry `R_LoadModelMDLX` (759), called from `games/warcraft-3/renderer/r_game.c:1419-1421` after an `ID_MDLX` magic check. Block table `R_MDLX` (703). Generic block walker `MSG_ReadBlock` (144-166) skips unknown tags. Coverage is broad: VERS, MODL, SEQS, GLBS, MTLS, TEXS, GEOS, GEOA, BONE, HELP, ATCH, PIVT, CAMS, EVTS, CLID, LITE, PREM, PRE2, RIBB, and the KGTR/KGRT/KGSC/KGAO-style keyframe tracks. Tags not found by grep: SNDS, BPOS, FAFX, CORN. **Gaps:** PREM model-particle emission is parsed but not run (TODO at `r_mdx_load.c:774`; a one-time warning is printed). There is **no MDL text-format parser**. Tests: `tests/test_renderer_model.c` (131 TEST cases, including PREM and RIBB loader tests) and `tests/test_mdx_texture.c`. Completeness: good for runtime rendering; sound events and some emitter variants are missing.

**BLP (textures).** `renderer/r_blp1.c` (284 lines) and `renderer/r_blp2.c` (362 lines). Dispatch by magic at `renderer/r_main.c:266-273` (BLP1, BLP2, DDS).
- BLP1: JPEG-compressed (`blp1_convert_jpeg`, 170) via vendored public-domain `renderer/stb/stb_image.h` (v2.30), plus paletted variants with alpha 0/1/4 (208-290).
- BLP2: encodings 1 (paletted, alpha 0/1/4/8), 2 (DXT1/3/5, decoder in-file around 314-362), and 3 (raw BGRA). Mip levels are computed (`blp2_nbMipLevels`, 46).
- Completeness: looks complete for WC3 classic textures. Tests are indirect: `tests/test_renderer_model.c` uses `blpgen` fixtures (`test_renderer_game.c` only mentions BLP in passing). I did not find a dedicated BLP decoder unit test.

**W3E (terrain).** Two readers, duplicated:
- Renderer: `games/warcraft-3/renderer/w3m/r_war3map.c:747-771` (`FileReadWar3Map`, called at 817). Vertex decode `R_ReadWar3MapVertex` (707).
- Common: `common/world.c:696-720` (`CM_ReadHeightmap`), vertex decode `CM_ReadWar3MapVertex` (656-686). This one is active: it is in the `cm_w3_readers[]` table at `games/warcraft-3/common/world_w3.c:176`, and the static functions come in through the unity build. The `__attribute__((unused))` tag on it is stale.
- Vertex bitfields (656-686) match the public spec: 14-bit water level plus edge bit; ground (4 bits), ramp, blight, water and boundary flags; variation; cliff and layer nibbles.
- **Gaps:** neither reader checks the `W3E!` magic or the version field. The renderer reader ignores the result of `SFileOpenFileEx` (750) and of every `SFileReadFile` (751-759). Completeness: full terrain data. Robustness: weak on malformed input.

**W3I (map info).** `common/world.c:63-250` (`CM_ReadInfoInto`), struct `common/mapinfo.h:205`. Field presence is gated on `fileFormat` at 75, 90-129, 150, 225 (versions 25, 28, 31, 32, 33). A second reader exists in `games/warcraft-3/menu/menu_map_data.c:39-110` for the map list. Completeness: broad for ROC and TFT. Test: `games/warcraft-3/tests/test_coordinates.c:7` reads `Maps/Transport.w3m`.

**DOO (doodads).** `common/world.c`:
- Magic `W3do`, versions 7 (ROC) and 8 (TFT) at 309-311. Header check in `CM_ReadPlacementHeader` (323-350).
- `CM_ReadDoodads` (456) for `war3map.doo`, with the TFT dropped-item-set block. `CM_ReadUnitDoodads` (630) for `war3mapUnits.doo`.
- Completeness: per-record field reads are present. The record loop does not check the `SFileReadFile` return values (`common/world.c` ~470-500), so a truncated file gives garbage rather than an error. Test: `games/warcraft-3/game/tests/t_doodad.c` (6 TEST cases); I did not read it in depth.

**SLK and TXT / INI (data tables).**
- Shared schema and API: `common/stb_slk.h` (144 lines, row decode plus `FS_SLKLookup` index).
- Implementation: `games/warcraft-3/sheet/sheet.c` (546 lines). SYLK reader `FS_ParseSLK_Buffer` (194-237) handles only `C` and `F` records with `X`, `Y`, `K` fields, which matches `games/warcraft-3/file-formats/slk.md`. INI/TXT reader `FS_ParseINI_Buffer` (263). Typed loader `FS_LoadSheetTyped` (395). Public `Stb_SlkLoad` / `Stb_IniCacheLoad` (484-540). Tokenizer in `sheet/parser.c` (93 lines).
- Consumers: schema tables in `games/warcraft-3/game/g_metadata.c` (for example Doodads at 547-570). Map-driven loads in `games/warcraft-3/renderer/r_game.c` (Terrain.slk, CliffTypes.slk, Water.slk, UI\\WorldEditData.txt) and `sound/s_sound.c:347` (UISounds.slk).
- Completeness: good. Tests: `games/warcraft-3/game/tests/t_slk.c` (87 TEST cases, including CR-only INI lines, duplicate keys, and map-archive overrides).

**FDF (UI frames).** `games/warcraft-3/common/stb_fdf.h` (2073 lines).
- Tokenizer: `parse_token` (542) and friends.
- Frame parser: `FDF_ParseFrame` (1826). Scene parser: `FDF_ParseScene` (1936). Buffer entry `UI_ParseFDF_Buffer` (1956), delimiters `,;{}`.
- File loader around 1990-2030 handles UTF-16LE with a BOM and marks files as loaded.
- Host services: `games/warcraft-3/menu/menu_fdf.c` (186 lines). Callers: `games/warcraft-3/menu/menu_main.c:361-370`.
- Generated bindings: `tools/fdfbindgen.c` (768 lines) writes `games/warcraft-3/game/generated/*.h`.
- Completeness: broad (setpoints, inheritance, templates, text, models, textures, UTF-16). Tests: `games/warcraft-3/tests/test_menu_fdf.c` (100 TEST cases). The docs are in `docs/games/warcraft-3/file-formats/fdf.md`.

## 3. W3G replays

**Not implemented.** No `.w3g` reader exists. The only related pieces:
- `games/warcraft-3/game/generated/replay_panel.h` is UI-only generated code.
- `games/warcraft-3/game/api/api_misc.h:856` (`DoNotSaveReplay`) is a stub that says `TODO: replay recording not yet implemented`.
- `games/warcraft-3/menu/screens/single_player.c:1055` sets an empty `ViewReplayButton` click handler.
- `docs/games/warcraft-3/file-docs/readme.md:51` links to an external W3G description. The repo has no `w3g.md`.

## 4. Test fixtures and the MPQ rules

**Rules** (`CONTRIBUTING.md`, "Test Fixtures and MPQ Assets"):
- Tests must not read `War3.mpq` or ignored local extraction folders.
- WC3 fixtures live in `games/warcraft-3/tests/resources-src/` (121 files). They are packed into `build/tests/tests.mpq` by `make test-assets` (`games/warcraft-3/game.mk:353` onward).
- Replacement files keep their retail name and path.

**How fixtures are produced:**
- Textures: `tools/blpgen.c` (deterministic BLP2 generator), called from `game.mk:356-365`.
- Models: `tools/mdxgen.c` (synthetic MDX with sequences such as `morph`, `anim_pulse`, `doodad`), called from `game.mk:368-380`.
- Maps: `tools/wc3fixturegen.py` writes a generated W3I v18 map plus W3E, then `mpqtool` packs it into `Maps/Transport.w3m` (`game.mk:391-395`).
- Checked-in fixtures: SLK and TXT tables (for example `Units/UnitBalance.slk`, `UI/MiscData.txt`), FDF files, and `UI/Glues/SpriteLayers/TopLeftPanel.mdx` (8196 bytes, synthetic `MDLX` with a `SEQS` block).
- Placeholder maps: `Maps/FrozenThrone/TwinRivers.w3x` (43 bytes) and `Maps/Melee/TwinRivers.w3m` (35 bytes) are text stubs (for example `fake map fixture: TwinRivers frozen throne`). They are only used by menu name and listing tests.

**Tests that run under `make test`:**
- `tests/test_mpq_compression.c` (`test-mpq-compression`, `games/warcraft-3/game.mk:322`). It covers the PKWARE sector vector, rejection of truncated input and overflow, sector layout of a written archive, and the retail hash probe. It is self-contained.
- The `make test` aggregate target (`games/warcraft-3/game.mk:311-314`) runs `test-mpq-compression` and the WC3 suites that use `tests.mpq`.

**Tests that need retail data (not in `make test`):**
- `games/warcraft-3/tests/test_mpq_compat.c` (`test-mpq-compat`, `game.mk:336`). It contains inline synthetic ADPCM mono and stereo vectors (264-300), but then opens `data/Warcraft III/War3.mpq` (default path at 67, open at 306) and exits on failure. The Huffman plus ADPCM path is only covered by a retail unit sound (about 360). This target is **not** in the `make test` list.

**Things that differ from CONTRIBUTING.md (observations, not fixed):**
- `tests/test_mpq_compression.c:10` and `:54` use fixed `/tmp/openrealm-mpq-*-XXXXXX` paths. CONTRIBUTING asks for `Test_TempPath("name")`.
- `test_mpq_compat.c` reads retail `War3.mpq`, which CONTRIBUTING forbids for tests. It is kept out of `make test`, which limits the damage.

**Coverage gaps for format parsing without retail data:**
- No in-tree test for standalone Huffman, the `0x41`/`0x81` Huffman+ADPCM path, or standalone ADPCM through the real archive read path. ADPCM vectors exist only in the retail-dependent binary.
- No test for the single-unit PKWARE / Huffman / ADPCM gap in section 1.
- No dedicated BLP decoder unit test.
- No `W3E!` magic or version rejection test, and no malformed-DOO test.
