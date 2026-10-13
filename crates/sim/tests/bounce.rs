//! The bounce target search against the original (`tools/emu/splash_emu.py`, kind `pick`: the real callback
//! `0x6F2BBAF0` over four candidates): 500 Unicorn vectors.

#[path = "splash/splash_pick.rs"]
mod pick_rows;

use sim::bounce::*;
use softfloat::F32;

#[test]
fn pick_nearest_matches() {
    for r in pick_rows::ROWS {
        // n, (accepted, dist) x4 (padded), index, best distance
        let n = r[0] as usize;
        let cands: Vec<(bool, F32)> = (0..n).map(|i| (r[1 + 2 * i] != 0, F32::from_bits(r[2 + 2 * i]))).collect();
        let (which, best) = pick_nearest(&cands);
        assert_eq!((which.map_or(u32::MAX, |i| i as u32), best.to_bits()), (r[9], r[10]), "{r:x?}");
    }
}

#[test]
fn after_hit_rules() {
    let f = |x: f32| F32::from_bits(x.to_bits());
    let mut n = 3;
    // the soft multiply truncates: 100 * (1 - 0.4) is not exactly 60.0
    let want = softfloat::arith::mul(f(100.0), softfloat::arith::sub(F32::ONE, f(0.4)));
    assert_eq!(after_hit(&mut n, f(100.0), f(0.4)), Some(want));
    assert_eq!(n, 2);
    let mut n = 1;
    assert_eq!(after_hit(&mut n, f(100.0), f(0.4)), None);
    let mut n = 5;
    assert_eq!(after_hit(&mut n, f(100.0), f(0.0)), Some(f(100.0)));
    assert_eq!(launch_factors(f(0.0), f(-0.0)), (f(0.5), f(0.25)));
    assert_eq!(launch_factors(f(0.3), f(0.1)), (f(0.3), f(0.1)));
    assert_eq!(bounce_targs(Some(0x2 | 0x1000 | 0x4000)), 0x1_0002);
    assert_eq!(bounce_targs(None), 0x1_0001);
}
