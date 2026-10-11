//! The buff strength comparisons, the aura refresh decision and the set-duration rule against vectors from the original
//! (`tools/emu/buffs_emu.py`, Game.dll 1.26a, Unicorn). Gated: set `BUFF_VECTORS` to the directory that holds the files
//! `compare.tsv`, `refresh.tsv` and `setdur.tsv` the tool writes; without it the tests only run their synthetic cases.
//! The vectors are not committed (they are produced from the user's own Game.dll).
//!
//! Counts of the last run: 3,600 comparison cases (three routines), 2,400 refresh cases, 1,200 set-duration cases,
//! 0 mismatches.

use std::collections::BTreeSet;
use std::path::PathBuf;

use sim::auras::{
    new_exceeds, refresh_decision, set_duration_rule, stored_exceeds, within_tolerance, AURA_SCAN_PERIOD,
};
use softfloat::F32;

fn dir() -> Option<PathBuf> {
    std::env::var_os("BUFF_VECTORS").map(PathBuf::from)
}

fn lines(name: &str) -> Option<Vec<Vec<String>>> {
    let path = dir()?.join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    Some(
        text.lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.split('\t').map(str::to_owned).collect())
            .collect(),
    )
}

fn bits(s: &str) -> F32 {
    F32::from_bits(s.parse::<u32>().expect("bits"))
}

#[test]
fn comparisons_match_the_original() {
    let Some(rows) = lines("compare.tsv") else { return };
    let mut n = 0;
    for r in &rows {
        match r[0].as_str() {
            "period" => assert_eq!(bits(&r[1]), AURA_SCAN_PERIOD, "the proximity period is DIV(2.0, 4.0)"),
            name => {
                let (new, stored) = (bits(&r[1]), bits(&r[2]));
                let want = r[3] != "0";
                let got = match name {
                    "new_gt" => new_exceeds(new, stored),
                    "stored_gt" => stored_exceeds(new, stored),
                    "equal" => within_tolerance(new, stored),
                    other => panic!("unknown {other}"),
                };
                assert_eq!(got, want, "{name} new={:#x} stored={:#x}", new.to_bits(), stored.to_bits());
                n += 1;
            }
        }
    }
    assert!(n >= 3000, "{n} cases");
}

#[test]
fn refresh_decision_matches_the_original() {
    let Some(rows) = lines("refresh.tsv") else { return };
    for r in &rows {
        let (new, stored) = (bits(&r[1]), bits(&r[2]));
        let s215 = r[3] != "0";
        let got: BTreeSet<&str> = r.get(4).map(|s| s.as_str()).unwrap_or("").split(',').filter(|s| !s.is_empty()).collect();
        let (refresh, store) = refresh_decision(new, stored, s215);
        let mut want = BTreeSet::new();
        if refresh {
            want.insert("setdur");
            if store {
                want.insert("slot218");
            }
            // the class answer is asked only when the new value is not greater and the two are within the tolerance
            if !new_exceeds(new, stored) && within_tolerance(new, stored) {
                want.insert("slot215");
            }
        }
        assert_eq!(got, want, "new={:#x} stored={:#x} s215={s215}", new.to_bits(), stored.to_bits());
    }
}

#[test]
fn set_duration_rule_matches_the_original() {
    let Some(rows) = lines("setdur.tsv") else { return };
    for r in &rows {
        let (dur, remaining) = (bits(&r[1]), bits(&r[2]));
        let got: BTreeSet<&str> = r[3].split(',').filter(|s| !s.is_empty()).collect();
        let flag80 = r[4] == "1";
        let (on, restart) = set_duration_rule(dur, Some(remaining));
        let mut want: BTreeSet<&str> = ["s109", "s114", "s201"].into_iter().collect();
        if !on {
            want.insert("cancel");
        } else if restart {
            want.insert("s210");
            want.insert("s211");
        }
        assert_eq!(got, want, "dur={:#x} remaining={:#x}", dur.to_bits(), remaining.to_bits());
        assert_eq!(flag80, on, "flag 0x80");
    }
}

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

#[test]
fn synthetic_strength_rules() {
    // a stronger stored value ignores the new application entirely
    assert_eq!(refresh_decision(f(3.0), f(4.5), false), (false, false));
    // equal strength refreshes the duration without storing
    assert_eq!(refresh_decision(f(3.0), f(3.0), false), (true, false));
    // equal strength with the class answering yes stores again
    assert_eq!(refresh_decision(f(3.0), f(3.0), true), (true, true));
    // a stronger new value stores
    assert_eq!(refresh_decision(f(4.5), f(3.0), false), (true, true));
    // the tolerance: 0.0005 apart is equal
    assert_eq!(refresh_decision(f(3.0005), f(3.0), false), (true, false));
    // a duration at or below zero cancels; a positive one restarts only above the time left
    assert_eq!(set_duration_rule(f(0.0), Some(f(1.0))), (false, false));
    assert_eq!(set_duration_rule(f(4.0), Some(f(1.0))), (true, true));
    assert_eq!(set_duration_rule(f(4.0), Some(f(4.0))), (true, false));
    assert_eq!(set_duration_rule(f(4.0), None), (true, true));
}
