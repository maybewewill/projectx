//! Natives that need no game state: real math, number/string conversion, string utilities and the
//! `Convert*` handle wrappers.
//!
//! Every native below carries a short behaviour note (in words) with its implementation address in
//! Game.dll 1.26a and a confidence tag:
//!
//! - `CONFIRMED`: read directly in the decompiled pseudo-C or in the spec, and unambiguous there.
//! - `UNSURE`: the decompile of the native body is missing or unreadable, or the behaviour is
//!   inferred. The implementation is our best reading and must be checked against the original.
//!
//! Real numbers are `softfloat::F32` bit patterns. Every real operation uses the bit-exact helpers of
//! the `softfloat` crate (docs/spec/softfloat.md). The helpers that read the game's lookup tables
//! (`Sin`, `Cos`, `Tan`, `Asin`, `Acos`, `Atan`, `Atan2`, `SquareRoot`, `Pow`) need the
//! `softfloat::Tables` of the VM. Without them these natives stop the thread with
//! `RunStatus::MissingTables`; they never fall back to IEEE math.

use softfloat::{arith, conv, math, Tables, F32};

use crate::vm::{NativeCtx, NativeRegistry, NativeResult, RunStatus, Value};

/// pi/180 as stored by Game.dll (`0x6FAAE5E8`), the factor used by `Deg2Rad` (spec softfloat 6).
const DEG2RAD: F32 = F32::from_bits(0x3C8E_FA35);
/// 180/pi as stored by Game.dll (`0x6FAAE60C`), the factor used by `Rad2Deg` (spec softfloat 6).
const RAD2DEG: F32 = F32::from_bits(0x4265_2EE1);

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

    // Deg2Rad (0x6f3b29d0). The argument times pi/180 with the soft-float multiply (CONFIRMED
    // helper and constant, spec softfloat 6). The wrapper's own body is not in the decompile export,
    // so the operand order is inferred from Rad2Deg. UNSURE.
    reg.register("Deg2Rad", |_, a| {
        ret(Value::Real(arith::mul(real_arg(a, 0), DEG2RAD)))
    });
    // Rad2Deg (0x6f3b29f0). The argument times 180/pi with the soft-float multiply (the same helper
    // as Deg2Rad; the constant is at DAT_6faae60c). UNSURE for the wrapper, as above.
    reg.register("Rad2Deg", |_, a| {
        ret(Value::Real(arith::mul(real_arg(a, 0), RAD2DEG)))
    });

    // --- trigonometry and powers ----------------------------------------------------------------

    // Sin (0x6f3b2a10), Cos (0x6f3b2a30), Tan (0x6f3b2a50). The wrappers call the soft-float helpers
    // SIN / COS / TAN (0x6F6EF820 / 0x6F6EF8D0 / 0x6F6F0140, spec softfloat 4.2), which read the sine
    // table. CONFIRMED that these helpers are used; the table approximation is not correctly rounded.
    // Argument checks in the wrapper are not examined. UNSURE for the wrapper.
    reg.register("Sin", |ctx, a| {
        with_tables(ctx, |t| math::sin(real_arg(a, 0), t))
    });
    reg.register("Cos", |ctx, a| {
        with_tables(ctx, |t| math::cos(real_arg(a, 0), t))
    });
    reg.register("Tan", |ctx, a| {
        with_tables(ctx, |t| math::tan(real_arg(a, 0), t))
    });

    // Asin (0x6f3b2a70), Acos (0x6f3b2ab0), Atan (0x6f3b2af0). The soft-float ASIN / ACOS / ATAN
    // helpers (spec softfloat 4.3, 4.4) with the arc-sine tables. The wrappers' argument range
    // checks are not ported (spec softfloat, open items): for |x| > 1 the helper result is returned
    // here, and the original's wrapper may return something else. UNSURE for the wrapper.
    reg.register("Asin", |ctx, a| {
        with_tables(ctx, |t| math::asin(real_arg(a, 0), t))
    });
    reg.register("Acos", |ctx, a| {
        with_tables(ctx, |t| math::acos(real_arg(a, 0), t))
    });
    reg.register("Atan", |ctx, a| {
        with_tables(ctx, |t| math::atan(real_arg(a, 0), t))
    });

    // Atan2 (0x6f3b2b10). The soft-float ATAN2 helper with y first (spec softfloat 4.4, standard
    // argument order). The wrapper body is not in the decompile export. UNSURE for the wrapper.
    reg.register("Atan2", |ctx, a| {
        with_tables(ctx, |t| math::atan2(real_arg(a, 0), real_arg(a, 1), t))
    });

    // SquareRoot (0x6f3b2ba0). The soft-float SQRT helper (0x6F6EF750, spec softfloat 4.1). It does
    // not read the tables, but the helper's signature takes them, so the native requires them too.
    // The wrapper body is not in the decompile export. UNSURE for the wrapper.
    reg.register("SquareRoot", |ctx, a| {
        with_tables(ctx, |t| math::sqrt(real_arg(a, 0), t))
    });

    // Pow (0x6f3b2c00). The soft-float POW helper (0x6F6F0810, spec softfloat 4.7): an integer
    // exponent goes through POW_INT, otherwise EXP(e * LN(base)). The wrapper's special cases are not
    // ported (spec softfloat, open items). UNSURE for the wrapper.
    reg.register("Pow", |ctx, a| {
        with_tables(ctx, |t| math::pow(real_arg(a, 0), real_arg(a, 1), t))
    });

    // --- integer / real conversion --------------------------------------------------------------

    // I2R (0x6f3b2cc0). CONFIRMED: the wrapper calls the soft-float integer-to-real helper
    // (0x6F6EF350, spec softfloat 3.5). It truncates the magnitude to 24 significant bits
    // (I2R(16777219) = 16777218). The wrapper body is a plain call.
    reg.register("I2R", |_, a| ret(Value::Real(conv::i2r(int_arg(a, 0)))));

    // R2I (0x6f3b2ce0). CONFIRMED: the wrapper's whole body is a call to the float-to-int helper
    // (0x6F6EEC70, `softfloat::conv::r2i`). It truncates toward zero, gives 0 for |x| < 1, and
    // saturates to INT_MAX/INT_MIN by sign for |x| >= 2^31, NaN included. Checked against the
    // original in Unicorn (21,626 cases, spec softfloat 3.5).
    reg.register("R2I", |_, a| ret(Value::Integer(conv::r2i(real_arg(a, 0)))));

    // S2I (0x6f3b2d10). CONFIRMED: the wrapper returns 0 for a null or empty string, otherwise the
    // C library `atoi` of the text (import MSVCR80 `atoi`). So leading white space and a sign are
    // accepted and parsing stops at the first non-digit ("  12x" gives 12, "1.9" gives 1). The
    // overflow result is UNSURE (see `atoi`). The text is not localised, so no TRIGSTR lookup.
    reg.register("S2I", |ctx, a| {
        let s = str_arg(ctx, a, 0);
        ret(Value::Integer(atoi(&s)))
    });

    // S2R (0x6f3b2d50). Behaviour: real value of a string, digits with one dot, read by the
    // original's parser FUN_6f6f0520 (spec softfloat 10: S2R is NOT ported). This native uses Rust's
    // decimal parser for the digits, rounded to f32 by the host. So the value is only as exact as
    // that parse, not the soft-float text parser. UNSURE.
    reg.register("S2R", |ctx, a| {
        let s = str_arg(ctx, a, 0);
        ret(Value::Real(F32::from_bits(parse_real(&s).to_bits())))
    });

    // I2S (0x6f3baaa0). CONFIRMED: the format string at 0x6f915d68 is "%d", written by the Storm
    // ordinal 578 (printf-style, 256-byte buffer, which cannot overflow for an i32). So: signed
    // decimal text, "-" for negative values, no padding.
    reg.register("I2S", |ctx, a| {
        let s = int_arg(a, 0).to_string();
        ret(Value::String(ctx.intern(&s)))
    });

    // R2S (0x6f3baaf0). Behaviour: text of the real with two decimals ("%.2f"-style, rounded). The
    // text is formatted by Rust, not by the original's R2S (0x6F6F0360, spec softfloat 10: not
    // ported). UNSURE.
    reg.register("R2S", |ctx, a| {
        let s = c_fmt_f(real_arg(a, 0).to_f32(), 0, 2);
        ret(Value::String(ctx.intern(&s)))
    });

    // R2SW (0x6f3bab40). Behaviour: like R2S with a width and a precision argument, "%*.*f"-style:
    // a negative width means left-justified with the absolute width, a negative precision means the
    // C default of 6 decimals. Formatted by Rust, as R2S. UNSURE.
    reg.register("R2SW", |ctx, a| {
        let s = c_fmt_f(real_arg(a, 0).to_f32(), int_arg(a, 1), int_arg(a, 2));
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

fn real_arg(a: &[Value], i: usize) -> F32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(F32::ZERO)
}

/// Runs a native that reads the lookup tables. Without tables the thread stops with
/// `MissingTables`: the native never falls back to IEEE math.
fn with_tables<F>(ctx: &NativeCtx<'_>, f: F) -> NativeResult
where
    F: FnOnce(&Tables) -> F32,
{
    match ctx.tables() {
        Some(t) => ret(Value::Real(f(t))),
        None => NativeResult::Fail(RunStatus::MissingTables),
    }
}

fn str_arg(ctx: &NativeCtx<'_>, a: &[Value], i: usize) -> String {
    let id = a.get(i).and_then(|v| v.as_string_id()).unwrap_or(0);
    ctx.string(id).to_string()
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
        let f = |n: i32| conv::i2r(n).to_f32();
        assert_eq!(f(16_777_219), 16_777_218.0);
        assert_eq!(f(-16_777_219), -16_777_218.0);
        assert_eq!(f(i32::MIN), -2_147_483_648.0);
        assert_eq!(f(i32::MAX), 2_147_483_520.0);
        assert_eq!(f(0), 0.0);
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
