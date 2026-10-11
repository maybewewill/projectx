//! The agile clock and its 300 s restart against vectors produced by the original routines
//! (`tools/emu/agile_clock_emu.py`, docs/spec/lockstep.md section 14).
//!
//! Each scenario starts timers (`S`) and adds the tick increment (or an override `I`) to the clock over
//! 90,000 to 150,000 ticks, crossing the 300 s horizon several times. The vectors hold the clock and row at
//! checkpoints (`C`), `TimerGetRemaining`/`Elapsed` results of live timers (`Q`), the first fires (`FL`), and a hash of
//! all fires (`F`): timer, expiry and queue time at the callback. No Game.dll bytes are in the file.

use std::collections::HashMap;

use sim::lockstep::TICK_INCREMENT_BITS;
use sim::timer::{clamp_timeout, Advance, AgileClock, TimerId, TimerQueue, HORIZON_BITS};
use softfloat::{arith, F32};

const VECTORS: &str = include_str!("agile_clock_vectors.txt");

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex")
}

fn fnv(fires: &[(u32, u32, u32)]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &(idx, exp, clk) in fires {
        for w in [idx, exp, clk] {
            for b in w.to_le_bytes() {
                h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
    }
    h
}

fn pump(q: &mut TimerQueue<u32>, target: f32, fires: &mut Vec<(u32, u32, u32)>) {
    while let Some(ev) = q.pop_due(target) {
        fires.push((ev.payload, ev.expiry.to_bits(), ev.expiry.to_bits()));
    }
}

#[derive(Default)]
struct Scenario {
    seed: u32,
    ticks: u64,
    starts: HashMap<u64, Vec<(u32, u32, bool)>>,
    incs: HashMap<u64, u32>,
    checks: HashMap<u64, (u32, u32, usize)>,
    queries: HashMap<u64, Vec<(u32, u32, u32)>>,
    first_fires: Vec<(u32, u32, u32)>,
    fire_count: usize,
    fire_hash: u64,
    rollovers: u32,
}

fn parse() -> Vec<Scenario> {
    let mut out = Vec::new();
    let mut cur: Option<Scenario> = None;
    for line in VECTORS.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[0] {
            "SCENARIO" => {
                cur = Some(Scenario {
                    seed: f[1].parse().unwrap(),
                    ticks: f[2].parse().unwrap(),
                    ..Default::default()
                })
            }
            "S" => cur.as_mut().unwrap().starts.entry(f[1].parse().unwrap()).or_default().push((
                f[2].parse().unwrap(),
                hex(f[3]),
                f[4] == "1",
            )),
            "I" => {
                cur.as_mut().unwrap().incs.insert(f[1].parse().unwrap(), hex(f[2]));
            }
            "C" => {
                cur.as_mut().unwrap().checks.insert(f[1].parse().unwrap(), (hex(f[2]), f[3].parse().unwrap(), f[4].parse().unwrap()));
            }
            "Q" => cur.as_mut().unwrap().queries.entry(f[1].parse().unwrap()).or_default().push((f[2].parse().unwrap(), hex(f[3]), hex(f[4]))),
            "FL" => cur.as_mut().unwrap().first_fires.push((f[1].parse().unwrap(), hex(f[2]), hex(f[3]))),
            "F" => {
                let s = cur.as_mut().unwrap();
                s.fire_count = f[1].parse().unwrap();
                s.fire_hash = u64::from_str_radix(f[2], 16).unwrap();
            }
            "ROLLOVERS" => cur.as_mut().unwrap().rollovers = f[1].parse().unwrap(),
            "END" => out.push(cur.take().unwrap()),
            other => panic!("unknown record {other}"),
        }
    }
    out
}

#[test]
fn clock_restart_matches_the_original_routines() {
    let scenarios = parse();
    assert_eq!(scenarios.len(), 3);
    for sc in &scenarios {
        let mut q: TimerQueue<u32> = TimerQueue::new();
        let mut clock = AgileClock::new();
        let mut ids: HashMap<u32, (TimerId, f32)> = HashMap::new();
        let mut fires = Vec::new();
        let mut rolled = 0;
        for t in 0..sc.ticks {
            for &(idx, to, rep) in sc.starts.get(&t).into_iter().flatten() {
                let id = q.start(f32::from_bits(to), rep, idx);
                ids.insert(idx, (id, f32::from_bits(to)));
            }
            let inc = sc.incs.get(&t).copied().unwrap_or(TICK_INCREMENT_BITS);
            match clock.plan(inc) {
                Advance::Within(bits) => {
                    clock.commit(Advance::Within(bits));
                    pump(&mut q, f32::from_bits(bits), &mut fires);
                }
                Advance::Rollover { excess } => {
                    rolled += 1;
                    pump(&mut q, f32::from_bits(HORIZON_BITS), &mut fires);
                    q.rebase(f32::from_bits(excess));
                    clock.commit(Advance::Rollover { excess });
                    pump(&mut q, f32::from_bits(excess), &mut fires);
                }
            }
            if let Some(&(bits, row, pending)) = sc.checks.get(&t) {
                assert_eq!(
                    (clock.bits(), clock.row(), q.len()),
                    (bits, row, pending),
                    "seed {} tick {t}",
                    sc.seed
                );
                assert_eq!(q.now().to_bits(), bits, "queue time follows the clock, seed {} tick {t}", sc.seed);
            }
            for &(idx, rem, el) in sc.queries.get(&t).into_iter().flatten() {
                let (id, timeout) = ids[&idx];
                let expiry = q.expiry_of(id).expect("live timer");
                let remaining = arith::sub(F32::from_bits(expiry.to_bits()), F32::from_bits(q.now().to_bits()));
                let elapsed = arith::sub(F32::from_bits(clamp_timeout(timeout).to_bits()), remaining);
                assert_eq!((remaining.to_bits(), elapsed.to_bits()), (rem, el), "seed {} tick {t} timer {idx}", sc.seed);
            }
        }
        assert_eq!(rolled, sc.rollovers, "seed {}", sc.seed);
        assert_eq!(&fires[..sc.first_fires.len()], &sc.first_fires[..], "seed {}", sc.seed);
        assert_eq!((fires.len(), fnv(&fires)), (sc.fire_count, sc.fire_hash), "seed {}", sc.seed);
    }
}

/// The tick-only scenario restarts by itself: 0.005 s adds reach 300 s after about 60,000 ticks, then again.
#[test]
fn plain_ticks_restart_the_clock_twice_in_150000_ticks() {
    let mut clock = AgileClock::new();
    let mut rows = Vec::new();
    for t in 0..150_000u64 {
        clock.advance(TICK_INCREMENT_BITS);
        if rows.last() != Some(&clock.row()) && clock.row() > 0 {
            rows.push(clock.row());
            // The clock restarts from the excess: always below the horizon and small.
            assert!(clock.now() < 1.0, "tick {t} {}", clock.now());
        }
    }
    assert_eq!(rows, vec![1, 2]);
    assert!(clock.absolute() > 700.0 && clock.absolute() < 800.0, "{}", clock.absolute());
}
