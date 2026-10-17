//! Walks the recorded hidden-word changes of a `--sim-sync --sync-trace` dump and estimates the real number of global-generator draws
//! per record (docs/spec/lockstep.md section 22). For every record with a nonzero real change, the smallest k (0..=KMAX) is chosen for which
//! `change ^ rol3(h[n] ^ h[n+k])` has no bit above bit 21 (the tag, rand and game words only reach the low bits or bit-20..23 patterns);
//! records where no k fits are reported as `?` and leave n unchanged.
//!
//! `cargo run -p engine --example drawwalk -- GAME.DLL TRACE.txt [KMAX] [seed_hex]`  Output: `slot real_n sim_n flag` per record.

use sim::rng::{Rng, RngTable, FIXED_GAME_SEED};
use sim::sync::rng_state_hash;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dll = std::fs::read(&a[0]).expect("dll");
    let table = RngTable::from_game_dll(&dll).expect("table");
    let trace = std::fs::read_to_string(&a[1]).expect("trace");
    let kmax: usize = a.get(2).map_or(12, |s| s.parse().unwrap());
    let seed = a.get(3).map_or(FIXED_GAME_SEED, |s| u32::from_str_radix(s, 16).unwrap());
    let mut rng = Rng::seeded(seed);
    let mut h = Vec::new();
    for _ in 0..3000 {
        h.push(rng_state_hash(&rng));
        rng.next_u32(&table);
    }
    // The first anchor: record 32 is draw 28 -> 29 (lockstep.md 21.1); start the walk at 28.
    let mut n: usize = 28;
    let mut first = true;
    for line in trace.lines() {
        let Some(rest) = line.trim_start().strip_prefix("slot") else { continue };
        let slot: u32 = rest.split_whitespace().next().unwrap().parse().unwrap();
        let Some(i) = line.find("real change ") else { continue };
        let ch = u32::from_str_radix(&line[i + 12..i + 20], 16).unwrap();
        let sim_n: i64 = line
            .find(" n ")
            .and_then(|p| line[p + 3..].split_whitespace().next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(-1);
        if slot < 32 && first {
            println!("{slot} {} {sim_n} start", 28);
            continue;
        }
        first = false;
        let mut flag = ".";
        if ch != 0 {
            let mut found = None;
            for k in 0..=kmax {
                let g = (h[n] ^ h[n + k]).rotate_left(3);
                if (ch ^ g) >> 22 == 0 {
                    found = Some(k);
                    break;
                }
            }
            match found {
                Some(k) => {
                    if k > 0 {
                        flag = "d";
                    }
                    n += k;
                }
                None => flag = "?",
            }
        }
        println!("{slot} {n} {sim_n} {flag}");
    }
}
