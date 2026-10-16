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
