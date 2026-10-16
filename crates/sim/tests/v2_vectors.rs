//! V2 verification wave: Rust models against the ORIGINAL Game.dll code run in Unicorn (`tools/emu/v2_*_emu.py`).
//! Vector files live in `tests/data/v2`.

use sim::hero_extra::fan_plan;
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex")
}

fn f(s: &str) -> F32 {
    F32::from_bits(hex(s))
}

#[test]
fn fan_of_knives_plan_matches_the_launcher() {
    let text = include_str!("data/v2/fan_plan.txt");
    let mut rows = 0;
    let (mut pooled, mut limited, mut short) = (0, 0, 0);
    for line in text.lines() {
        let (head, tail) = line.split_once('|').expect("separator");
        let h: Vec<&str> = head.split_whitespace().collect();
        let (a, b, limit, n) = (f(h[0]), f(h[1]), h[2].parse::<i32>().unwrap(), h[3].parse::<usize>().unwrap());
        let want: Vec<u32> = tail.split_whitespace().map(hex).collect();
        let got: Vec<u32> = fan_plan(n, a, b, limit).iter().map(|x| x.to_bits()).collect();
        assert_eq!(got, want, "row {line}");
        rows += 1;
        pooled += (b.to_f32() != 0.0) as u32;
        limited += (limit != 0) as u32;
        short += (want.len() < n) as u32;
    }
    assert!(rows >= 400 && pooled > 100 && limited > 100 && short > 100, "{rows} {pooled} {limited} {short}");
}

fn tables() -> Option<softfloat::Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(softfloat::Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

#[test]
fn spirit_link_split_matches_the_original() {
    use sim::unit_abilities_m::spirit_link_split;
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: spirit link vectors skipped");
        return;
    };
    let text = include_str!("data/v2/spiritlink.txt");
    let (mut rows, mut gated, mut shared_rows) = (0, 0, 0);
    for line in text.lines() {
        let (head, tail) = line.split_once('|').expect("separator");
        let h: Vec<&str> = head.split_whitespace().collect();
        let (amount, frac) = (f(h[0]), f(h[1]));
        let n: usize = h[2].parse().unwrap();
        let gate = h[3] == "1";
        let (rflags, rclass) = (hex(h[4]), hex(h[5]));
        let w: Vec<&str> = tail.split_whitespace().collect();
        let after = hex(w[0]);
        let nshares: usize = w[1].parse().unwrap();
        let active = gate && amount.to_f32() != 0.0 && rflags & 0x10 == 0 && rclass != 0x1000000;
        if !active {
            assert_eq!((after, nshares), (amount.to_bits(), 0), "row {line}");
            gated += 1;
            rows += 1;
            continue;
        }
        let (own, per) = spirit_link_split(amount, frac, n, |a, b| softfloat::arith::div(a, b, &t));
        assert_eq!(own.to_bits(), after, "own, row {line}");
        match per {
            Some(p) if n > 0 => {
                assert_eq!(nshares, n, "row {line}");
                for sh in &w[2..] {
                    let p4: Vec<&str> = sh.split(':').collect();
                    assert_eq!(hex(p4[1]), p.to_bits(), "share, row {line}");
                    assert_eq!(hex(p4[2]), 0x1000000, "class, row {line}");
                    assert_eq!(hex(p4[3]), (rflags & !4) | 0x42, "flags, row {line}");
                }
                shared_rows += 1;
            }
            _ => assert_eq!(nshares, 0, "row {line}"),
        }
        rows += 1;
    }
    assert!(rows >= 400 && gated > 50 && shared_rows > 60, "{rows} {gated} {shared_rows}");
}

#[test]
fn thorns_reflect_matches_the_original() {
    use sim::undead_ne::thorns_reflect;
    let text = include_str!("data/v2/thorns.txt");
    let (mut rows, mut delivered, mut flat, mut floored) = (0, 0, 0, 0);
    for line in text.lines() {
        let (head, tail) = line.split_once('|').expect("separator");
        let h: Vec<&str> = head.split_whitespace().collect();
        let (strength, amount, reduced) = (f(h[0]), f(h[1]), f(h[2]));
        let percent = h[3] == "1";
        let (rflags, rclass) = (hex(h[4]), hex(h[5]));
        let (allied, invuln, blocked, illusion) = (h[6] == "1", h[7] == "1", h[8] == "1", h[9] == "1");
        let w: Vec<&str> = tail.split_whitespace().collect();
        let gates = rflags & 1 == 0 && rclass == 0x10 && !allied && !invuln && !blocked;
        let (got_amt, effect) = if w[0] == "-" { (None, w[1]) } else { (Some(hex(w[0])), w[2]) };
        assert_eq!(effect, if gates { "1" } else { "0" }, "effect, row {line}");
        if gates && !illusion {
            let want = thorns_reflect(strength, amount, reduced, percent);
            assert_eq!(got_amt, Some(want.to_bits()), "amount, row {line}");
            assert_eq!(hex(w[1]), 0x100000, "class, row {line}");
            delivered += 1;
            flat += (!percent) as u32;
            floored += (want.to_f32() == 1.0) as u32;
        } else {
            assert_eq!(got_amt, None, "no record, row {line}");
        }
        rows += 1;
    }
    assert!(rows >= 400 && delivered > 100 && flat > 20 && floored > 20, "{rows} {delivered} {flat} {floored}");
}

#[test]
fn mana_burn_impact_matches_the_original() {
    use sim::hero_extra::mana_burn_result;
    let text = include_str!("data/v2/manaburn.txt");
    let (mut rows, mut blocked_rows, mut clipped) = (0, 0, 0);
    for line in text.lines() {
        let (head, tail) = line.split_once('|').expect("separator");
        let h: Vec<&str> = head.split_whitespace().collect();
        let (mana, amount) = (f(h[0]), f(h[1]));
        let (invuln, has_ability, immune) = (h[2] == "1", h[3] == "1", h[4] == "1");
        let w: Vec<&str> = tail.split_whitespace().collect();
        assert_eq!(w.len(), 4, "row {line}");
        assert_eq!(w[2], "4000", "record class, row {line}");
        let blocked = invuln || !has_ability || immune;
        let (rest, dmg) = mana_burn_result(mana, amount, blocked);
        assert_eq!(rest.to_bits(), hex(w[0]), "mana after, row {line}");
        assert_eq!(dmg.to_bits(), hex(w[1]), "damage, row {line}");
        // record flag 0x10 exactly when the amount is zero
        assert_eq!(hex(w[3]), if dmg.to_f32() == 0.0 { 0x10 } else { 0 }, "flags, row {line}");
        rows += 1;
        blocked_rows += blocked as u32;
        clipped += (mana.to_f32() < amount.to_f32() && !blocked) as u32;
    }
    assert!(rows >= 400 && blocked_rows > 80 && clipped > 50, "{rows} {blocked_rows} {clipped}");
}
