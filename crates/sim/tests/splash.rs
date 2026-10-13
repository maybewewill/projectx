//! The msplash selection routines against the original (`tools/emu/splash_emu.py`): 500 Unicorn vectors each for the
//! weapon ring spec, the candidate filter and the ring factor. No `GAME_DLL` needed.

#[path = "splash/splash_filter.rs"]
mod filter_rows;
#[path = "splash/splash_ring.rs"]
mod ring_rows;
#[path = "splash/splash_spec.rs"]
mod spec_rows;

use sim::splash::*;
use softfloat::F32;

#[test]
fn ring_spec_matches() {
    for r in spec_rows::ROWS {
        // has_aux, aux x6, out x6
        let aux = [r[1], r[2], r[3], r[4], r[5], r[6]];
        let got = ring_spec(if r[0] != 0 { Some(aux) } else { None });
        assert_eq!(&got[..], &r[7..13], "{r:x?}");
    }
}

fn bit(mask: u32, a: u32, b: u32) -> bool {
    (mask >> (a.wrapping_mul(7).wrapping_add(b.wrapping_mul(3)) & 15)) & 1 != 0
}

#[test]
fn candidate_filter_matches() {
    let mut accepted = 0;
    for r in filter_rows::ROWS {
        // u5c u20 u24c u33 u248 eth mim inv towner sowner atk w misc src pass shared out
        let c = Candidate {
            dead: r[0] & 0x100 != 0,
            invulnerable_flag: r[1] & 8 != 0,
            targ_type: r[2],
            type_char: r[3] as u8,
            class_word: r[4],
            ethereal: r[5] as i32,
            magic_immune: r[6] as i32,
            invulnerable: r[7] != 0,
            owner: r[8],
        };
        let source = match r[13] {
            0 => Source::Gone,
            1 => Source::Other(r[9]),
            _ => Source::Itself,
        };
        let (mb, pb) = attack_type_blocks(r[12] != 0, r[10]);
        let (pass, shared) = (r[14], r[15]);
        let got = candidate_filter(&c, r[11], source, r[9], mb, pb, |a, b| bit(pass, a, b), |a, b| bit(shared, a, b));
        assert_eq!(u32::from(got), r[16], "{r:x?}");
        accepted += r[16];
    }
    assert!(accepted > 20, "the vectors must exercise acceptance");
}

#[test]
fn ring_factor_matches() {
    for r in ring_rows::ROWS {
        // a b c hf qf ma mb nz count factor tests
        let (a, b, c) = (F32::from_bits(r[0]), F32::from_bits(r[1]), F32::from_bits(r[2]));
        let (hf, qf) = (F32::from_bits(r[3]), F32::from_bits(r[4]));
        let mut tests = 0;
        let got = ring_factor(a, b, c, hf, qf, |radius| {
            tests += 1;
            if radius.to_bits() == r[0] {
                r[5] != 0
            } else {
                assert_eq!(radius.to_bits(), r[1], "{r:x?}");
                r[6] != 0
            }
        });
        let (count, factor) = match got {
            Some(f) => (1, f.to_bits()),
            None => (0, 0xFFFF_FFFF),
        };
        assert_eq!((count, factor, tests), (r[8], r[9], r[10]), "{r:x?}");
    }
}

#[test]
fn query_radius_picks_the_outermost() {
    let f = |x: f32| F32::from_bits(x.to_bits());
    assert_eq!(query_radius(f(50.0), f(100.0), f(150.0)), f(150.0));
    assert_eq!(query_radius(f(50.0), f(100.0), f(0.0)), f(100.0));
    assert_eq!(query_radius(f(50.0), f(0.0), f(0.0)), f(50.0));
}
