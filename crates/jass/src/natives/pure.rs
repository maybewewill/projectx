//! Natives that need no game state: real math, number/string conversion, string utilities and the
//! `Convert*` handle wrappers.
//!
//! Every native below carries a short behaviour note (in words) with its implementation address in
//! Game.dll 1.26a and a confidence tag:
//!
//! - `CONFIRMED`: read directly in the decompiled pseudo-C and unambiguous there.
//! - `UNSURE`: the decompile of the native body is missing or unreadable, or the behaviour is
//!   inferred. The implementation is our best reading and must be checked against the original.
//!
//! Real results are computed in f64 and rounded to f32 where the original stores a float. The
//! MSVCR80 `_CI*` functions are not bit-exact here: `UNVERIFIED: MSVCR80 _CI* bit-exactness`.

use crate::vm::{real, NativeCtx, NativeRegistry, NativeResult, Value};

/// pi/180 as f32, the factor used by `Deg2Rad`.
const DEG2RAD: f32 = std::f32::consts::PI / 180.0;
/// 180/pi as f32, the factor used by `Rad2Deg`.
const RAD2DEG: f32 = 180.0 / std::f32::consts::PI;

/// The 44 `Convert*` natives that wrap an integer into a handle type (signature `(I)H<type>`),
/// in the order of the natives table. Their stubs are 16 bytes apart at 0x6f3b2600..=0x6f3b28b0.
const CONVERT_NATIVES: [&str; 44] = [
    "ConvertRace",
    "ConvertAllianceType",
    "ConvertRacePref",
    "ConvertIGameState",
    "ConvertFGameState",
    "ConvertPlayerState",
    "ConvertPlayerScore",
    "ConvertPlayerGameResult",
    "ConvertUnitState",
    "ConvertAIDifficulty",
    "ConvertGameEvent",
    "ConvertPlayerEvent",
    "ConvertPlayerUnitEvent",
    "ConvertUnitEvent",
    "ConvertWidgetEvent",
    "ConvertDialogEvent",
    "ConvertLimitOp",
    "ConvertUnitType",
    "ConvertGameSpeed",
    "ConvertPlacement",
    "ConvertStartLocPrio",
    "ConvertGameDifficulty",
    "ConvertGameType",
    "ConvertMapFlag",
    "ConvertMapVisibility",
    "ConvertMapSetting",
    "ConvertMapDensity",
    "ConvertMapControl",
    "ConvertPlayerColor",
    "ConvertPlayerSlotState",
    "ConvertVolumeGroup",
    "ConvertCameraField",
    "ConvertRarityControl",
    "ConvertBlendMode",
    "ConvertTexMapFlags",
    "ConvertFogState",
    "ConvertEffectType",
    "ConvertVersion",
    "ConvertItemType",
    "ConvertAttackType",
    "ConvertDamageType",
    "ConvertWeaponType",
    "ConvertSoundType",
    "ConvertPathingType",
];

/// Registers all natives of this module on `reg`.
pub fn register(reg: &mut NativeRegistry) {
    // --- angles ---------------------------------------------------------------------------------

    // Deg2Rad (0x6f3b29d0). Behaviour: multiplies the argument by pi/180, using the VM's real
    // multiply. The body is not in the decompile export, so the constant and the operation are
    // inferred from the sibling Rad2Deg. UNSURE.
    reg.register("Deg2Rad", |_, a| {
        ret(Value::Real(real::mul(real_arg(a, 0), DEG2RAD)))
    });
    // Rad2Deg (0x6f3b29f0). Behaviour: multiplies the argument by 180/pi through the software
    // multiply helper FUN_6f6eee20 (the constant lives at DAT_6faae60c, value not extracted).
    // The result operand of that call is not visible in the decompile. UNSURE.
    reg.register("Rad2Deg", |_, a| {
        ret(Value::Real(real::mul(real_arg(a, 0), RAD2DEG)))
    });

    // --- trigonometry and powers ----------------------------------------------------------------

    // Sin (0x6f3b2a10), Cos (0x6f3b2a30), Tan (0x6f3b2a50). Behaviour: the sine, cosine or tangent
    // of the argument in radians, rounded to f32. The wrappers call software float helpers
    // (FUN_6f6ef820 / FUN_6f6ef8d0 / FUN_6f6f0140), not the MSVCR80 `_CI*` names that the earlier
    // math spec assumed. Here f64 math is used. UNVERIFIED: MSVCR80 _CI* bit-exactness. UNSURE.
    reg.register("Sin", |_, a| math1(a, f64::sin));
    reg.register("Cos", |_, a| math1(a, f64::cos));
    reg.register("Tan", |_, a| math1(a, f64::tan));

    // Asin (0x6f3b2a70), Acos (0x6f3b2ab0), Atan (0x6f3b2af0). Behaviour: inverse trigonometric
    // functions, result in radians. Out-of-domain input gives NaN (Asin/Acos), no clamping.
    // Bodies not read in the decompile. UNVERIFIED: MSVCR80 _CI* bit-exactness. UNSURE.
    reg.register("Asin", |_, a| math1(a, f64::asin));
    reg.register("Acos", |_, a| math1(a, f64::acos));
    reg.register("Atan", |_, a| math1(a, f64::atan));

    // Atan2 (0x6f3b2b10). Behaviour: atan2(y, x) with the first argument as y. Body not in the
    // decompile export. UNVERIFIED: MSVCR80 _CI* bit-exactness. UNSURE.
    reg.register("Atan2", |_, a| {
        let y = real_arg(a, 0) as f64;
        let x = real_arg(a, 1) as f64;
        ret(Value::Real(y.atan2(x) as f32))
    });

    // SquareRoot (0x6f3b2ba0). Behaviour: square root of the argument. The x87 FSQRT at double
    // precision is IEEE-correct, so f64 sqrt rounded to f32 gives the same value (the rounding
    // cannot double-round for sqrt). The wrapper body is not in the decompile. UNSURE.
    reg.register("SquareRoot", |_, a| math1(a, f64::sqrt));

    // Pow (0x6f3b2c00). Behaviour: x raised to y. Body not in the decompile export; the game
    // calls the CRT `_CIpow` per math.md. UNVERIFIED: MSVCR80 _CI* bit-exactness. UNSURE.
    reg.register("Pow", |_, a| {
        let x = real_arg(a, 0) as f64;
        let y = real_arg(a, 1) as f64;
        ret(Value::Real(x.powf(y) as f32))
    });

    // --- integer / real conversion --------------------------------------------------------------

    // I2R (0x6f3b2cc0). CONFIRMED: the wrapper calls the software integer-to-float helper
    // FUN_6f6ef350 and returns its result. That helper takes the magnitude, shifts it so its top
    // bit lands at bit 23 and drops the bits below, so the conversion truncates the magnitude
    // toward zero and does not round to nearest. Exact for |i| <= 2^24.
    reg.register("I2R", |_, a| ret(Value::Real(i2r(int_arg(a, 0)))));

    // R2I (0x6f3b2ce0). CONFIRMED: the wrapper's whole body is a call to the float-to-int helper
    // FUN_6f6eec70, which works on the bits of the f32. It truncates toward zero, gives 0 for
    // |x| < 1, and saturates to INT_MAX/INT_MIN by sign for |x| >= 2^31, NaN included (see `r2i`).
    // Checked against the original in Unicorn (tests/natives_pure.rs, tools/emu/verify_strings.py).
    reg.register("R2I", |_, a| ret(Value::Integer(r2i(real_arg(a, 0)))));

    // S2I (0x6f3b2d10). CONFIRMED: the wrapper returns 0 for a null or empty string, otherwise the
    // C library `atoi` of the text (import MSVCR80 `atoi`). So leading white space and a sign are
    // accepted and parsing stops at the first non-digit ("  12x" gives 12, "1.9" gives 1). The
    // overflow result is UNSURE (see `atoi`). The text is not localised, so no TRIGSTR lookup.
    reg.register("S2I", |ctx, a| {
        let s = str_arg(ctx, a, 0);
        ret(Value::Integer(atoi(&s)))
    });

    // S2R (0x6f3b2d50). Behaviour: real value of a string using `parse_real` (CONFIRMED parser
    // shape from FUN_6f6f0520: digits, one dot, stops at the first other character, a leading space
    // gives 0). Sign handling and the exact software-float rounding are UNSURE.
    reg.register("S2R", |ctx, a| {
        let s = str_arg(ctx, a, 0);
        ret(Value::Real(parse_real(&s)))
    });

    // I2S (0x6f3baaa0). CONFIRMED: the format string at 0x6f915d68 is "%d", written by the Storm
    // ordinal 578 (printf-style, 256-byte buffer, which cannot overflow for an i32). So: signed
    // decimal text, "-" for negative values, no padding.
    reg.register("I2S", |ctx, a| {
        let s = int_arg(a, 0).to_string();
        ret(Value::String(ctx.intern(&s)))
    });

    // R2S (0x6f3baaf0). Behaviour: text of the real with two decimals ("%.2f"-style, rounded). Body
    // and format string not in the decompile export. UNSURE.
    reg.register("R2S", |ctx, a| {
        let s = c_fmt_f(real_arg(a, 0), 0, 2);
        ret(Value::String(ctx.intern(&s)))
    });

    // R2SW (0x6f3bab40). Behaviour: like R2S with a width and a precision argument, "%*.*f"-style:
    // a negative width means left-justified with the absolute width, a negative precision means the
    // C default of 6 decimals. Body not in the decompile export. UNSURE.
    reg.register("R2SW", |ctx, a| {
        let s = c_fmt_f(real_arg(a, 0), int_arg(a, 1), int_arg(a, 2));
        ret(Value::String(ctx.intern(&s)))
    });

    // --- strings --------------------------------------------------------------------------------

    // SubString (0x6f3baba0, helper 0x6f4c60b0). CONFIRMED structure from the disassembly: start and
    // end are 16-bit values (see `substring`). The end is clamped to the length. An end before the
    // start gives the tail from the start. Empty source or start past the end takes an error path
    // whose result is UNSURE and is modelled as "". Negative indices are UNSURE (the original would
    // read before the buffer) and are clamped to 0.
    reg.register("SubString", |ctx, a| {
        let s = str_arg(ctx, a, 0);
        let out = substring(&s, int_arg(a, 1), int_arg(a, 2));
        ret(Value::String(ctx.intern(&out)))
    });

    // StringLength (0x6f3bac20). CONFIRMED structure: null or empty gives 0, otherwise the byte
    // length (strlen, import ordinal 506) of the string as the string fetch helper returns it. The
    // fetch helper 0x6f3baa40 also resolves "TRIGSTR_nnn" to the map's localised text, which this
    // VM does not have (see `OPEN` below), so a TRIGSTR argument differs. Byte count of UTF-8 text
    // differs from the original's ANSI bytes. LIKELY.
    reg.register("StringLength", |ctx, a| {
        let s = str_arg(ctx, a, 0);
        ret(Value::Integer(s.len() as i32))
    });

    // StringCase (0x6f3bac40). CONFIRMED from the disassembly: the source is copied with the Storm
    // ordinal 501 into a 1024-byte buffer (so at most 1023 bytes survive), then case-converted in
    // place by ordinal 510 when the boolean is true and ordinal 579 when false. ordinal 510 is taken
    // to be the upper-case routine (LIKELY, Storm names are not in the export). Only ASCII letters
    // change. A null or empty source returns the null string in the original (UNSURE, modelled as
    // ""). The TRIGSTR resolution of StringLength applies here too. Confidence of the case mapping
    // is LIKELY.
    reg.register("StringCase", |ctx, a| {
        let s = str_arg(ctx, a, 0);
        let upper = a.get(1).and_then(|v| v.as_boolean()).unwrap_or(false);
        let out = string_case(&s, upper);
        ret(Value::String(ctx.intern(&out)))
    });

    // StringHash (0x6f3b2d30). CONFIRMED wrapper: null or empty gives 0 (no hash of ""). Otherwise
    // the Storm ordinal 590 is applied to the text. The Storm code is not in the upload. Game.dll
    // carries its own copy of the MPQ string hash (0x6f68cc10, with the crypt table built at
    // 0x6f68cb00), and that copy was checked in Unicorn against `string_hash` on observed pairs
    // (tests/natives_pure.rs). Taking the Game.dll hash with type 0 as Storm's SStrHash is LIKELY,
    // not verified. See `mpq_hash`.
    reg.register("StringHash", |ctx, a| {
        let s = str_arg(ctx, a, 0);
        ret(Value::Integer(string_hash(&s)))
    });

    // OPEN: the TRIGSTR_nnn lookup (both StringLength and StringCase) needs the map's string table
    // (war3map.wts), which is game state. Not implemented here.

    // --- Convert* -------------------------------------------------------------------------------

    // Each Convert*(i) native (0x6f3b2600..=0x6f3b28b0). CONFIRMED: all 44 bodies are the same
    // `return param_1`, i.e. the integer is the handle value unchanged. There is no preallocated
    // handle table (no index into an array, no range check, no lookup), so the handle value is the
    // integer itself, and 0 is the null handle. ConvertUnits (0x6f71d9d0, signature (II)B, table
    // `ai`) is a different kind of native and is not part of this set.
    for name in CONVERT_NATIVES {
        reg.register(name, |_, a| ret(Value::Handle(int_arg(a, 0) as u32)));
    }
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn ret(v: Value) -> NativeResult {
    NativeResult::Return(v)
}

fn int_arg(a: &[Value], i: usize) -> i32 {
    a.get(i).and_then(|v| v.as_integer()).unwrap_or(0)
}

fn real_arg(a: &[Value], i: usize) -> f32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(0.0)
}

fn str_arg(ctx: &NativeCtx<'_>, a: &[Value], i: usize) -> String {
    let id = a.get(i).and_then(|v| v.as_string_id()).unwrap_or(0);
    ctx.string(id).to_string()
}

/// Applies a one-argument f64 function to a real argument and rounds the result to f32.
fn math1(a: &[Value], f: fn(f64) -> f64) -> NativeResult {
    ret(Value::Real(f(real_arg(a, 0) as f64) as f32))
}

/// Integer to real with truncation of the magnitude (see the I2R note). The magnitude is
/// normalised by shifting its top bit to bit 23; bits below that are dropped, not rounded.
pub fn i2r(i: i32) -> f32 {
    if i == 0 {
        return 0.0;
    }
    let mag = i.unsigned_abs();
    let top = 31 - mag.leading_zeros();
    let mant = if top <= 23 {
        mag << (23 - top)
    } else {
        mag >> (top - 23)
    };
    let f = f32::from_bits(((top + 127) << 23) | (mant & 0x7f_ffff));
    if i < 0 {
        -f
    } else {
        f
    }
}

/// Real to integer, as the original's helper 0x6f6eec70 does it, from the bits of the f32:
/// - |x| < 1 (biased exponent below 0x7f, including zeros and denormals) gives 0;
/// - the magnitude is the 24-bit mantissa shifted right (truncation toward zero) or left, so the
///   fraction is dropped, never rounded;
/// - exponent above 0x9d (|x| >= 2^31, infinities and NaN) saturates to INT_MIN for a set sign bit
///   and INT_MAX otherwise. (Rust's `as` would give 0 for NaN, which is not what the original does.)
pub fn r2i(r: f32) -> i32 {
    let bits = r.to_bits();
    let neg = bits >> 31 != 0;
    let e = (bits >> 23) & 0xff;
    if e < 0x7f {
        return 0;
    }
    if e > 0x9d {
        return if neg { i32::MIN } else { i32::MAX };
    }
    let m = (bits & 0x7f_ffff) | 0x80_0000;
    let mag = if e <= 0x96 {
        m >> (0x96 - e)
    } else {
        m << (e - 0x96)
    };
    let v = mag as i32;
    if neg {
        v.wrapping_neg()
    } else {
        v
    }
}

/// C library `atoi` (MSVCR80 import used by S2I). Skips leading white space (space, \t, \n, \v,
/// \f, \r), takes one optional sign, then decimal digits up to the first other character. The
/// digits are accumulated at 64-bit precision with saturation (strtoll-style) and the result is
/// truncated to 32 bits. Overflow behaviour of MSVCR80 is UNSURE: this takes the 64-bit saturation
/// and truncation reading, which only differs from a 32-bit saturating reading past 2^31.
pub fn atoi(s: &str) -> i32 {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut acc: u128 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        acc = (acc * 10 + u128::from(b[i] - b'0')).min(u128::from(u64::MAX));
        i += 1;
    }
    let v: i64 = if neg {
        if acc > 1u128 << 63 {
            i64::MIN
        } else {
            (acc as i64).wrapping_neg()
        }
    } else if acc > i64::MAX as u128 {
        i64::MAX
    } else {
        acc as i64
    };
    v as i32
}

/// StringHash of a JASS string: 0 for the empty string, otherwise the MPQ-style hash with hash
/// type 0 (see `mpq_hash`).
pub fn string_hash(s: &str) -> i32 {
    if s.is_empty() {
        return 0;
    }
    mpq_hash(s.as_bytes(), 0) as i32
}

/// The MPQ-style string hash as Game.dll implements it at 0x6f68cc10: seeds 0x7fed7fed and
/// 0xeeeeeeee; each byte is folded to upper case if it is `a`..`z` (ASCII only), `/` becomes `\`,
/// then `seed1 = table[type*0x100 + c] ^ (seed1 + seed2)` and
/// `seed2 = c + seed1 + seed2 + (seed2 << 5) + 3` (wrapping). The result is `seed1`. Bytes above
/// 0x7f are used as they are. Checked on observed pairs from the original (CONFIRMED for Game.dll).
pub fn mpq_hash(name: &[u8], hash_type: u32) -> u32 {
    let table = crypt_table();
    let base = (hash_type as usize) * 0x100;
    let mut seed1: u32 = 0x7fed_7fed;
    let mut seed2: u32 = 0xeeee_eeee;
    for &byte in name {
        let mut c = byte;
        if c.is_ascii_lowercase() {
            c -= 0x20;
        }
        if c == b'/' {
            c = b'\\';
        }
        seed1 = table[base + c as usize] ^ seed1.wrapping_add(seed2);
        seed2 = u32::from(c)
            .wrapping_add(seed1)
            .wrapping_add(seed2)
            .wrapping_add(seed2 << 5)
            .wrapping_add(3);
    }
    seed1
}

/// The 0x500-entry crypt table of the MPQ hash, built once at run time with the public generator
/// (seed 0x00100001, `seed = seed * 125 + 3 mod 0x2aaaab`, two 16-bit halves per entry). It is
/// computed by code, not read from the binary. Game.dll builds the same table at 0x6f68cb00, and
/// the two were compared in Unicorn.
fn crypt_table() -> &'static [u32; 0x500] {
    static TABLE: std::sync::OnceLock<[u32; 0x500]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 0x500];
        let mut seed: u32 = 0x0010_0001;
        for i in 0..0x100 {
            let mut idx = i;
            for _ in 0..5 {
                seed = (seed * 125 + 3) % 0x2a_aaab;
                let hi = (seed & 0xffff) << 16;
                seed = (seed * 125 + 3) % 0x2a_aaab;
                t[idx] = hi | (seed & 0xffff);
                idx += 0x100;
            }
        }
        t
    })
}

/// StringCase on the bytes of `s`: copied into a 1024-byte buffer (at most 1023 bytes are kept),
/// then ASCII letters are converted to upper or lower case. See the native's note.
pub fn string_case(s: &str, upper: bool) -> String {
    let bytes = &s.as_bytes()[..s.len().min(1023)];
    let mut out = bytes.to_vec();
    if upper {
        out.make_ascii_uppercase();
    } else {
        out.make_ascii_lowercase();
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Text of a real in C `%*.*f` style: `width` (negative means left-justified with its absolute
/// value) and `precision` (negative means 6). NaN and infinities print as `nan` / `inf`.
pub fn c_fmt_f(v: f32, width: i32, precision: i32) -> String {
    let left = width < 0;
    let width = width.unsigned_abs() as usize;
    let prec = if precision < 0 { 6 } else { precision as usize };
    let v = v as f64;
    let body = if v.is_nan() {
        if v.is_sign_negative() {
            "-nan".to_string()
        } else {
            "nan".to_string()
        }
    } else if v.is_infinite() {
        if v < 0.0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        }
    } else {
        format!("{v:.prec$}")
    };
    if body.len() >= width {
        body
    } else if left {
        format!("{body:<width$}")
    } else {
        format!("{body:>width$}")
    }
}

/// Parses the leading real of a string (S2R note, decompile FUN_6f6f0520).
///
/// Rules: one optional leading `+` or `-`; then digits with at most one `.`; the first other
/// character (space, tab, `;`, letter, a second dot) ends the number, so trailing text is ignored.
/// A string with no digits, or one that starts with a space, gives 0. A leading `.5` and a trailing
/// `5.` are accepted. The sign is applied here; the original's handling of the sign is UNSURE.
pub fn parse_real(s: &str) -> f32 {
    let b = s.as_bytes();
    let mut i = 0;
    let mut neg = false;
    if let Some(&c) = b.first() {
        if c == b'+' || c == b'-' {
            neg = c == b'-';
            i = 1;
        }
    }
    let mut text = String::new();
    let mut seen_dot = false;
    let mut digits = 0usize;
    while let Some(&c) = b.get(i) {
        if c.is_ascii_digit() {
            text.push(c as char);
            digits += 1;
        } else if c == b'.' && !seen_dot {
            seen_dot = true;
            text.push('.');
        } else {
            break;
        }
        i += 1;
    }
    if digits == 0 {
        return 0.0;
    }
    if text.starts_with('.') {
        text.insert(0, '0');
    }
    if text.ends_with('.') {
        text.push('0');
    }
    let v: f32 = text.parse().unwrap_or(0.0);
    if neg {
        -v
    } else {
        v
    }
}

/// SubString (see the native's note). `start` and `end` are first truncated to 16 bits, because the
/// original's helper takes `short` arguments. Returns "" when the source is empty or `start` is
/// past its end (UNSURE: the original raises an error there).
pub fn substring(s: &str, start: i32, end: i32) -> String {
    let b = s.as_bytes();
    let n = b.len() as i32;
    let start = start as i16 as i32;
    let end = end as i16 as i32;
    if n == 0 || start > n {
        return String::new();
    }
    let start = start.clamp(0, n) as usize;
    let end = end.clamp(0, n) as usize;
    let slice = if end < start {
        // The original writes a terminator at `end` and then copies from `start` up to the real
        // terminator, so an end before the start yields the tail (CONFIRMED shape, UNSURE effect).
        &b[start..]
    } else {
        &b[start..end]
    };
    String::from_utf8_lossy(slice).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i2r_truncates_above_24_bits() {
        // 2^24 + 3 is a tie between 2^24 + 2 and 2^24 + 4; truncation picks 2^24 + 2.
        assert_eq!(i2r(16_777_219), 16_777_218.0);
        assert_eq!(i2r(-16_777_219), -16_777_218.0);
        assert_eq!(i2r(i32::MIN), -2_147_483_648.0);
        assert_eq!(i2r(i32::MAX), 2_147_483_520.0);
        assert_eq!(i2r(0), 0.0);
    }

    #[test]
    fn parse_real_rules() {
        assert_eq!(parse_real("1.5"), 1.5);
        assert_eq!(parse_real("2.5abc"), 2.5);
        assert_eq!(parse_real(" 3"), 0.0);
        assert_eq!(parse_real(""), 0.0);
        assert_eq!(parse_real("-"), 0.0);
        assert_eq!(parse_real("+7"), 7.0);
        assert_eq!(parse_real(".5"), 0.5);
        assert_eq!(parse_real("5."), 5.0);
        assert_eq!(parse_real("1.2.3"), 1.2);
    }

    #[test]
    fn fixed_format() {
        assert_eq!(c_fmt_f(1.23456, 0, 2), "1.23");
        assert_eq!(c_fmt_f(-0.5, 0, 2), "-0.50");
        assert_eq!(c_fmt_f(1.23456, 8, 3), "   1.235");
        assert_eq!(c_fmt_f(1.5, -6, 1), "1.5   ");
        assert_eq!(c_fmt_f(1.5, 0, -1), "1.500000");
    }

    #[test]
    fn substring_clamps_and_wraps() {
        assert_eq!(substring("hello", 1, 3), "el");
        assert_eq!(substring("hello", 2, 99), "llo");
        assert_eq!(substring("hello", 3, 2), "lo");
        assert_eq!(substring("hello", 5, 5), "");
        assert_eq!(substring("hello", 65_537, 65_539), "el");
        assert_eq!(substring("", 0, 0), "");
        assert_eq!(substring("hello", 6, 6), "");
    }
}
