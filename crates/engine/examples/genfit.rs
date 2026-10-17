//! Fits the recorded hidden-word changes of a `--sim-sync --sync-trace` dump to windows of the fixed-seed global generator
//! (docs/spec/lockstep.md section 21). For each slot with a real change, prints every `(a, b)` (b - a <= SPAN, a <= MAXA) for which
//! `rol3(h[a] ^ h[b])` equals the change exactly (`exact`), or differs from it only in the low `LOW` bits (`near`).
//!
//! `cargo run -p engine --example genfit -- GAME.DLL TRACE.txt [slot_lo slot_hi] [LOW] [SPAN] [MAXA] [seed_hex]`

use sim::rng::{Rng, RngTable, FIXED_GAME_SEED};
use sim::sync::rng_state_hash;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dll = std::fs::read(&a[0]).expect("dll");
    let table = RngTable::from_game_dll(&dll).expect("table");
    let trace = std::fs::read_to_string(&a[1]).expect("trace");
    let lo: u32 = a.get(2).map_or(0, |s| s.parse().unwrap());
    let hi: u32 = a.get(3).map_or(100, |s| s.parse().unwrap());
    let low: u32 = a.get(4).map_or(0, |s| s.parse().unwrap());
    let span: usize = a.get(5).map_or(40, |s| s.parse().unwrap());
    let maxa: usize = a.get(6).map_or(120, |s| s.parse().unwrap());
    let seed = a
        .get(7)
        .map_or(FIXED_GAME_SEED, |s| u32::from_str_radix(s, 16).unwrap());
    let mut rng = Rng::seeded(seed);
    let mut h = Vec::new();
    for _ in 0..(maxa + span + 2) {
        h.push(rng_state_hash(&rng));
        rng.next_u32(&table);
    }
    if std::env::var_os("GENFIT_STAT").is_some() {
        // How often does a single draw leave no trace above bit 21 (so that it hides under the tag bits)?
        let n = h.len() - 1;
        let low = (0..n).filter(|&i| (h[i] ^ h[i + 1]).rotate_left(3) >> 22 == 0).count();
        println!("single draws with no bit above 21: {low} of {n}");
        for k in [2usize, 3, 4] {
            let m = (0..n - k).filter(|&i| (h[i] ^ h[i + k]).rotate_left(3) >> 22 == 0).count();
            println!("{k} draws with no bit above 21: {m} of {}", n - k);
        }
    }
    let mask: u32 = if low == 0 { 0 } else { (1u64 << low).wrapping_sub(1) as u32 };
    for line in trace.lines() {
        let Some(rest) = line.trim_start().strip_prefix("slot") else { continue };
        let slot: u32 = rest.split_whitespace().next().unwrap().parse().unwrap();
        if slot < lo || slot > hi {
            continue;
        }
        let Some(i) = line.find("real change ") else { continue };
        let ch = u32::from_str_radix(&line[i + 12..i + 20], 16).unwrap();
        if ch == 0 {
            continue;
        }
        let mut out = String::new();
        for x in 0..=maxa {
            for k in 1..=span {
                let g = (h[x] ^ h[x + k]).rotate_left(3);
                if g == ch {
                    out += &format!(" exact({x}->{})", x + k);
                } else if low > 0 && (g ^ ch) & !mask == 0 {
                    out += &format!(" near({x}->{} res {:x})", x + k, g ^ ch);
                }
            }
        }
        println!("slot {slot} change {ch:08x}:{out}");
    }
}
