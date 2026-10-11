//! The unit-cell real rule: C `atof` (strtod prefix, C locale) rounded to f64 and then to f32
//! (`gamedata::real::atof_f32`, LIKELY for the Storm ordinal 574 import; Storm.dll is not available to check).
//! Synthetic inputs only.

use gamedata::real::{atof, atof_f32, atof_prefix};
use gamedata::unittype::cell_real;

#[test]
fn prefix_takes_the_longest_number_after_leading_space() {
    assert_eq!(atof_prefix("1.5x"), Some("1.5"));
    assert_eq!(atof_prefix(" -2e3"), Some("-2e3"));
    assert_eq!(atof_prefix("\t\n\x0b\x0c\r +7"), Some("+7"));
    assert_eq!(atof_prefix(".5"), Some(".5"));
    assert_eq!(atof_prefix("5."), Some("5."));
    assert_eq!(atof_prefix("-.5e-3 tail"), Some("-.5e-3"));
    assert_eq!(atof_prefix("abc"), None);
    assert_eq!(atof_prefix(""), None);
    assert_eq!(atof_prefix("-"), None);
    assert_eq!(atof_prefix("."), None);
    assert_eq!(atof_prefix("+ 1"), None, "no space between sign and digits");
    assert_eq!(atof_prefix("  "), None);
}

#[test]
fn an_exponent_without_digits_is_not_part_of_the_number() {
    assert_eq!(atof_prefix("1e"), Some("1"));
    assert_eq!(atof_prefix("1e+"), Some("1"));
    assert_eq!(atof_prefix("1e-x"), Some("1"));
    assert_eq!(atof_prefix("2E3"), Some("2E3"));
    assert_eq!(atof_prefix("2e+3x"), Some("2e+3"));
    assert_eq!(atof("1e"), Some(1.0));
}

#[test]
fn hexadecimal_inf_and_nan_are_not_accepted() {
    // Not part of the model (UNSURE for the Storm import): the prefix is the leading decimal part only.
    assert_eq!(atof_prefix("0x1p3"), Some("0"));
    assert_eq!(atof_f32("0x10"), Some(0.0));
    assert_eq!(atof_prefix("inf"), None);
    assert_eq!(atof_prefix("nan"), None);
}

#[test]
fn values_are_correctly_rounded_to_f64_then_to_f32() {
    assert_eq!(atof("0.1"), Some(0.1f64));
    assert_eq!(atof_f32("0.1").map(f32::to_bits), Some(0x3DCC_CCCD));
    assert_eq!(atof_f32("1.35").map(f32::to_bits), Some(1.35f32.to_bits()));
    assert_eq!(atof_f32("-2e3"), Some(-2000.0));
    assert_eq!(atof_f32(" -2e3"), Some(-2000.0));
    assert_eq!(atof_f32(".5"), Some(0.5));
    assert_eq!(atof_f32("5."), Some(5.0));
    assert_eq!(atof_f32("1.5x"), Some(1.5));
    // 2^24 + 1 is exactly halfway between 2^24 and 2^24 + 2 in f32: ties go to the even mantissa (2^24).
    assert_eq!(atof_f32("16777217"), Some(16_777_216.0));
    // Above the midpoint rounds up.
    assert_eq!(atof_f32("16777217.5"), Some(16_777_218.0));
}

#[test]
fn overflow_gives_infinity_and_underflow_gives_zero_or_subnormal() {
    // strtod returns HUGE_VAL (infinity) for a prefix out of range. CHOICE, UNSURE for the Storm import.
    assert_eq!(atof_f32("1e400"), Some(f32::INFINITY));
    assert_eq!(atof_f32("-1e400"), Some(f32::NEG_INFINITY));
    assert_eq!(atof_f32("3.5e38"), Some(f32::INFINITY), "above f32::MAX rounds to inf");
    assert_eq!(atof_f32("1e-400"), Some(0.0));
    let sub = atof_f32("1e-40").expect("number");
    assert!(sub > 0.0 && sub < f32::MIN_POSITIVE, "subnormal f32");
}

#[test]
fn huge_digit_strings_are_handled() {
    let long = "1".repeat(400);
    assert_eq!(atof_f32(&long), Some(f32::INFINITY));
    let exp = format!("1e{}", "9".repeat(30));
    assert_eq!(atof_f32(&exp), Some(f32::INFINITY));
    assert_eq!(atof_f32("1e-99999999999999999999999"), Some(0.0));
}

#[test]
fn text_without_a_number_is_none_and_the_cell_rule_gives_zero() {
    assert_eq!(atof_f32("x"), None);
    assert_eq!(cell_real("x"), 0.0);
    assert_eq!(cell_real(""), 0.0);
    assert_eq!(cell_real("-0"), 0.0);
    assert_eq!(cell_real("-0").to_bits(), (-0.0f32).to_bits(), "sign of zero kept");
}
