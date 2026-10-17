//! The follower's walk step against the original (`docs/spec/movement-8.md`).
//!
//! `tests/data/follow_w.txt` comes from `tools/emu/follow_emu.py`: Unicorn runs of the real `0x6F49DF00` (reach test,
//! one-cell trace of footprint classes 0 to 3, blocker scan, blocked branch, detour draw) on synthetic grids, movers
//! and followers. Integers and float bit patterns only. Regenerate with
//!
//!   GAME_DLL=/path/Game.dll py tools/emu/follow_emu.py crates/sim/tests/data/follow_w.txt 1500 7
//!
//! Needs `GAME_DLL` (the soft-float tables and the generator table); skipped without it.

use sim::follow::{walk_step, Follower, FollowHost, NodeExtra, MOVER_TAG};
use sim::occupancy::{Entry, NodeRec, Occupancy};
use sim::rng::{Rng, RngTable};
use softfloat::{F32, Tables};

const W: &str = include_str!("data/follow_w.txt");

struct Cur<'a> {
    v: &'a [u32],
    i: usize,
}

impl Cur<'_> {
    fn next(&mut self) -> u32 {
        let x = self.v[self.i];
        self.i += 1;
        x
    }
}

struct Mover {
    tag: u32,
    serial: u32,
    v: (F32, F32),
    unit: usize,
    valid: bool,
}

struct Unit {
    serial: u32,
    flags: u32,
    stamp: u32,
    valid: bool,
}

struct Host<'a> {
    movers: Vec<Mover>,
    units: Vec<Unit>,
    fols: Vec<Follower>,
    rng: Rng,
    table: &'a RngTable,
}

impl FollowHost for Host<'_> {
    fn follower(&mut self, m: u32) -> &mut Follower {
        &mut self.fols[m as usize]
    }
    fn tag_ok(&self, m: u32) -> bool {
        self.movers[m as usize].tag == MOVER_TAG
    }
    fn velocity(&self, m: u32) -> (F32, F32) {
        self.movers[m as usize].v
    }
    fn unit_of(&self, m: u32) -> Option<u32> {
        let u = self.movers[m as usize].unit;
        self.units[u].valid.then_some(u as u32)
    }
    fn unit_flag8(&self, unit: u32) -> bool {
        self.units[unit as usize].flags & 8 != 0
    }
    fn unit_stamp(&self, unit: u32) -> u32 {
        self.units[unit as usize].stamp
    }
    fn handle_of(&self, m: u32) -> (u32, u32) {
        (m, self.movers[m as usize].serial)
    }
    fn handle_valid(&self, h: (u32, u32)) -> bool {
        let nm = self.movers.len() as u32;
        let nu = self.units.len() as u32;
        if h.0 >= nm + nu {
            return false;
        }
        if h.0 < nm {
            let m = &self.movers[h.0 as usize];
            m.valid && m.serial == h.1
        } else {
            let u = &self.units[(h.0 - nm) as usize];
            u.valid && u.serial == h.1
        }
    }
    fn draw(&mut self) -> u32 {
        self.rng.next_u32(self.table)
    }
}

#[test]
fn walk_step_matches_the_original() {
    let Some(path) = std::env::var_os("GAME_DLL") else {
        eprintln!("GAME_DLL not set: skipped");
        return;
    };
    let dll = std::fs::read(path).expect("dll");
    let tables = Tables::from_game_dll(&dll).expect("tables");
    let rtable = RngTable::from_game_dll(&dll).expect("rng table");
    let (mut cases, mut draws, mut codes) = (0, 0, [0u32; 5]);
    let (mut marked, mut asked) = (0, 0);
    let extra_file = std::env::var_os("FOLLOW_VECTORS").map(|p| std::fs::read_to_string(p).expect("vectors"));
    let text: &str = extra_file.as_deref().unwrap_or(W);
    for (ln, line) in text.lines().enumerate() {
        let Some((inp, outp)) = line.split_once(" | ") else { continue };
        let hexes = |s: &str| -> Vec<u32> {
            s.split_whitespace().map(|t| u32::from_str_radix(t, 16).expect("hex")).collect()
        };
        let iv = hexes(&inp[2..]);
        let ov = hexes(outp);
        let mut c = Cur { v: &iv, i: 0 };
        let nm = c.next() as usize;
        let nu = c.next() as usize;
        let (w, h) = (c.next() as i32, c.next() as i32);
        let counter = c.next();
        let (ra, rb) = (c.next(), c.next());
        let me = c.next();
        let ncells = c.next() as usize;
        let mut occ = Occupancy::new(w, h);
        for i in 0..ncells {
            occ.cells[i] = c.next();
        }
        let npool = c.next() as usize;
        for _ in 0..npool {
            let w0 = c.next();
            let node = c.next();
            occ.pool.push(Entry { w0, node });
        }
        let nnodes = c.next() as usize;
        let mut extra: Vec<NodeExtra> = Vec::new();
        for _ in 0..nnodes {
            let flags = c.next();
            let stamp = c.next();
            let word40 = c.next();
            let owner = c.next();
            occ.nodes.push(NodeRec { stamp, refs: 0, rect: [0; 4] });
            extra.push(NodeExtra { flags, word40, owner: if owner == u32::MAX { None } else { Some(owner) } });
        }
        occ.stamp = counter;
        let mut movers = Vec::new();
        for _ in 0..nm {
            let (tag, serial, vx, vy, unit, valid) = (c.next(), c.next(), c.next(), c.next(), c.next(), c.next());
            movers.push(Mover { tag, serial, v: (F32(vx), F32(vy)), unit: unit as usize, valid: valid != 0 });
        }
        let mut units = Vec::new();
        for _ in 0..nu {
            let (serial, flags, stamp, valid) = (c.next(), c.next(), c.next(), c.next());
            units.push(Unit { serial, flags, stamp, valid: valid != 0 });
        }
        let mut fols = Vec::new();
        for _ in 0..nm {
            let (flags, wait, retry, blk, bad, mask, radius, px, py, own) =
                (c.next(), c.next(), c.next(), c.next(), c.next(), c.next(), c.next(), c.next(), c.next(), c.next());
            let mut f = Follower::new(mask, F32(radius));
            f.flags = flags;
            f.wait = wait;
            f.retry = retry;
            f.reissue = (F32(px), F32(py));
            f.own_node = if own == u32::MAX { None } else { Some(own) };
            if blk != u32::MAX {
                f.blocker = (blk, movers[blk as usize].serial.wrapping_add(bad));
            }
            fols.push(f);
        }
        let nwp = c.next() as usize;
        let wps: Vec<(F32, F32)> = (0..nwp).map(|_| (F32(c.next()), F32(c.next()))).collect();
        let idx = c.next() as usize;
        let pos = (F32(c.next()), F32(c.next()));
        for f in fols.iter_mut() {
            f.index = idx as u32;
            f.count = nwp as u32;
        }
        let mut host = Host {
            movers,
            units,
            fols,
            rng: Rng { a: ra, b: rb },
            table: &rtable,
        };
        let wp = wps[idx];
        let fols_radius = host.fols[me as usize].radius.0;
        let out = walk_step(&mut occ, &mut extra, &mut host, me, pos, wp, &tables)
            .unwrap_or_else(|| panic!("line {ln}: waypoint reached"));
        // expected, in the order of follow_emu.collect()
        let mut o = Cur { v: &ov, i: 0 };
        let want_ret = o.next();
        let (wx, wy) = (o.next(), o.next());
        let nl = o.next() as usize;
        let want_list: Vec<Option<u32>> = (0..nl)
            .map(|_| {
                let v = o.next();
                if v == u32::MAX { None } else { Some(v) }
            })
            .collect();
        let want_counter = o.next();
        let want_stamps: Vec<u32> = (0..nnodes).map(|_| o.next()).collect();
        let (wa, wb) = (o.next(), o.next());
        assert_eq!(out.code, want_ret, "line {ln}: return");
        let (ex, ey) = if out.restored { (0, 0) } else { (wp.0 .0, wp.1 .0) };
        assert_eq!((ex, ey), (wx, wy), "line {ln}: waypoint copy");
        assert_eq!(
            out.list,
            want_list,
            "line {ln}: list (class {} pos {:?} wp {:?} grid {w}x{h})",
            sim::follow::size_class(F32(fols_radius)),
            (pos.0.to_f32(), pos.1.to_f32()),
            (wp.0.to_f32(), wp.1.to_f32())
        );
        assert_eq!(occ.stamp, want_counter, "line {ln}: counter");
        let got_stamps: Vec<u32> = occ.nodes.iter().map(|n| n.stamp).collect();
        assert_eq!(got_stamps, want_stamps, "line {ln}: stamps");
        assert_eq!((host.rng.a, host.rng.b), (wa, wb), "line {ln}: generator");
        for (i, f) in host.fols.iter().enumerate() {
            let want = [o.next(), o.next(), o.next(), o.next(), o.next(), o.next(), o.next()];
            let got = [f.flags, f.wait, f.retry, f.blocker.0, f.blocker.1, f.index, f.count];
            assert_eq!(got, want, "line {ln}: follower {i} (flags wait retry blocker idx count)");
        }
        cases += 1;
        if host.fols[me as usize].blocker.0 != u32::MAX && !out.restored {
            marked += 1;
        }
        if host.fols.iter().enumerate().any(|(i, f)| i as u32 != me && f.wait >= 20) {
            asked += 1;
        }
        if out.drew {
            draws += 1;
        }
        codes[out.code as usize] += 1;
    }
    eprintln!("follow vectors: {cases} cases, {draws} draws, codes {codes:?}, blocker marked {marked}, others told to wait {asked}");
    assert!(cases >= 300, "too few cases: {cases}");
}

const A: &str = include_str!("data/follow_a.txt");

#[test]
fn request_gate_matches_the_original() {
    let mut n = 0;
    for (ln, line) in A.lines().enumerate() {
        let v: Vec<u32> = line.split_whitespace().skip(1).map(|t| u32::from_str_radix(t, 16).unwrap()).collect();
        let (k, last, now, want_ok, want_last, want_out) = (v[0], v[1], v[2], v[3], v[4], v[5]);
        let mut l = last;
        let (ok, el) = sim::follow::request_gate(&mut l, now, sim::follow::REQUEST_INTERVAL[k as usize]);
        assert_eq!((ok as u32, l, el), (want_ok, want_last, want_out), "line {ln}");
        n += 1;
    }
    assert!(n >= 300);
}
