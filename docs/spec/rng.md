# Warcraft III 1.26a game.dll: random number generation (interop spec)

Scope: behaviour of gameplay and script randomness in the DLL, for a clean-room reimplementation.
Binary analysed: game.dll, MD5 ba5a2fe39f08a8272ddb1a45dfb62569 (from the Ghidra import log), image base 0x6F000000.

Confidence labels: CONFIRMED = read from the binary and checked by emulation where noted; LIKELY = strong structural evidence, not checked end to end; UNSURE = a guess or an open item.

## 0. Method and how the claims were checked

- The Ghidra export (decomp chunks) is not reliable for this area. The native handler cluster is merged into one wrong function, and about 18,000 tiny functions were skipped. All analysis below uses direct x86 disassembly of the DLL plus the decompile where it was consistent.
- Every generator routine was re-implemented in Python and compared with the original machine code run under an x86 emulator (unicorn), with the DLL mapped at its base and the RNG global, state and float constants set up. Results:
  - Seeder: 51 seeds, 0 mismatches. CONFIRMED.
  - Step (advance one generator): 200 random states, 0 mismatches, including the returned value. CONFIRMED.
  - GetRandomInt: 300 random cases including low greater than high and low equal to high, with the resulting generator state checked each time: 0 mismatches. CONFIRMED.
  - Dice sampler (count up to 16): 300 cases, 0 mismatches. CONFIRMED.
  - Stream table initialisation and SetRandomSeed: 10 cases, all 45 streams and the global state match. CONFIRMED.
  - Per-stream real value on [0, 1): 150 of 150 exact, assuming the two runtime constants are 0.0 and 1.0 (see section 9). Other scalings were tested with 2.5 and with -3.0 to 4.0: 121 of 150 and 109 of 150 exact, the rest within the soft-float rounding described in 3.3. CONFIRMED for the formula, LIKELY for the constants.
  - GetRandomReal: the structure matches, but the float helper routines are not IEEE round-to-nearest or simple truncation, so results differ by 1 or 2 units in the last place in some cases. See section 3.3. CONFIRMED that it differs, UNSURE on the exact rounding rule.
- Validation scripts and the extracted table are in the scratchpad directory: rngtools/ (emu.py, test_*.py) and specs/rng_table_6f953650.bin.
- Important: the notes in scratchpad/openrealm/ say that GetRandomInt uses libc rand(). The DLL does not do this. Those notes describe a different source tree and should not be used as a spec for this binary. CONFIRMED.

## 1. Natives and where they go

| JASS native | Signature | Handler (implementation address) | Generator used | Confidence |
|---|---|---|---|---|
| GetRandomInt | (int, int) returns int | 0x6F3B3560 | Global gameplay generator G | CONFIRMED |
| GetRandomReal | (real, real) returns real | 0x6F3B35A0 | G | CONFIRMED |
| SetRandomSeed | (int) returns nothing | 0x6F3B36D0 | Reseeds G and all 45 streams | CONFIRMED |
| ChooseRandomCreep | | 0x6F3B3660, calls FUN_6F29FD90 | Stream 32 | CONFIRMED |
| ChooseRandomNPBuilding | | 0x6F3B3680, calls FUN_6F294740 | Stream 34 | CONFIRMED |
| ChooseRandomItem | | 0x6F3B3690, calls FUN_6F2B9E10 | Stream 35 | CONFIRMED |
| ChooseRandomItemEx | | 0x6F3B36B0, calls FUN_6F2B9E10 | Stream 35 | CONFIRMED |
| PlaceRandomUnit | | 0x6F3CB620 | No random sink found within two call levels | UNSURE |
| PlaceRandomItem | | 0x6F3CB700 | No random sink found within two call levels | UNSURE |

Notes:
- Registration: each native is registered by pushing a signature string, loading the name string into edx and the handler into ecx, then calling a shared register routine. Name strings are at 0x6F945784 (GetRandomInt), 0x6F945774 (GetRandomReal), 0x6F945590 (SetRandomSeed). Signatures: "(II)I", "(RR)R", "(I)V". CONFIRMED.
- A second registration table (around 0x6F71D9E0) lists the same names with a different pointer (0x6F71D9D0). Which table is live is UNSURE. The handler addresses above come from the first table.
- GetRandomPercentageBJ and GetRandomDirectionBJ are not in the DLL. They are script-level wrappers in the common script library, so the spec does not need them. CONFIRMED.
- Calling convention: GetRandomInt and SetRandomSeed take their arguments on the stack in order (first argument at the lowest address) and return with plain ret. GetRandomReal takes pointers to float32 values, not the values. CONFIRMED from the code. The mapping from JASS argument order to the first and second parameter is LIKELY.

## 2. The generator

### 2.1 State

- One generator is an 8-byte state: word A (accumulator, 32 bits) and word B (packed indices, 32 bits). CONFIRMED.
- Every generator uses this same state layout and the same step function. The difference between generators is only which state is passed in.
- Generator G is the state object referenced by the global pointer at 0x6FAB73D8. The global holds the address of the 0x978-byte Ipse object, and the state sits at the start of that object (offsets 0 and 4). CONFIRMED.
- The 45 streams are 8-byte records laid out after a 4-byte header in an array owned by the engine's subsystem 13. Stream k is at array base + 4 + 8k, with A at +0 and B at +4. The array base is reached through the subsystem-13 object: its field at +0x10, then that object's field at +0xC. CONFIRMED.

### 2.2 Step (advance one generator, return a 32-bit output)

Table T: 256 bytes at address 0x6F953650, located in read-only data. It is a permutation of 0..255 (every byte value appears once). The table is given in section 11 and saved as specs/rng_table_6F953650.bin. SHA-256 of the 256 bytes: ab809fa11d38d2f59a23157edec2c1cea21e74e2ae53b2f1fc0b76c9aaba129f. CONFIRMED.

Steps, with B split into four bytes (b3 = bits 24-31, b2 = bits 16-23, b1 = bits 8-15, b0 = bits 0-7):

1. For each byte, compute a new index: subtract a per-byte offset and, if the result is negative, add a per-byte modulus:

| Byte | Offset | Modulus | Resulting index used for table lookup and for new B |
|---|---|---|---|
| b3 (bits 24-31) | 4 | 188 | p |
| b2 (bits 16-23) | 12 | 212 | q |
| b1 (bits 8-15) | 24 | 236 | r |
| b0 (bits 0-7) | 28 | 244 | s |

   Rule: index = byte - offset if byte >= offset, else byte - offset + modulus. CONFIRMED.

2. Read four little-endian 32-bit words from table T at byte offsets p, q, r and s. Unaligned reads are intended: the offset is a byte offset into T. Offsets are at most 251, so the reads stay inside the 256-byte table. CONFIRMED.

3. Mix = ROTL32(T[r], 3) XOR ROTL32(T[q], 2) XOR T[s] XOR ROTL32(T[p], 1). Here T[x] means the 32-bit word read at byte offset x. CONFIRMED.

4. New A = (old A + Mix) mod 2^32. The step returns this new A value as its output. CONFIRMED.

5. New B = (p << 24) | (q << 16) | (r << 8) | s. CONFIRMED.

### 2.3 Seed (initialise one generator from a 32-bit seed value S)

- A = S.
- B = ((S mod 59) << 10) | ((S mod 61) << 2) | ((S mod 53) << 18) | (((17 * floor(S / 47) + S) mod 64) << 26), all unsigned 32-bit arithmetic. The four fields do not overlap, so OR and addition are the same. CONFIRMED (binary and emulation).

### 2.4 Stream initialisation (fill all 45 streams from a value X)

1. Let L be a local generator seeded with X (section 2.3).
2. For k = 0 to 44: advance L by one step (section 2.2) and take its output value V_k. Seed stream k with V_k (section 2.3).

This writes 45 generators. CONFIRMED (emulated against all 45 streams).

## 3. Mapping outputs to integer and real ranges

### 3.1 Integer draw (used by GetRandomInt on G, and for stream picks)

Let r be the output of one step.

- Pick from [0, n): floor(r * n / 2^32), i.e. the high 32 bits of the 64-bit product. CONFIRMED.
- GetRandomInt(lo, hi), where lo is the first argument and hi the second:
  - If lo equals hi: return lo, and do NOT advance G. CONFIRMED (the generator state is unchanged in the emulation).
  - If lo is less than hi: size = hi - lo + 1, result = lo + floor(r * size / 2^32). Range is inclusive of both ends. CONFIRMED.
  - If lo is greater than hi: size = lo - hi + 1, result = lo + floor(r * size / 2^32). So the range runs upward from lo to lo + (lo - hi), inclusive. The bounds are NOT swapped. CONFIRMED (emulated, 0 mismatches over 300 cases, including reversed bounds).
- Results are 32-bit signed integers.

### 3.2 Per-stream integer pick

Stream k with range n: pick = floor(r_k * n / 2^32), where r_k is the output of one step of stream k. Used by the ChooseRandom natives to choose an index in a filtered candidate list. CONFIRMED (structure) and LIKELY (use).

### 3.3 Real draw (GetRandomReal and per-stream real)

- Let m be the low 23 bits of the step output. U = m / 2^23 exactly. This is the value obtained by building the float with exponent 127 and mantissa m, then subtracting 1.0. CONFIRMED. The subtraction of 1.0 is exact, so U has no rounding.
- GetRandomReal(a, b), where a and b are passed by pointer to float32:
  - Always advances G by exactly one step, including when a equals b. CONFIRMED.
  - Range = |b - a| (computed with the float helper below). CONFIRMED.
  - Result = a + Range * U, with the multiply and add done by the float helpers. CONFIRMED structure.
  - If b is at least a, the result is in [a, b). If b is less than a, the result is in [a, 2a - b), i.e. it runs upward from a by |b - a| (same as the integer case). CONFIRMED.
- Float helpers: the original uses its own single-precision soft-float routines, not the FPU and not IEEE rounding. Add (FUN_6F6EF000), subtract (FUN_6F6EEEF0, which negates the second operand and adds), multiply (FUN_6F6EEE20) and integer-to-float (FUN_6F6EF350). Measured against the emulator on 400 random operations each: add and subtract match correctly rounded float32 in about 76% of cases, multiply in about 51%. Neither round-to-nearest nor simple round-toward-zero matches exactly.
  - Consequence: GetRandomReal results may differ from an IEEE float32 implementation by 1 to 2 units in the last place in a noticeable fraction of calls (about 40% of GetRandomReal calls with a simple round-to-nearest model, about 7% with a truncate model). For lockstep parity, port the three helper routines bit-for-bit. UNSURE on the exact rounding rule; this is the main open item.
- Per-stream real (used by combat chance rolls): value = C0 + U * (C1 - C0), with the stream's own step output as U. With C0 = 0.0 and C1 = 1.0 this is simply U in [0, 1). CONFIRMED structure; the constants are runtime values (section 9), so LIKELY for [0, 1).

### 3.4 Dice roll (sum of dice) and its sampler

- Sampler for count up to 16: for each of count dice, take one step of the stream, and add floor(r * sides / 2^32). The state advances once per die. CONFIRMED (emulation, 300 cases).
- Roll wrapper (used for attack damage and other dice): total = sum over the dice of (floor(r * sides / 2^32) + 1), then + count, then + bonus. So each die gives 1..sides, plus a flat bonus. CONFIRMED (arithmetic read from the wrapper).
- If count is less than 1, the wrapper returns 0 and logs "Bogus request to roll 0 dice". CONFIRMED.
- For count greater than 16 the sampler does not loop. It uses a normal-approximation path (square root, a Gaussian draw on the same stream, rounding, clamping to [0, (sides - 1) * count]). Exact constants UNSURE.

## 4. Seeding: where the seed comes from

### 4.1 Start of a game (network / game-setup path)

Function FUN_6F5C37A0. It computes a seed S and then:
- seeds G with S (section 2.3), and
- initialises the 45 streams from S directly (section 2.4, with X = S; no extra step before the streams). CONFIRMED.

How S is computed:
- S starts at 0.
- It loops over 16 player-slot indices. For each slot that passes a validity check (a lookup in the game data), S := ROTL32(S XOR N, 3), where N is a per-game count returned by the helper FUN_6F5C0780 (the slot count, LIKELY).
- The validity check and the meaning of N are only partly understood. CONFIRMED for the fold itself; UNSURE for the check and for N.
- Override flag: if bit 0x8000 of the game-settings flags (the flags field at offset 0x38 of the settings object referenced by the global at 0x6FAB65F4, object offset 0x30) is set, the seed is the constant 0x77617233 instead of the fold, and the streams are initialised from that constant. Otherwise the fold is used. CONFIRMED structure; the meaning of the flag is LIKELY ("fixed seed").
- A second site in the same function seeds G with the fold value but does not reinitialise the streams. Its branch condition is UNSURE.

### 4.2 Second game-start path

Function FUN_6F5C0EE0 seeds G with a seed passed as a stack argument, then initialises the streams from the same value (section 2.4). The caller of FUN_6F5C0EE0 was not identified. Whether this is the replay-playback path or a join path is UNSURE.

### 4.3 SetRandomSeed (JASS, at any time)

1. G := seed(s) (section 2.3).
2. t := the output of one step of G (so G advances once).
3. Initialise the 45 streams from t (section 2.4).

Effect: SetRandomSeed reseeds both the script generator and every combat and creep stream. CONFIRMED (emulated, 10 cases).

### 4.4 Engine start (before any game)

G is created during engine start (function FUN_6F468B30, via FUN_6F46DA10) and seeded with the constant 0x69707365. This is a placeholder: game setup reseeds G. CONFIRMED.

The stream array is not known to be initialised before the first game-setup or SetRandomSeed call. The initial stream state is UNSURE.

### 4.5 Time and tick count

- The DLL calls GetTickCount and time functions in several places, but none of them feeds the seeds of G or the streams. CONFIRMED.
- The one tick-count seed is in CRT startup code (section 5), which affects only the C runtime rand() and is not used by the gameplay generators.

## 5. C runtime rand() and srand()

The DLL imports rand, srand, time and ctime from the MSVCR80 runtime. Call sites found by scanning the code for every reference to the rand and srand import slots (both direct calls and register-loaded calls):

| Address | Function | Use | Subsystem (confidence) |
|---|---|---|---|
| 0x6F72B015 (srand) | FUN_6F72B000 | srand(GetTickCount()) at engine start, called from FUN_6F008420 which reads config.txt and Engine.cpp strings | Engine start / presentation (LIKELY) |
| 0x6F55546F (srand, constant 0x4FA7) | FUN_6F555450 | Seeds the C runtime with a constant | Font and UI resource code (LIKELY) |
| 0x6F5559AD (srand, constant 0x150B) | FUN_6F5559A0 | Seeds the C runtime with a constant | Font and UI resource code (LIKELY) |
| 0x6F6EC619 (srand, argument) | FUN_6F6EC5F0 | Argument is 0 (called from FUN_6F6EC6D0, which is called with 0 from FUN_6F6B2030) | Session/unknown (UNSURE) |
| 0x6F4F3BFD, 0x6F4F3C08 (rand) | FUN_6F4F3AD0 | Particle emitter init (called from the CPlaneParticleEmitter constructor FUN_6F4F1830) | Presentation (LIKELY) |
| 0x6F4F3DE7 (rand, register load) | FUN_6F4F3CC0 | Indirect rand calls | UNSURE |
| 0x6F4DDBD9 (rand, register load) | FUN_6F4DDAB0 | Indirect rand calls | UNSURE |
| 0x6F51237D (rand) | FUN_6F5122B0 | Called from FUN_6F512530 | UNSURE (possibly sound or UI) |
| 0x6F52AA6A (rand) | FUN_6F52A9F0 | | UNSURE |
| 0x6F52D19C (rand) | FUN_6F52D110 | | UNSURE |
| 0x6F5559C0 (rand) | FUN_6F5559A0 | Font/UI resource code | UI (LIKELY) |
| 0x6F6EC622 (rand) | FUN_6F6EC5F0 | After srand, see above | UNSURE |
| 0x6F71D0D1 (rand) | FUN_6F71D0C0 | Wrapper: integer in [lo, hi), rand() % (hi - lo) + lo, returns lo if range below 1. 17 callers | Editor, file, terrain, FX (LIKELY, by string evidence) |
| 0x6F71D0F1 (rand) | FUN_6F71D0F0 | Wrapper: real in [lo, hi), lo + (hi - lo) * rand() / RAND_MAX-like constant. 6 callers | Same as above (LIKELY) |

Checks: none of the JASS natives or the combat entry points checked (damage, evade, critical, miss, curse, silence, attack init, deflection, reaction delay, ChooseRandom*, GetRandomInt, GetRandomReal, SetRandomSeed, game-setup seeding) reaches a rand or srand site within two call levels. CONFIRMED. The 17 and 6 callers of the two wrappers were examined by name only; one of them ("Blight puffs used", FUN_6F756400) is UNSURE, because it might be gameplay or presentation. A gameplay caller of the wrappers would be a determinism risk and should be checked before porting.

Conclusion: CRT rand is not part of the gameplay random stream. Gameplay uses only G and the 45 streams.

## 6. Gameplay consumers and which stream they use

Stream identifiers are the indices the caller passes. The 45 streams are numbered 0 to 44.

| Stream | Consumer (function, address) | Kind of draw | Confidence |
|---|---|---|---|
| G | JASS GetRandomInt, GetRandomReal | integer or real from the global generator | CONFIRMED |
| 2 | Attack damage: CAbilityAttack::GetDamageAmount (0x6F0C6940). Dice roll with count, sides and bonus taken from per-attack fields | dice roll | CONFIRMED stream, LIKELY meaning; field names UNSURE |
| 2 | Attack init: CAbilityAttack::Initialize (0x6F0D1590) | real in [0,1) | LIKELY |
| 2 | Evasion: CAbilityEvasion::EvadeAttack (0x6F08BD20) | real in [0,1) | LIKELY |
| 2 | Critical strike: CAbilityCriticalStrike::EvadeAttack (0x6F1A34C0) | real in [0,1) | LIKELY (critical chance) |
| 2 | Miss: CMissile::ChanceToMiss (0x6F2BEAC0) | real in [0,1) | LIKELY (miss chance) |
| 2 | On-hit curse: CBuffCurse::CurseAttack (0x6F07F850), CBuffSilence::CurseAttack (0x6F1D5DD0) | real in [0,1) | LIKELY (proc chance) |
| 9 | Reaction delay (FUN_6F0CB340) | real in [0,1) | LIKELY |
| 43 (0x2B) | Deflection: DefendDeflection (FUN_6F098D20, FUN_6F0991F0) | real in [0,1) | LIKELY |
| 32 (0x20) | ChooseRandomCreep (via FUN_6F29FD90) | integer pick from filtered list | CONFIRMED use |
| 34 (0x22) | ChooseRandomNPBuilding (via FUN_6F294740) | integer pick from filtered list | CONFIRMED use |
| 35 (0x23) | ChooseRandomItem and ChooseRandomItemEx (via FUN_6F2B9E10) | integer pick from filtered list | CONFIRMED use |
| 33 (0x21) | Consumer not identified (FUN_6F29FF40) | integer pick | UNSURE |
| 42 (0x2A) | Three integer picks with n = 50 (FUN_6F0B97B0), n = 100 and n = 10 (FUN_6F0C0D80) | percentage-style chance rolls | CONFIRMED stream; LIKELY chance rolls |
| 3, 4, 8, 11, 12, 13, 14, 16 to 31, 36, 37, 39, 40, 41, 44 | Other ability dice and real rolls, about 60 call sites in total. Examples: stream 23 for 2d10 rolls (FUN_6F081420 and FUN_6F088490, count 2, sides 10, bonus 0); stream 17 for 1d10 (FUN_6F154700); stream 24 for a dice roll (FUN_6F12DAA0); stream 29 for a real roll (FUN_6F1EA080). | dice or real | Stream numbers CONFIRMED from the call sites (taken from the constant loaded before each call); purposes UNSURE |

Creep drops:
- Drop selection is through ChooseRandomItem / ChooseRandomItemEx (stream 35) and ChooseRandomCreep (stream 32), not through G. CONFIRMED use of streams; LIKELY that these are the drop paths.
- Random placement (PlaceRandomUnit / PlaceRandomItem): no random sink found within two call levels, so the source is UNSURE. It may reach a ChooseRandom* call deeper in the call tree.

Ordering: all attacks share stream 2, so the order in which units attack and roll determines the outcome. A reimplementation must use the same sequence of draws on the same streams in the same simulation order to stay in lockstep.

## 7. Summary of the seed chain at game start

Restatement of sections 2 to 4. Confidence is the same as in those sections (CONFIRMED except for the fold check, the flag meaning and the replay link).

1. Engine start: G := seed(0x69707365). Placeholder.
2. Game setup (FUN_6F5C37a0 path): S := fold over slots, or 0x77617233 if the flag is set. G := seed(S). Streams := init(S).
3. During play: GetRandomInt and GetRandomReal advance G one step per call (except GetRandomInt with equal bounds). Combat and creep rolls advance the stream they name.
4. SetRandomSeed(s) at any time: G := seed(s); t := step(G); streams := init(t).

## 8. What is not yet confirmed

- Exact float helper rounding (section 3.3). Blocker for bit-exact GetRandomReal.
- The meaning of the flag bit 0x8000 and the validity check in the fold (section 4.1).
- Whether the fold value S is the same 32-bit value stored in the replay game-start record. The task statement says the replay stores a seed; the writer of that record was not located in this pass. UNSURE.
- The caller of FUN_6F5C0EE0 (section 4.2).
- The 45-stream initial state before the first game setup (section 4.4).
- The normal-approximation path for dice counts above 16 (section 3.4).
- PlaceRandomUnit / PlaceRandomItem source (section 6).
- Purpose of each individual stream other than those listed (section 6).

## 9. Runtime constants (not in the file image)

The image stores zeros at these addresses. Their runtime values are inferred:

| Address | Used by | Assumed value | Evidence |
|---|---|---|---|
| 0x6FAAE464 | GetRandomReal and the per-stream real: adding it to u turns u into U | -1.0 | With -1.0, the emulated GetRandomReal matches the model to within 2 ulp; with 0.0 it would not match at all. LIKELY |
| 0x6FAAE470 | Zero return of the dice wrapper; C0 in the per-stream real | 0.0 | Used as the zero value on the error path. LIKELY |
| 0x6FAAE4F4 | C1 in the per-stream real | 1.0 | Required so that the per-stream real is in [0,1). LIKELY |
| 0x6FAAE614 | Comparison constant in GetRandomReal | 0.0 | Does not change the result (both branches give the same value). LIKELY |
| 0x6F876898 | Used by a different real helper (2.0 as a double) | 2.0 | Read from the image. CONFIRMED |

## 10. Summary table of constants

| Constant | Value | Where |
|---|---|---|
| State size | 8 bytes (A, B) | all generators |
| Table | 256-byte permutation at 0x6F953650 | step |
| Index offsets | 4, 12, 24, 28 | step, bytes b3, b2, b1, b0 |
| Index moduli | 188, 212, 236, 244 | step |
| Rotations in mix | 1, 2, 3 (byte b3 index rotated by 1, b2 by 2, b1 by 3, b0 not rotated) | step |
| Seed moduli | 59, 61, 53; divisor 47; multiplier 17; mask 64 | seed |
| Seed shifts | 10, 2, 18, 26 | seed |
| Stream count | 45 | stream init |
| Stream stride | 8 bytes, header 4 bytes | stream array |
| Game-start fixed seed | 0x77617233 | game setup, flag bit 0x8000 |
| Engine placeholder seed | 0x69707365 | Ipse object creation |
| Dice limit for fast path | 16 | sampler |
| Constant srand seeds | 0x4FA7, 0x150B (CRT, UI) | section 5 |

## 11. Appendix: lookup table T (256 bytes, offset from 0x6F953650)

Each row is 16 bytes, from offset 0x00 to 0xF0. Values are hexadecimal.

| Offset | Bytes |
|---|---|
| 00 | 8e 14 27 99 fd aa c7 08 d5 e6 3e 1f f6 bb 55 da |
| 10 | 75 a0 4a 6a e8 bd 97 ff de 9b bc 9f 81 8a a1 46 |
| 20 | 6e 0b e3 63 76 7a 6c 5d 88 d3 69 ca c3 47 b9 25 |
| 30 | 83 ab a2 3f a6 41 7c ba e5 ac 95 01 7e cf 09 c1 |
| 40 | d9 62 70 71 8d db 05 02 24 87 ef 54 c6 d4 37 30 |
| 50 | d0 1b cb 7b b8 e4 d8 ec 49 ce ad dc 13 a9 94 c4 |
| 60 | 8f 39 ae 0d 18 52 dd 0e 78 fa f5 85 58 d2 af 6d |
| 70 | a4 b2 53 3b 51 a5 50 be fc 2d f4 11 48 98 16 f1 |
| 80 | 86 df 3d 66 5e 44 2e 2f 36 07 6b 17 8b 29 4c b6 |
| 90 | e2 89 5f e7 cd a7 21 e1 4d c9 65 ed fe ee 9c 23 |
| A0 | 33 7d b7 04 9e 9a 2a 40 b3 10 5b f3 82 77 1c 92 |
| B0 | 20 4e 1e 57 22 72 06 8c 67 2c 73 fb 59 c2 0a bf |
| C0 | 79 5c f9 0c 28 1a 12 68 74 34 19 42 b1 c0 84 f8 |
| D0 | 38 f0 15 9d 60 f2 3a 6f b4 90 eb 91 1d 7f 35 61 |
| E0 | 5a 32 03 56 a3 c5 2b 93 80 0f 4b 43 f7 a8 e0 3c |
| F0 | 96 d1 64 26 d7 45 cc 4f c8 b0 e9 b5 00 d6 31 ea |

Reads of a 32-bit word at offset x take bytes x through x+3 (little-endian). The table is read-only in the image and is the same in every run.
