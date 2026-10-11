//! Movement remainder against the original (`docs/spec/movement-7.md`): the velocity notification, the facing smoother and
//! the push-apart record.
//!
//! Vectors come from `tools/emu/movement7_emu.py` (Unicorn runs of the real routines on a synthetic unit). They are outputs
//! of the user's own `Game.dll` and are not committed, except a small sample of the facing smoother restricted to cases
//! that never touch a lookup table. Run
//!
//!   GAME_DLL=/path/Game.dll python tools/emu/movement7_emu.py FS /tmp/fs.txt 3000 1   (also SN, PA)
//!   MOVER7_VECTORS=/tmp GAME_DLL=/path/Game.dll cargo test -p sim --test movement7_vectors -- --nocapture
//!
//! `MOVER7_VECTORS` is the directory holding `fs.txt`, `sn.txt` and `pa.txt`.

use sim::heroes::GridCell;
use sim::mover::{
    direction_of, facing_class, push_apart, smooth_facing, FacingStep, Kin, PushHost, PushRecord, PushResult,
};
use sim::rng::{Rng, RngTable};
use softfloat::{Tables, F32};

fn f(bits: u32) -> F32 {
    F32(bits)
}

fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    std::fs::read(path).ok()
}

fn tables() -> Option<Tables> {
    Tables::from_game_dll(&game_dll()?).ok()
}

fn hexes(line: &str) -> Vec<u32> {
    line.split_whitespace()
        .skip(1)
        .map(|t| u32::from_str_radix(t, 16).expect("hex field"))
        .collect()
}

fn lines(kind: &str, file: &str, embedded: &'static str) -> Vec<String> {
    if let Ok(dir) = std::env::var("MOVER7_VECTORS") {
        let p = std::path::Path::new(&dir).join(file);
        if let Ok(text) = std::fs::read_to_string(&p) {
            return text.lines().filter(|l| l.starts_with(kind)).map(|s| s.to_string()).collect();
        }
    }
    embedded.lines().filter(|l| l.starts_with(kind)).map(|s| s.to_string()).collect()
}

#[test]
fn class_rows_are_the_start_up_literals() {
    use sim::mover::{FACING_BITS, FACING_TEXT, PUSH_BITS, PUSH_TEXT};
    let Some(t) = tables() else {
        eprintln!("skipped: GAME_DLL not set (S2R divides)");
        return;
    };
    let p = |s: &str| softfloat::text::s2r(Some(&t), s.as_bytes()).expect("s2r").0;
    for (text, bits) in FACING_TEXT.iter().zip(FACING_BITS.iter()) {
        let got: Vec<u32> = text.iter().map(|s| p(s)).collect();
        assert_eq!(got, bits.to_vec(), "{text:?}");
    }
    for (text, bits) in PUSH_TEXT.iter().zip(PUSH_BITS.iter()) {
        let got: Vec<u32> = text.iter().map(|s| p(s)).collect();
        assert_eq!(got, bits.to_vec(), "{text:?}");
    }
}

#[test]
fn facing_smoother_matches_the_original() {
    let tb = tables();
    let (mut cases, mut skipped, mut bad, mut finished) = (0, 0, 0, 0);
    for line in lines("FS", "fs.txt", include_str!("data/mover7_fs.txt")) {
        let h = hexes(&line);
        // class face cur rate linked | c8 cc u4 u8 node_a8 node_b4 | tabled
        assert_eq!(h.len(), 5 + 6 + 1, "{line}");
        let tabled = h[11] != 0;
        let dummy = Tables::default();
        let t = match tb.as_ref() {
            Some(t) => t,
            None if !tabled => &dummy,
            None => {
                skipped += 1;
                continue;
            }
        };
        let class = facing_class(h[0]);
        let step = smooth_facing(f(h[2]), f(h[3]), f(h[1]), &class, t);
        let (want_angle, want_rate) = (h[5], h[6]);
        let (got_angle, got_rate) = match step {
            FacingStep::Finished => {
                finished += 1;
                (h[1], 0)
            }
            FacingStep::Turned { angle, rate } => (angle.0, rate.0),
        };
        cases += 1;
        // linked: the finish routine unlinks the mover (u4 = u8 = 0)
        let unlinked = h[7] == 0 && h[8] == 0;
        let link_ok = h[4] == 0 || (matches!(step, FacingStep::Finished) == unlinked);
        if (got_angle, got_rate) != (want_angle, want_rate) || !link_ok {
            bad += 1;
            if bad <= 5 {
                eprintln!("FS mismatch\n  in  {line}\n  got {got_angle:08x} {got_rate:08x} {step:?}");
            }
        }
    }
    eprintln!("FS cases={cases} skipped={skipped} finished={finished} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn velocity_notification_matches_the_original() {
    let Some(t) = tables() else {
        eprintln!("skipped: GAME_DLL not set (sine, cosine, reciprocal and arc-cosine tables)");
        return;
    };
    let (mut cases, mut bad) = (0, 0);
    for line in lines("SN", "sn.txt", "") {
        let h = hexes(&line);
        // o14 t0 row0 cx cy vx vy cap flags gA(3) gB(3) facing speed heading listed | out(8) facing c0 c4 u4 u8 | clamp
        assert_eq!(h.len(), 19 + 8 + 5 + 1, "{line}");
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
        k.set_speed_heading_notify(&g, f(h[16]), f(h[17]), &t);
        let o = &h[19..];
        let want = (o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[8], o[9], o[10]);
        let got = (k.snap_t.0, k.snap_row as u32, k.cx.0, k.cy.0, k.vx.0, k.vy.0, k.cap.0, k.facing.0, k.last_speed.0, k.face_error.0);
        // the mover is linked into the smoother list after the call whatever the velocity (the stop sequence too)
        let linked_after = o[11] != 0 || o[12] != 0;
        cases += 1;
        if got != want || !linked_after {
            bad += 1;
            if bad <= 5 {
                eprintln!("SN mismatch\n  in  {line}\n  got {got:08x?}\n  want {want:08x?} linked_after={linked_after}");
            }
        }
    }
    eprintln!("SN cases={cases} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn follower_waypoint_test_matches_the_original() {
    use sim::mover::{follower_reach, Follower, Reach};
    let (mut cases, mut bad, mut ties) = (0, 0, 0);
    for line in lines("WR", "wr.txt", include_str!("data/mover7_wr.txt")) {
        let h = hexes(&line);
        // pos(2) wp(2) idx cnt wait flags blocked c2e0_ret | ret idx flags wait blocked c380 c2e0
        assert_eq!(h.len(), 10 + 7, "{line}");
        let mut st = Follower { index: h[4], count: h[5], wait: h[6], flags: h[7], blocked: h[8] };
        let mut c2e0_calls = 0u32;
        let (code, reach) =
            follower_reach(&mut st, (f(h[0]), f(h[1])), (f(h[2]), f(h[3])), || {
                c2e0_calls += 1;
                h[9] != 0
            });
        let o = &h[10..];
        let got = [code, st.index, st.flags, st.wait, st.blocked, matches!(reach, Reach::Advance) as u32, c2e0_calls];
        let want = [o[0], o[1], o[2], o[3], o[4], o[5], o[6]];
        cases += 1;
        // a tie: the squared distance equals 0.49 * 0.49
        let d2 = softfloat::arith::dist2(f(h[2]), f(h[3]), f(h[0]), f(h[1]));
        let c = softfloat::arith::mul(F32(0x3EFA_E148), F32(0x3EFA_E148));
        ties += (d2 == c) as usize;
        if got != want {
            bad += 1;
            if bad <= 5 {
                eprintln!("WR mismatch\n  in  {line}\n  got  {got:08x?} {reach:?}\n  want {want:08x?}");
            }
        }
    }
    eprintln!("WR cases={cases} ties={ties} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn group_limit_and_gate_match_the_original() {
    use sim::mover::{group_gate, group_limit, GroupLeader, GroupMember};
    let (mut cases, mut bad, mut gated, mut overridden) = (0, 0, 0, 0);
    for line in lines("GL", "gl.txt", include_str!("data/mover7_gl.txt")) {
        let h = hexes(&line);
        // n gflags leader lim lmin | (cap rflags uflag) * n | limit lead_min gate
        let n = h[0] as usize;
        assert_eq!(h.len(), 5 + 3 * n + 3, "{line}");
        let members: Vec<GroupMember> = (0..n)
            .map(|k| GroupMember { cap: f(h[5 + 3 * k]), flags: h[6 + 3 * k], unit_flags: h[7 + 3 * k] as u8 })
            .collect();
        let mut leader = GroupLeader { limit: f(h[3]), minimum: f(h[4]) };
        let has_leader = h[2] != 0;
        let limit = group_limit(&members, if has_leader { Some(&mut leader) } else { None });
        let gate = group_gate(h[1] as u8, &members);
        let o = &h[5 + 3 * n..];
        let lead_min = if has_leader { leader.minimum.0 } else { o[1] };
        cases += 1;
        gated += gate as usize;
        overridden += (has_leader && h[3] != 0x7F7F_FFFF) as usize;
        if limit.0 != o[0] || lead_min != o[1] || gate != (o[2] != 0) {
            bad += 1;
            if bad <= 5 {
                eprintln!("GL mismatch\n  in  {line}\n  got {:08x} {lead_min:08x} {gate}", limit.0);
            }
        }
    }
    eprintln!("GL cases={cases} gate_true={gated} leader_limit_set={overridden} mismatches={bad}");
    assert_eq!(bad, 0);
}

#[test]
fn walk_with_notification_matches_the_original() {
    use sim::mover::{walk, PathState, Pilot, SteerParams};
    let Some(t) = tables() else {
        eprintln!("skipped: GAME_DLL not set (the walk needs every table)");
        return;
    };
    let (mut cases, mut bad, mut faced) = (0, 0, 0);
    for line in lines("WN", "wn.txt", "") {
        let h = hexes(&line);
        // same layout as the WK vectors of movement6_emu.py
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
        let path = if p[17] == 0 { PathState::Open((f(p[15]), f(p[16]))) } else { PathState::Blocked };
        let tr = walk(&mut k, &pilot, &g, goal, path, &t);
        k.notify_facing(&t);
        let o = &h[27..];
        let want = (o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[8], o[9], o[10]);
        let got = (k.snap_t.0, k.snap_row as u32, k.cx.0, k.cy.0, k.vx.0, k.vy.0, k.cap.0, k.facing.0, k.last_speed.0, k.face_error.0);
        cases += 1;
        faced += (k.facing.0 != p[8]) as usize;
        if got != want || (tr.speed.0, tr.heading.0) != (h[25], h[26]) {
            bad += 1;
            if bad <= 5 {
                eprintln!("WN mismatch\n  in  {line}\n  got  {got:08x?}\n  want {want:08x?}");
            }
        }
    }
    eprintln!("WN cases={cases} facing_changed={faced} mismatches={bad}");
    assert_eq!(bad, 0);
}

struct Mock<'a> {
    own: Kin,
    nbs: Vec<(Kin, GridCell)>,
    g: GridCell,
    ok: bool,
    rng: Rng,
    table: &'a RngTable,
    t: &'a Tables,
    placed: Option<(F32, F32)>,
    set: bool,
    gat: Option<(u32, u32)>,
}

impl PushHost for Mock<'_> {
    fn position(&mut self) -> (F32, F32) {
        self.own.position(&self.g)
    }
    fn place_ok(&mut self, target: (F32, F32)) -> bool {
        self.placed = Some(target);
        self.ok
    }
    fn move_to(&mut self, _target: (F32, F32)) {
        // the harness hook records the call only; the unit stays where it is
        self.set = true;
    }
    fn gather(&mut self, class: u32, rank: u32, _radius: F32) -> Vec<(F32, F32)> {
        self.gat = Some((rank, class));
        self.nbs.iter().map(|(k, g)| k.position(g)).collect()
    }
    fn random_direction(&mut self) -> (F32, F32) {
        let u = self.rng.random_fraction(self.table);
        direction_of(u, self.t)
    }
}

fn kin_from(o14: u32, t0: u32, row0: u32, cx: u32, cy: u32, vx: u32, vy: u32) -> Kin {
    let _ = o14;
    Kin {
        snap_t: f(t0),
        snap_row: row0 as i32,
        cx: f(cx),
        cy: f(cy),
        vx: f(vx),
        vy: f(vy),
        cap: F32::ZERO,
        moving: false,
        facing: F32::ZERO,
        last_speed: F32::ZERO,
        face_error: F32::ZERO,
    }
}

#[test]
fn push_apart_matches_the_original() {
    let (Some(t), Some(dll)) = (tables(), game_dll()) else {
        eprintln!("skipped: GAME_DLL not set");
        return;
    };
    let table = RngTable::from_game_dll(&dll).expect("rng table");
    let (mut cases, mut bad) = (0, 0);
    let (mut cooling, mut moving, mut settled, mut pushed, mut draws, mut placed_n) = (0, 0, 0, 0, 0, 0);
    for line in lines("PA", "pa.txt", "") {
        let h = hexes(&line);
        let n = h[21] as usize;
        assert_eq!(h.len(), 22 + 7 * n + 5 + 7, "{line}");
        let grid_a = GridCell { origin: f(h[13]), row: h[14] as i32, scale: f(h[15]) };
        let grid_b = GridCell { origin: f(h[16]), row: h[17] as i32, scale: f(h[18]) };
        let pick = |o14: u32| if o14 & 0x8000_0000 != 0 { grid_b } else { grid_a };
        let own = kin_from(h[6], h[7], h[8], h[9], h[10], h[11], h[12]);
        let mut nbs = Vec::new();
        for k in 0..n {
            let b = &h[22 + 7 * k..22 + 7 * k + 7];
            nbs.push((kin_from(b[0], b[1], b[2], b[3], b[4], b[5], b[6]), pick(b[0])));
        }
        let o = &h[22 + 7 * n..];
        let mut host = Mock {
            own,
            nbs,
            g: pick(h[6]),
            ok: h[5] != 0,
            rng: Rng { a: h[19], b: h[20] },
            table: &table,
            t: &t,
            placed: None,
            set: false,
            gat: None,
        };
        let mut rec = PushRecord { flags: h[1], vx: f(h[2]), vy: f(h[3]) };
        let before = host.rng;
        let res = push_apart(&mut rec, f(h[4]), &mut host, &t);
        match res {
            PushResult::Cooling => cooling += 1,
            PushResult::Moving => moving += 1,
            PushResult::Settled => settled += 1,
            PushResult::Pushed => pushed += 1,
        }
        if host.rng != before {
            draws += 1;
        }
        let got = [rec.vx.0, rec.vy.0, rec.flags, host.rng.a, host.rng.b];
        let want = [o[0], o[1], o[2], o[3], o[4]];
        // gather called, rank byte, class nibble, placement tested, target, position set
        let gat_want = (o[5] != 0, o[6], o[7]);
        let gat_got = (host.gat.is_some(), host.gat.map_or(0, |g| g.0), host.gat.map_or(0, |g| g.1));
        let pl_want = (o[8] != 0, o[9], o[10]);
        let pl_got = (host.placed.is_some(), host.placed.map_or(0, |p| p.0 .0), host.placed.map_or(0, |p| p.1 .0));
        let set_want = o[11] != 0;
        if host.placed.is_some() {
            placed_n += 1;
        }
        cases += 1;
        if got != want || gat_got != gat_want || pl_got != pl_want || host.set != set_want {
            bad += 1;
            if bad <= 5 {
                eprintln!(
                    "PA mismatch\n  in  {line}\n  got  {got:08x?} gather={gat_got:?} placed={pl_got:?} set={}\n  want {want:08x?} gather={gat_want:?} placed={pl_want:?} set={set_want}",
                    host.set
                );
            }
        }
    }
    eprintln!(
        "PA cases={cases} mismatches={bad} (cooling={cooling} moving={moving} settled={settled} pushed={pushed} rng_draws={draws} placement_tests={placed_n})"
    );
    assert_eq!(bad, 0);
}
