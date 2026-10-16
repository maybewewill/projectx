//! Dumps the deterministic generator sequences of a replay's game: for the global generator and each of the 45
//! streams, the byte-hash (`sim::sync::rng_state_hash`) of the state after `n` draws from the start state
//! right after boot. Used to decode which draws the real game made between two 0x22 records
//! (docs/spec/lockstep.md section 18). Output: `G n hash` / `S k n hash` lines.
//!
//! `cargo run -p engine --example rngseq -- --dll GAME.DLL --mpq A --mpq B --replay R.w3g MAP.w3x`

use engine::cli::{inputs, BootArgs};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut args = BootArgs::default();
    let mut it = argv.iter();
    let mut draws_g = 4000usize;
    let mut draws_s = 400usize;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dll" => args.dll = it.next().cloned(),
            "--mpq" => args.mpqs.push(it.next().cloned().unwrap()),
            "--replay" => args.replay = it.next().cloned(),
            "--draws-g" => draws_g = it.next().unwrap().parse().unwrap(),
            "--draws-s" => draws_s = it.next().unwrap().parse().unwrap(),
            p => args.map = Some(p.to_string()),
        }
    }
    let replay_path = args.replay.clone().expect("--replay");
    let bytes = std::fs::read(&replay_path).expect("read replay");
    let inp = inputs(&args).expect("inputs");
    let (engine, setup) = engine::Engine::boot_from_replay(&bytes, inp).expect("boot");
    eprintln!("seed {:#010x} (record {:#010x})", setup.seed, setup.record_seed);
    let table = engine.rng_table().expect("RNG table (needs --dll)").clone();
    let streams = engine.host().rng().clone();
    let mut g = streams.global;
    for n in 0..=draws_g {
        println!("G {n} {:08x}", sim::sync::rng_state_hash(&g));
        g.next_u32(&table);
    }
    for k in 0..sim::rng::STREAM_COUNT {
        let mut s = streams.stream(k);
        for n in 0..=draws_s {
            println!("S {k} {n} {:08x}", sim::sync::rng_state_hash(&s));
            s.next_u32(&table);
        }
    }
}
