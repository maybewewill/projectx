//! The push-apart neighbour gather against the original (`docs/spec/movement-7.md` section 8).
//!
//! The vectors in `tests/data/push_*.txt` come from `tools/emu/push_gather_emu.py` (W gather walk, S gather box) and
//! `tools/emu/push_occupancy_emu.py` (O occupant maintenance, F footprint box): Unicorn runs of the real routines on
//! synthetic grids. They contain integers and float bit patterns only, no game data. Regenerate with e.g.
//!
//!   GAME_DLL=/path/Game.dll py tools/emu/push_gather_emu.py W crates/sim/tests/data/push_gather_w.txt 250 5

use sim::occupancy::{cell_box, node_passes, Entry, NodeFacts, NodeRec, Occupancy, Query, NIL};
use softfloat::F32;

const W: &str = include_str!("data/push_gather_w.txt");
const S: &str = include_str!("data/push_gather_s.txt");
const O: &str = include_str!("data/push_occupancy_o.txt");
const FV: &str = include_str!("data/push_occupancy_f.txt");

fn hexes(line: &str) -> Vec<u32> {
    line.split_whitespace().skip(1).map(|t| u32::from_str_radix(t, 16).expect("hex")).collect()
}

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

#[test]
fn gather_walk_matches_the_original() {
    let (mut cases, mut taken) = (0, 0);
    for line in W.lines().filter(|l| l.starts_with("W ")) {
        let v = hexes(line);
        let mut c = Cur { v: &v, i: 0 };
        let pitch = c.next() as i32;
        let b = [c.next() as i32, c.next() as i32, c.next() as i32, c.next() as i32];
        let stamp0 = c.next();
        let rect = [c.next() as i32, c.next() as i32, c.next() as i32, c.next() as i32];
        let (self_mover, filt, nib) = (c.next(), c.next(), c.next());
        let ncells = c.next() as usize;
        let cells: Vec<u32> = (0..ncells).map(|_| c.next()).collect();
        let npool = c.next() as usize;
        let pool: Vec<Entry> = (0..npool).map(|_| Entry { w0: c.next(), node: c.next() }).collect();
        let nn = c.next() as usize;
        let nodes: Vec<(u32, u32, u32)> = (0..nn).map(|_| (c.next(), c.next(), c.next())).collect();
        let nm = c.next() as usize;
        let movers: Vec<(u32, u32, u32, u32, Option<u32>)> = (0..nm)
            .map(|_| {
                let (tag, o14, life, speed, has, rec) = (c.next(), c.next(), c.next(), c.next(), c.next(), c.next());
                (tag, o14, life, speed, if has != 0 { Some(rec) } else { None })
            })
            .collect();
        let nout = c.next() as usize;
        let want: Vec<u32> = (0..nout).map(|_| c.next()).collect();
        let stamp_end = c.next();
        let want_stamps: Vec<u32> = (0..nn).map(|_| c.next()).collect();

        let mut occ = Occupancy::new(pitch, (ncells as i32) / pitch);
        occ.bounds = b;
        occ.cells = cells;
        occ.pool = pool;
        occ.stamp = stamp0;
        occ.nodes = nodes.iter().map(|&(_, _, s)| NodeRec { stamp: s, refs: 0, rect: [0; 4] }).collect();
        let q = Query { filter: filt, nibble: nib };
        let got = occ.gather(rect, |n| {
            let (flags, mi, _) = nodes[n as usize];
            let m = movers[mi as usize];
            node_passes(
                &NodeFacts {
                    flags,
                    is_self: mi == self_mover,
                    tag_ok: m.0 == 0x6070_6375,
                    bit31: m.1 & 0x8000_0000 != 0,
                    radius: F32(m.2),
                    last_speed: F32(m.3),
                    rec: m.4,
                },
                &q,
            )
        });
        assert_eq!(got, want, "{line}");
        assert_eq!(occ.stamp, stamp_end, "{line}");
        let got_stamps: Vec<u32> = occ.nodes.iter().map(|n| n.stamp).collect();
        assert_eq!(got_stamps, want_stamps, "{line}");
        cases += 1;
        taken += got.len();
    }
    assert!(cases >= 200 && taken > 50, "{cases} {taken}");
}

#[test]
fn gather_box_matches_the_original() {
    let mut n = 0;
    for line in S.lines().filter(|l| l.starts_with("S ")) {
        let v = hexes(line);
        // cx cy vx vy t scale radius filt nib | px py r filt nib grid | rect*4
        let (cx, cy, vx, vy, t) = (F32(v[0]), F32(v[1]), F32(v[2]), F32(v[3]), F32(v[4]));
        let (scale, radius) = (F32(v[5]), F32(v[6]));
        // 0x6F38B0B0: the position at the elapsed value t
        let px = softfloat::arith::add(cx, softfloat::arith::mul(vx, t));
        let py = softfloat::arith::add(cy, softfloat::arith::mul(vy, t));
        assert_eq!([px.0, py.0], [v[9], v[10]], "{line}");
        let got = cell_box(px, py, radius, scale);
        let want = [v[15] as i32, v[16] as i32, v[17] as i32, v[18] as i32];
        assert_eq!(got, want, "{line}");
        n += 1;
    }
    assert!(n >= 200);
}

#[test]
fn footprint_box_matches_the_original() {
    let mut n = 0;
    for line in FV.lines().filter(|l| l.starts_with("F ")) {
        let v = hexes(line);
        let got = cell_box(F32(v[0]), F32(v[1]), F32(v[2]), F32(v[3]));
        let want = [v[4] as i32, v[5] as i32, v[6] as i32, v[7] as i32];
        assert_eq!(got, want, "{line}");
        n += 1;
    }
    assert!(n >= 200);
}

#[test]
fn occupant_maintenance_matches_the_original() {
    let (mut cases, mut gathered, mut destroyed) = (0, 0, 0);
    for line in O.lines().filter(|l| l.starts_with("O ")) {
        let mut tok = line.split_whitespace().skip(1);
        let mut hex = || u32::from_str_radix(tok.next().expect("token"), 16).expect("hex");
        let pitch = hex() as i32;
        let rows = hex() as i32;
        let nn = hex() as usize;
        let nops = hex() as usize;
        // the ops are letters, so re-read the line as strings
        let mut occ = Occupancy::new(pitch, rows);
        for _ in 0..nn {
            occ.add_node();
        }
        let mut got_out: Vec<Vec<u32>> = Vec::new();
        let mut toks: Vec<&str> = line.split_whitespace().skip(5).collect();
        toks.reverse();
        let pop = |t: &mut Vec<&str>| -> String { t.pop().expect("token").to_string() };
        let num = |t: &mut Vec<&str>| -> u32 { u32::from_str_radix(&pop(t), 16).expect("hex") };
        for _ in 0..nops {
            match pop(&mut toks).as_str() {
                "M" => {
                    let n = num(&mut toks);
                    let r = [num(&mut toks) as i32, num(&mut toks) as i32, num(&mut toks) as i32, num(&mut toks) as i32];
                    occ.move_footprint(n, r);
                }
                "C" => occ.clean_dirty(),
                "K" => {
                    let n = num(&mut toks);
                    occ.kill_node(n);
                }
                "G" => {
                    let r = [num(&mut toks) as i32, num(&mut toks) as i32, num(&mut toks) as i32, num(&mut toks) as i32];
                    got_out.push(occ.gather(r, |_| true));
                }
                other => panic!("op {other}"),
            }
        }
        let ng = num(&mut toks) as usize;
        assert_eq!(ng, got_out.len(), "{line}");
        for g in &got_out {
            let n = num(&mut toks) as usize;
            let want: Vec<u32> = (0..n).map(|_| num(&mut toks)).collect();
            assert_eq!(g, &want, "{line}");
            gathered += n;
        }
        let ncells = num(&mut toks) as usize;
        let cells: Vec<u32> = (0..ncells).map(|_| num(&mut toks)).collect();
        assert_eq!(occ.cells, cells, "{line}");
        let npool = num(&mut toks) as usize;
        let pool: Vec<Entry> = (0..npool).map(|_| Entry { w0: num(&mut toks), node: num(&mut toks) }).collect();
        assert_eq!(occ.pool, pool, "{line}");
        assert_eq!((occ.free, occ.live, occ.stamp), (num(&mut toks), num(&mut toks), num(&mut toks)), "{line}");
        let nw = num(&mut toks) as usize;
        let bm: Vec<u32> = (0..nw).map(|_| num(&mut toks)).collect();
        assert_eq!(occ.dirty, bm, "{line}");
        for n in 0..nn {
            let (stamp, refs) = (num(&mut toks), num(&mut toks));
            let rect = [num(&mut toks) as i32, num(&mut toks) as i32, num(&mut toks) as i32, num(&mut toks) as i32];
            assert_eq!(occ.nodes[n], NodeRec { stamp, refs, rect }, "node {n} {line}");
        }
        let nd = num(&mut toks) as usize;
        let want_destroyed: Vec<u32> = (0..nd).map(|_| num(&mut toks)).collect();
        assert_eq!(occ.destroyed, want_destroyed, "{line}");
        destroyed += nd;
        assert!(toks.is_empty(), "{line}");
        cases += 1;
    }
    let _ = (NIL, &mut destroyed);
    assert!(cases >= 200 && gathered > 20, "{cases} {gathered} {destroyed}");
}
