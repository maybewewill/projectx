//! The leading-part reader against the original's reader (Unicorn, `tools/emu/replay_leading_emu.py`).
//!
//! `leading_vectors.rs` holds 24 synthetic bodies with what the original produced for each.

mod leading_vectors;

use w3g::leading::read_leading;

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn text(s: &str) -> Vec<u8> {
    unhex(s)
}

#[test]
fn matches_the_original_on_every_vector() {
    assert!(leading_vectors::CASES.len() >= 20);
    let mut ok_cases = 0;
    for (i, c) in leading_vectors::CASES.iter().enumerate() {
        let body = unhex(c.body);
        let got = read_leading(&body, c.build);
        if c.error {
            assert!(got.is_err(), "case {i}: the original stops, the port does not");
            continue;
        }
        let l = got.unwrap_or_else(|e| panic!("case {i}: port stops ({e}), the original does not"));
        ok_cases += 1;
        assert_eq!(l.name_a, text(c.name_a), "case {i} name_a");
        if let Some(b) = c.name_b {
            assert_eq!(l.name_b.as_deref(), Some(&text(b)[..]), "case {i} name_b");
        } else {
            assert!(l.name_b.is_none(), "case {i} name_b present");
        }
        assert_eq!(l.c32, text(c.c32), "case {i} c32");
        assert_eq!(l.c16, text(c.c16), "case {i} c16");
        assert_eq!(l.stat, text(c.stat), "case {i} stat");
        assert_eq!(l.u32_b0, c.u32_b0, "case {i} u32 +0xB0");
        assert_eq!(l.u32_b4, c.u32_b4, "case {i} u32 +0xB4");
        assert_eq!(Some(l.blob.as_slice()), c.blob.map(text).as_deref(), "case {i} blob");
        assert_eq!(l.tail20.map(|t| t.to_vec()), c.tail20.map(text), "case {i} tail20");
        assert_eq!(Some(l.name_tail.as_slice()), c.name_tail.map(text).as_deref(), "case {i} tail");
    }
    assert!(ok_cases >= 10, "too few successful vectors: {ok_cases}");
}

/// The body that `crates/w3g/tests/synthetic.rs` writes (the `GameInfo` layout: u32, host record,
/// game name, settings, slot count, then the 0x19 block). The original's reader does not accept it
/// as a 1.26a body with build 6059 (it stops in the start blob, `pos = limit + 1`). This is the
/// disagreement recorded in docs/spec/replay-load.md, section 3.
#[test]
fn the_gameinfo_layout_is_refused_by_the_original_reader() {
    let body = unhex(
        "100100000001486f7374000100546573742047616d6500000901810101017501a9\
         5501efbfaddf4d8b6171735d47736f857b656f5569736fc56f655d29332945bb63696f49736d651b732f7733790149076f73750101000c0000000100000000000000160247756573740001000000000019190002016402000000020164026402000101040164785634120002",
    );
    assert!(read_leading(&body, 6059).is_err());
}
