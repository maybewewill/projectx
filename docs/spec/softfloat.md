# Game.dll 1.26a: software float helpers (spec)

Status 2026-10-10. Scope: every routine that the JASS virtual machine and the math natives
use for `real` values, found by starting at the VM interpreter (`0x6F45E9D0`) and at the
math natives in the native table, and following their callees. The Rust port is in
`crates/softfloat`; the differential test is `tools/emu/softfloat_vectors.py`.

Everything below was checked against the original code (Unicorn, the user's own
`Game.dll`) unless marked **open**. Addresses are virtual addresses with image base
`0x6F000000`.

No code is quoted. Descriptions are behaviour, written from the disassembly and the
decompiled listing.

---

## 1. Calling convention

The helpers are fastcall-like and never use the FPU for the value itself. Operands and
results are IEEE binary32 bit patterns stored in memory, and every helper takes pointers.

| Kind | ECX | EDX | stack argument | return |
|---|---|---|---|---|
| binary (add, sub, mul, div, pow, atan2) | result pointer | pointer to operand A | pointer to operand B (`ret 4`) | EAX = result pointer |
| unary (sqrt, sin, cos, tan, asin, acos, atan, exp, ln, trunc, floor, ceil, round) | result pointer | pointer to operand | none | EAX = result pointer |
| I2R (`0x6F6EF350`) | result pointer | the integer itself | none | EAX = result pointer |
| F2I, R2I (`0x6F6EEC20`, `0x6F6EEC70`) | pointer to operand | - | none | EAX = integer |
| compares (`0x6F00C110`, `0x6F01FD40/60/80`, `0x6F009AE0`) | pointer to A | - | pointer to B (`ret 4`) | EAX = 0 or 1 |
| approximate equality (`0x6F00C3F0`) | pointer to A | pointer to B | none | EAX = 0 or 1 |
| sincos pair (`0x6F6EF990`) | pointer to x | pointer to sin result | pointer to cos result (`ret 4`) | none |

The result pointer is also returned in EAX, so chains of calls reuse the temporary slot.
Arguments that a Rust caller passes by value are the same bits.

## 2. Helper inventory

| Address | Name | Status |
|---|---|---|
| `0x6F6EEC00` | count leading zeros (32-bit, 32 for zero) | ported (`clz`, internal) |
| `0x6F6EEC20` | F2I: real to int, truncating, wraps | ported (`conv::f2i`) |
| `0x6F6EEC70` | R2I: real to int, truncating, saturates | ported (`conv::r2i`) |
| `0x6F6EED10` | integer square root (32-bit, floor) | ported (internal to sqrt) |
| `0x6F6EEE20` | MUL | ported (`arith::mul`) |
| `0x6F6EEEF0` | SUB (`a - b`) | ported (`arith::sub`) |
| `0x6F6EF000` | ADD | ported (`arith::add`) |
| `0x6F6EF100` | RECIP (table) | ported (`arith::recip`) |
| `0x6F6EFE10` | DIV | ported (`arith::div`) |
| `0x6F6EF170` | FLOOR | ported (`math::floor`) |
| `0x6F6EF200` | CEIL | ported (`math::ceil`) |
| `0x6F6EF290` | TRUNC (to integral real) | ported (`math::trunc`) |
| `0x6F6EF330` | ROUND (`floor(x + 0.5)`) | ported (`math::round`) |
| `0x6F6EF350` | I2R: int to real, truncating | ported (`conv::i2r`) |
| `0x6F6EF750` | SQRT | ported (`math::sqrt`) |
| `0x6F6EF820` | SIN | ported (`math::sin`) |
| `0x6F6EF8D0` | COS | ported (`math::cos`) |
| `0x6F6EF990` | SINCOS pair (sin and cos in one reduction) | ported (as sin + cos) |
| `0x6F6EFB00` | ACOS | ported (`math::acos`) |
| `0x6F6EFC70` | ASIN | ported (`math::asin`) |
| `0x6F6EFE50` | (not identified, used 18 times) | **open** |
| `0x6F6EFF20` | ln series core | ported (internal to ln) |
| `0x6F6EFFE0` | ln mantissa dispatch | ported (only the path used for `[1, 2)`) |
| `0x6F6F0060` | LN | ported (`math::ln`) |
| `0x6F6F00E0` | integer power by squaring | ported (`math::pow_int`) |
| `0x6F6F0140` | TAN | ported (`math::tan`) |
| `0x6F6F0170` | ATAN | ported (`math::atan`) |
| `0x6F6F02D0` | ATAN2 | ported (`math::atan2`) |
| `0x6F6F0360` | R2S / R2SW (format a real as text) | **open** |
| `0x6F6F0520` | S2R (parse a real from text) | **open** |
| `0x6F6F0690` | EXP core | ported (internal to exp) |
| `0x6F6F07B0` | EXP wrapper (negative argument via RECIP) | ported (`math::exp`) |
| `0x6F6F0810` | POW | ported (`math::pow`) |
| `0x6F6F08B0` | (not examined) | **open** |
| `0x6F6EED50`, `0x6F6EFE00` | thin wrappers around CRT-style helpers at `0x6F4C2CD0`, `0x6F4C22C0` | not needed (identified as wrappers) |
| `0x6F009AE0` | compare: a == b (ordered) | ported (`arith::eq`) |
| `0x6F00C110` | compare: a < b (ordered) | ported (`arith::lt`) |
| `0x6F01FD40` | compare: a > b (ordered) | ported (`arith::gt`) |
| `0x6F01FD60` | compare: a <= b (ordered) | ported (`arith::le`) |
| `0x6F01FD80` | compare: a < b (same as `0x6F00C110`) | ported (`arith::lt`) |
| `0x6F00C3F0` | approximate equality: abs(a - b) < 0.001 | ported (`arith::approx_eq`) |

The VM's unary minus on reals is an inline sign flip (`x ^ 0x80000000`), with no call.
It is ported as `arith::neg`.

## 3. Arithmetic

### 3.1 ADD (`0x6F6EF000`) and SUB (`0x6F6EEEF0`)

- **Zero exponents.** If the exponent field of A is zero (zero, or any denormal), the result
  is B unchanged, even if B is non-zero. If the exponent field of B is zero, the result is A
  unchanged.
- **Operands.** Each mantissa gets its implicit leading one (`0x800000`), is doubled, and is
  negated (two's complement) if the sign bit is set. So the signed value is `2 * m`.
- **Alignment.** Let `d = eB - eA` (exponent fields, 32-bit). If `d <= 0` and `d > -0xB800000`,
  the B mantissa is shifted right arithmetically by `(eA - eB) >> 23`, and the result exponent
  is `eA`. If `d > 0` and `d < 0xB800000`, the A mantissa is shifted right by `d >> 23` and the
  result exponent is `eB`. Otherwise the smaller operand is dropped and the other is returned
  unchanged (so an exponent gap of 23 or more gives the larger operand).
- **Sum and normalisation.** The aligned signed values are added. A zero sum gives `+0`.
  Otherwise the sign is taken from the sum, its magnitude is normalised so that the leading
  one lands on bit 23 by a shift (left or right, arithmetic on a positive value, so truncating),
  and the exponent becomes `base + (7 - clz) << 23`, with `clz` the leading-zero count of the
  magnitude. All of this is 32-bit wrapping arithmetic.
- **Truncation.** Bits shifted out are discarded (round toward zero for the result, but the
  alignment of a negative smaller operand rounds toward minus infinity). No rounding mode.
- **Overflow.** The exponent is not checked: when `base + (7 - clz) << 23` leaves the 32-bit
  range the value wraps. No infinity or NaN is ever produced.
- **SUB** is exactly `ADD(a, b XOR 0x80000000)`. This includes the zero-exponent cases.

Checked: 10,000 random cases (see section 8).

### 3.2 MUL (`0x6F6EEE20`)

- **Full path** (both fractions non-zero, exponents irrelevant): each fraction is taken as
  `(f | 0xFF800000) << 8` in 32 bits, which is the two's complement form of `f - 2^23` scaled
  by 256. The high 32 bits of the unsigned 64-bit product are then normalised by one bit (if
  the top bit is set, a shift of one), shifted right by 7 and masked to 23 bits. The exponent
  is `eA + eB - 127` (plus one step if the product needed the shift). The sign is the XOR of
  the signs.
- **Power-of-two path** (one fraction is zero): the exponents are added with the same bias,
  and the fraction is the OR of the two fractions (which is the non-zero one).
- **Underflow.** If the biased result exponent minus one is negative (result exponent below 1),
  the result is `+0`. Overflow wraps.
- **Zero.** If either exponent field is zero and the fraction path does not apply, the result
  is `+0`. Note that the zero test is on the **fraction** for the full path: a denormal with a
  non-zero fraction goes through the full path, and a `+0`/`-0` exponent is only checked after
  the fraction test.

Checked: 10,000 random cases.

### 3.3 DIV (`0x6F6EFE10`) and RECIP (`0x6F6EF100`)

- `DIV(a, b)`: if the two operands have identical bits, the result is `1.0` (so `0/0` and
  `x/x` are `1.0`). Otherwise it is `MUL(a, RECIP(b))`.
- `RECIP(b)`: the fraction of `b` selects one of 1024 segments by its top 10 bits. The segment
  end values come from the reciprocal table (section 5). The next 13 fraction bits give the
  interpolation weight, replicated to 32 bits. The result is the first table value minus
  `(difference * weight) >> 32`, then the exponent field is rebuilt as `(253 - eB) << 23` (done
  by subtracting `eB << 23` and adding `253 << 23`). If the result, less `0x800000`, is negative,
  the result is `+0`. The sign of `b` is kept.
- So division is **not correctly rounded**: `a / b` is `a` times a 13-bit-interpolated
  reciprocal, with truncation at every step.

Checked: 10,500 random cases (including `x/x`).

### 3.4 Compares

All of these use the x87 `fcompp`/`fucompp` instructions on the two f32 values loaded into the
FPU. The results are exact IEEE comparisons (no soft-float involved), with NaN giving false.

- `lt(a, b)` = `a < b` (`0x6F00C110`, `0x6F01FD80`)
- `le(a, b)` = `a <= b` (`0x6F01FD60`)
- `gt(a, b)` = `a > b` (`0x6F01FD40`)
- `eq(a, b)` = `a == b`, so `+0 == -0` (`0x6F009AE0`)
- `approx_eq(a, b)` = `|SUB(a, b)| < 0.001` (`0x6F00C3F0`). The difference is the soft SUB,
  and the constant is `0x3A83126F`. **This is the VM's real `==`.** Note the VM's `!=` on
  reals (opcode `0x1B`) is a plain integer compare of the bit patterns in the disassembly
  and the decompile. Not verified here (it belongs to the VM spec).

### 3.5 Conversions

- **I2R** (`0x6F6EF350`): `0` gives `+0`. Otherwise the magnitude (`neg` for negative inputs,
  which keeps `0x80000000` for `INT_MIN`) is normalised so that its leading one is at bit 23
  (shift left, or arithmetic shift right, which truncates). The exponent is `158 - clz`, and the
  sign is the integer's sign bit. So at most 24 significant bits survive: `I2R(16777219) =
  16777218.0`.
- **F2I** (`0x6F6EEC20`, internal): a biased exponent below 127 gives 0. Otherwise the 24-bit
  mantissa (with implicit one) is shifted by `150 - exponent`, right for non-negative shifts
  and left otherwise, with the x86 rule that the shift count is taken mod 32 (so large
  exponents wrap). The sign is applied at the end (two's complement).
- **R2I** (`0x6F6EEC70`, the JASS native path): same as F2I, but a biased exponent above
  `0x9D` (157) saturates: `INT_MIN` for a negative sign bit, `INT_MAX` otherwise. Infinities
  and NaN patterns follow the same rule.

Checked: 10,000 random cases each.

## 4. Transcendental helpers

Table names refer to section 5, constants to section 6.

### 4.1 SQRT (`0x6F6EF750`)
- Zero, negative (sign set) and `-0` give `+0`.
- The mantissa is turned into a 32-bit value `0x80000000 | (f << 8) | (f >> 15)`, its integer
  square root (16 bits, restoring method, `0x6F6EED10`) gives `r`, and the estimate mantissa is
  `0x3F800000 | (((r - 0xB504) * 0xB505) >> 8)`.
- The exponent is halved (floor of `e / 2`), with the `sqrt(2)` mantissa `0x3504F3` for odd
  exponents. The two factors are multiplied with MUL.
- This is not correctly rounded.

### 4.2 SIN, COS, TAN (`0x6F6EF820`, `0x6F6EF8D0`, `0x6F6F0140`, `0x6F6EF990`)
- **Reduction.** `x` is multiplied (soft MUL) by `4/pi * 2^17` (constant `0x6FADA910`) and
  truncated (F2I) to an integer `k`.
- **Indexing.** Bits 18-19 of `k` give the quadrant `q`. Bits 8-17 give the table index `i`
  (0..1023), and bits 0-7 give the weight `w`, which is replicated to a 32-bit fraction
  (`w * 0x01010101`).
- **Interpolation.** For even `q` the value is `T[i] + ((T[i+1] - T[i]) * fraction) >> 32`. For
  odd `q` the table is walked backwards: `T[1024-i] - ((T[1024-i] - T[1023-i]) * fraction) >> 32`.
  `T` is the sine quarter-wave table (section 5).
- **Sign and scale.** Bit 1 of `q` negates the value. The value is converted with I2R, and
  then divided by `2^31` by subtracting 31 from its exponent field (only if that field is
  non-zero).
- **COS** is the same, with `q + 1`. **TAN** is `DIV(SIN(x), COS(x))`. The sin/cos pair
  (`0x6F6EF990`) computes the same two values in one reduction; the results are identical to
  the separate routines.

Checked: sin, cos, tan with 10,000 random cases each (arguments mostly in `[-10, 10]`, some
larger and some special values).

### 4.3 ASIN, ACOS (`0x6F6EFC70`, `0x6F6EFB00`)
- **Small arguments** (`|x|` bits up to `0x3F7E8000`, about `0.992`): `x * 2^30` is truncated,
  clamped to `+-0x3FFFFFFF`, and the magnitude index and weight are taken as in the sin
  reduction. The table (section 5, the arc-sine table) is interpolated, with a mirrored table
  for negative indices. ASIN subtracts the constant `0x3243F6A8` from the integer; ACOS does not
  add it, and ACOS does not negate the index. The result is I2R-converted and scaled by `2^-29`.
- **Large arguments.** The magnitude is `min(|x|, 1.0)` in bits. The integer difference
  `0x3F800000 - magnitude` is normalised by its leading-zero count, and a second table
  (section 5) is interpolated, scaled by `2^-34`. Then ASIN gives `+pi/2 - value` (or
  `-pi/2 + value` for negative `x`), and ACOS gives `value` for positive `x` and `pi - value`
  for negative `x`.

Checked: 10,000 random cases each.

### 4.4 ATAN, ATAN2 (`0x6F6F0170`, `0x6F6F02D0`)
- **ATAN.** `v = |x|`. If `v > 1`, `v` is replaced by `1/v` (flag). If `v > 2 - sqrt(3)`
  (`0x3E8930A3`), `v` is replaced by `(v - 1/sqrt(3)) / (1 + v/sqrt(3))` (flag, adds `pi/6` at
  the end). The core is `X * (c0 + c1*s) / (1 + c2*s)` with `X = v`, `s = X*X` and the three
  coefficients from the constant pool. The flag for `v > 1` gives `pi/2 - result`. A negative
  non-zero `x` negates the result.
- **ATAN2(y, x)** (standard convention; the original takes `y` in EDX and `x` on the stack).
  If `x` has a zero exponent the result starts at `pi/2`; otherwise it is `atan(|DIV(y, x)|)`.
  A negative non-zero `x` gives `pi - r`, and a negative non-zero `y` negates `r`.

Checked: 10,000 random cases each.

### 4.5 EXP (`0x6F6F07B0`, core `0x6F6F0690`)
- A negative non-zero argument gives `RECIP(exp(-x))`, otherwise the core.
- **Core.** The argument is scaled by 4 (exponent field + 2) if its exponent is non-zero.
  Its integer part `k` (TRUNC) and fraction `f = x - k` (soft SUB) are split. The fraction is
  divided by 4 by subtracting `1 << 24` from its bits, but only when a test on
  `f - 0x01800000` (sign of the XOR with `f`) passes, otherwise it is zero. `e^g` is a Horner
  chain of the Taylor coefficients `1, 1, 1/2, 1/6, 1/24, 1/120` (constants in section 6). The
  result is multiplied by `(e^(1/4))^k` (`0x3FA45AF2`, by squaring, see POW_INT), with `k` taken
  from `F2I(TRUNC(x'))`.

Checked: 2,884 random cases (arguments of all magnitudes, including negative).

### 4.6 LN (`0x6F6F0060`, series `0x6F6EFF20`, mantissa dispatch `0x6F6EFFE0`)
- `m = 1.f` (in `[1, 2)`), `e` the unbiased exponent.
- `series(m)`: `q = (m - 1) / (m + 1)` (soft ADD and DIV), `s = q*q`, `X = q * (1 + c1*s)`,
  where `c1 = -4/15` (`0xBE88D424`); the exponent of `X` is incremented once (doubling) when it
  is not zero; `Y = 1 + c2*s` with `c2 = -0.6` (`0xBF19BF59`). Result: `DIV(2X, Y)`.
- `ln(x) = MUL(ADD(I2R(e), MUL(series(m), log2(e))), ln2)`.
- The dispatch at `0x6F6EFFE0` has two further branches (for `m` below about `0.88`), which
  never run for `m` in `[1, 2)`. They are not ported.

Checked: 3,000 random cases (including zero, negative and denormal inputs).

### 4.7 POW (`0x6F6F0810`)
- If the exponent is an integer and not negative (`-0` included; the integer test is
  `exp == TRUNC(exp)`), the result is `POW_INT(base, F2I(exp))`.
- Otherwise, a base with a zero exponent field gives `+0`, and anything else gives
  `EXP(MUL(exp, LN(base)))`.
- **POW_INT** (`0x6F6F00E0`): result starts at `1.0`. For each bit of `n` from the bottom, the
  result is multiplied by the current power when the bit is set, and the power is squared.
  **Open divergence:** the original shifts `n` arithmetically, so a negative `n` never reaches
  zero and the original does not return (it hangs). This port shifts logically (at most 32
  steps). Callers reach it only with `n >= 0` through POW, but EXP can reach a negative `n` for
  very large arguments; there the original hangs and this port returns a value.

Checked: 2,829 random cases (171 skipped because the original does not return for them).

## 5. Lookup tables (read from the user's own Game.dll at runtime)

No table contents are in the repository. `Tables::from_game_dll` reads the bytes at these
virtual addresses from the PE file.

| Name | VA | Entries (u32) | Used by |
|---|---|---|---|
| reciprocal | `0x6F9774C8` | 1025 | RECIP (division; atan for `v > 1`; exp for a negative argument) |
| sine quarter-wave | `0x6F9784D0` | 1025 | SIN, COS, TAN |
| arc sine | `0x6F9794D8` | 1025 | ASIN, ACOS (small arguments) |
| arc sine, near 1 | `0x6F97A4E0` | 153 | ASIN, ACOS (large arguments) |

The sine table is a quarter-wave of `2^31 * sin(i * pi/2 / 1024)`; index 1024 is `2^31 - 1`.
The reciprocal table is 1025 values that decrease from `0x800000`. These are descriptions of
the data shape only.

## 6. Constants

Almost all float constants of the helpers are written into the image's data by a start-up
initialiser: about 125 tiny stubs at `0x6F84DC00..0x6F84E340` and more at `0x6F863A00..0x6F864000`.
Some of them are computed by the engine's own I2R and DIV at start-up (for example
`0x6FADA908 = DIV(100, 101)`). The values that the helpers use are below. Where the value is a
standard mathematical constant, the bit pattern is given as the stored value.

| Constant | VA | Value (bits) | Used by |
|---|---|---|---|
| -1.0 | `0x6FAAE464` | `0xBF800000` | ADD(-1), ln series |
| 0.0 | `0x6FAAE470` | `0x00000000` | pow, atan2, zero results |
| 0.001 | `0x6FAAE47C` | `0x3A83126F` | approx_eq |
| 0.5 | `0x6FAAE4D0` | `0x3F000000` | ROUND |
| 1.0 | `0x6FAAE4F4` | `0x3F800000` | many |
| pi/180 | `0x6FAAE5E8` | `0x3C8EFA35` | (native Deg2Rad, not a helper) |
| 180/pi | `0x6FAAE60C` | `0x42652EE1` | (native Rad2Deg, not a helper) |
| log2(e) | `0x6FAAE5DC` | `0x3FB8AA3B` | ln |
| ln 2 | `0x6FAAE5D8` | `0x3F317218` | ln |
| -pi/2 | `0x6FAAE5E4` | `0xBFC90FDB` | asin |
| pi/2 | `0x6FAAE5FC` | `0x3FC90FDB` | atan2, atan, asin |
| pi | `0x6FAAE600` | `0x40490FDB` | acos, atan2 |
| pi/6 | `0x6FAAE5F4` | `0x3F060A92` | atan |
| 4/pi * 2^17 | `0x6FADA910` | `0x4822F983` | sin/cos/tan reduction |
| 1/sqrt(3) | `0x6FADA900` | `0x3F13CD3A` | atan |
| -1/sqrt(3) | `0x6FADA904` | `0xBF13CD3A` | atan (reduction numerator) |
| 2 - sqrt(3) | `0x6FADA8FC` | `0x3E8930A3` | atan threshold |
| atan coefficient c0 | `0x6FADA8F0` | `0x3F7FFFF0` | atan |
| atan coefficient c1 | `0x6FADA8F4` | `0x3E8415A6` | atan |
| atan coefficient c2 | `0x6FADA8F8` | `0x3F17592E` | atan |
| exp e^(1/4) | `0x6FADA8DC` | `0x3FA45AF2` | exp |
| exp Taylor g^1/2 | `0x6FADA8E0` | `0x3EFFFFBD` | exp |
| exp Taylor g^1/6 | `0x6FADA8E4` | `0x3E2AB479` | exp |
| exp Taylor g^1/24 | `0x6FADA8E8` | `0x3D296EC9` | exp |
| exp Taylor g^1/120 | `0x6FADA8EC` | `0x3C1A534C` | exp |
| ln series c1 | `0x6FADA8D4` | `0xBE88D424` | ln |
| ln series c2 | `0x6FADA8D8` | `0xBF19BF59` | ln |
| ASIN offset | immediate | `0x3243F6A8` | asin (subtracted), acos (none) |
| ASIN offset 2 | immediate | `0x6487ED51` | asin/acos (negative index branch) |
| sqrt mantissa | immediate | `0x3504F3` | sqrt (odd exponent) |
| sqrt estimate | immediate | `0xB504`, `0xB505` | sqrt |

## 7. Quirks to keep (all reproduced)

- `ADD`/`SUB`/`MUL` never produce infinity or NaN. Exponent overflow wraps.
- Zero and denormal inputs with a non-zero fraction are treated as normal in MUL, DIV and
  SUB's `b`-side. Zero exponents short-circuit ADD.
- Division is not correctly rounded (reciprocal table plus one multiply).
- `CEIL(+-0) = 1.0` (the original's zero test returns one). `FLOOR(-0) = +0`.
- Compares of the VM are IEEE, but the VM's `==` on reals is `|a - b| < 0.001`.
- `F2I` wraps, `R2I` saturates.
- Trig functions are table approximations, not correctly rounded.

## 8. Verification (differential, Unicorn against the original)

Method: `tools/emu/softfloat_vectors.py` maps the user's `Game.dll`, runs the start-up
initialiser, and calls each helper with random and edge-case inputs (random bit patterns,
values near `2^24`, powers of two, denormals, zeros, infinities, NaNs, negatives, moderate
and large magnitudes). The Rust port replays the recorded vectors bit for bit.

Results are in section 9 below.

## 9. Results

RESULTS_PLACEHOLDER

## 10. Open items

- `0x6F6EFE50`, `0x6F6F08B0`: identified only as used by other helpers; not examined in depth.
- `0x6F6F0360` (R2S / R2SW) and `0x6F6F0520` (S2R): text formatting and parsing. Not ported.
  R2S uses the C runtime `sprintf` for the integer part, and soft arithmetic for the fraction.
- The natives around these helpers (for example `Asin`, `Acos`, `Sqrt`, `Pow`, `Atan2`,
  `Deg2Rad`) check their arguments before calling the helpers, and return `0.0` or `1.0`
  for some inputs. Those wrappers are not part of this crate.
- The `0x6F6EFFE0` dispatch branches for `m` below about `0.88` are not ported (unused by LN).
- POW with a negative integer exponent reaching `POW_INT` with a negative count: the original
  never returns; this port returns a value (see section 4.7).
- `docs/spec/math.md` section 0 claims that the transcendental functions go through MSVCR80
  `_CI*` helpers. That is contradicted by this spec: the natives go through the soft helpers
  above. That file is not edited here.
