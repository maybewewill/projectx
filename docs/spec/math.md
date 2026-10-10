# Warcraft III Game.dll 1.26a: math primitives (spec)

Scope: findings from the decompiled chunks in `scratchpad/decomp/` and the Ghidra project in `scratchpad/proj2/`. The DLL binary itself is not on disk, and the raw image bytes could not be carved out of the Ghidra block file, so nothing below was checked against raw opcodes. Every claim is tagged:

- CONFIRMED: read directly in the decompile or index.
- LIKELY: strong inference from code shape or from the CRT/Ghidra structure.
- UNSURE: not resolved.

Addresses are given as they appear in the decompile (image base 0x6F000000 range).

---

## 0. Important correction to the working premise

The premise "no fsin/fcos/fsqrt, so trig and sqrt are software" does not match the decompile.

- CONFIRMED: every sqrt, sin, cos, tan, atan, atan2, asin, acos, pow, fmod, log10 and exp call in the game code goes through MSVC x87 CRT helper names: `_CIsqrt`, `_CIsin`, `_CIcos`, `_CItan`, `_CIatan`, `_CIatan2`, `_CIasin`, `_CIacos`, `_CIpow`, `_CIfmod`, `_CIlog10`, `_CIexp`. About 72 sqrt, 71 sin and 69 cos call sites were counted.
- CONFIRMED: these helpers take their argument on the x87 stack (ST0) and return the result in ST0, and the wrappers around them show no range reduction of their own.
- CONFIRMED: the DLL is built with Visual Studio 2010 (FLIRT "Library: Visual Studio 2010 Release" headers). The `_CI*` names appear in the Ghidra symbol table next to static-CRT names (`memset`, `qsort`, `_HUGE`, `ainCRTStartup`, `__dllonexit`), so the CRT is linked statically.
- LIKELY: the VS2010 `_CI*` helpers are thin wrappers around the x87 instructions FSQRT, FSIN, FCOS, FPTAN/FPATAN, F2XM1 and FYL2X (with an fprem-based range-reduction fallback for out-of-range sin/cos). If so, the DLL contains these opcodes inside the CRT code, and the premise is wrong. This is why the fsin/fcos scan can report zero: such a scan only covers the game's own code, not the statically linked CRT helpers.
- CONFIRMED: the helper bodies are NOT in the exported chunks (no `// ====` block exists for any `_CI*` name), so the decompile cannot show the opcodes.
- Recommended check (not done here): scan the whole image, all sections, for the byte sequences D9 FA (fsqrt), D9 FE (fsin), D9 FF (fcos), D9 F3 (fpatan), D9 F2 (fptan) and D9 F0/F1 (f2xm1/fyl2x). Also locate the `_CIsin` / `_CIsqrt` entry points and disassemble them.

Table search result: no lookup tables, no Newton-iteration loops and no hand-written polynomial approximations were found in the decompile. The only math constants visible are the DAT_ / _DAT_ addresses listed in section 5, and their values are not in the decompile. CONFIRMED: no sine/cosine/sqrt table exists in the chunks I searched. LIKELY: none exists in the binary either, since the trig calls go through the CRT.

---

## 1. Native-to-implementation mapping

- CONFIRMED: the JASS native names and their signatures are registered in two large functions: `FUN_6f3d4020` (chunk_017, 23 KB of registration calls) and `FUN_6f71d9e0` (chunk_027). They include the strings "Deg2Rad", "Rad2Deg", "Atan2", "SquareRoot" and the signatures "(RR)R" and "(R)R".
- CONFIRMED: each registration passes a name pointer (a DAT_ address) and a signature string. The implementation pointer is not visible in those calls.
- UNSURE: which implementation function backs Sin, Cos, Tan, Atan2, Asin, Acos, Pow, SquareRoot, Deg2Rad and Rad2Deg. The native table is not in the chunks, so the exact mapping is unresolved.
- Candidates (not confirmed as natives): the small `_CI*` wrappers in section 3.

---

## 2. Square root and length helpers

Sqrt is always the CRT `_CIsqrt` helper (section 0). No separate software sqrt exists.

Helpers that depend on sqrt:

| Address | Behaviour | Confidence |
|---|---|---|
| FUN_6f301be0 | 3D vector length: sqrt of (z*z + x*x + y*y), returned as float. | CONFIRMED |
| FUN_6f521dd0 | 3D distance between two points: sqrt of the sum of squared component differences (second point in EAX). | CONFIRMED |
| FUN_6f011550 | 3D normalize: divides x, y, z by sqrt(x*x+y*y+z*z), using the reciprocal. No zero-length guard. | CONFIRMED |
| FUN_6f0114f0 | 2D normalize (x, y), using the reciprocal. No zero-length guard. | CONFIRMED |
| FUN_6f522230 | 2D normalize of two separate float pointers, using the reciprocal of the sqrt. No guard. | CONFIRMED |
| FUN_6f38d9e0 | 2D scale-and-normalize: scales a 2D vector by a given factor divided by the sqrt of the input length. | LIKELY |
| FUN_6f38b7c0 | Point along a segment: computes t = scale / length, then output = t*(b-a)+a for 3 components. Likely "move a point toward a target by a distance". | LIKELY |
| FUN_6f73aef0 | Returns (a.y - b.y) divided by a sqrt whose argument is computed upstream. Exact formula not visible. | UNSURE |
| FUN_6f4ac110 | Point on the unit sphere from z: x = sqrt(1-z*z)*cos(t), y = sqrt(1-z*z)*sin(t), z = z. | CONFIRMED (shape) |

CONFIRMED: none of the normalize helpers checks for a zero-length input. With all x87 exceptions masked (see section 4), a zero vector gives Inf or NaN rather than an error. Preserve this behaviour when reimplementing.

---

## 3. Trig wrappers and angle helpers

| Address | Behaviour | Confidence |
|---|---|---|
| FUN_6f4e6bd0 | Writes sin(theta) to offset +0x24 and cos(theta) to offset +0x20 of a struct (single precision). | CONFIRMED |
| FUN_6f4e6c10 | Reads an angle through FUN_6f4e7ea0 (wrap, section 3.1). If the wrapped angle changed, sets a dirty bit (+0x0D, bit 3) and stores the new angle at +0x1C. Then computes sin and cos as FUN_6f4e6bd0 does. | CONFIRMED |
| FUN_6f4aca60 | Writes (cos, sin) of its implicit angle to a 2-float output. Calls FUN_6f011390 first (unknown helper). | CONFIRMED |
| FUN_6f7e4560 | Angle from a cosine-like input: if |x - C| < eps returns 0; otherwise returns acos(x), and if a sign field is negative returns (PI_CONST - acos(x)). C is probably 1.0. | LIKELY |
| FUN_6f4b20e0 | Asin/atan2 pair for a 3D direction. If the input is >= 1.0, returns atan2 with a fixed constant; if it is above a small threshold, computes atan2, asin and atan2 again; otherwise negates atan2 and uses a fixed constant. | CONFIRMED (shape), values UNSURE |
| FUN_6f00d340 | Uses cos and sqrt with two clamped ratios. Used as a ramp function (1 when inside a radius, linear fall-off outside). | LIKELY |
| FUN_6f00d5c0 | Uses sqrt and cos to scale a value by (cos(theta)+1)*0.5-like factor, with a threshold and a linear ramp. | LIKELY |

### 3.1 Angle wrap helper FUN_6f4e7ea0

- CONFIRMED: takes one float on ST0, calls FUN_6f7e11e0 (float-to-int64 helper, section 4) to get an integer n, and returns x - C*n. If x is negative it uses n-1 instead of n (so the result is non-negative), using the same constant C = DAT_6fa86c60.
- LIKELY: C is the full turn in the units of the caller (2*PI for radians). The caller probably pre-divides by C. This pre-division is not visible in this function.
- UNSURE: exact tie behaviour at half-turn boundaries.

---

## 4. Float-to-int conversion

### 4.1 FPU setup

- CONFIRMED: `FUN_6f6ce230` calls `_clearfp()` and then `_control87(0x0009001F, 0x000FFFFF)`.
- Decoded new control word 0x0009001F:
  - Precision control bits 16-17 = 01, which is 53-bit precision. CONFIRMED against the stated premise.
  - Rounding control bits 10-11 = 00, which is round-to-nearest-even. CONFIRMED.
  - Exception masks 0-4 all set (0x1F): invalid, denormal, zero-divide, overflow, underflow are masked. CONFIRMED.
  - Bit 19 set (0x80000), which is the denormal-operand exception mask (_EM_DENORMAL in the MSVC headers). LIKELY.
- Mask 0x000FFFFF covers the bits above, so all of them are written.

### 4.2 Helper FUN_6f7e11e0 (the main R2I path)

- CONFIRMED: about 180 call sites in the decompile.
- CONFIRMED: behaviour when the global `DAT_6fae759c` is 0 (the only path that appears to be used, since no writer of that flag was found in the decompile):
  1. Round ST0 with FISTP. Ghidra models FISTP as `round(ST0)`, so the result follows the current FPU rounding mode, which is round-to-nearest-even (section 4.1). This is 64-bit.
  2. If the result is non-zero (or the high word is non-zero), correct it for the fraction using the 0x80000000 test on the difference between ST0 and the rounded integer. This makes negative inputs round toward the nearest integer in the same direction and gives a 64-bit result.
  3. Return the 64-bit integer. Callers normally use the low 32 bits.
- CONFIRMED: when `DAT_6fae759c` is non-zero, the helper returns a plain truncating (int) cast of ST0 instead.
- So the default float-to-int in the game is round-to-nearest, not truncation. R2I-style conversions that need truncation must switch the control word (see 4.3).

### 4.3 Explicit rounding-mode switches

- CONFIRMED: the decompile shows 44 places where a function reads the FPU control word as `in_FPUControlWord` (Ghidra artifact). These are where code depends on the current mode.
- CONFIRMED: about 298 float-to-int casts in the decompile are written as ROUND(...), which is FISTP under the current mode.
- CONFIRMED: no FISTTP (truncating) forms appear in the decompile, so truncation is not expressed in these chunks.
- Stated figure from the brief: 648 fldcw sites in the binary. Not verified here, because the raw bytes are not available.
- UNSURE: which of the 648 sites truncate (chop) and which round. The decompile collapses them.

### 4.4 Other rounding helpers

- CONFIRMED: `floor((double)x)` appears 16 times and `ceil` 14 times, calling the CRT.
- LIKELY: patterns `ROUND(x + 0.5)` and `ROUND(x - 0.5)` (about 40 sites) implement round-half-up and round-half-away-from-zero by hand, on top of the nearest mode. This is consistent with the 0.5 constant in section 5.

### 4.5 Out-of-range values (LIKELY, from helper shape)

- Values larger than the 64-bit range are not defined by the helper above. The x87 invalid-operation exception is masked, so the FISTP result is the "integer indefinite" value 0x8000000000000000 (LIKELY).
- The 32-bit callers take the low word of that, which gives 0x00000000 for the low part. This is LIKELY, not confirmed.

---

## 5. Constants (addresses known, values NOT extracted)

The decompile refers to these constants by address. Their values are in .rdata and need the raw binary to read.

| Address | Usage in decompile | Likely value | Confidence |
|---|---|---|---|
| DAT_6f875120 / _DAT_6f875120 | Multiplier with (cos+1) and an additive term in ROUND(x + C) | 0.5 | LIKELY |
| DAT_6f953cb0 | Used as PI in the acos reflection (FUN_6f7e4560) | PI or 2*PI | LIKELY |
| DAT_6fa86c60 | Period in the angle wrap (FUN_6f4e7ea0) | 2*PI or 1 (turn) | UNSURE |
| DAT_6f876620 | Compared against input in the acos guard (FUN_6f7e4560) | 1.0 | LIKELY |
| DAT_6f92e790 | Epsilon in the acos guard | small positive | UNSURE |
| DAT_6f953d20 | Returned for the input >= 1.0 case in FUN_6f4b20e0 | PI/2 | LIKELY |
| DAT_6f93660c | Returned for the input <= -threshold case in FUN_6f4b20e0 | -PI/2 | LIKELY |
| DAT_6f875aa0 | Scale factor in several ROUND(x * C) sites | 1.0 or a scale | UNSURE |
| DAT_6f876610 / DAT_6f876618 | Ramp bounds in FUN_6f00d5c0 | radius bounds | UNSURE |
| DAT_6fae759c | Mode flag for the float-to-int helper (0 = FPU rounding path) | 0 | CONFIRMED (read as 0 in default path) |

Candidate float bit patterns, computed here, so that the binary can be checked against them later. These are the standard float encodings of the values, not values read from the binary:

- pi = 0x40490FDB (3.14159274...)
- 2*pi = 0x40C90FDB
- pi/2 = 0x3FC90FDB
- pi/180 = 0x3C8EFA35 (0.01745329...)
- 180/pi = 0x42652EE1 (57.29578...). Note the last digit is 1, not 0.
- 0.5 = 0x3F000000
- 1.0 = 0x3F800000

To confirm, read the four-byte values at the DAT_ addresses above in the raw image and compare with these.

---

## 6. Angle conventions (natives)

- CONFIRMED: the JASS natives Deg2Rad and Rad2Deg exist in the registration (section 1).
- UNSURE: the constants they use. The likely candidates are pi/180 (0x3C8EFA35) and 180/pi (0x42652EE1). The implementation pointer was not resolved (section 1), so this is not confirmed.
- LIKELY: the trig helpers take radians, because the wrappers pass the value straight to the x87 helpers without a scale factor (section 3).
- UNSURE: whether any native converts degrees internally.

---

## 7. What was NOT found

- No sine, cosine, sqrt or atan lookup table in the chunks.
- No Newton-iteration loops and no polynomial approximations.
- No `fsin`, `fcos` or `fsqrt` pseudo-ops in the decompile (because they live in the CRT helpers, section 0).
- No writer of the float-to-int mode flag `DAT_6fae759c` in the chunks.
- No native-to-implementation mapping (section 1).

## 8. Next steps for verification

1. Scan the whole image (all sections) for the x87 opcode bytes listed in section 0, and for the `_control87` call that sets 0x0009001F.
2. Find the `_CIsin`, `_CIsqrt`, `_CIatan2` entry points (IAT or FLIRT) and disassemble them to confirm the x87 instruction and the fprem fallback.
3. Read the values at the DAT_ addresses in section 5.
4. Resolve the native table (the JASS registration structure) to map Sin, Cos, Atan2, SquareRoot, Deg2Rad and Rad2Deg to their implementation functions.
5. Check which of the 648 fldcw sites chop and which round.
