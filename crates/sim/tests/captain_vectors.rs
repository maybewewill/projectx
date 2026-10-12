//! Replays the Unicorn vectors of the captain routines of Game.dll 1.26a against `sim::captains`:
//! the assault request control flow (`0x6F2D6EA0`, `tools/emu/captain_assault_emu.py`) and the creep-camp chooser
//! (`0x6F2E3980`, `tools/emu/creep_camp_emu.py`). The vectors are synthetic states with the original's answers only.

use sim::captains::{assault_flow, choose_camp, CampView, Pass};

#[test]
fn assault_request_matches_the_original() {
    let text = include_str!("vectors/captain_assault_seed1.tsv");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let typ = u32::from_str_radix(f[0], 16).unwrap();
        let qty: i32 = f[1].parse().unwrap();
        let flags: u32 = f[2].parse().unwrap();
        let mut script: Vec<i32> = f[3].split(',').map(|x| x.parse().unwrap()).collect();
        script.reverse();
        let mut calls: Vec<String> = Vec::new();
        let ok = assault_flow(typ, qty, |pass, t, q| {
            let (kind, flag) = match pass {
                Pass::Group => ("A", 0),
                Pass::Recruit(first) => ("B", u32::from(first)),
            };
            calls.push(format!("{kind}:{t:X}:{flag}"));
            *q = q.wrapping_sub(script.pop().unwrap_or(0));
        });
        let out_flags = if ok { flags } else { flags & !1 };
        let got = format!("{}|{}|{}", u8::from(ok), out_flags, calls.join(";"));
        assert_eq!(got, f[4], "line: {line}");
        n += 1;
    }
    assert_eq!(n, 600);
}

#[test]
fn creep_camp_choice_matches_the_original() {
    let text = include_str!("vectors/creep_camp_seed1.tsv");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (min, max): (i32, i32) = (f[0].parse().unwrap(), f[1].parse().unwrap());
        let fly = f[2] == "1";
        let bound: u32 = f[3].parse().unwrap();
        let camps: Vec<CampView> = if f[4] == "-" {
            Vec::new()
        } else {
            f[4].split(';')
                .map(|c| {
                    let p: Vec<i32> = c.split('/').map(|x| x.parse().unwrap()).collect();
                    CampView { count: p[0], skip: p[1] != 0, level: p[2], flyers: p[3] != 0 }
                })
                .collect()
        };
        let mut costs: Vec<u32> = if f[5] == "-" { Vec::new() } else { f[5].split(',').map(|x| x.parse().unwrap()).collect() };
        costs.reverse();
        let got = choose_camp(&camps, min, max, fly, bound, |_, _| costs.pop().unwrap());
        let want: i32 = f[6].parse().unwrap();
        assert_eq!(got.map_or(-1, |i| i as i32), want, "line: {line}");
        n += 1;
    }
    assert_eq!(n, 600);
}
