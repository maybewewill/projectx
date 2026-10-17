//! Unit push-apart in the movement walk (`docs/spec/movement-7.md` sections 5 and 8).
//!
//! The original keeps a **record** per unit whose type has `repulse` set; the movement walk `0x6F483300` calls the
//! push-apart kernel (`crate::mover::push_apart`, CONFIRMED) for every second record of the world list, alternating,
//! after the mover walks and the facing smoothers. The kernel asks a host for the neighbours of the unit: the units
//! whose footprints lie in the cells of the box `position +- table radius`, walked as in
//! [`crate::occupancy`] (CONFIRMED by Unicorn runs of the real routines).
//!
//! What this module decides on its own (UNSURE, flagged in the spec):
//!
//! * The record list order: a new record is linked at the **head** (`0x6F493830`, CONFIRMED), records are created in
//!   unit key order at the first walk after the unit exists.
//! * The footprint radius of a unit (mover `+0x90`) is its type's `collision` / 32 cells, and the footprints are
//!   registered with the occupant lists at the start of each walk, units in key order. The original registers a
//!   footprint when the position object changes (`0x6F4A6D70` after each rebase), a finer cadence.
//! * The dirty cells are cleaned once per walk, before the push records run (the original cleans from a queue
//!   callback, `0x6F46E5F0`, whose timing is not decoded).
//! * The high nibble of the filter byte (`0x6F29E8F0`: result of the unit's virtual slot `0xEC`, or `0xF` when the
//!   unit flag `+0x60` bit 0 is set) is taken as 0.
//! * The placement test `0x6F494E10` (size class and pathing probe `0x6F4A3810`) is replaced by "the target cell is
//!   inside the grid".

use std::collections::BTreeMap;

use softfloat::{arith, conv, math, F32, Tables};

use crate::mover::{self, push_apart, PushHost, PushRecord};
use crate::natives::game::{Game, ObjKey, Object};
use crate::occupancy::{cell_box, node_passes, NodeFacts, Occupancy, Query};
use crate::pathing::grid::scale_by_1_32;

/// The footprint scale `[grid+0x68]`: 1.0 (set by the grid initialiser `0x6F49F5D0` from `0x6FAAE4F4`).
const ONE: F32 = F32::from_bits(0x3F80_0000);

/// The record flag word of a unit type (`0x6F495ED0` with the arguments of `0x6F2A0830`): the table row, the filter
/// word `(slot 0xEC << 4) | repulseGroup` and the priority nibble, composed by the three setters `0x6F494FB0`,
/// `0x6F494FD0`, `0x6F494FF0` in that order. CONFIRMED (disassembly).
pub fn record_flags(repulse: [i32; 4], class_nibble: u32) -> u32 {
    let row = repulse[1] as u8 as u32;
    let word = ((class_nibble & 0xF) << 4 | (repulse[2] as u32 & 0xF)) as u16 as u32;
    let nib = repulse[3] as u8 as u32;
    let mut f = 0u32;
    f = (f & 0xFFF0_FFFF) | (row << 16);
    f = (f & 0xF00F_FFFF) | (word << 20);
    f = (f & 0x0FFF_FFFF) | (nib << 28);
    f
}

/// The push-apart state of a game.
#[derive(Debug, Default)]
pub struct PushState {
    /// The occupant lists; `None` until the host enables push-apart with the grid size.
    occ: Option<Occupancy>,
    /// The record list in walk order (head first).
    records: Vec<(ObjKey, PushRecord)>,
    /// Unit -> occupant node.
    nodes: BTreeMap<ObjKey, u32>,
    /// Occupant node -> unit.
    node_unit: Vec<ObjKey>,
    /// `world+0x53C`: toggles 1, 0, 1, 0, ... on each walk.
    parity: u32,
    /// Units seen (registered or rejected): a unit is looked at once.
    seen: BTreeMap<ObjKey, bool>,
}

impl PushState {
    /// The records, head first (for tests).
    pub fn records(&self) -> &[(ObjKey, PushRecord)] {
        &self.records
    }

    /// The occupant lists (for tests).
    pub fn occupancy(&self) -> Option<&Occupancy> {
        self.occ.as_ref()
    }
}

impl Game {
    /// Turns the push-apart on for a pathing grid of `pitch` x `rows` cells (the host calls it with the map's pathing
    /// size). Without it units are never pushed.
    pub fn enable_push_apart(&mut self, pitch: i32, rows: i32) {
        self.push.occ = Some(Occupancy::new(pitch, rows));
    }

    /// The number of push records (units with `repulse`).
    pub fn push_state_records(&self) -> usize {
        self.push.records.len()
    }

    /// Whether push-apart is on.
    pub fn push_apart_enabled(&self) -> bool {
        self.push.occ.is_some()
    }

    /// The push-apart pass of one movement walk (`0x6F483300`, after the walks and the smoothers).
    pub(crate) fn push_walk(&mut self, t: &Tables) {
        let Some(mut occ) = self.push.occ.take() else {
            return;
        };
        self.push_register_new(&mut occ);
        self.push_update_footprints(&mut occ);
        occ.clean_dirty();
        self.push.parity = if self.push.parity == 0 { 1 } else { 0 };
        let parity = self.push.parity;
        let order: Vec<ObjKey> = self.push.records.iter().map(|r| r.0).collect();
        for (i, unit) in order.into_iter().enumerate() {
            // CONFIRMED (0x6F4833A8..): the call happens for the records whose index plus the counter is even
            if !(i as u32 + parity).is_multiple_of(2) {
                continue;
            }
            let Some(slot) = self.push.records.iter().position(|r| r.0 == unit) else {
                continue;
            };
            let mut rec = self.push.records[slot].1;
            let last_speed = self.movers.get(&unit).map_or(F32::ZERO, |m| m.last_speed);
            {
                let mut host = GameHost { game: self, occ: &mut occ, unit, t };
                push_apart(&mut rec, last_speed, &mut host, t);
            }
            if let Some(s) = self.push.records.iter_mut().find(|r| r.0 == unit) {
                s.1 = rec;
            }
        }
        self.push.occ = Some(occ);
    }

    /// Looks at units not seen yet, in key order: a live unit whose type has `repulse` gets a node and a record at the
    /// head of the list.
    fn push_register_new(&mut self, occ: &mut Occupancy) {
        for i in 0..self.object_count() {
            let key = ObjKey(i as u32);
            if self.push.seen.contains_key(&key) {
                continue;
            }
            let Object::Unit(u) = self.object(key) else {
                continue;
            };
            if u.dead || u.removed {
                continue;
            }
            let rep = self.unit_types().get(u.type_id).map_or([0; 4], |ty| ty.repulse);
            self.push.seen.insert(key, rep[0] != 0);
            if rep[0] == 0 {
                continue;
            }
            let node = occ.add_node();
            self.push.nodes.insert(key, node);
            self.push.node_unit.push(key);
            let rec = PushRecord { flags: record_flags(rep, 0), vx: F32::ZERO, vy: F32::ZERO };
            self.push.records.insert(0, (key, rec));
        }
    }

    /// Moves the footprints of the registered units to their evaluated positions; dead or removed units are marked dead
    /// and leave the record list.
    fn push_update_footprints(&mut self, occ: &mut Occupancy) {
        let units: Vec<(ObjKey, u32)> = self.push.nodes.iter().map(|(&k, &n)| (k, n)).collect();
        let g = self.layer();
        for (key, node) in units {
            let (gone, type_id) = match self.object(key) {
                Object::Unit(u) => (u.dead || u.removed, u.type_id),
                _ => (true, 0),
            };
            if gone {
                occ.kill_node(node);
                self.push.nodes.remove(&key);
                self.push.records.retain(|r| r.0 != key);
                continue;
            }
            let radius = self.footprint_radius(type_id);
            let (x, y) = self.kin_of(key).position(&g);
            occ.move_footprint(node, cell_box(x, y, radius, ONE));
        }
    }

    /// The footprint radius in cells (UNSURE: type `collision` / 32).
    fn footprint_radius(&self, type_id: u32) -> F32 {
        let c = self.unit_types().get(type_id).map_or(F32::ZERO, |ty| ty.collision);
        scale_by_1_32(c)
    }
}

/// The world services of the kernel, answered from the game.
struct GameHost<'a> {
    game: &'a mut Game,
    occ: &'a mut Occupancy,
    unit: ObjKey,
    t: &'a Tables,
}

impl PushHost for GameHost<'_> {
    fn position(&mut self) -> (F32, F32) {
        let g = self.game.layer();
        self.game.kin_of(self.unit).position(&g)
    }

    fn place_ok(&mut self, target: (F32, F32)) -> bool {
        let cx = conv::f2i(math::floor(target.0));
        let cy = conv::f2i(math::floor(target.1));
        cx >= 0 && cy >= 0 && cx < self.occ.pitch && cy < self.occ.rows
    }

    fn move_to(&mut self, target: (F32, F32)) {
        let g = self.game.layer();
        let mut kin = self.game.kin_of(self.unit);
        let pos = kin.position(&g);
        let delta = (arith::sub(target.0, pos.0), arith::sub(target.1, pos.1));
        kin.rebase(&g, delta);
        self.game.commit_kin(self.unit, &kin);
    }

    fn gather(&mut self, class: u32, rank: u32, radius: F32) -> Vec<(F32, F32)> {
        let (x, y) = self.position();
        let rect = cell_box(x, y, radius, ONE);
        let query = Query { filter: rank, nibble: class };
        let game: &Game = self.game;
        let own = self.unit;
        let facts = |node: u32| -> bool {
            let key = game.push.node_unit[node as usize];
            let Object::Unit(u) = game.object(key) else {
                return false;
            };
            let rec = game.push.records.iter().find(|r| r.0 == key).map(|r| r.1.flags);
            let f = NodeFacts {
                flags: 0,
                is_self: key == own,
                tag_ok: !(u.dead || u.removed),
                bit31: false,
                radius: game.footprint_radius(u.type_id),
                last_speed: game.movers.get(&key).map_or(F32::ZERO, |m| m.last_speed),
                rec,
            };
            node_passes(&f, &query)
        };
        let nodes = self.occ.gather(rect, facts);
        let g = self.game.layer();
        nodes
            .into_iter()
            .map(|n| {
                let key = self.game.push.node_unit[n as usize];
                self.game.kin_of(key).position(&g)
            })
            .collect()
    }

    fn random_direction(&mut self) -> (F32, F32) {
        match self.game.rng_parts() {
            Some((table, streams)) => {
                crate::rng::trace::note(|| "push-apart".into());
                let u = streams.global.random_fraction(table);
                mover::direction_of(u, self.t)
            }
            None => (F32::ZERO, F32::ZERO),
        }
    }
}
