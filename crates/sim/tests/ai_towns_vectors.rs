//! The AI town helpers against the original (`tools/emu/ai_towns_emu.py`, Unicorn).
//!
//! `vectors/ai_towns_seed1.tsv` holds synthetic states (sites, slots, units, race) and the answers of the original
//! bodies `0x6F2D11A0` (mines), `0x6F2D1210` (gold), `0x6F2D1310` (hall), `0x6F2D12B0` (mine), `0x6F2D5380`
//! (town with mine) and `0x6F2D5330` (add slot). The replay needs no DLL and holds no game data.

use std::collections::BTreeMap;

use sim::ai_towns::{Site, Towns};
use sim::natives::game::ObjKey;

struct Unit {
    ty: u32,
    flags: u32,
    gold: i32,
    live: bool,
}

fn parse_ref(text: &str) -> Option<ObjKey> {
    let v: i64 = text.parse().unwrap();
    (v >= 0).then_some(ObjKey(v as u32))
}

#[test]
fn town_helpers_match_the_original_on_the_vectors() {
    let text = include_str!("vectors/ai_towns_seed1.tsv");
    let mut checks = 0usize;
    for (n, line) in text.lines().enumerate() {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 7, "line {n}");
        let op = f[0];
        let race: u32 = f[1].parse().unwrap();
        let slots_v: Vec<i32> = f[2].split(',').map(|x| x.parse().unwrap()).collect();
        let slots = [slots_v[0], slots_v[1], slots_v[2], slots_v[3]];
        let sites: Vec<Site> = f[3]
            .split(';')
            .map(|s| {
                let p: Vec<&str> = s.split(',').collect();
                Site {
                    x: 0.0,
                    y: 0.0,
                    hall: parse_ref(p[0]),
                    mines: [parse_ref(p[1]), parse_ref(p[2])],
                }
            })
            .collect();
        let mut units: BTreeMap<u32, Unit> = BTreeMap::new();
        for u in f[4].split(' ').filter(|u| !u.is_empty()) {
            let p: Vec<&str> = u.split(':').collect();
            units.insert(
                p[0].parse().unwrap(),
                Unit {
                    ty: p[1].parse().unwrap(),
                    flags: p[2].parse().unwrap(),
                    gold: p[3].parse().unwrap(),
                    live: p[4] == "1",
                },
            );
        }
        let live = |k: ObjKey| units.get(&k.0).is_some_and(|u| u.live);
        let player = 3;
        let mut towns = Towns::from_state(sites, player, slots);
        let want = f[6];
        let got = match op {
            "mines" => towns.mines_owned(player, &live).to_string(),
            "gold" => towns
                .gold_owned(player, &live, &|k| units[&k.0].gold)
                .to_string(),
            "hall" => {
                u8::from(towns.town_has_hall(player, f[5].parse().unwrap(), &live)).to_string()
            }
            "mine" => {
                u8::from(towns.town_has_mine(player, f[5].parse().unwrap(), &live)).to_string()
            }
            "withmine" => towns
                .town_with_mine(player, race, &live, &|k| units[&k.0].ty, &|k| {
                    units[&k.0].flags & 0x200_0000 != 0
                })
                .to_string(),
            "addslot" => {
                let slot = towns.add_slot(player, f[5].parse().unwrap());
                let after: Vec<String> = towns.slots(player).iter().map(i32::to_string).collect();
                format!("{slot}|{}", after.join(","))
            }
            other => panic!("unknown op {other}"),
        };
        assert_eq!(got, want, "line {n}: {op} {line}");
        checks += 1;
    }
    assert_eq!(checks, 6426);
}
