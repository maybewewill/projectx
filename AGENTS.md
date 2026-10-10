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
  softfloat/  bit-exact port of Game.dll's software float (~27 functions, 540k cases, 0 mismatches) [v1]
  sim/        deterministic simulation: rng.rs + softfloat.rs (add/sub/mul) bit-exact [v0.1]; units/abilities/pathing todo
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
- `RecoverClassesFromRTTIScript` **got stuck** (>2 h in one phase) and was stopped. Replacement: `tools/analysis/rtti_vtables.py` (vtable → COL → TypeDescriptor): **3,675 vtables, 19,298 virtual functions tied to classes** (e.g. `CUnit` = 118 virtual methods). Map `address → Class::vfN` in `scratchpad/vfunc_names.tsv` (regenerated by the script; not in the repo).

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
- Statuses: 1 done, 2 budget, 3 sleep, 4 yield, 5 function not found, 6 null, 7 divide by zero. Threads are in a circular list. (⚠ `0x6F44B400` is **not** the scheduler, most likely a thread-state serializer, per game-loop.md; the real scheduler is not found yet.) No call-depth limit found.

### 4.3g Software float (important!)
- From the natives analysis: real arithmetic in the JASS VM and in natives goes through **Blizzard's own software float helpers** (multiply `0x6F6EEE20`, Sin/Cos/Tan wrappers `0x6F3B2A10/2A30/2A50` call software helpers `0x6F6EF820/0x6F6EF8D0/0x6F6F0140`, not `_CI*`). `I2R` truncates the mantissa to 24 bits (`I2R(16777219) = 16777218`), not round-to-nearest. S2R parser `0x6F6F0520`. GetRandomReal uses the same helpers.
- ✅ **`crates/softfloat` (docs/spec/softfloat.md) is a bit-exact port of the whole library**, checked in Unicorn on **two independent vector sets of ~10,000 cases per function (≈540k cases total), 0 mismatches** (re-run by the lead with SOFTFLOAT_VECTORS + GAME_DLL):
  add `0x6F6EF000`, sub `0x6F6EEEF0`, mul `0x6F6EEE20`, div `0x6F6EFE10` (recip `0x6F6EF100`), i2r `0x6F6EF350`, f2i `0x6F6EEC20`, r2i `0x6F6EEC70`, compares lt/le/gt/eq (`0x6F00C110/0x6F01FD60/0x6F01FD40/0x6F009AE0`), approx_eq `0x6F00C3F0` (|a-b|<0.001), sqrt `0x6F6EF750`, sin/cos/tan `0x6F6EF820/0x6F6EF8D0/0x6F6F0140`, asin/acos `0x6F6EFC70/0x6F6EFB00`, atan/atan2 `0x6F6F0170/0x6F6F02D0`, exp `0x6F6F07B0`, ln `0x6F6F0060`, pow `0x6F6F0810`, floor/ceil/round/trunc.
  Tables (recip `0x6F9774C8`, sine `0x6F9784D0`, asin `0x6F9794D8`, 1025×u32 each) are read at runtime from Game.dll and **not committed**. The original's quirks are reproduced (ceil(±0)=1.0, floor(-0)=+0).
  ✅ R2S/R2SW `0x6F6F0360`, S2R `0x6F6F0520` and JASS real literals (`softfloat::text`) bit-exact: 12,000 cases per function in Unicorn, 0 mismatches. R2S = **3 decimals**; S2R and the lexer compute via soft DIV (`"1.5"` → 0x3FC00001; the literal `0.1` → **0x3DCCCCCE**, not 0x3DCCCCCD; the lexer does **not** use atof). Use `jass::parse_with(src, Some(&tables))` in production code.
  Open: pow with a negative integer count hangs in the original (the port diverges, UNVERIFIED); native argument wrappers (Asin/Acos range, Pow special cases) not ported; `0x6F6EFE50`, `0x6F6F08B0` not identified. `crates/sim/softfloat.rs` should be switched over to this crate.
- ⚠ docs/spec/math.md section 0 (the `_CI*` theory) is **wrong for gameplay math**: the natives and the VM use the software helpers above. `_CI*` from MSVCR80 is probably only used in rendering/UI (still to check).
- The bodies of all 1,291 natives are restored (`ExportNatives.java` creates functions at the addresses from natives-1.26a.tsv; output `scratchpad/decomp/natives.c`, not in the repo).
- Natives pass 2 (checked): **R2I** = truncation toward 0, saturation at |x|≥2^31 incl. NaN (21,626 cases in Unicorn, 0 mismatches). **StringHash** = MPQ string hash type 0 via Storm ord. 590 (Game.dll's own copy `0x6F68CC10` matches us on 33 cases; that Storm is the same is LIKELY). **S2I** = atoi (leading spaces and sign allowed). **Convert*** = just `return param_1`, no tables. **StringLength/StringCase** substitute `TRIGSTR_nnn` from war3map.wts (not done yet). Storm.dll + msvcr80.dll are needed for full checks.

### 4.3e Native table
- `docs/spec/natives-1.26a.tsv`: **1,291 natives** (1,167 `common`, registered in `0x6F3D4020`; plus 124 AI natives in `0x6F71D9E0`) with signatures and implementation addresses. Extracted by `tools/analysis/extract_natives.py` (parses push name/signature/function before the register call).

### 4.3h Game loop (spec: docs/spec/game-loop.md, partial)
- Replay pump `0x6F54EB00` (block ids from a ring buffer; 0x1E/0x1F share a handler; exits after 0x1F) → enqueue `0x6F54CAD0`. Step framing `0x6F65C7D0`/`0x6F65C840` (on exit from the outermost level, queued net events are applied in list order). Feeder `0x6F6637C0`.
- CRC-32 (reflected, 0xEDB88320; builder `0x6F649360`, update `0x6F649410`, finalize = NOT) is used to check the file footer. What goes into the per-turn sync checksum (CSyncData vtable+4, CNetData+0x2278) is not found.
- **Pass 2 (docs/spec/game-loop-2.md):**
  - The JASS interpreter is called from only 4 places (wrappers `0x6F45F8B0/0x6F45FA30/0x6F45FD00`, reached from `0x6F44B640/0x6F44B6A0`). **There is no polling of sleeping threads**: TriggerSleepAction (`0x6F3B2DB0` → `0x6F44B2F0`) stores status 3 and the **raw f32 bits of the seconds** (no conversion/minimum), then the frame resume posts message 0xD01BF → a CAgentTimer starts → on expiry the handler `0x6F2E3320` resumes the thread (LIKELY).
  - **Timers**: TimerStart → `0x6F3F6890` → message 0x80204 → `CAgentTimer::vf2 0x6F477FF0`; timeout in **f32 seconds**, negative → 0. Queue = **1-based binary min-heap** `CPriorityQ<CSiRequest<CAgentTimer,ulong>>` (push `0x6F46A030`), order: **expiry ↑, then sequence number ↑ (FIFO)**. Two queues (manager+0x40 / +0x44 "(l)").
  - Pump `0x6F46E370`: pops while expiry ≤ target, **now := expiry of the popped timer** for the duration of the callback. Driver `0x6F46E520`; per-frame `0x6F3A2EC0` → `0x6F470C70`, `0x6F46FFE0` with target at `0x6FAAE4A0`.
  - Game ("agile") clock = float seconds, separate from the net step counter (CNetData+0x628).
  - ✅ (lead, capstone scan) **`0x6FAAE4A0` = the constant 0.1f**: the only write anywhere in .text is the static initializer `0x6F84DD30: mov [0x6FAAE4A0], 0x3DCCCCCD`; everything else is reads or pushes of the address (~90 sites, used as "pointer to 0.1"). So `0x6F3A2EC0` advances the agile clock by **0.1 s** per call → most likely **one simulation step = 100 ms**, matching the typical replay time slots (LIKELY; how many steps per turn of N ms still needs confirming).

### 4.3i Attack/damage (spec: docs/spec/attack-damage.md)
- `CAbilityAttack::GetDamageAmount 0x6F0C6940` → dice on **stream 2**, clamp ≥ 0 (**min 1 not seen**, unlike OpenRealm). Target side `0x6F2A40D0`: invulnerability → evasion `0x6F08BD20` → handlers of categories 1, 2..8 → armor (physical only, class bit 0x10) → damage-type table → ethereal table → HP.
- Armor ≥ 0: M = 1/(1+K·A), K = Misc `DefenseArmor`. Armor < 0: a 21-entry table at `0x6FAB4300` indexed by min(trunc(-A),20) (values filled at runtime). DamageBonus 7×8 table at `0x6FAB4358` (row order differs from OpenRealm), EtherealDamageBonus from Misc.
- ✅ Dice sampler `0x6F26CAD0`: each die 0..sides-1 (300/300 in Unicorn). Where +1 per die is added is not found. >16 dice → Gaussian path.
- Timing (cooldown/dmgpt/backSw in seconds) → step conversion not found (same issue as game-loop).

### 4.3j Pathing (spec: docs/spec/pathing.md, pass 1, partial)
- Cell = **32 world units**; index = row·width + col. Cell word: top byte = 8 pathing-type bits (1 = blocked; unwalkable 0x02, unflyable 0x04, unbuildable 0x08, blighted 0x20, unfloat 0x40, unamph 0x80), low 24 bits = head of the occupant list (0xFFFFFF = empty). World→cell: subtract origin, ×1/32 (exponent arithmetic), floor `0x6F6EF170`, f2i `0x6F6EEC20`.
- **Pass 2 (docs/spec/pathing-2.md):** A* over **regions** (not cells): loop `0x6F49A170`, binary min-heap of 12-byte entries (1-based, unsigned key) `0x6F4985F0/0x6F498580`. Cost: step = floor(24·dist(region seed cells)), h = floor(24·dist to goal), both through an approximate integer Newton sqrt `0x6F4BABB0`. **Expansion order N, E, S, W, NE, SE, SW, NW**; a diagonal only if both orthogonal neighbors are passable; relax only on strictly better g. Cap: 400 pops per call; unreachable → path to the nearest node (min squared distance, first wins). No smoothing. Large units: footprint checks (`0x6F38ADA0` size class).
- **Pass 3 (docs/spec/pathing-3.md):** there is **no flood fill**. "Regions" are aligned squares of 1/2/4/8 cells (pyramid levels 0–3, loaded from the map by `0x6F481340`, not rebuilt during the game). An id is assigned **lazily** on the first touch of a record in `0x6F498A60` = the current length of the search's node list; the seed = the query cell. Ids restart from 0 on every request (`0x6F49A3F0`). Lookup returns the id of the record one level below the first non-mixed one; state 01 → -1, state 10 → -1 (level 0) / -2 (higher). Regions are **not** connected components (one id can span a wall). Cell expansion `0x6F499E90` N,E,S,W,NE,SE,SW,NW confirmed; the block expander `0x6F499FA0` (level ≥ 1) is not fully worked out. ✅ **Reworked and checked in Unicorn (0 mismatches)**: lookup/labeller `0x6F498A60/0x6F498A00` (8,000 cases, ~110k lookups, full state after every lookup), reset/setup `0x6F49A3F0/0x6F49A640`, start node `0x6F49A240`, loop `0x6F49A170`, relax/push/unlink/close, cell expander, block expander (~14.5k expansions; beyond the generated cases UNVERIFIED). Corrections: step = isqrt(576·d²), h the same to the goal, nearest-node fallback by plain d²; setup takes float cells (floor+f2i) without origin/scale; node template from .data; heap at S+0x70 (count includes the sentinel). ✅ **The whole request pipeline is checked**: dispatcher `0x6F49A800` (1,178 requests: direct 531 / straight 269 / partial 304 / reached 74), direct move `0x6F49A5E0`, waypoints `0x6F49A470` (cell units; node → (x+0.75, y+0.75), mode 2 → (x+1.25, y+1.25); flag node adds (-128000.0, flag); the first point = goal/center, the last = start; no smoothing), cell center `0x6F497D30`, **mode 2 (2-cell footprint)**, fractional inputs. 0 mismatches. Open: start outside the pyramid (the original works on stale state), size classes ≥2, the world↔cell (×32) layer around the dispatcher, path-object growth failure.
- Movement chain (LIKELY): `0x6F490FC0` → `0x6F4907F0` → `0x6F495320` → `0x6F49E020/0x6F49DAC0`. Not found: region flood fill, per-tick turning/acceleration, unit push-apart (CPoSeparate/CProximityMap).

### 4.3c Handles (spec: docs/spec/handles-2.md) ✅ checked in Unicorn (21/21, `tools/emu/handle_alloc_emu.py`)
- GetHandleId `0x6F3B2D00` = identity. **Base 0x100000**, new id = 0x100000 + table size (allocator `0x6F430C80`); freed ids go on a **LIFO stack** (push `0x6F3A3D20`) → the most recently freed one is reused.
- Entry is 12 bytes: refcount, agent smart pointer, flag. Created with count 1; release `0x6F42DD40` → when it reaches 0 the id goes on the free stack. Hashtable saves AddRef the id (it stays reserved). Agent death (event 0x80267, `0x6F42DD80`) releases the creation reference (LIKELY).
- JASS variables do **not** change refcounts (UNSURE: no such path found in the VM). Table reset `0x6F42D2E0` leaves dummy entry 0 → **the first real id is likely 0x100001**.

### 4.3f Game RNG (spec: docs/spec/rng.md) ✅ checked by emulating the original
- Natives: `GetRandomInt 0x6F3B3560`, `GetRandomReal 0x6F3B35A0`, `SetRandomSeed 0x6F3B36D0`. **CRT rand() is not used in gameplay** (only UI/particles/editor; srand(GetTickCount) at startup). OpenRealm is wrong here.
- Generator: 8-byte state (A,B); B's bytes index a 256-byte permutation table at `0x6F953650`; mix = rotl(T,3)^rotl(T,2)^T^rotl(T,1); A += mix; output = A. Seeding: A=S, B = S mod 59/61/53 and (17*(S/47)+S) mod 64 in bit fields.
- 45 separate streams (combat = stream 2: dice/evasion/crit/miss; 32/34/35 for ChooseRandom*; 42 for percentages, ...). SetRandomSeed reseeds the global generator and all streams.
- GetRandomInt(lo,hi): lo==hi → lo without a draw; otherwise lo + floor(r*(|hi-lo|+1)/2^32) (lo>hi goes up from lo).
- **Checked in Unicorn against the original** (`tools/emu/`): seed, step, GetRandomInt (300 cases), SetRandomSeed + 45 streams: **0 mismatches**.
- ✅ **Rust port `crates/sim` (rng.rs + softfloat.rs) is bit-exact**: seeder/step/GetRandomInt/GetRandomReal 3000/3000 each (GetRandomReal also on random bit patterns incl. NaN/inf/denormals), SetRandomSeed + 45 streams 300/300, stream init 300/300, soft add/sub/mul 3000+3000 each. Tests with `GAME_DLL=...` compare against vectors from the original.
  - GetRandomReal: range = |b-a|, U = (low23(step) | 0x3F800000) - 1.0, result = a + range*U, always one step, everything through the truncating soft-float.
  - GetRandomInt(i32::MIN, i32::MAX): size wraps to 0 → returns lo (one step still taken).
  - Open: the constant at `0x6FAAE464` is assumed = -1.0 (zeros in the file, filled at runtime; needs reading from a live process). The game-start seed fold over slots is not implemented (input to `start_game(s)`).
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

**Overall: ~22%** of the 1:1 rewrite.

| Part | Weight | Done | Comment |
|---|---|---|---|
| File formats (MPQ, BLP, MDX, SLK, maps, replays, JASS parser) | 5% | ~70% | written, not checked on real files |
| Game.dll analysis (names, classes, specs) | 10% | ~25% | full decompile export; specs: JASS front end, JASS VM, math, handles |
| Simulation (units, abilities, damage, pathfinding, AI, orders, RNG) | 40% | ~16% | RNG, softfloat, text, handles, pathing A* (pyramid, search core) bit-exact; timers/clock |
| JASS VM + ~1000 natives | 12% | ~35% | parser (precedence = original), VM on softfloat; natives ~250 of 1291 (math/strings/text bit-exact, timers/triggers per spec) |
| Rendering (MDX, terrain, effects) | 12% | ~5% | viewer skeleton, static model |
| UI (FDF, menus, HUD) | 8% | 0% | |
| Network/lockstep, wasm, sound | 8% | 0% | |
| Replay test harness against the original | 5% | ~15% | w3g parser + Unicorn harness for functions |

## 6. Status / next steps

- [x] PE analysis, Ghidra survey, FPU analysis
- [x] RTTI: vtable→class map (our script), 19,298 virtual functions named; full decompile export (70,526 functions)
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
- [x] `crates/sim` timer.rs + script.rs: AgileClock (softfloat add 0.1, drifts below n·0.1: 3DCCCCCD, 3E4CCCCD, 3E999999…), TimerQueue (1-based min-heap, expiry→seq), ScriptHost (JASS sleep → timer). 17 tests. ✅ fixed: `Vm::resume(thread)`, the sleep wake = softfloat add. Periodic re-arm = expiry+period (UNVERIFIED).
- [x] **softfloat integrated**: JASS VM real = `softfloat::F32` (add/sub/mul/div/neg/i2r/compares; real `==` = approx_eq |a-b|<0.001), trig natives through softfloat + tables, no IEEE fallback (`RunStatus::MissingTables`). `crates/sim` uses `crates/softfloat` (own copy removed). Open: Asin/Acos/Pow wrappers, int vs real comparison through I2R (UNVERIFIED).
- [x] `crates/sim/pathing`: **isqrt `0x6F4BABB0` bit-exact** (115,536 inputs, 0 mismatches), **heap push `0x6F498580` / pop `0x6F4985F0` bit-exact** (241k operations, the whole array state after every op, 0 mismatches). grid (32 units, world→cell through softfloat) has unit tests only. Region A* search reworked to the pyramid model and **checked in emulation** (see 4.3j).
- [x] `crates/sim/handles.rs`: HandleTable **bit-exact vs the original allocator** (2×800 operations in Unicorn incl. edge cases: every id, count, table length, free-stack length). The emulator found 2 quirks: releasing at count 0 doesn't go below 0 and pushes nothing; a freed id can be pushed twice → object→id links are validated against the entry. JASS: extension slot `Vm::insert_extension`, `GetHandleId` = identity. Open: object death (event 0x80267) is checked only against the model.
- [x] `crates/sim/natives`: **timer and trigger natives** (~40: CreateTimer/TimerStart/Pause/Resume/Elapsed/Remaining, CreateTrigger/conditions/actions/Evaluate/Execute/RegisterTimerEvent, boolexpr And/Or/Not, ...). Found: TriggerExecute ignores conditions and the enable flag; the event path does eval++ → enable check → conditions (AND, no early exit) → exec++ → actions; TriggerEvaluate without counters. Not run against the original; UNSURE: order of handler vs listener on expiry, tie-break (global FIFO vs per-timer +0x50), And/Or without short-circuit. VM fix: the verifier rejected `GetHandleId(CreateTrigger())`. **Tests must use `parse_with(tables)`** for bit-exact literals.
- [x] `crates/sim/natives`: **hashtable** (all types incl. 39 handle types; one handle slot per key across types; a save releases the old reference and AddRefs the new one → a saved handle keeps its id after Destroy; LoadXHandle → null if the type no longer matches), **gamecache** (no disk), **location/rect** (rect clamped to the playable area, center = (max+min)·0.5 via softfloat). **Known divergence**: flushing a hashtable releases references in key order (BTreeMap), the original in its hash-map order → with several handles flushed at once, the LIFO order of freed ids can differ (needs that hash order implemented). Found so far: TSHashTable, 12-byte buckets (index = key & mask, 4 initially), **append at the chain tail, destroy walks buckets 0..mask, tail-first (newest first) within each** (checked without growth); growth when score > 13 (mask < 0x1FFF), rehash not confirmed yet — work in progress. The playable area isn't loaded from the map yet. Gamecache keys are exact strings (the original uses STRIPACKED hashing).
- [ ] `crates/inject`: i686-pc-windows hook DLL skeleton (needs the target + mingw)
- [ ] Wine + WC3 1.26a test environment (the user supplies the game install and replays)

**Needed from the user:** WC3 1.26a install (or War3.mpq/War3x.mpq) and several 1.26a `.w3g` replays. These do not go in the repo.

## 7. Log

- **2026-10-10 (14)**: `crates/softfloat`: the whole software float library is bit-exact (540k cases). 0x6FAAE4A0 = 0.1f (step 100 ms).
- **2026-10-10 (13)**: specs game-loop (2 passes) and attack-damage (dice confirmed in Unicorn). RTTI→vtable map. Two agent launches were interrupted by the safety classifier → those tasks were dropped.
- **2026-10-10 (12)**: `crates/sim` (RNG + softfloat add/sub/mul) bit-exact vs the original (re-ran it myself with GAME_DLL). Natives pass 2. Running: softfloat (full library), game-loop spec, attack/damage spec.
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
