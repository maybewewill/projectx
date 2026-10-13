//! Unicorn vectors of the Inferno landing context (`tools/emu/undead_ne4_emu.py`, CHECK=inferno): the level record
//! fields that `0x6F117B40` hands to `CBuffInfernal` and which `inferno_ctx` reads.

use sim::cast::SpellLevel;
use sim::undead_ne::inferno_ctx;
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex")
}

fn f(s: &str) -> F32 {
    F32::from_bits(hex(s))
}

#[test]
fn inferno_context_roles_match_the_original() {
    let text = include_str!("data/undead_ne/inferno_ctx.txt");
    let mut n = 0;
    for line in text.lines() {
        let p: Vec<&str> = line.split(" | ").collect();
        let rec: Vec<&str> = p[1].split_whitespace().collect();
        let ctx: Vec<&str> = p[2].split_whitespace().collect();
        // rec = targs area dataA dataB dataC unitid buff0 dur herodur
        let mut data = [F32::ZERO; 9];
        data[0] = f(rec[2]);
        data[1] = f(rec[3]);
        data[2] = f(rec[4]);
        let level = SpellLevel {
            cast: F32::ZERO,
            dur: f(rec[7]),
            hero_dur: f(rec[8]),
            cost: 0,
            cool: F32::ZERO,
            area: f(rec[1]),
            rng: F32::ZERO,
            data,
            unit_id: hex(rec[5]),
        };
        let c = inferno_ctx(&level);
        // the original's context words: [targs, area, dataA, dataB, dataC, unitid, buff0, dur, herodur, consts...]
        assert_eq!(c.area.to_bits(), hex(ctx[1]), "{line}");
        assert_eq!(c.damage.to_bits(), hex(ctx[2]), "{line}");
        assert_eq!(c.life.to_bits(), hex(ctx[3]), "{line}");
        assert_eq!(c.delay.to_bits(), hex(ctx[4]), "{line}");
        assert_eq!(c.unit_id, hex(ctx[5]), "{line}");
        assert_eq!(c.dur.to_bits(), hex(ctx[7]), "{line}");
        assert_eq!(c.hero_dur.to_bits(), hex(ctx[8]), "{line}");
        n += 1;
    }
    assert!(n >= 300);
}
