//! V5: Sleep (`AUsl`) against the original: the effect `0x6F1FBA80` (`tools/emu/v5_sleep_emu.py`,
//! `tests/data/v5/sleep_effect.txt`, 400 cases) and the wake handler `0x6F1F6870` (`sleep_wake.txt`, 400 cases).

use sim::cast::SpellLevel;
use sim::undead_ne::{sleep_plan, sleep_wakes, SleepEvent};
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn text(ev: &SleepEvent) -> String {
    match ev {
        SleepEvent::MainRefresh(d) => format!("refresh358 {:08x}", d.to_bits()),
        SleepEvent::MainNew(d) => format!("init ; apply354 {:08x}", d.to_bits()),
        SleepEvent::KeeperStunRefresh(d) => format!("refresh360st {:08x}", d.to_bits()),
        SleepEvent::KeeperPauseRefresh(d) => format!("refresh360sp {:08x}", d.to_bits()),
        SleepEvent::KeeperPauseNew(d) => format!("init ; apply358 {:08x}", d.to_bits()),
    }
}

#[test]
fn sleep_effect_matches_the_original() {
    let (mut n, mut dead, mut stun, mut pause, mut fresh) = (0, 0, 0, 0, 0);
    for line in include_str!("data/v5/sleep_effect.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().skip(1).collect();
        let level = SpellLevel {
            dur: F32::from_bits(hex(h[0])),
            hero_dur: F32::from_bits(hex(h[1])),
            data: [F32::from_bits(hex(h[3])), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
            ..SpellLevel::default()
        };
        let extra = F32::from_bits(hex(h[2]));
        let (hero, is_dead) = (h[4] == "1", h[5] == "1");
        let plan = sleep_plan(&level, hero, extra, is_dead, h[6] == "1", h[7] == "1", h[8] == "1");
        let got = if plan.is_empty() { "-".to_string() } else { plan.iter().map(text).collect::<Vec<_>>().join(" ; ") };
        assert_eq!(got, tail.trim(), "row {line}");
        n += 1;
        dead += is_dead as u32;
        stun += tail.contains("360st") as u32;
        pause += tail.contains("360sp") as u32;
        fresh += tail.contains("apply358") as u32;
    }
    assert!(n >= 400 && dead > 10 && stun > 20 && pause > 20 && fresh > 50, "{n} {dead} {stun} {pause} {fresh}");
}

#[test]
fn sleep_wake_rule_matches_the_original() {
    let (mut n, mut ended) = (0, 0);
    for line in include_str!("data/v5/sleep_wake.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().skip(1).collect();
        let wakes = sleep_wakes(F32::from_bits(hex(h[0])), h[1] == "1", h[2] == "1");
        assert_eq!(tail.trim(), if wakes { "1" } else { "0" }, "row {line}");
        n += 1;
        ended += wakes as u32;
    }
    assert!(n >= 400 && ended > 100, "{n} {ended}");
}
