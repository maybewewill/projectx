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
