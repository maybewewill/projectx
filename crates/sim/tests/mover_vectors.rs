//! Mover kernels against the original (`docs/spec/movement-6.md`).
//!
//! The vectors come from `tools/emu/movement6_emu.py` (Unicorn runs of the real routines on a synthetic unit). They are
//! outputs of the user's own `Game.dll` and are not committed. Run
//!
//!   GAME_DLL=/path/Game.dll python tools/emu/movement6_emu.py VS /tmp/vs.txt 3000 1   (also CS TA AD HD)
//!   MOVER_VECTORS=/tmp GAME_DLL=/path/Game.dll cargo test -p sim --test mover_vectors -- --nocapture
//!
//! `MOVER_VECTORS` is the directory holding `vs.txt`, `cs.txt`, `ta.txt`, `ad.txt`, `hd.txt`, `sv.txt`, `na.txt`, `wp.txt`,
//! `sk.txt`, `st.txt` and `wk.txt`. Without it the tests run on the small embedded samples, which hold only table-free
//! kernels (`VS`/`CS` without the clamp, `TA`, `WP`, `ST`: add, subtract, multiply, square root, truncate). No output of
//! the game's lookup tables (reciprocal, sine, arc-sine) is committed; the kernels that need them (`SV`, `NA`, `AD`, `HD`,
//! `SK`, `WK`, the clamped `VS`/`CS`) run only from `MOVER_VECTORS` with `GAME_DLL`.

use sim::heroes::GridCell;
use sim::mover::{heading_error, heading_of, steer, Kin, SteerParams};
use softfloat::{F32, Tables};

fn f(bits: u32) -> F32 {
    F32(bits)
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(path).ok()?;
    Tables::from_game_dll(&bytes).ok()
}

fn hexes(line: &str) -> Vec<u32> {
    line.split_whitespace()
        .skip(1)
        .map(|t| u32::from_str_radix(t, 16).expect("hex field"))
        .collect()
}

fn lines(kind: &str, file: &str, embedded: &'static str) -> Vec<String> {
    if let Ok(dir) = std::env::var("MOVER_VECTORS") {
        let p = std::path::Path::new(&dir).join(file);
        if let Ok(text) = std::fs::read_to_string(&p) {
            return text.lines().filter(|l| l.starts_with(kind)).map(|s| s.to_string()).collect();
        }
    }
    embedded.lines().filter(|l| l.starts_with(kind)).map(|s| s.to_string()).collect()
}

/// A velocity or cap set: fields `o14 t0 row0 cx cy vx vy cap flags gA(t,row,scale) gB(t,row,scale) in.. out(8) clamp`.
fn run_set(kind: &str, file: &str, embedded: &'static str, n_in: usize) -> (usize, usize, usize) {
    let tb = tables();
    let (mut cases, mut skipped, mut bad) = (0, 0, 0);
    for line in lines(kind, file, embedded) {
        let h = hexes(&line);
        assert_eq!(h.len(), 15 + n_in + 8 + 1, "{line}");
        let o14 = h[0];
        let grid_a = GridCell { origin: f(h[9]), row: h[10] as i32, scale: f(h[11]) };
        let grid_b = GridCell { origin: f(h[12]), row: h[13] as i32, scale: f(h[14]) };
        let g = if o14 & 0x8000_0000 != 0 { grid_b } else { grid_a };
        let mut k = Kin {
            snap_t: f(h[1]),
            snap_row: h[2] as i32,
            cx: f(h[3]),
            cy: f(h[4]),
            vx: f(h[5]),
            vy: f(h[6]),
            cap: f(h[7]),
            moving: h[8] & 0x2000_0000 != 0,
            facing: F32::ZERO,
            last_speed: F32::ZERO,
            face_error: F32::ZERO,
        };
        let clamped = h[h.len() - 1] != 0;
        let dummy = Tables::default();
        let t = match tb.as_ref() {
            Some(t) => t,
            // Without the user's tables only the cases that never reach the reciprocal run (the square root is
            // table-free).
            None if !clamped => &dummy,
            None => {
                skipped += 1;
                continue;
            }
        };
        let out = &h[h.len() - 9..h.len() - 1];
        if kind == "VS" {
            k.set_velocity(&g, (f(h[15]), f(h[16])), t);
        } else {
            k.set_cap(&g, f(h[15]), t);
        }
        let want = (out[0], out[1], out[2], out[3], out[4], out[5], out[6]);
        let got = (k.snap_t.0, k.snap_row as u32, k.cx.0, k.cy.0, k.vx.0, k.vy.0, k.cap.0);
        let flag_want = out[7] & 0x2000_0000 != 0;
        cases += 1;
        if got != want || k.moving != flag_want {
            bad += 1;
            if bad <= 5 {
                eprintln!("{kind} mismatch\n  in  {line}\n  got {got:08x?} moving={}\n  want {want:08x?} moving={flag_want}", k.moving);
            }
        }
    }
    (cases, skipped, bad)
}

#[test]
fn velocity_set_matches_the_original() {
    let (cases, skipped, bad) = run_set("VS", "vs.txt", include_str!("data/mover_vs.txt"), 2);
    eprintln!("VS cases={cases} skipped={skipped} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn cap_set_matches_the_original() {
    let (cases, skipped, bad) = run_set("CS", "cs.txt", include_str!("data/mover_cs.txt"), 1);
    eprintln!("CS cases={cases} skipped={skipped} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn steer_matches_the_original() {
    let (mut cases, mut bad) = (0, 0);
    for line in lines("TA", "ta.txt", include_str!("data/mover_ta.txt")) {
        let h = hexes(&line);
        let p = SteerParams { accel: f(h[0]), turn_cap: f(h[1]), window: f(h[2]) };
        let (mut rd, mut rh) = (f(h[3]), f(h[4]));
        steer(&p, &mut rd, &mut rh, f(h[5]), h[6] != 0);
        cases += 1;
        if (rd.0, rh.0) != (h[7], h[8]) {
            bad += 1;
            if bad <= 5 {
                eprintln!("TA mismatch {line}\n  got {:08x} {:08x}", rd.0, rh.0);
            }
        }
    }
    eprintln!("TA cases={cases} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn heading_and_error_match_the_original() {
    let Some(t) = tables() else {
        eprintln!("skipped: GAME_DLL not set (arccosine and division tables)");
        return;
    };
    let (mut cases, mut bad) = (0, 0);
    for line in lines("HD", "hd.txt", include_str!("data/mover_hd.txt")) {
        let h = hexes(&line);
        let got = heading_of(f(h[0]), f(h[1]), f(h[2]), &t);
        cases += 1;
        if got.0 != h[3] {
            bad += 1;
            if bad <= 5 {
                eprintln!("HD mismatch {line}\n  got {:08x}", got.0);
            }
        }
    }
    for line in lines("AD", "ad.txt", include_str!("data/mover_ad.txt")) {
        let h = hexes(&line);
        let got = heading_error(f(h[0]), f(h[1]), f(h[2]), &t);
        cases += 1;
        if got.0 != h[3] {
            bad += 1;
            if bad <= 5 {
                eprintln!("AD mismatch {line}\n  got {:08x}", got.0);
            }
        }
    }
    eprintln!("HD/AD cases={cases} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn speed_heading_writer_matches_the_original() {
    let Some(t) = tables() else {
        eprintln!("skipped: GAME_DLL not set (sine, cosine and reciprocal tables)");
        return;
    };
    let (mut cases, mut bad) = (0, 0);
    for line in lines("SV", "sv.txt", include_str!("data/mover_sv.txt")) {
        let h = hexes(&line);
        // o14 t0 row0 cx cy vx vy cap flags gA(3) gB(3) facing speed heading | out(8) facing c0 c4 clamp
        assert_eq!(h.len(), 18 + 8 + 3 + 1, "{line}");
        let o14 = h[0];
        let grid_a = GridCell { origin: f(h[9]), row: h[10] as i32, scale: f(h[11]) };
        let grid_b = GridCell { origin: f(h[12]), row: h[13] as i32, scale: f(h[14]) };
        let g = if o14 & 0x8000_0000 != 0 { grid_b } else { grid_a };
        let mut k = Kin {
            snap_t: f(h[1]),
            snap_row: h[2] as i32,
            cx: f(h[3]),
            cy: f(h[4]),
            vx: f(h[5]),
            vy: f(h[6]),
            cap: f(h[7]),
            moving: h[8] & 0x2000_0000 != 0,
            facing: f(h[15]),
            last_speed: F32::ZERO,
            face_error: F32::ZERO,
        };
        k.set_speed_heading(&g, f(h[16]), f(h[17]), &t);
        let o = &h[18..];
        let want = (o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[8], o[9], o[10]);
        let got = (k.snap_t.0, k.snap_row as u32, k.cx.0, k.cy.0, k.vx.0, k.vy.0, k.cap.0, k.facing.0, k.last_speed.0, k.face_error.0);
        let flag_want = o[7] & 0x2000_0000 != 0;
        cases += 1;
        if got != want || k.moving != flag_want {
            bad += 1;
            if bad <= 5 {
                eprintln!("SV mismatch
  in  {line}
  got {got:08x?} moving={}
  want {want:08x?} moving={flag_want}", k.moving);
            }
        }
    }
    eprintln!("SV cases={cases} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn angle_normaliser_matches_the_original() {
    let Some(t) = tables() else {
        eprintln!("skipped: GAME_DLL not set");
        return;
    };
    let (mut cases, mut bad) = (0, 0);
    for line in lines("NA", "na.txt", include_str!("data/mover_na.txt")) {
        let h = hexes(&line);
        let got = sim::mover::normalise_angle(f(h[0]), &t);
        cases += 1;
        if got.0 != h[1] {
            bad += 1;
            if bad <= 5 {
                eprintln!("NA mismatch {line}
  got {:08x}", got.0);
            }
        }
    }
    eprintln!("NA cases={cases} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn walk_matches_the_original() {
    use sim::mover::{walk, PathState, Pilot};
    let Some(t) = tables() else {
        eprintln!("skipped: GAME_DLL not set (the walk needs every table)");
        return;
    };
    let (mut cases, mut bad, mut arrived, mut blocked, mut moving) = (0, 0, 0, 0, 0);
    for line in lines("WK", "wk.txt", include_str!("data/mover_wk.txt")) {
        let h = hexes(&line);
        // o14 gA(3) gB(3) | t0 row0 cx cy vx vy cap flags facing b0 b4 b8 bc goal(2) wp(2) code | rd rh | out(8) facing c0 c4
        assert_eq!(h.len(), 7 + 18 + 2 + 11, "{line}");
        let o14 = h[0];
        let grid_a = GridCell { origin: f(h[1]), row: h[2] as i32, scale: f(h[3]) };
        let grid_b = GridCell { origin: f(h[4]), row: h[5] as i32, scale: f(h[6]) };
        let g = if o14 & 0x8000_0000 != 0 { grid_b } else { grid_a };
        let p = &h[7..25];
        let mut k = Kin {
            snap_t: f(p[0]),
            snap_row: p[1] as i32,
            cx: f(p[2]),
            cy: f(p[3]),
            vx: f(p[4]),
            vy: f(p[5]),
            cap: f(p[6]),
            moving: p[7] & 0x2000_0000 != 0,
            facing: f(p[8]),
            last_speed: F32::ZERO,
            face_error: F32::ZERO,
        };
        let pilot = Pilot {
            steer: SteerParams { accel: f(p[10]), turn_cap: f(p[11]), window: f(p[12]) },
            radius: f(p[9]),
        };
        let goal = (f(p[13]), f(p[14]));
        let code = p[17];
        let path = if code == 0 { PathState::Open((f(p[15]), f(p[16]))) } else { PathState::Blocked };
        let tr = walk(&mut k, &pilot, &g, goal, path, &t);
        let want_trace = (h[25], h[26]);
        let o = &h[27..];
        let want = (o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[8], o[9], o[10]);
        let got = (k.snap_t.0, k.snap_row as u32, k.cx.0, k.cy.0, k.vx.0, k.vy.0, k.cap.0, k.facing.0, k.last_speed.0, k.face_error.0);
        let flag_want = o[7] & 0x2000_0000 != 0;
        cases += 1;
        arrived += tr.arrived as usize;
        blocked += (code != 0) as usize;
        moving += (k.vx.0 != 0 || k.vy.0 != 0) as usize;
        if got != want || k.moving != flag_want || (tr.speed.0, tr.heading.0) != want_trace {
            bad += 1;
            if bad <= 5 {
                eprintln!(
                    "WK mismatch\n  in  {line}\n  got {got:08x?} moving={} trace=({:08x},{:08x})\n  want {want:08x?} moving={flag_want} trace={want_trace:08x?}",
                    k.moving, tr.speed.0, tr.heading.0
                );
            }
        }
    }
    eprintln!("WK cases={cases} arrived={arrived} blocked={blocked} moving_after={moving} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn angle_wrap_matches_the_original() {
    let (mut cases, mut bad) = (0, 0);
    for line in lines("WP", "wp.txt", include_str!("data/mover_wp.txt")) {
        let h = hexes(&line);
        let got = sim::mover::wrap_positive(f(h[0]));
        cases += 1;
        if got.0 != h[1] {
            bad += 1;
            if bad <= 5 {
                eprintln!("WP mismatch {line}\n  got {:08x}", got.0);
            }
        }
    }
    eprintln!("WP cases={cases} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn speed_sink_matches_the_original() {
    use sim::mover::{speed_bound, speed_to_cap};
    let Some(t) = tables() else {
        eprintln!("skipped: GAME_DLL not set (the speed bound divides)");
        return;
    };
    // the bound the map loader writes (mgr+0x80)
    assert_eq!(speed_bound(&t).0, 0x4402_AAAB);
    let (mut cases, mut bad) = (0, 0);
    for line in lines("SK", "sk.txt", include_str!("data/mover_sk.txt")) {
        let h = hexes(&line);
        // o14 t0 row0 cx cy vx vy cap flags gA(3) gB(3) speed | out(8) clamp
        assert_eq!(h.len(), 16 + 8 + 1, "{line}");
        let o14 = h[0];
        let grid_a = GridCell { origin: f(h[9]), row: h[10] as i32, scale: f(h[11]) };
        let grid_b = GridCell { origin: f(h[12]), row: h[13] as i32, scale: f(h[14]) };
        let g = if o14 & 0x8000_0000 != 0 { grid_b } else { grid_a };
        let mut k = Kin {
            snap_t: f(h[1]),
            snap_row: h[2] as i32,
            cx: f(h[3]),
            cy: f(h[4]),
            vx: f(h[5]),
            vy: f(h[6]),
            cap: f(h[7]),
            moving: h[8] & 0x2000_0000 != 0,
            facing: F32::ZERO,
            last_speed: F32::ZERO,
            face_error: F32::ZERO,
        };
        let cap = speed_to_cap(f(h[15]), speed_bound(&t));
        k.set_cap(&g, cap, &t);
        let o = &h[16..];
        let want = (o[0], o[1], o[2], o[3], o[4], o[5], o[6]);
        let got = (k.snap_t.0, k.snap_row as u32, k.cx.0, k.cy.0, k.vx.0, k.vy.0, k.cap.0);
        let flag_want = o[7] & 0x2000_0000 != 0;
        cases += 1;
        if got != want || k.moving != flag_want {
            bad += 1;
            if bad <= 5 {
                eprintln!("SK mismatch\n  in  {line}\n  got {got:08x?} moving={}\n  want {want:08x?} moving={flag_want}", k.moving);
            }
        }
    }
    eprintln!("SK cases={cases} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn position_setter_on_a_moving_unit_matches_the_original() {
    use sim::unit_position::world_to_cell;
    let (mut cases, mut bad) = (0, 0);
    for line in lines("ST", "st.txt", include_str!("data/mover_st.txt")) {
        let h = hexes(&line);
        // o14 t0 row0 cx cy vx vy cap flags gA(3) gB(3) w6c w70 x y | out(8)
        assert_eq!(h.len(), 19 + 8, "{line}");
        let o14 = h[0];
        let grid_a = GridCell { origin: f(h[9]), row: h[10] as i32, scale: f(h[11]) };
        let grid_b = GridCell { origin: f(h[12]), row: h[13] as i32, scale: f(h[14]) };
        let g = if o14 & 0x8000_0000 != 0 { grid_b } else { grid_a };
        let mut k = Kin {
            snap_t: f(h[1]),
            snap_row: h[2] as i32,
            cx: f(h[3]),
            cy: f(h[4]),
            vx: f(h[5]),
            vy: f(h[6]),
            cap: f(h[7]),
            moving: h[8] & 0x2000_0000 != 0,
            facing: F32::ZERO,
            last_speed: F32::ZERO,
            face_error: F32::ZERO,
        };
        // the setter of Game::store_unit_position for a unit in motion
        let f_pos = k.position(&g);
        let p = (world_to_cell(f(h[17]), f(h[15])), world_to_cell(f(h[18]), f(h[16])));
        let delta = (softfloat::arith::sub(p.0, f_pos.0), softfloat::arith::sub(p.1, f_pos.1));
        k.rebase(&g, delta);
        let o = &h[19..];
        let want = (o[0], o[1], o[2], o[3], o[4], o[5], o[6]);
        let got = (k.snap_t.0, k.snap_row as u32, k.cx.0, k.cy.0, k.vx.0, k.vy.0, k.cap.0);
        cases += 1;
        if got != want {
            bad += 1;
            if bad <= 5 {
                eprintln!("ST mismatch\n  in  {line}\n  got {got:08x?}\n  want {want:08x?}");
            }
        }
    }
    eprintln!("ST cases={cases} mismatches={bad}");
    assert_eq!(bad, 0);
}
