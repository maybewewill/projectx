# projectx — WC3 1.26a TFT engine rewrite in Rust (agent handoff)

> A living document. **Every agent updates it at the end of a session**: the "Status", "Findings" and "Log" sections.
> Last updated: 2026-10-10

## 1. Goal

1. Rewrite Warcraft III: The Frozen Throne **1.26a** (`Game.dll`) in Rust **1:1 in behavior**:
   the simulation must match the original bit for bit, with no desync against original replays and clients.
2. Then optimize and extend freely (new renderer, WebAssembly/browser, our own features).
3. Target platforms: native (Windows/Linux) and **wasm32** in the browser.

## 2. Hard rules

- Code borrowed from MIT projects (OpenRealm) → note it in `THIRD_PARTY_NOTICES.md` and in the module header.

- **Do not commit decompiled Blizzard code** (Ghidra pseudo-C, disassembly listings) or game assets (MPQ, BLP, MDX, maps, sounds, `Game.dll`) to the repo.
  Only our own code, our own scripts, and *descriptions* of behavior (specs) go in the repo.
- Work flow is "Ghidra → spec in plain words → our own Rust code from the spec" (as close to clean-room as possible).
- At runtime, game data comes from the **user's own copy** of the game (the user's own MPQ files).
- The simulation must stay deterministic: no `HashMap` with random seeding, no system time, no float ops whose result depends on the platform.
- Work branch: `ccr-2af97213-0yojbn` (or the one set in the session).

## 3. Architecture (plan)

```
crates/
  w3g/        replay parser .w3g (1.26a) — the test oracle            [v0.1: header, blocks, players, slots, seed, ticks; actions raw]
  mpq/        MPQ archive reader                                        [v0.2: + Huffman (profile 7) and ADPCM mono/stereo, ported from OpenRealm]
  wc3-math/   bit-exact WC3 math (x87 rounding, software sin/cos/sqrt)  [todo — waiting on Ghidra]
  jass/       JASS lexer/parser/VM                                      [v0.3: + bytecode compiler and VM (8-byte instructions, threads, sleep, 300k budget)]
  sim/        deterministic simulation (units, abilities, pathing)      [todo]
  blp/        BLP1 textures → RGBA8 (paletted + JPEG), builds for wasm32   [v0.1, Haiku agent]
  gamedata/   SLK tables + profile .txt (INI), merging layers, deterministic BTreeMap [v0.1, Haiku agent]
  w3map/      map files: w3e terrain, w3i info (v25), doo doodads (v8/11), shd, wpm [v0.1, Haiku agent]
  mdx/        MDX v800 models: SEQS/GLBS/TEXS/MTLS/GEOS/BONE/HELP/PIVT + tracks; other chunks raw [v0.1, Haiku agent]
  viewer/     MDX/BLP viewer on wgpu 30 + winit 0.30, native + wasm32 (WebGPU/WebGL2) [v0.1, Haiku agent; never actually drawn]
  inject/     i686-pc-windows DLL: hooks Game.dll functions with Rust versions
              (incremental replacement inside the running original, OpenRCT2 approach) [todo]
tools/
  ghidra/     our Ghidra headless scripts
  analysis/   our Python scripts for PE analysis (pefile + capstone)
```

**Two halves of the engine:**
- **Simulation**: units, abilities, damage, pathfinding, orders, JASS VM, RNG, timers. **Must match bit for bit.**
- **Presentation**: rendering, UI, sound, network transport. Only needs to *behave the same*, so rewrite it freely.

**Correctness strategy:** (the Unicorn harness `tools/emu/` already works: we call an original Game.dll function and compare it with the model)
1. Replace functions one at a time inside the running original (Wine + DLL injection, same 32-bit ABI).
2. Test oracle: `.w3g` replays (initial state + seed + all commands). Run them through the original and through ours, then compare state every tick.
3. When the whole simulation is in Rust, drop Game.dll and build for wasm.

## 4. Findings (research)

### 4.1 Game.dll binary
| | |
|---|---|
| SHA1 | `c5d8624b67129dd156bfbb64734f94c84d828605` |
| Size | 12,042,240 bytes; PE32 DLL, i386, ImageBase `0x6F000000`, export `GameMain` |
| Build | TimeDateStamp `0x4D83BB00` (2011-03-18), MSVC 2005 (MSVCR80), PDB `e:\Drive1\temp\buildwar3x\War3\bin\Game.pdb` |
| `.text` | 8.6 MB, ~3.56M instructions (linear sweep) |
| Protection | **none**: entropy 6.37, no packer or VM obfuscation |
| RTTI | **5,369 class names** (`CUnit`, `CAbilityAttack`, `NTempest::CDynTable<…>`, `NIpse::CPaCell`, …) |
| Source paths | ~400 references to `.cpp/.h` (assert strings). Root paths: `war3\source\*` (225), `engine\source\*` (26) |
| JASS | ≥662 native signatures found by regex (the real count is ~1000+; the regex is approximate) |
| Imports | Storm.dll (112), mss32/Miles (110), KERNEL32 (111), USER32 (104), OPENGL32 (50), WSOCK32 (25), MSVCR80 (85), … |

### 4.2 Ghidra 11.4.2 (headless, full auto-analysis)
- **70,526 functions**, 6.5 MB in function bodies.
- Size buckets <32 / <128 / <512 / <2K / <8K / ≥8K bytes: **30,915 / 25,896 / 12,256 / 1,375 / 82 / 2**.
  44% are tiny (SEH `Unwind@…`, getters, wrappers). The meaningful work is ~15–25k functions.
- Auto-named (not `FUN_`) before RTTI recovery: 11,081.
- Decompile on a random sample of 1,500: **100% succeeded, 0 failures, 6% with WARNING**, 8 ms/function, ~21 lines on average.
  Extrapolated: **~1.55M lines of pseudo-C** in total.
- RTTI class recovery (`RecoverClassesFromRTTIScript`) is running; results go in the "Log".

### 4.3 Floating point (critical for determinism)
- x87: **77,147** instructions; SSE: only 103.
- `fsin/fcos/fsincos/fpatan/fptan/fsqrt/f2xm1/fyl2x/fprem/frndint` = **0 inside Game.dll** — **but that is because math is imported from the CRT**:
  Game.dll imports `_CIsin, _CIcos, _CItan, _CIasin, _CIacos, _CIatan, _CIatan2, _CIsqrt, _CIpow, _CIexp, _CIlog10, _CIfmod, floor, ceil` **from MSVCR80.dll**.
  (⚠ The earlier conclusion "math is software inside the DLL" was **wrong**, corrected 2026-10-10.)
  → For bit-exactness you have to reproduce **MSVCR80's** implementation. As far as is known, MSVCR80 chooses between an x87 path (fsin/fcos/fpatan; results depend on the CPU microcode) and SSE2 software implementations depending on the CPU and `_set_SSE2_enable`. **TODO**: get `msvcr80.dll` (the exact version from the game install/WinSxS), analyze `_CIsin` etc., find out which path is used on modern CPUs. `fsqrt` at PC_53 is IEEE-exact (= f64 sqrt), so sqrt is not a problem.
  → Vector helpers (3D length `0x6F301BE0`, distance `0x6F521DD0`, normalize `0x6F011550`, 2D normalize `0x6F0114F0`/`0x6F522230`) use the CRT sqrt, with no zero check.
- Main float→int helper `0x6F7E11E0` (~180 calls): FISTP in the **current** rounding mode (i.e. round-to-nearest, not truncation); the alternative branch depends on the flag `0x6FAE759C`. Which of the 648 `fldcw` sites truncate is unknown.
- `fldcw`: 648 sites (mostly switching rounding mode for float→int; `fistp` = 349).
- Imports `_control87`, `_clearfp`, `rand`, `srand`, `floor` from MSVCR80.
- **FPU mode found**: function `0x6F6CE230` = `_clearfp(); _control87(0x9001F, 0xFFFFF);`
  → **`_PC_53` (53-bit double precision), `_RC_NEAR` (round to nearest), all exceptions masked including denormal.**
  It's called from 8 sites (`0x6F526505` jmp, `0x6F526EF0`, `0x6F526F4E`, `0x6F52CFDB`, `0x6F52CFE7`, `0x6F52D041`, `0x6F52D0BE`, `0x6F78B53E`).
  Most likely the engine resets the FPU mode before simulation steps on purpose (protection against D3D8, which sets `_PC_24` without `FPU_PRESERVE`). TODO: name these callers in Ghidra.
- **Consequence for Rust/wasm**: `float` variables live in memory as `f32`, intermediate x87 values in registers are rounded to 53 bits.
  So: compute in `f64` and round to `f32` **at exactly the points where the original stores the value to memory** (`fstp dword`).
  Differences from pure f64 only at extreme exponents (x87 keeps the 15-bit exponent even with PC_53), which in practice means denormals/overflow. Store points have to be taken from the decompiled code; this is part of each function's spec.
- `Direct3DCreate8`/`d3d8` are loaded dynamically (strings at file offsets `0x9575E4`/`0x9575F4`), not through imports.

### 4.3b JASS front end (spec: docs/spec/jass-frontend.md, Haiku, partly UNSURE)
- Lexer is **flex** (`0x6F45FF30`), parser is **yacc** (stack 600). Integers: decimal/octal (leading 0)/hex (`0x`, `$`), 32-bit wrap with no checks (matches our lexer). Rawcode 1 or 4 characters, with escapes. **String literals are not unescaped in the lexer** (our lexer does it; check where the original does it).
- There is an AST node for `%` (NodeMod) — check whether the grammar accepts it.
- ✅ **Precedence decoded from the yacc tables and checked against the real parser in Unicorn** (docs/spec/jass-precedence.md; 21 named + 60 random expressions match the real engine, 3000 random trees match the table). Loosest first:
  `and` (right) < `or` (right) < `== !=` (left) < `< <= > >=` (left) < `+ -` (left) < `* / %` (left) < prefix `not`, unary `-`.
  **`or` binds tighter than `and`** (`a and b or c` = `a and (b or c)`), `not` is tighter than comparisons (`not a == b` = `(not a) == b`). The grammar accepts `%`. Our parser is **already fixed** to this (test `precedence_matches_original_parser`).
- Symbol table: 1024-bucket hash. The declaration-order rule is not found yet.

### 4.3d JASS VM (spec: docs/spec/jass-vm.md, Haiku, partly UNSURE)
- Interpreter `0x6F45E9D0`. Bytecode: **8-byte instructions** (byte0 = right slot, byte1 = left slot, byte2 = dest/cond, byte3 = opcode, bytes4-7 = imm32), ~40 opcodes.
- Types: 4 integer, 5 real (f32 bits; arithmetic is **claimed** to be software-float with a truncating multiply, UNSURE, important to check), 6 string (16 bytes, ref-counted, interned), 8 boolean, 3 code.
- Integer +,-,* wrap. **Integer division by 0 and real division by 0.0 abort the thread** (status 7). Modulo by 0 gives 0.
- Execution budget: the executor runs at most N instructions; at the call sites N = **300000**. The agent guessed a timer, but **this matches the well-known JASS "op limit" ≈ 300k** → most likely an instruction limit per run (LIKELY). What happens on overflow (thread ends vs continues next tick) needs checking: the spec says status 2 and the thread stays runnable.
- Statuses: 1 done, 2 budget, 3 sleep, 4 yield, 5 function not found, 6 null, 7 divide by zero. Threads are in a circular list and run in list order every tick (`0x6F44B400`, LIKELY). No call-depth limit found.

### 4.3g Software float (important!)
- From the natives analysis: real arithmetic in the JASS VM and in natives goes through **Blizzard's own software float helpers** (multiply `0x6F6EEE20`, Sin/Cos/Tan wrappers `0x6F3B2A10/2A30/2A50` call software helpers `0x6F6EF820/0x6F6EF8D0/0x6F6F0140`, not `_CI*`). `I2R` truncates the mantissa to 24 bits (`I2R(16777219) = 16777218`), not round-to-nearest. S2R parser `0x6F6F0520`. GetRandomReal uses the same helpers.
- → Exact port of the softfloat library is **TOP PRIORITY for determinism**, checked through `tools/emu/`. Where `_CI*` from MSVCR80 is actually used (other code paths) needs a separate look.
- The bodies of all 1,291 natives are restored (`ExportNatives.java` creates functions at the addresses from natives-1.26a.tsv; output `scratchpad/decomp/natives.c`, not in the repo).
- Natives pass 2 (checked): **R2I** = truncation toward 0, saturation at |x|≥2^31 incl. NaN (21,626 cases in Unicorn, 0 mismatches). **StringHash** = MPQ string hash type 0 via Storm ord. 590 (Game.dll's own copy `0x6F68CC10` matches us on 33 cases; that Storm is the same is LIKELY). **S2I** = atoi (leading spaces and sign allowed). **Convert*** = just `return param_1`, no tables. **StringLength/StringCase** substitute `TRIGSTR_nnn` from war3map.wts (not done yet). Storm.dll + msvcr80.dll are needed for full checks.

### 4.3e Native table
- `docs/spec/natives-1.26a.tsv`: **1,291 natives** (1,167 `common`, registered in `0x6F3D4020`; plus 124 AI natives in `0x6F71D9E0`) with signatures and implementation addresses. Extracted by `tools/analysis/extract_natives.py` (parses push name/signature/function before the register call).

### 4.3c Handles (spec: docs/spec/handles.md, mostly UNSURE)
- Containers `Agent2HandleReg` (hash) and `Handle2AgentReg` (array of 12-byte entries with a ref-counted pointer). Base 0x100000 not found as a constant; allocation and reuse order not found. Needs a second pass.

### 4.3f Game RNG (spec: docs/spec/rng.md) ✅ checked by emulating the original
- Natives: `GetRandomInt 0x6F3B3560`, `GetRandomReal 0x6F3B35A0`, `SetRandomSeed 0x6F3B36D0`. **CRT rand() is not used in gameplay** (only UI/particles/editor; srand(GetTickCount) at startup). OpenRealm is wrong here.
- Generator: 8-byte state (A,B); B's bytes index a 256-byte permutation table at `0x6F953650`; mix = rotl(T,3)^rotl(T,2)^T^rotl(T,1); A += mix; output = A. Seeding: A=S, B = S mod 59/61/53 and (17*(S/47)+S) mod 64 in bit fields.
- 45 separate streams (combat = stream 2: dice/evasion/crit/miss; 32/34/35 for ChooseRandom*; 42 for percentages, ...). SetRandomSeed reseeds the global generator and all streams.
- GetRandomInt(lo,hi): lo==hi → lo without a draw; otherwise lo + floor(r*(|hi-lo|+1)/2^32) (lo>hi goes up from lo).
- **Checked in Unicorn against the original** (`tools/emu/`): seed, step, GetRandomInt (300 cases), SetRandomSeed + 45 streams: **0 mismatches**. GetRandomReal: 125/300 differ by ≤1.2e-4 → the original's soft-float helpers still need porting.
- The 256-byte table is **not committed**; the engine should read it from the user's Game.dll (or the original data). Game seed = rotl fold over slots (0x8000 → constant 0x77617233); the link to the seed field in .w3g isn't confirmed yet.

### 4.4 Known desync traps (must be reproduced)
- Order of agent/handle allocation (handle IDs are visible from JASS).
- Iteration order in containers keyed by pointer → the memory layout/allocator has to be emulated.
- RNG: the game's own generator and CRT `rand()`; the seed comes from `.w3g` (GameStartRecord).
- Float→int conversion with rounding mode switched through `fldcw`.

### 4.5 Market research: AI decompilation (2025–2026, mostly secondary sources)
- Snowboard Kids (N64), 2,145 functions: 84 days with agents (the sequel took 596 days without them).
- LSD: Dream Emulator (PS1), 1,446 functions: 32 days.
- CoD MW2 (PC), ~16k functions: ~70% in 2 months, up to 17 agents.
- An Xbox 360 game, **69,301 functions** (≈ Game.dll): 44,226 matched in 10 weeks.
- Caveat: "compiles" ≠ "behaves the same" (arXiv 2609.05370). Differential tests are a must.

### 4.6 Estimate
- RTTI/vtables + AI naming: 1–3 weeks.
- Simulation (~30–40% of the code) to replays running without desync: ~1.5–2 months with an agent swarm.
- Whole DLL: ~3–4 months.

### 4.7 OpenRealm (github.com/corepunch/open-realm): main open-source reference
- MIT, C, ~365k lines, active (commit 2026-10-10). WC3 is the main target and already playable (Windows/Linux/macOS/iPad); also WoW and SC2.
- Architecture is Quake 2-style (client/server, `game.dll` + universal client), **not WC3 lockstep**, so it gives no 1:1 replay compatibility, but it is a big reference for behavior (abilities, damage, JASS natives, FDF UI, formats) and has docs in `docs/games/warcraft-3/`.
- MIT allows borrowing code with attribution (keep the copyright notice). Full reports (Haiku agents, read-only, with file:line references): `docs/research/openrealm/01..04-*.md`.
- **Conclusions for us:**
  - Netcode: authoritative server, 10 Hz (`FRAMETIME=100`), snapshots. No lockstep, no replays, no .w3g parser.
  - RNG: global libc `rand()`, which rendering and sound also draw from → not deterministic. **Not suitable for 1:1.**
  - Retail analysis was done on **game.dll 1.27.1.7085**, not 1.26a → their addresses don't carry over. But the *method* is valuable: `tools/ghidra` + a **Unicorn differential** (runs the original x86 function against C on 12k inputs). Take this for our function-level tests.
  - JASS: AST interpreter; **arithmetic is right-associative** (verified: `jparser.c` is right-recursive, `10-3-2 = 9`) → not a reference for precedence. and/or on one level, no short-circuit (both unverified against retail).
  - MPQ (`common/mpq.c`): has ADPCM and Huffman (in the 0x41/0x81 combination), a reference for our missing codecs. DCL via vendored `blast` (zlib license).
  - Formats: MDX, BLP1/BLP2, W3E, W3I, DOO, SLK, FDF parsers exist, a reference for `crates/formats`.
  - Gameplay docs (`docs/games/warcraft-3/*`): damage formula (armor K=0.06; 1/(1+K·A) / 2-(1-K)^-A, min 1), damage/armor type table from Misc.slk, 16-entry order queue, 32-unit pathing grid, A* + flow fields (an approximation, not retail). A good list of mechanics and gaps, but numbers have to be checked against 1.26a.

## 5. How to reproduce the analysis

```bash
pip install pefile capstone
python3 -I tools/analysis/pe_info.py   path/to/Game.dll
python3 -I tools/analysis/code_stats.py path/to/Game.dll
python3 -I tools/analysis/fpu_usage.py path/to/Game.dll
# Ghidra 11.4.2 (JDK 21):
analyzeHeadless <projdir> war3 -import Game.dll -scriptPath tools/ghidra -postScript Survey.java <outdir>
analyzeHeadless <projdir> war3 -process Game.dll -noanalysis \
  -scriptPath "tools/ghidra;$GHIDRA/Ghidra/Features/Decompiler/ghidra_scripts" \
  -postScript RecoverClassesFromRTTIScript.java -postScript PostRtti.java <outdir>
```
Full analysis of a 12 MB DLL takes ~30–60 minutes on 4 cores. The cloud container is temporary, so the Ghidra project is not saved.

## 6.0 Progress (update after every agent/step)

**Overall: ~9.5%** of the 1:1 rewrite.

| Part | Weight | Done | Comment |
|---|---|---|---|
| File formats (MPQ, BLP, MDX, SLK, maps, replays, JASS parser) | 5% | ~70% | written, not checked on real files |
| Game.dll analysis (names, classes, specs) | 10% | ~25% | full decompile export; specs: JASS front end, JASS VM, math, handles |
| Simulation (units, abilities, damage, pathfinding, AI, orders, RNG) | 40% | ~2% | RNG spec checked in emulation (Rust port still to do) |
| JASS VM + ~1000 natives | 12% | ~20% | parser (precedence = original), type checker, VM; natives ~65 of 1291, unconfirmed |
| Rendering (MDX, terrain, effects) | 12% | ~5% | viewer skeleton, static model |
| UI (FDF, menus, HUD) | 8% | 0% | |
| Network/lockstep, wasm, sound | 8% | 0% | |
| Replay test harness against the original | 5% | ~15% | w3g parser + Unicorn harness for functions |

## 6. Status / next steps

- [x] PE analysis, Ghidra survey, FPU analysis
- [ ] RTTI class recovery + export of the function list (running)
- [x] `_control87` mode: PC_53 / RC_NEAR (see 4.3)
- [ ] Find the software sin/cos/sqrt (NTempest math) and the float→int conversion functions (`fldcw` + `fistp`)
- [x] `crates/w3g` v0.1: header v0/v1, zlib blocks, players, stat string, slots, **seed**, time slots, chat, leave, 0x22 checksum blocks. Synthetic test only.
- [ ] `crates/w3g`: check on real 1.26a replays (`cargo run -p w3g --example dump -- file.w3g`), parse actions (0x10–0x1E ability/order, 0x16 selection, etc.)
- [x] `crates/mpq` v0.1: header search (512-byte aligned, HM3W prefix, MPQ\x1B user data), hash/block tables, encryption + FIX_KEY, single-unit/sectored, zlib/bzip2/PKWARE DCL, `(listfile)`. Synthetic tests (5).
- [x] `crates/mpq` v0.2: Huffman + ADPCM (Haiku agent, port from OpenRealm, see THIRD_PARTY_NOTICES.md). Checked only by round-trips with a test encoder.
- [ ] `crates/mpq`: check codecs on real WAVs (the Huffman model updates the tree only on escape, as in the reference; check first if decoding fails), check on real War3.mpq/War3x.mpq (`cargo run -p mpq --example mpqls -- War3.mpq`), streaming instead of loading the whole file
- [x] `crates/jass` v0.1: lexer (rawcodes, hex/$/octal, real, escapes), parser of the whole grammar (type/globals/native/function/locals/set/call/if-elseif-else/loop/exitwhen/return/debug), 5 tests.
- [x] JASS operator precedence matches the original (2026-10-10).
- [ ] `crates/jass`: check on common.j/blizzard.j from the user's MPQ; literal overflow, `%` in the AST (`parser.cpp`, `scanner.cpp`); type checker; VM (bytecode like the original, handles, threads/TriggerSleepAction, op limit)
- [x] `crates/blp` v0.1 (9 tests, wasm32 OK). **TODO on real files**: BLP1-JPEG is, as far as is known, 4-component BGRA without color conversion (alpha in the 4th channel); right now zune-jpeg converts it to RGB and drops alpha. This probably needs raw component output. Also check JPEG mip levels > 0 and the nibble order of 4-bit alpha.
- [x] `crates/gamedata` v0.1 (21 tests). **TODO on real files**: a duplicate row key in an SLK is currently an error; it should probably be first-wins or last-wins like the game (find out in Game.dll). Case-sensitivity of profile keys is unknown.
- [x] `crates/w3map` v0.1 (15 tests incl. truncation/corruption sweeps). **TODO on a real map**: the special-doodads section layout (currently assumed version 0 with strict checking, so the whole DOO parse may fail), SHD (maybe bit-packed), WPM header, the tail of w3i (upgrades/tech/random tables) is kept raw.
- [x] `crates/mdx` v0.1 (12 tests, corruption sweep). **TODO on real models**: TEXS/MODL unknown u32 fields, GEOS tail order (UVAS/UVBS), SEQS flags, KMTF tangents; after an unknown track tag the rest of the node is skipped. Interpolation 0/1/2/3 = none/linear/hermite/bezier (believed correct).
- [x] `crates/jass` v0.2: type checker (39 tests). Statements in the AST now have `line`. **Known mismatch to fix**: it currently allows calling a function before it is declared; in retail JASS, as far as is known, a function has to be declared above the call site (this is why vJass has `.evaluate`). Check in Game.dll and switch to the strict rule.
- [x] `crates/viewer` v0.1 (17 tests on pure logic, builds for wasm32). **Not checked**: never drawn (no GPU), no wasm-bindgen/JS glue, bind pose only (no animation/skinning), one texture for all geosets.
- [x] `crates/jass` v0.3: compiler to bytecode + VM + deterministic scheduler (30 VM tests). **UNVERIFIED vs the original**: real arithmetic is IEEE f32 (the spec hints at software float), R2I truncates, opcode numbering and codegen are ours (so the op limit won't count the same as the original until the codegen matches), and/or without short-circuit, behavior on array index out of range, strings are never freed.
- [x] `crates/jass` natives v1: math/strings/Convert* (`natives/pure.rs`, 17 tests), pass 2: R2I/StringHash/Convert* confirmed (Unicorn), S2I/I2S/SubString/StringCase from disassembly.
- [ ] `crates/inject`: i686-pc-windows hook DLL skeleton (needs the target + mingw)
- [ ] Wine + WC3 1.26a test environment (the user supplies the game install and replays)

**Needed from the user:** WC3 1.26a install (or War3.mpq/War3x.mpq) and several 1.26a `.w3g` replays. These do not go in the repo.

## 7. Log

- **2026-10-10 (11)**: precedence decoded and checked in emulation → parser fixed. Natives v1 merged. Found the software float.
- **2026-10-10 (10)**: RNG spec (Haiku agent + Unicorn); I re-ran it: 0 mismatches for Int/seed/streams. `tools/emu/` committed.
- **2026-10-10 (9)**: JASS VM merged (Haiku). Native table extracted (1,291). Launched the agent for pure natives (math/strings/Convert*).
- **2026-10-10 (8)**: full decompile export (70,526 functions, 1.58M lines, 0 failures; scratchpad only). Analysis agents: specs `docs/spec/{jass-frontend,jass-vm,math,handles}.md`. **Correction**: math comes from MSVCR80 `_CI*`, not software inside Game.dll.
- **2026-10-10 (7)**: merged `crates/blp` (Haiku agent, worktree). Then `crates/gamedata`, `crates/w3map`, mpq codecs. Then `crates/mdx`, the jass type checker, the viewer, the JASS VM. 175 tests in the workspace, clippy clean.
- **2026-10-10 (6)**: 4 Haiku agents studied OpenRealm (reports in `docs/research/openrealm/`). Verified the right-associativity claim myself.
- **2026-10-10 (5)**: `crates/jass` v0.1. Precedence follows pjass (and/or on one level) and is marked TODO to verify against Game.dll.
- **2026-10-10 (4)**: `crates/mpq` v0.1 (reader + tests + `examples/mpqls.rs`). Dependencies: flate2, bzip2 (pure-Rust backend libbz2-rs-sys, works in wasm), explode (DCL).
- **2026-10-10 (3)**: Found the FPU mode: `_control87(0x9001F)` = PC_53 + RC_NEAR. Added `tools/analysis/fpu_control.py`, `find_callers.py`.
- **2026-10-10 (2)**: Cargo workspace + `crates/w3g` (parser, 3 tests, clippy clean, `examples/dump.rs`). The stat-string layout (flags/width/height/crc/path/host) is best-effort and needs checking on real replays.
- **2026-10-10**: PE analysis, Ghidra survey (70,526 functions, 100% decompiled in the sample), FPU analysis (no hardware transcendental instructions), research on AI decompilation. Created AGENTS.md, tools/. Started RTTI recovery.
