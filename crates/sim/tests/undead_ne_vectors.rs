//! Unicorn vectors of the S28 numeric cores (`tools/emu/undead_ne2_emu.py`, original Game.dll code run for real):
//! Death Pact `0x6F09FD50`, Vampiric Aura handler `0x6F1AD980`, Blink point validation `0x6F1AEB10`.
//! The Blink vectors need the soft-float tables (`GAME_DLL=...`); the others run from the committed files.

use sim::undead_ne::{blink_project, death_pact_amounts, nonzero, vampiric_outcome, VampiricGate};
use softfloat::{Tables, F32};

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex")
}

fn f(s: &str) -> F32 {
    F32::from_bits(hex(s))
}

#[test]
fn death_pact_matches_the_original() {
    let text = include_str!("data/undead_ne/deathpact.txt");
    let (mut kill_rows, mut other) = (0, 0);
    for line in text.lines() {
        let (head, tail) = line.split_once(" | ").expect("separator");
        let h: Vec<&str> = head.split_whitespace().collect();
        let life = f(h[0]);
        let data = [f(h[1]), f(h[2]), f(h[3]), f(h[4]), f(h[5])];
        let flag = hex(h[6]);
        let tflags = hex(h[7]);
        let events: Vec<&str> = tail.split_whitespace().skip(1).collect();
        if data[4].to_f32() != 0.0 {
            // The drain variant (DataE != 0) is not modelled: the original does not kill.
            assert!(!events.iter().any(|e| e.starts_with("credit")), "row {line}");
            other += 1;
            continue;
        }
        kill_rows += 1;
        let (mana, heal) = death_pact_amounts(life, &data);
        let mut want = vec!["credit:00000001".to_string(), "setlife:00000000".to_string()];
        if flag & 0x4000_0000 == 0 {
            if nonzero(mana) {
                want.push(format!("mana:{:08x}", mana.to_bits()));
            }
            if nonzero(heal) {
                want.push(format!("life:{:08x}", heal.to_bits()));
            }
        }
        assert_eq!(events, want, "row {line}");
        assert_eq!(tflags, flag | 0x1000, "row {line}");
    }
    assert!(kill_rows >= 200 && kill_rows + other >= 500, "{kill_rows} + {other}");
}

#[test]
fn vampiric_aura_matches_the_original() {
    let text = include_str!("data/undead_ne/vampiric.txt");
    let mut n = 0;
    let mut healed = 0;
    for line in text.lines() {
        let (head, tail) = line.split_once(" | ").expect("separator");
        let h: Vec<&str> = head.split_whitespace().collect();
        let gate = VampiricGate {
            ignore_flag: h[4] != "0",
            victim_class: hex(h[5]),
            enemy: h[0] != "0",
            invulnerable: h[3] != "0",
            blocked: h[1] != "0",
            illusion: hex(h[6]) != 0 || hex(h[7]) != 0,
            illusion_ok: h[2] != "0",
        };
        let (reached, heal) = vampiric_outcome(&gate, f(h[9]), f(h[10]));
        let mut want = Vec::new();
        if let Some(x) = heal {
            want.push(format!("heal:{:08x}", x.to_bits()));
            healed += 1;
        }
        if reached {
            want.push("effect:00000001".to_string());
        }
        let got: Vec<String> = if tail.trim() == "none" {
            Vec::new()
        } else {
            tail.split_whitespace().map(str::to_string).collect()
        };
        assert_eq!(got, want, "row {line}");
        n += 1;
    }
    assert!(n >= 500 && healed >= 50, "{n} rows, {healed} heals");
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

#[test]
fn blink_projection_matches_the_original() {
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: Blink vectors skipped");
        return;
    };
    let text = include_str!("data/undead_ne/blink.txt");
    let mut n = 0;
    for line in text.lines() {
        let v: Vec<&str> = line.split_whitespace().collect();
        let got = blink_project((f(v[0]), f(v[1])), (f(v[2]), f(v[3])), f(v[4]), &t);
        assert_eq!(v[7], "00000000", "row {line}: the placement stub succeeds");
        assert_eq!((got.0.to_bits(), got.1.to_bits()), (hex(v[5]), hex(v[6])), "row {line}");
        n += 1;
    }
    assert!(n >= 500);
}
