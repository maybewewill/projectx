# projectx — WC3 1.26a TFT engine rewrite in Rust (agent handoff)

> A living document. **Every agent updates it at the end of a session**: the "Status", "Findings" and "Log" sections.
> Last updated: 2026-10-10

## 1. Goal

1. Rewrite Warcraft III: The Frozen Throne **1.26a** (`Game.dll`) in Rust **1:1 in behavior**:
   the simulation must match the original bit for bit, with no desync against original replays and clients.
2. Then optimize and extend freely (new renderer, WebAssembly/browser, our own features).
3. Target platforms: native (Windows/Linux) and **wasm32** in the browser.

## 2. Hard rules

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
  mpq/        MPQ archive reader                                        [v0.1: tables, encryption, zlib/bzip2/DCL; Huffman/ADPCM todo]
  wc3-math/   bit-exact WC3 math (x87 rounding, software sin/cos/sqrt)  [todo — waiting on Ghidra]
  jass/       JASS lexer/parser/VM                                      [v0.1: lexer + parser + AST; VM todo]
  sim/        deterministic simulation (units, abilities, pathing)      [todo]
  formats/    BLP, MDX, W3E/W3I/DOO/SLK                                 [todo]
  render/     wgpu renderer (native + WebGPU/WebGL)                     [todo]
  inject/     i686-pc-windows DLL: hooks Game.dll functions with Rust versions
              (incremental replacement inside the running original, OpenRCT2 approach) [todo]
tools/
  ghidra/     our Ghidra headless scripts
  analysis/   our Python scripts for PE analysis (pefile + capstone)
```

**Two halves of the engine:**
- **Simulation**: units, abilities, damage, pathfinding, orders, JASS VM, RNG, timers. **Must match bit for bit.**
- **Presentation**: rendering, UI, sound, network transport. Only needs to *behave the same*, so rewrite it freely.

**Correctness strategy:**
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
- `fsin/fcos/fsincos/fpatan/fptan/fsqrt/f2xm1/fyl2x/fprem/frndint` = **0**.
  → Transcendental functions and sqrt are **computed in software inside the DLL** (tables or polynomials), so they can be reproduced bit for bit.
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

## 6. Status / next steps

- [x] PE analysis, Ghidra survey, FPU analysis
- [ ] RTTI class recovery + export of the function list (running)
- [x] `_control87` mode: PC_53 / RC_NEAR (see 4.3)
- [ ] Find the software sin/cos/sqrt (NTempest math) and the float→int conversion functions (`fldcw` + `fistp`)
- [x] `crates/w3g` v0.1: header v0/v1, zlib blocks, players, stat string, slots, **seed**, time slots, chat, leave, 0x22 checksum blocks. Synthetic test only.
- [ ] `crates/w3g`: check on real 1.26a replays (`cargo run -p w3g --example dump -- file.w3g`), parse actions (0x10–0x1E ability/order, 0x16 selection, etc.)
- [x] `crates/mpq` v0.1: header search (512-byte aligned, HM3W prefix, MPQ\x1B user data), hash/block tables, encryption + FIX_KEY, single-unit/sectored, zlib/bzip2/PKWARE DCL, `(listfile)`. Synthetic tests (5).
- [ ] `crates/mpq`: Huffman + ADPCM (WAV), check on real War3.mpq/War3x.mpq (`cargo run -p mpq --example mpqls -- War3.mpq`), streaming instead of loading the whole file
- [x] `crates/jass` v0.1: lexer (rawcodes, hex/$/octal, real, escapes), parser of the whole grammar (type/globals/native/function/locals/set/call/if-elseif-else/loop/exitwhen/return/debug), 5 tests.
- [ ] `crates/jass`: check on common.j/blizzard.j from the user's MPQ; **verify operator precedence and literal overflow against the original parser in Game.dll** (`parser.cpp`, `scanner.cpp`); type checker; VM (bytecode like the original, handles, threads/TriggerSleepAction, op limit)
- [ ] `crates/inject`: i686-pc-windows hook DLL skeleton (needs the target + mingw)
- [ ] Wine + WC3 1.26a test environment (the user supplies the game install and replays)

**Needed from the user:** WC3 1.26a install (or War3.mpq/War3x.mpq) and several 1.26a `.w3g` replays. These do not go in the repo.

## 7. Log

- **2026-10-10 (5)**: `crates/jass` v0.1. Precedence follows pjass (and/or on one level) and is marked TODO to verify against Game.dll.
- **2026-10-10 (4)**: `crates/mpq` v0.1 (reader + tests + `examples/mpqls.rs`). Dependencies: flate2, bzip2 (pure-Rust backend libbz2-rs-sys, works in wasm), explode (DCL).
- **2026-10-10 (3)**: Found the FPU mode: `_control87(0x9001F)` = PC_53 + RC_NEAR. Added `tools/analysis/fpu_control.py`, `find_callers.py`.
- **2026-10-10 (2)**: Cargo workspace + `crates/w3g` (parser, 3 tests, clippy clean, `examples/dump.rs`). The stat-string layout (flags/width/height/crc/path/host) is best-effort and needs checking on real replays.
- **2026-10-10**: PE analysis, Ghidra survey (70,526 functions, 100% decompiled in the sample), FPU analysis (no hardware transcendental instructions), research on AI decompilation. Created AGENTS.md, tools/. Started RTTI recovery.
