//! Unicorn vectors of the S37 numeric cores (`tools/emu/undead_ne4_emu.py`, original Game.dll code run for real).
//! The swath engine of Carrion Swarm / Shockwave needs the soft-float tables (`GAME_DLL=...`).

use sim::undead_ne::{aoe_marker_duration, gt_eps, safe_div, swath_cap_after, swath_countdown, swath_damage_each, swath_launch, swath_radius, swath_rearm, SwathData};
use softfloat::{Tables, F32};

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex")
}

fn f(s: &str) -> F32 {
    F32::from_bits(hex(s))
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

#[test]
fn swarm_launch_geometry_matches_the_original() {
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: swarm launch vectors skipped");
        return;
    };
    let text = include_str!("data/undead_ne/swarm_launch.txt");
    let (mut facing_rows, mut dir_rows) = (0, 0);
    for line in text.lines() {
        let (head, tail) = line.split_once(" | ").expect("separator");
        let h: Vec<&str> = head.split_whitespace().collect();
        let w: Vec<&str> = tail.split_whitespace().collect();
        let (area, dist, cx, cy, tx, ty, facing) = (f(h[0]), f(h[1]), f(h[2]), f(h[3]), f(h[4]), f(h[5]), f(h[6]));
        let got = swath_launch((cx, cy), (tx, ty), facing, area, dist, &t);
        assert_eq!(
            [got.start.0.to_bits(), got.start.1.to_bits(), got.end.0.to_bits(), got.end.1.to_bits()],
            [hex(w[0]), hex(w[1]), hex(w[2]), hex(w[3])],
            "row {line}"
        );
        let (dx, dy) = (tx.to_f32() - cx.to_f32(), ty.to_f32() - cy.to_f32());
        if dx * dx + dy * dy >= 400.0 {
            dir_rows += 1;
        } else {
            facing_rows += 1;
        }
    }
    assert!(dir_rows >= 150 && facing_rows >= 50, "{dir_rows} {facing_rows}");
}

#[test]
fn swarm_tick_matches_the_original() {
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: swarm tick vectors skipped");
        return;
    };
    let text = include_str!("data/undead_ne/swarm_tick.txt");
    let (mut with_units, mut rearmed, mut capped, mut shared) = (0, 0, 0, 0);
    for line in text.lines() {
        let parts: Vec<&str> = line.split(" | ").collect();
        let h: Vec<&str> = parts[0].split_whitespace().collect();
        let lives: Vec<F32> = parts[1].split_whitespace().map(f).collect();
        let afters: Vec<F32> = parts[2].split_whitespace().map(f).collect();
        let b: Vec<&str> = parts[3].split_whitespace().collect();
        let hits: Vec<&str> = parts[4].split_whitespace().collect();
        let r: Vec<&str> = parts[5].split_whitespace().collect();
        let data = SwathData {
            start: (f(h[0]), f(h[1])),
            distance: f(h[2]),
            width0: f(h[3]),
            width1: f(h[4]),
            damage: f(h[5]),
            cap: f(h[6]),
        };
        let pos = (f(h[7]), f(h[8]));
        let n: usize = h[9].parse().unwrap();
        assert_eq!(lives.len(), n);
        // the query circle: radius, centre = the missile position
        assert_eq!(swath_radius(&data, pos, &t).to_bits(), hex(b[0]), "radius {line}");
        assert_eq!((pos.0.to_bits(), pos.1.to_bits()), (hex(b[1]), hex(b[2])), "centre {line}");
        let mut cap = data.cap;
        if n > 0 {
            with_units += 1;
            let each = swarm_each(&data, n, &t);
            assert_eq!(hits.len(), n, "{line}");
            for hh in &hits {
                assert_eq!(each.to_bits(), hex(hh), "damage {line}");
            }
            if each.to_bits() == data.damage.to_bits() {
                capped += 1;
            } else {
                shared += 1;
            }
            for i in 0..n {
                cap = swath_cap_after(cap, lives[i], afters[i]);
            }
        } else {
            assert_eq!(hits, vec!["-"], "{line}");
        }
        assert_eq!(cap.to_bits(), hex(r[0]), "cap {line}");
        assert_eq!(swath_countdown(F32::from_bits(0x4040_0000)).to_bits(), hex(r[1]), "countdown {line}");
        let rearm = swath_rearm(cap);
        assert_eq!(rearm, r[2] == "1", "rearm {line}");
        if rearm {
            rearmed += 1;
        }
    }
    assert!(with_units >= 300 && rearmed >= 100 && capped >= 50 && shared >= 50, "{with_units} {rearmed} {capped} {shared}");
}

fn swarm_each(d: &SwathData, n: usize, t: &Tables) -> F32 {
    swath_damage_each(d, n, t)
}

#[test]
fn robust_compare_and_safe_division_rules() {
    let Some(t) = tables() else {
        return;
    };
    let z = F32::ZERO;
    let one = F32::from_bits(0x3F80_0000);
    // 0 / 0 = 1, x / 0 = +-FLT_MAX, NaN / 0 = +FLT_MAX
    assert_eq!(safe_div(z, z, &t).to_bits(), 0x3F80_0000);
    assert_eq!(safe_div(one, z, &t).to_bits(), 0x7F7F_FFFF);
    assert_eq!(safe_div(F32::from_bits(0xBF80_0000), F32::from_bits(0x8000_0000), &t).to_bits(), 0xFF7F_FFFF);
    assert_eq!(safe_div(F32::from_bits(0x7FC0_0000), z, &t).to_bits(), 0x7F7F_FFFF);
    // a > b only by at least 0.001
    assert!(gt_eps(one, z));
    assert!(!gt_eps(z, one));
    assert!(!gt_eps(F32::from_bits(0x3A80_0000), z)); // 0.00097 < 0.001
    assert!(gt_eps(F32::from_bits(0x3A83_126F), z)); // exactly 0.001
}

#[test]
fn immolation_marker_duration_matches_the_original() {
    let text = include_str!("data/undead_ne/immo_marker.txt");
    let mut n = 0;
    for line in text.lines() {
        let h: Vec<&str> = line.split_whitespace().collect();
        let got = aoe_marker_duration(f(h[0]));
        // the original returns the pointer's value for a non-positive input: 0.0; NaN compares false like the model
        assert_eq!(got.to_bits(), hex(h[1]), "apply {line}");
        assert_eq!(got.to_bits(), hex(h[2]), "refresh {line}");
        n += 1;
    }
    assert!(n >= 300);
}
