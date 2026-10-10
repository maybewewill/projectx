# OpenRealm research: netcode, RNG, parity, WC3 size

Repository: `/home/user/corepunch/open-realm` (read-only; git HEAD `42d11d3`, single commit in this checkout, so history is not available for maturity analysis). Paths below are relative to that root unless absolute.

---

## 1. Simulation and network model

**Verdict: authoritative server with fixed-step simulation and Quake 2-style delta snapshots. Not lockstep.** Clients never re-simulate; they receive server state and interpolate.

### Fixed tick
- `common/shared.h:60` defines `FRAMETIME 100` (ms), i.e. a 10 Hz simulation tick.
- `server/sv_main.c:261-265` `SV_RunGameFrame()` increments `sv.framenum`, adds `FRAMETIME` to `sv.time`, then calls `ge->RunFrame()`.
- `server/sv_main.c:290-333` `SV_Frame(msec)` is driven by wall-clock time. It always reads packets, returns early while paused (`:318-322` sends keepalive snapshots only), and runs at most one game frame per call once `svs.realtime >= sv.next_frame_msec`. It then advances the deadline by `FRAMETIME` and sends client messages (`:331-332`).
- `server/server.h:133-139` `SV_ClampSimulationDeadline()` rebases the deadline when it is more than one `FRAMETIME` overdue, so a stall does not become a burst of catch-up ticks. `docs/architecture/server.md:38-42` explains the same policy and cites Quake II.
- `common/main.c:557` and `:560` call `SV_Frame(msec)` and `CL_Frame(msec)` from one loop in the same process. `:557` passes `FRAMETIME` instead of wall msec when `com_fast_forward` is set.
- `server/sv_game.c:96-98` `SV_GetTime()` returns `sv.time`. The game clock is `level.time = gi.GetTime()` at `games/warcraft-3/game/g_main.c:1128`, so game time is simulation time, not wall time. `G_Time()` is at `games/warcraft-3/game/g_local.h:3869`.
- Game frame order: `G_RunFrame()` at `games/warcraft-3/game/g_main.c:1120-1160`: timers, events, JASS, bots, clients, entities, path jobs.

### Input and snapshots
- Client to server input is a command, not a lockstep input stream: `clc_input` carries an `inputCmd_t` (`common/shared.h:631-640`, union of focus, view angles/distance, or move buttons/msec). `server/sv_parse.c:18-24` decodes it and calls `ge->ClientInput`.
- Note: `docs/architecture/server.md:51` says `clc_move` stores a `usercmd_t` for `ge->ClientThink`. That is stale. The code uses `clc_input`, `clc_camera_position`, `clc_stringcmd` and `clc_request_unit_ui` (`server/sv_parse.c:18-36`). `ClientThink` is not referenced in the server or game code I searched.
- Server snapshots are delta-compressed per client (`server/sv_ents.c:1-12` header; `server/sv_ents.c:417` for rate gating).
- `docs/architecture/network.md:138-257` describes the wire model:
  - Lossless transports (loopback, EOS) carry no sequence header (`:158-165`).
  - UDP (`NA_IP`) uses a Quake 2-style netchan: sequence + ack header, `Netchan_Process` drops stale or duplicate packets (`:166-172`).
  - Two streams: unreliable snapshots, and a reliable resend stream for configstrings (`:179-190`).
  - Deltas are based on the last acknowledged snapshot (`:192-203`). The server keeps `UPDATE_BACKUP` snapshots (`common/common.h:17`, value 16). A full snapshot (`-1`) is sent when the base is too old.
  - Client `nodelta` request forces a full frame (`:205-210`).
  - `sv_rate` (`common/cvar.c:559`, default 0 = unlimited) models a slow link: snapshots due while the link drains are skipped, not queued (`:211-215`).
- Doc drift: `docs/architecture/network.md:158` says "protocol 20", but `common/net.h:19` sets `BZ_PROTOCOL_VERSION 23`.
- Client presentation: `client/cl_view.c:677` sets `lerpfrac = (cl.time - cl.frame.servertime) / FRAMETIME`, clamped to [0,1]. The client interpolates between two server samples. Client-side prediction is limited to camera/orbit (`client/cl_input.c:38-41`, `:136`; `tests/test_net.c:3072-3109`). `AGENTS.md:397-398` states the client should decode, interpolate two samples, and predict only the local generic input.

### Determinism, desync, replays
- Lockstep desync does not apply: only the server simulates, so bit-identical results across machines are not required for correctness. Protocol desync is handled by full-snapshot fallback (see above).
- Replays: **not implemented.** `games/warcraft-3/game/api/api_misc.h:856` `DoNotSaveReplay` is a stub with `/* TODO: replay recording not yet implemented */`. No `.w3g` parser or writer exists in `.c`/`.h`/`.py` (case-insensitive grep). `docs/games/warcraft-3/file-docs/readme.md:51` lists `w3g.md` under `campaign-replay/`, but that file is not in `docs/games/warcraft-3/file-docs/` (contents: doo.md, fdf.md, mpq.md, readme.md, shd-wpm.md, w3e.md, w3x-w3m.md).
- `docs/architecture/server.md:36` says the fixed step "makes replay and deterministic simulation practical". That is an aspiration, not a claim of current replay support.
- Save/load does not persist the libc RNG state (`games/warcraft-3/game/g_save.c` has no `rand`/`srand` references). Only per-unit `wander_random_state` is saved (`g_save.c:877`). A load therefore cannot reproduce the RNG stream position.

### Randomness
Two RNG systems exist.

1. **Gameplay RNG: process-global libc `rand()`/`srand()`.**
   - `games/warcraft-3/game/api/api_misc.h:1238-1242` `GetRandomInt` uses `rand() % (high-low+1)`.
   - `api_misc.h:1244-1249` `GetRandomReal` uses `(float)rand()/RAND_MAX`.
   - `api_misc.h:1426-1430` `SetRandomSeed` calls `srand((unsigned)seed)`.
   - `api_misc.h:1388-1395` (`JassChooseRandomItem`) relies on `rand()` and mentions "SetRandomSeed() remains predictable".
   - Other gameplay callers: `games/warcraft-3/game/m_unit.c:352-361` (bounty dice), `skills/s_attack.c:176,433,445` (damage dice, miss, bash), `skills/s_summon.c:175-176`, `skills/s_area_spell.c:86-87`, `skills/s_hero_passives.c:908-944`, `skills/s_dark_portal.c:10`, `skills/s_requested_abilities.c:280`, `skills/s_human_abilities.c:760`, `skills/s_creep_abilities.c:283`, `g_model.c:543`.
   - No seed is set at startup for the game in the code I searched. Seeding happens only through JASS `SetRandomSeed` or test code.
2. **Per-unit LCG for wander: deterministic and isolated.**
   - `games/warcraft-3/game/skills/s_wander.c:22-37`: `state = (number+1)*2654435761 ^ (spawn_time+1)`, advanced by `*1664525 + 1013904223`, uniform via `/2^32`.
   - Comment at `:23` says it is not the process RNG. This is the design model OpenRealm should copy.

**Contamination of the gameplay RNG stream by presentation code.** Renderer, sound and client code also call libc `rand()` in the same process:
- `renderer/r_emit.h:18-29` (`FX_GenerateRandomDirection`, `FX_GenerateRandomOrigin`) is used by `games/warcraft-3/renderer/mdx/r_mdx_particles.c:30-33` and `:91` (`R_EmitParticles`, driven by `tr.viewDef.deltaTime`).
- `sound/s_sound.c:284-285` (sound variant pick).
- `client/cl_music.c:419` (random playlist start).
- `games/warcraft-3/renderer/r_weather.c:122-123` says cosmetic randomness is kept off the gameplay RNG. The particle and sound paths above do not follow that rule.
- Effect: gameplay `GetRandomInt` results depend on how many render frames or sounds occurred. Because `srand` does not persist, this makes gameplay randomness non-reproducible across runs with different render cadence. The render cadence is wall-clock and not fixed. This is the most important determinism finding. Mitigation: give presentation its own RNG state, or put gameplay RNG in a game-owned generator.
- Tests fix seeds with `srand(1)` / `srand(42)` (for example `games/warcraft-3/game/tests/t_api.c:6799-6827`, `t_slk.c:1298`, `tests/test_sound.c:418`). `tools/mdxtool.c:36,1591` seeds particle RNG for golden renders.
- The `common/mpq.c:404-455` seeds are MPQ hash/encryption, not gameplay.

### Floating point
- Game code is float-dominant. In `games/warcraft-3/game`, I counted roughly 2397 lines with `float` vs 53 with `double` (line-count, includes comments).
- Positions and math use `vec2_t`/`vec3_t` with float fields (`shared/types/vector2.h`, `vector3.h`) and `cosf`/`sinf`.
- Doubles appear in specific places:
  - Camera shake phase: `games/warcraft-3/game/g_main.c:862-875`. The comment at `:859-861` explains the sample rate and that the shake runs at the 10 Hz tick.
  - Goldmine layout: `skills/s_goldmine.c:890-934`.
  - Gold clamp: `skills/s_item.c:650-673`.
  - Authored formula parsing: `m_unit.c:2212-2244`.
- Build flags: `Makefile:5` `-Wall -Wmisleading-indentation -fno-common`; `Makefile:12` adds `-O2` only in release. I found no `-ffast-math` or `-ffp-contract` anywhere in the repo.
- Consequence: IEEE results depend on the compiler, libm and x87-vs-SSE settings. Cross-platform bit identity is not guaranteed. This is acceptable only because there is no lockstep. It would break any future lockstep or replay-verification scheme.

---

## 2. `tools/parity/` and retail comparison

### Contents of `tools/parity/`
| File | Role |
|---|---|
| `README.md` | Documents the workflow, sections: SC2 picker, Wine retail comparison (`:26`), catalog generation (`:71`), `shot.sh` (`:141`), golden renders (`:160`). |
| `wc3.sh` | Launches retail or OpenRealm with the same installation and map under Wine, logs command, edition, map, revision and output to `build/parity/logs/`. |
| `wc3_maps.py`, `map_archive.py`, `map_picker.py` | Map catalogue from `UI/CampaignStrings*.txt` and archive listfiles; curses picker. |
| `compare_wc3_ui_layout.py` | Joins OpenRealm `WC3_UI_TRACE` (`stage=source`/`final`) with Warsmash `stage=final` records by FDF frame name. Warsmash is another reimplementation, not retail. |
| `render_golden.sh`, `golden/`, `golden_manifest.txt` | Deterministic `mdxtool` renders (fixed frame, seeded particle RNG) compared to committed PNGs with `imgdiff`. Needs GL, opt-in (`games/warcraft-3/game.mk:209-213`). |
| `shot.sh` | macOS window capture of OpenRealm and the "Legacy" client v1.29.2 (`README.md:141-158`). |
| `sc2.sh`, `sc2_maps.py` | StarCraft II map picker. |

### Method
- Primary method is **black-box comparison**: run retail `war3.exe` under Wine and OpenRealm on the same map, record logs, video or screenshots, and compare. `tools/parity/README.md:30` says: "No disassembler or debugger is required."
- The README defers to docs for evidence. Visual and audio judgement is manual, with recording by OBS or a desktop recorder (`tools/parity/README.md:129-133`, bark comparisons). Successful loading proves startup only, not gameplay fidelity.
- Golden tests are regression tests against OpenRealm's own output, not against retail.

### Retail reverse engineering (not in `tools/parity/`)
The reverse-engineering method sits in `tools/ghidra/`, `tools/frida/`, `tools/r2_ability.sh`, and the docs.

- **Ghidra** (headless and GUI): `tools/ghidra/MapAudio.java`, `AudioAnchors.java`, `AudioFunctions.java`, `MapCursor.java`. Each checks the binary SHA-256 before applying names, comments and types. `MapCursor.java` and `docs/games/warcraft-3/cursor-rendering.md:19-23` say unknown fields stay undefined and decompiler prototypes are not forced.
- **Original-code differential**: `tools/ghidra/verify_audio_admission.py` runs the original x86 `6f0af5e0` routine in Unicorn and compares it with production `S_AdmitSound` (via `tools/ghidra/audio_admission_probe.c`). 12,000 cases, zero mismatches (`docs/games/warcraft-3/audio-retail-analysis.md:238-244`).
- **Frida** (attach-only observers, Wine): `tools/frida/trace_wc3_audio.py`, `trace_wc3_cursor.py`, `trace_wc3_weather_alpha.py`, `trace_wc3_weather_density.py`, plus `wc3_*.js` agents. They relocate addresses from the actual module base. `audio-retail-analysis.md:262-268`.
- **radare2 / r2ghidra**: `tools/r2_ability.sh` (`:1-15`) prints FourCC, RTTI and `pdg` decompilation of one address in `Game.dll`. Used for ability-class work (`docs/games/warcraft-3/ability-inheritance-binary.md`). That doc analyses the 2002 demo DLL and TFT 1.29.2.9231 `Warcraft III.exe`.
- **Camera tracing**: `tools/retail_camera_trace.py` converts CAMTRACE lines copied from Warcraft III; `docs/games/warcraft-3/retail-camera-tracing.md` uses ROC 1.29.2 under Wine.

### Version: 1.26a is not in this repo
- Searched all `.md`, `.c`, `.h`, `.py`, `.java`, `.sh`, `Makefile`: **no "1.26a" reference**.
- The retail binary the docs use is **`game.dll` 1.27.1.7085** (TFT), SHA-256 `d51e5680243fc90e19c9d6074f7fac433c466d3cf5f46e2364291725574d8236`, image base `0x6f000000`. Cited in `docs/games/warcraft-3/audio-retail-analysis.md:5-9`, `cursor-rendering.md:5-7`, `portrait-and-hover-parity.md:5-8`, `nightelfx01-flow.md:6-8`, `scroll-cursors.md:8`.
- Parity README describes "Warcraft III 1.27b build 7085" (`tools/parity/README.md:117-121`). Legacy reference is v1.29.2 (`:141-158`). The demo DLL is 2002 (`ability-inheritance-binary.md:1-12`).
- So the request for "patch 1.26a" addresses has no source. Use the 1.27.1.7085 addresses below and treat them as **not portable** to other builds (`audio-retail-analysis.md:7-8`).

### Recovered addresses (1.27.1.7085, image base 0x6f000000)
Descriptive names only, not PDB symbols. From `docs/games/warcraft-3/audio-retail-analysis.md:83-104` and related docs.

Audio (`audio-retail-analysis.md:89-104`):
- `6f3593e0` play unit response; `6f34b150` play sound label; `6f34fee0` choose variant.
- `6f0823d0` create/play backend sound; `6f0afe00` start instance; `6f0af5e0` admit instance.
- `6f0abd50` lowest-priority channel head; `6f0af9b0` start 2D sample.
- `6f35b0b0` unit cooldown check (GetTickCount, +0xfa = 250 ms); `6f092a10` millisecond clock thunk.
- Response kinds: `6f690ea0` What, `6f690dd0` Pissed, `6f690d70` Yes, `6f690cf0` YesAttack, `6f690ef0` Warcry; `6f6955c0` reset counters.
- Static data: channel config `6fab5ec8` (16 rows × 20 bytes; caps `16,3,3,3,3,8,2,3,5,3,3,8,1,6,2,2`); authored flag names `6fab7c50`; RTTI `CSoundWar3::vftable` at `6fa9efc0`; `CSoundListener::vftable` at `6fbdb124`.
- Global cap is 24 active instances (`:120-125`).

Cursor (`cursor-rendering.md`): `0x6f11dd70` cursor alloc (0x1b0 bytes); `0x6f38a9c0` mode dispatch; `0x6f1a42b0` selection sequence (ECX=sprite, EDX=token array); `0x6f3528e0` token registration; `0x6fabb554` RGB table (18 triplets); `0x6f11d520` update slot +0x2c.

Cinematic/portrait (`portrait-and-hover-parity.md:9-20`): `0x6f2124f0` SetCinematicScene; `0x6f373670` route transmissions; `0x6f3d0a90` scene expiry; `0x6f3cd2c0` talk tag 0x26; `0x6f394d50` world hover.

Group iteration (`nightelfx01-flow.md:61-67`): `ForGroup` `0x6f1fe690` → `0x6f22ff30`; iterator `0x6f22ed70`; temp array `0x6f2939f0`.

Weather (`weather.md:333-336`): `ParticleEmitter_Update` applies `rate × delta` per tick.

### Verdict on parity coverage
- Retail-compared areas are narrow: audio admission and response, cursor mode and animation, cinematic portraits, camera shake, rain alpha and density, NightElfX01 group iteration. Each doc says what is not reproduced (for example `weather.md:354` multiple emitters, `audio-retail-analysis.md` pitch/distance gaps).
- The parity harness itself is manual for gameplay and audio. The automated parity-style tests are the golden renders (opt-in) and the admission differential.

---

## 3. WC3 target: size, maturity, build and test

### Directory sizes (`games/warcraft-3/`, C/H line counts)
| Directory | Files (c/h/py/txt/md) | C/H lines | Role |
|---|---|---|---|
| `game/` | 251 | ~151,500 | Game logic (`libgame`), JASS natives, skills, save/load, bots. |
| `menu/` | 43 | ~10,800 | Menu and glue UI. |
| `renderer/` | 23 | ~9,800 | MDX, W3M terrain, weather, lightning, cursors. |
| `tests/` | 48 | ~8,700 | Standalone test binaries and fixtures. |
| `jass/` | 11 | ~4,900 | JASS interpreter glue and JASS-to-native dispatch. |
| `common/` | 11 | ~3,400 | Map, UI and shared WC3 types. |
| `sheet/` | 2 | ~640 | SLK table reader. |
| `ui/` | 1 | ~160 | UI layout. |
| `share/` | 0 code | assets | Installed read-only data (`Makefile:217-226`). |
| `file-formats/` | 0 code | docs | Format notes. |

Also: `game.mk` (build fragment), `sounds.md`, `jass-native-coverage.md`, `demo-ability-classes.txt`, `tft-ability-classes.txt`.

Docs: `docs/games/warcraft-3/` has about 200 Markdown files covering features, retail evidence, and workflows.

### Tests
- `games/warcraft-3/game/tests/`: 65 `t_*.c` files; roughly 2,400 test-macro occurrences. `game/tests/t_destructable.c:928-964` shows the style (`T_EQ`, `T_ASSERT`, `T_NULL`).
- Reported run in `docs/games/warcraft-3/audio-retail-analysis.md:438-440`: ROC and TFT each **34,771 assertions in 1,924 tests**; mixer suite 1,678 assertions in 17 tests. This is a doc claim; I did not run the suite.
- Standalone binaries: `tests/` (35 entries, including `test_net.c`, `test_sound.c`, `test_renderer_*.c`, `test_mpq_compression.c`, Python audits).

### Build (`Makefile`, `games/warcraft-3/game.mk`)
- `Makefile:213` `default: build`; `Makefile:214` `build: install-share`; `game.mk:141-142` `build: wc3-build` and `wc3-build: shared jass sheet renderer game menu openwarcraft3 tools jass-tool`.
- `game.mk:149` `openwarcraft3: $(BINARY) install-eos-config`.
- Variables: `Makefile:5-70` `BUILD ?= debug|release` (release adds `-O2`), `MSAA`, `GL_BACKEND`, `GLSL`, debug switches (`WC3_DEBUG_*`), `WC3_FOW_PACKED_MASK`, `FFMPEG`, `EOS`.
- Output: `build/bin/openwarcraft3`, `build/lib/libgame.*`, `build/share/<game>/`.

### Test targets
- `game.mk:308` `test:` runs in parallel (`-j$(TEST_JOBS)`, default 16): `test-core`, `test-eos-release`, `test-linux-media-release`, `test-menu-boundary`, `test-assets`, `test-gl-shader`, `test-commands`, `test-jass-build`, `test-galaxy`, `test-server-net`, `test-sound`, renderer and UI suites, WoW suites, `test-menu`, `test-wc3-engine`, `test-client-camera`, `test-wc3-hero-saveload-audit`, `test-render-harness`, `test-mpq-compression`.
- `game.mk:270-275` `test-wc3-engine`: runs `$(WC3_TEST_BINARY)` in 4 shards × 2 variants (classic and `-tft`), `+dedicated 1 +test '$(WC3_PATTERN)'` with `WC3_PATTERN ?= *` (`game.mk:265`).
- `game.mk:278` `test-client-camera`: `+test 'client_camera.*'`.
- `game.mk:353` `test-assets` builds fixtures from `tests/resources` (`blpgen`, `mdxgen`, `mpqtool`, `mdxtool`), so most tests need no retail data.
- `game.mk:170-178` Python audits (`test-wc3-map-audit`, `test-wc3-hero-saveload-audit`).
- `game.mk:192-195` `test-render-harness` (parity launcher and map tests).
- Opt-in (not in `make test`):
  - `game.mk:209` `test-render-golden` (needs display; `tools/parity/render_golden.sh`).
  - `game.mk:318` `test-stress` (33.6 kbps lossy netchan sim).
  - `game.mk:257-260` `test-eos-service` (needs `EOS=1` and EOS SDK).
  - `game.mk:282` `test-jass-build`.

### Maturity
- Large, active, and well documented: 151k lines of game C, about 200 WC3 docs, about 2,400 test macros, and a retail-evidence workflow.
- Limits: this checkout has one commit (`42d11d3`, 2026-10-10), so I cannot date the project from history. Several docs say they are "not yet" complete (`weather.md:354`, `audio-retail-analysis.md` gaps, `DoNotSaveReplay` stub, missing replay and `w3g.md`).
- Multiple WC3 reference binaries are used in docs (1.27.1.7085, 1.29.2, 2002 demo). Each has its own addresses. Do not mix them.

---

## Key findings (ranked)
1. **Gameplay randomness is shared with presentation code via libc `rand()`** (`api_misc.h:1238-1249`, `renderer/r_emit.h:18-29`, `sound/s_sound.c:284`, `client/cl_music.c:419`). Render and audio call counts change gameplay results. Fix before any replay or determinism work. The `s_wander.c` per-unit LCG is the model.
2. **No replay, no `.w3g` support** (`api_misc.h:856` stub; `file-docs/readme.md:51` references a missing doc). `server.md:36` overstates readiness.
3. **Save/load does not capture RNG state** (`g_save.c`).
4. **Fixed 10 Hz server tick, wall-clock scheduled** (`shared.h:60`, `sv_main.c:290-333`). Not lockstep. Desync is not a concern, but floating-point results are platform-dependent.
5. **Docs drift**: `network.md` says protocol 20 (code: 23); `server.md:51` names `clc_move` (code: `clc_input`).
6. **No "1.26a" anywhere**. Retail reference is `game.dll` 1.27.1.7085 (SHA-256 `d51e5680...8236`, base `0x6f000000`).
7. **Parity is mostly black-box Wine comparison**; the Ghidra/Frida/r2 method lives in `tools/ghidra`, `tools/frida`, `tools/r2_ability.sh`; the `tools/parity/README.md` explicitly says no disassembler is needed.
8. **Large mature WC3 target**: `game/` 151k lines; `make test` runs about 19 parallel suites; WC3 engine tests use fixtures, not retail data.
