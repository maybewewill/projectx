//! Construction: the registration transition against the original (`0x6F29F850`), and the payment and refund
//! rules (`0x6F0EDA70`, `0x6F0F7750`) through the public API. The transition vectors are written by
//! `tools/emu/construction_counts_emu.py` from Game.dll 1.26a (Unicorn); the file needs no DLL.

use sim::construction::{
    place_check, transition, ConstructionTimers, Charge, Effect, Food, PendingCharge, PlaceCheck,
};
use sim::economy::PlayerEconomy;

const VECTORS: &str = include_str!("vectors/construction_counts_seed7.tsv");

fn effect_text(e: &Effect) -> String {
    match *e {
        Effect::RemoveInProgress { type_id } => format!("remove_inprog,{},0", type_id),
        Effect::RemoveCompleted { type_id } => format!("remove_completed,{}", type_id),
        Effect::AddCompleted { type_id, flag } => {
            format!("add_completed,{},{}", type_id, if flag { 32 } else { 0 })
        }
        Effect::BeginInProgress { type_id, flag } => {
            format!("begin_inprog,{},{}", type_id, if flag { 32 } else { 0 })
        }
        Effect::FoodMade(n) => format!("food_made,{}", n),
        Effect::FoodUsed(n) => format!("food_used,{}", n),
    }
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn transition_matches_the_original_vectors() {
    let mut cases = 0;
    for line in VECTORS.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 8, "bad vector line: {line}");
        let reg = hex(f[0]);
        let flags = hex(f[1]);
        let old: u32 = f[2].parse().unwrap();
        let new: u32 = f[3].parse().unwrap();
        let food = Food {
            made: f[4].parse().unwrap(),
            used: f[5].parse().unwrap(),
        };
        let reg_out = hex(f[6]);
        let want: Vec<&str> = if f[7].is_empty() {
            Vec::new()
        } else {
            f[7].split(';').collect()
        };
        let (got_reg, effects) = transition(reg, flags, old, new, food);
        let got: Vec<String> = effects.iter().map(effect_text).collect();
        assert_eq!(got_reg, reg_out, "registration word for {line}");
        assert_eq!(got, want, "effects for {line}");
        cases += 1;
    }
    assert_eq!(cases, 1000);
}

fn players() -> Vec<PlayerEconomy> {
    vec![PlayerEconomy::new(0), PlayerEconomy::new(1)]
}

#[test]
fn a_build_is_paid_at_placement_and_refunded_in_full_before_placement() {
    let mut p = players();
    p[0].stock = [800 * 10, 200 * 10];
    let cost = Charge { gold: 150, lumber: 100 };
    let mut pending = PendingCharge::default();
    match place_check(&mut p, 0, cost) {
        PlaceCheck::Paid(c) => pending.record(c),
        PlaceCheck::Insufficient => panic!("should be covered"),
    }
    assert_eq!(p[0].stock, [650 * 10, 100 * 10]);
    assert!(pending.recorded);
    let back = pending.refund(&mut p, 0);
    assert_eq!(back, cost);
    assert_eq!(p[0].stock, [800 * 10, 200 * 10]);
}

#[test]
fn an_uncovered_build_takes_nothing() {
    let mut p = players();
    p[1].stock = [100 * 10, 0];
    assert_eq!(
        place_check(&mut p, 1, Charge { gold: 150, lumber: 0 }),
        PlaceCheck::Insufficient
    );
    assert_eq!(p[1].stock, [100 * 10, 0]);
}

#[test]
fn construction_run_completes_on_its_timer() {
    let mut t = ConstructionTimers::new();
    let site = t.start(0x6831_6670, 40);
    assert_eq!(t.advance(39.0), Vec::<u32>::new());
    assert_eq!(t.advance(40.0), vec![site]);
    assert!(t.site(site).is_none());
}
