//! Purge's decaying slow against the original (`tools/emu/purge_steps_emu.py`: the real `CBuffPurge` apply `0x6F0EF400`
//! and update handlers `0x6F0D9EE0` / `0x6F0D9F80` run in Unicorn, the sinks and the timer start are recording stubs;
//! 600 random cases over three seeds, twelve updates of each schedule per case). Needs `GAME_DLL` for the soft-float
//! reciprocal table and returns early without it. Numbers only; no game data.

use sim::unit_spells::{purge_attack_update, purge_move_update, purge_schedule};
use softfloat::{arith, Tables, F32};

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn hex_list(s: &str) -> Vec<u32> {
    s.split_whitespace().map(|x| u32::from_str_radix(x, 16).unwrap()).collect()
}

#[test]
fn purge_schedule_matches_the_original_on_600_vectors() {
    let Some(t) = tables() else {
        return;
    };
    let div = |a: F32, b: F32| arith::div(a, b, &t);
    let text = include_str!("vectors/purge_steps.txt");
    let mut n = 0;
    let mut stopped_early = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let parts: Vec<&str> = line.split('|').collect();
        assert_eq!(parts.len(), 4, "{line}");
        let head = hex_list(parts[0]);
        let (a, b, dur) = (F32::from_bits(head[0]), F32::from_bits(head[1]), F32::from_bits(head[2]));
        let want_timers = hex_list(parts[1]);
        let want_move = hex_list(parts[2]);
        let want_attack = hex_list(parts[3]);
        let (mv, mstep, mtime) = purge_schedule(a, dur, &div);
        let (av, astep, atime) = purge_schedule(b, dur, &div);
        let timers: Vec<u32> = [mtime, atime].into_iter().flatten().map(|x| x.to_bits()).collect();
        assert_eq!(timers, want_timers, "timers of row {n}: {line}");
        // movement: the initial sink is the negated magnitude, then one sink value per update until the slow is ~0
        let mut got_move = vec![arith::neg(mv).to_bits()];
        let mut v = mv;
        for _ in 0..12 {
            match purge_move_update(v, mstep) {
                Some((new, sink)) => {
                    got_move.push(sink.to_bits());
                    v = new;
                }
                None => break,
            }
        }
        assert_eq!(got_move, want_move, "movement sinks of row {n}: {line}");
        let mut got_attack = vec![arith::neg(av).to_bits()];
        let mut v = av;
        for _ in 0..12 {
            match purge_attack_update(v, astep) {
                Some((new, first, second)) => {
                    got_attack.push(first.to_bits());
                    got_attack.push(second.to_bits());
                    v = new;
                }
                None => break,
            }
        }
        assert_eq!(got_attack, want_attack, "attack sinks of row {n}: {line}");
        n += 1;
        stopped_early += usize::from(got_move.len() < 13);
    }
    assert_eq!(n, 600);
    assert!(stopped_early > 200, "the decay must reach zero in many rows, {stopped_early}");
}
