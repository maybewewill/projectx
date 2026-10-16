//! V6 verification wave: item and neutral ability cores against `Game.dll` 1.26a (`docs/spec/abilities-items.md`
//! section 19, `docs/spec/abilities-neutral.md` section 9).
//!
//! `v6_vectors.txt` is the output of `tools/emu/v6_emu.py OUT 350 6`: every row is the behaviour of the ORIGINAL code
//! (Unicorn) on fake objects, with the callees that need the object model stubbed and recorded (the row formats are in
//! the module notes of the script). Rows that need the softfloat tables of the user's `Game.dll` read `GAME_DLL`.

use softfloat::{Tables, F32};

const ROWS: &str = include_str!("v6_vectors.txt");

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("hex {s}"))
}

fn rows(kind: &str) -> Vec<Vec<&'static str>> {
    ROWS.lines()
        .filter(|l| l.split(' ').next() == Some(kind))
        .map(|l| l.split(' ').collect())
        .collect()
}

fn f(b: u32) -> F32 {
    F32::from_bits(b)
}

#[allow(dead_code)]
fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

#[test]
fn town_portal_destination_step_matches_the_original() {
    let rs = rows("TN");
    assert!(rs.len() >= 300);
    let (mut replaced, mut stopped) = (0, 0);
    for r in &rs {
        let (cx, cy, ux, uy, best) = (h(r[1]), h(r[2]), h(r[3]), h(r[4]), h(r[5]));
        let (have, first, excl) = (r[6] == "1", r[7] == "1", r[8] == "1");
        assert_eq!(r[9], "|");
        let got = sim::item_abilities::town_portal_step(excl, (f(cx), f(cy)), (f(ux), f(uy)), have.then(|| f(best)), first);
        // with nothing kept the kept distance is whatever stale bits the context held: only compare it when it means something
        let best_bits = if have || got.replaced { got.best } else { h(r[11]) };
        assert_eq!((got.replaced, best_bits, got.keep_going), (r[10] == "1", h(r[11]), r[12] == "1"), "row {r:?}");
        replaced += usize::from(got.replaced);
        stopped += usize::from(!got.keep_going);
    }
    assert!(replaced > 50 && stopped > 10, "the rows reach the replace and stop paths ({replaced}, {stopped})");
}

#[test]
fn transmute_buff_amounts_match_the_original() {
    let rs = rows("TM");
    assert!(rs.len() >= 300);
    let (mut gave, mut skipped, mut bountied) = (0, 0, 0);
    for r in &rs {
        // TM s0 s1 flag ill goldcost woodcost bg bw bflag | called gold wood c d setlife
        let (s0, s1) = (h(r[1]), h(r[2]));
        let (flag, ill) = (r[3] == "1", r[4] == "1");
        let (gc, wc) = (r[5].parse::<u32>().unwrap() as i32, r[6].parse::<u32>().unwrap() as i32);
        let (bg, bw) = (r[7].parse::<i32>().unwrap(), r[8].parse::<i32>().unwrap());
        let bflag = r[9] == "1";
        assert_eq!(r[10], "|");
        if r[11] == "0" {
            assert!(ill, "row {r:?}: nothing is given only for an illusion");
            skipped += 1;
            continue;
        }
        assert!(!ill, "row {r:?}");
        // the gold factor is the float at `eax` (struct+4 = s1), the lumber factor the one behind the third argument (s0)
        let bounty = (flag && bflag).then_some((bg, bw));
        let (gold, wood) = sim::neutral_heroes2::transmute_amounts(f(s1), f(s0), gc, wc, bounty);
        assert_eq!((gold, wood), (r[12].parse::<i32>().unwrap(), r[13].parse::<i32>().unwrap()), "row {r:?}");
        gave += 1;
        bountied += usize::from(bounty.is_some());
    }
    assert!(gave > 150 && skipped > 20 && bountied > 20, "paths ({gave}, {skipped}, {bountied})");
}

#[test]
fn creep_level_rule_matches_the_original() {
    let rs = rows("CL");
    assert!(rs.len() >= 300);
    let (mut refused, mut kept, mut buffed) = (0, 0, 0);
    for r in &rs {
        // CL race tlevel buffcode blevel upper ill mhero munit max | refuses
        let n = |i: usize| r[i].parse::<i32>().unwrap();
        let (race, tlevel, buffcode, blevel) = (n(1), n(2), n(3), n(4));
        let (upper, ill, mhero, munit, max) = (n(5) == 1, n(6) == 1, n(7), n(8), n(9));
        assert_eq!(r[10], "|");
        // buffcode 2 = a level buff whose slot 0x36C answers yes
        let buff = (buffcode == 2).then_some(blevel);
        let level = sim::neutral_heroes2::unit_level_value(tlevel, buff, upper, ill, mhero, munit);
        let got = sim::neutral_heroes2::creep_level_refuses(race, level, max);
        assert_eq!(got, r[11] == "1", "row {r:?}");
        refused += usize::from(got);
        kept += usize::from(!got);
        buffed += usize::from(buff.is_some());
    }
    assert!(refused > 30 && kept > 100 && buffed > 30, "paths ({refused}, {kept}, {buffed})");
}

#[test]
fn charm_effect_matches_the_original() {
    let rs = rows("CE");
    assert!(rs.len() >= 300);
    for r in &rs {
        // CE ill hasowner | kind arg
        assert_eq!(r[3], "|");
        let kills = sim::neutral_spells::charm_kills(r[1] == "1");
        assert_eq!(if kills { "1" } else { "2" }, r[4], "row {r:?}");
        if kills {
            assert_eq!(r[5], "6faae470", "the life setter gets the constant 0.0, row {r:?}");
        }
    }
}
