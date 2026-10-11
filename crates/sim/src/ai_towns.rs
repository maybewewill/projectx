//! The AI town model: map-wide town sites and the four town slots of each AI.
//!
//! Structure (CONFIRMED from the bodies of `GetMinesOwned` `0x6F2DDFD0`, `GetGoldOwned` `0x6F2DE000`, `TownWithMine`
//! `0x6F2DE030`, `TownHasMine` `0x6F2DE060`, `TownHasHall` `0x6F2DE0A0` and their helpers `0x6F2D11A0`, `0x6F2D1210`,
//! `0x6F2D5380`, `0x6F2D12B0`, `0x6F2D1310`):
//!
//! * The engine keeps one map-wide list of **town sites** (`0x6FAB4E90`, count `0x6FAB4E74`). A site holds a hall
//!   reference (record `+0x48`) and up to two mine references (`+0x54` and `+0x60`). A reference is an (id, tag)
//!   pair; it "resolves" when the agent exists and its removed flag (`+0x20`) is clear. A dead unit still resolves.
//! * Each AI has four **town slots** (AI state `+0x1B8`..`+0x1C4`). A slot is `-1` (empty) or an index into the
//!   site list. A slot index above 3 is never valid (the helpers test `slot > 3` first).
//! * Adding a site to an AI (`0x6F2D5330`): if the AI already has the site in a slot, that slot is returned;
//!   otherwise the first empty slot takes it; when all four are taken nothing is stored (CONFIRMED).
//! * A gold mine is a unit with the mine ability `Agmb` (`0x6F082470` returns its code, `0x41676D62`). Registering
//!   a mine (`0x6F2E5030` → `0x6F2E4FC0`/`0x6F2E37E0`): the first mine of a site goes to the `+0x54` reference;
//!   a second, different live unit goes to `+0x60`.
//! * Registering a town hall (`0x6F2DBB90`, for units with class bit `0x40`): the unit goes to the hall reference
//!   of its site, replacing an earlier live hall, and the site is added to the owner's slots. A haunted mine
//!   (`ugol`) of an AI owner also adds its site to the slots (`0x6F2DF280`).
//!
//! What is **not** decoded and is therefore UNSURE here: *when* the engine registers units (the model registers
//! lazily, in object order, when a town native runs), and *which site* a unit joins. The original asks the path
//! cost routine `0x6F4758D0` for the cost from the site (limit 80 for a hall search, 150 for the mine search) and
//! takes the cheapest below the limit; the model takes the nearest site within [`SITE_RADIUS`] world units
//! (straight-line distance) instead. Positions only decide grouping; the natives below read references only.

use std::collections::{BTreeMap, BTreeSet};

use crate::natives::game::ObjKey;

/// How far (world units, straight line) a unit may be from a site's anchor and still join it (UNSURE: stands in
/// for the path-cost limit of the original).
pub const SITE_RADIUS: f32 = 1024.0;

/// Number of town slots per AI (CONFIRMED: the helpers loop over 4 and reject indices above 3).
pub const TOWN_SLOTS: usize = 4;

/// An empty slot.
pub const NO_TOWN: i32 = -1;

/// The class bit of a town hall in the unit classification word (`+0x248`): `UNIT_TYPE_TOWNHALL`.
pub const CLASS_TOWNHALL: u32 = 0x40;

/// The mine type codes the model knows: neutral gold mine, haunted gold mine, entangled gold mine ('egol').
pub const GOLD_MINE: u32 = 0x6E67_6F6C; // 'ngol'
/// See [`GOLD_MINE`].
pub const HAUNTED_MINE: u32 = 0x7567_6F6C; // 'ugol'
/// See [`GOLD_MINE`].
pub const ENTANGLED_MINE: u32 = 0x6567_6F6C; // 'egol'

/// One town site.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Site {
    /// The anchor position (the first unit that created the site).
    pub x: f32,
    pub y: f32,
    /// The hall reference (`+0x48`).
    pub hall: Option<ObjKey>,
    /// The mine references (`+0x54`, `+0x60`).
    pub mines: [Option<ObjKey>; 2],
}

/// The unit data the registration needs, filled by the caller from the game.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitInfo {
    pub key: ObjKey,
    pub type_id: u32,
    pub owner: u32,
    pub x: f32,
    pub y: f32,
    pub class_flags: u32,
    /// The unit carries a gold amount (is a mine).
    pub is_mine: bool,
}

/// The town sites and the town slots of every AI.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Towns {
    pub sites: Vec<Site>,
    slots: BTreeMap<u32, [i32; TOWN_SLOTS]>,
    /// Objects below this index were registered already.
    scanned: usize,
}

impl Towns {
    /// The slots of `player` (all empty when the player has none).
    pub fn slots(&self, player: u32) -> [i32; TOWN_SLOTS] {
        self.slots
            .get(&player)
            .copied()
            .unwrap_or([NO_TOWN; TOWN_SLOTS])
    }

    /// The site in slot `slot` of `player`, with the original's gates: slot above 3 or empty → none.
    pub fn site_of_slot(&self, player: u32, slot: i64) -> Option<&Site> {
        if !(0..TOWN_SLOTS as i64).contains(&slot) {
            return None;
        }
        let idx = self.slots(player)[slot as usize];
        usize::try_from(idx).ok().and_then(|i| self.sites.get(i))
    }

    /// How many objects the caller has already registered.
    pub fn scanned(&self) -> usize {
        self.scanned
    }

    /// Records that objects below `n` were looked at.
    pub fn set_scanned(&mut self, n: usize) {
        self.scanned = n;
    }

    /// `0x6F2D5330`: the slot of `site` for `player`, taking the first empty slot when it is not there. Returns the
    /// slot index; 4 when all slots are taken by other sites.
    pub fn add_slot(&mut self, player: u32, site: usize) -> usize {
        let slots = self.slots.entry(player).or_insert([NO_TOWN; TOWN_SLOTS]);
        let site = site as i32;
        if let Some(i) = slots.iter().position(|&s| s == site) {
            return i;
        }
        match slots.iter().position(|&s| s == NO_TOWN) {
            Some(i) => {
                slots[i] = site;
                i
            }
            None => TOWN_SLOTS,
        }
    }

    /// The nearest site within [`SITE_RADIUS`] of (x, y); ties go to the lower index.
    fn nearest(&self, x: f32, y: f32) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (i, s) in self.sites.iter().enumerate() {
            let d = ((s.x - x) * (s.x - x) + (s.y - y) * (s.y - y)).sqrt();
            if d <= SITE_RADIUS && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    }

    fn site_for(&mut self, x: f32, y: f32) -> usize {
        match self.nearest(x, y) {
            Some(i) => i,
            None => {
                self.sites.push(Site {
                    x,
                    y,
                    ..Site::default()
                });
                self.sites.len() - 1
            }
        }
    }

    /// Registers a gold mine (`0x6F2E5030`). `live` tells whether a reference still resolves.
    pub fn add_mine(&mut self, u: &UnitInfo, live: &dyn Fn(ObjKey) -> bool) -> usize {
        let i = self.site_for(u.x, u.y);
        let site = &mut self.sites[i];
        let first_live = site.mines[0].filter(|&k| live(k));
        match first_live {
            Some(k) if k != u.key => site.mines[1] = Some(u.key),
            Some(_) => {}
            None => site.mines[0] = Some(u.key),
        }
        i
    }

    /// Registers a town hall of `u.owner`'s AI (`0x6F2DBB90` → `0x6F2E5060`): the hall reference of its site, then
    /// the owner's slot.
    pub fn add_hall(&mut self, u: &UnitInfo) -> usize {
        let i = self.site_for(u.x, u.y);
        self.sites[i].hall = Some(u.key);
        self.add_slot(u.owner, i);
        i
    }

    /// A haunted mine of an AI owner (`0x6F2DF280`): the nearest site that has a mine takes a slot of the owner.
    pub fn add_haunted_slot(&mut self, u: &UnitInfo) {
        let mut best: Option<(usize, f32)> = None;
        for (i, s) in self.sites.iter().enumerate() {
            if s.mines[0].is_none() {
                continue;
            }
            let d = ((s.x - u.x) * (s.x - u.x) + (s.y - u.y) * (s.y - u.y)).sqrt();
            if d <= SITE_RADIUS && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        if let Some((i, _)) = best {
            self.add_slot(u.owner, i);
        }
    }

    /// Registers `units` (in the order given) for the AI players in `ai_players`. Mines always register; halls and
    /// haunted mines only for a unit owned by an AI player.
    pub fn register(
        &mut self,
        units: &[UnitInfo],
        ai_players: &BTreeSet<u32>,
        live: &dyn Fn(ObjKey) -> bool,
    ) {
        for u in units {
            if u.is_mine {
                self.add_mine(u, live);
                if u.type_id == HAUNTED_MINE && ai_players.contains(&u.owner) {
                    self.add_haunted_slot(u);
                }
            } else if u.class_flags & CLASS_TOWNHALL != 0 && ai_players.contains(&u.owner) {
                self.add_hall(u);
            }
        }
    }

    // ---- The five town natives (CONFIRMED bodies; the registration above is UNSURE) ----

    /// `TownHasHall(i)` `0x6F2D1310`.
    pub fn town_has_hall(&self, player: u32, slot: i64, live: &dyn Fn(ObjKey) -> bool) -> bool {
        self.site_of_slot(player, slot)
            .and_then(|s| s.hall)
            .is_some_and(live)
    }

    /// `TownHasMine(i)` `0x6F2D12B0`: either mine reference resolves.
    pub fn town_has_mine(&self, player: u32, slot: i64, live: &dyn Fn(ObjKey) -> bool) -> bool {
        self.site_of_slot(player, slot)
            .is_some_and(|s| s.mines.iter().flatten().any(|&k| live(k)))
    }

    /// `GetMinesOwned` `0x6F2D11A0`: the live mine references over the four slots' sites. A site listed in two slots
    /// counts twice (CONFIRMED: the loop visits slots, not distinct sites).
    pub fn mines_owned(&self, player: u32, live: &dyn Fn(ObjKey) -> bool) -> i32 {
        let mut n = 0;
        for slot in 0..TOWN_SLOTS as i64 {
            if let Some(s) = self.site_of_slot(player, slot) {
                n += s.mines.iter().flatten().filter(|&&k| live(k)).count() as i32;
            }
        }
        n
    }

    /// `GetGoldOwned` `0x6F2D1210`: the sum of `gold(mine)` over the live mine references of the four slots' sites.
    pub fn gold_owned(
        &self,
        player: u32,
        live: &dyn Fn(ObjKey) -> bool,
        gold: &dyn Fn(ObjKey) -> i32,
    ) -> i32 {
        let mut total = 0i32;
        for slot in 0..TOWN_SLOTS as i64 {
            if let Some(s) = self.site_of_slot(player, slot) {
                for &k in s.mines.iter().flatten().filter(|&&k| live(k)) {
                    total = total.wrapping_add(gold(k));
                }
            }
        }
        total
    }

    /// `TownWithMine` `0x6F2D5380` (Unicorn-checked, `tools/emu/ai_towns_emu.py`). `race` is the player's race value
    /// (1 human, 2 orc, 3 undead, 4 night elf); `type_of` gives a unit's type code; `building` tells whether the unit
    /// has the flag `0x2000000` of its word `+0x5C` (set while a structure is under construction, `construction.md`).
    /// The first slot (0..3) whose first mine reference resolves wins when:
    /// * human or orc: the hall reference resolves and the hall is not under construction (the mine's flag is not read);
    /// * any other race: the mine is not under construction and, for the night elf (4), is an entangled gold mine.
    ///
    /// Returns -1 when no slot qualifies.
    pub fn town_with_mine(
        &self,
        player: u32,
        race: u32,
        live: &dyn Fn(ObjKey) -> bool,
        type_of: &dyn Fn(ObjKey) -> u32,
        building: &dyn Fn(ObjKey) -> bool,
    ) -> i32 {
        for slot in 0..TOWN_SLOTS {
            let Some(site) = self.site_of_slot(player, slot as i64) else {
                continue;
            };
            let Some(mine) = site.mines[0].filter(|&k| live(k)) else {
                continue;
            };
            let ok = match race {
                1 | 2 => site.hall.is_some_and(|h| live(h) && !building(h)),
                4 => !building(mine) && type_of(mine) == ENTANGLED_MINE,
                _ => !building(mine),
            };
            if ok {
                return slot as i32;
            }
        }
        -1
    }

    /// A town state built directly (tests and vector replays): the sites, and the slots of `player`.
    pub fn from_state(sites: Vec<Site>, player: u32, slots: [i32; TOWN_SLOTS]) -> Towns {
        let mut t = Towns {
            sites,
            ..Towns::default()
        };
        t.slots.insert(player, slots);
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mine(k: u32, x: f32, y: f32) -> UnitInfo {
        UnitInfo {
            key: ObjKey(k),
            type_id: GOLD_MINE,
            owner: 15,
            x,
            y,
            class_flags: 0,
            is_mine: true,
        }
    }
    fn hall(k: u32, owner: u32, x: f32, y: f32) -> UnitInfo {
        UnitInfo {
            key: ObjKey(k),
            type_id: 0x6874_6F77,
            owner,
            x,
            y,
            class_flags: CLASS_TOWNHALL,
            is_mine: false,
        }
    }
    fn all_live(_: ObjKey) -> bool {
        true
    }
    fn ai(p: u32) -> BTreeSet<u32> {
        [p].into_iter().collect()
    }

    #[test]
    fn hall_and_mine_share_a_site_and_the_hall_takes_slot_zero() {
        let mut t = Towns::default();
        t.register(
            &[
                mine(1, 0.0, 0.0),
                mine(2, 5000.0, 0.0),
                hall(3, 1, 400.0, 0.0),
            ],
            &ai(1),
            &all_live,
        );
        assert_eq!(t.sites.len(), 2);
        assert_eq!(t.slots(1), [0, -1, -1, -1]);
        assert!(t.town_has_hall(1, 0, &all_live));
        assert!(t.town_has_mine(1, 0, &all_live));
        assert_eq!(t.mines_owned(1, &all_live), 1);
        assert_eq!(
            t.town_with_mine(1, 1, &all_live, &|_| GOLD_MINE, &|_| false),
            0
        );
    }

    #[test]
    fn slot_gates_follow_the_original() {
        let mut t = Towns::default();
        t.register(
            &[mine(1, 0.0, 0.0), hall(3, 1, 100.0, 0.0)],
            &ai(1),
            &all_live,
        );
        assert!(!t.town_has_hall(1, 4, &all_live));
        assert!(!t.town_has_hall(1, -1, &all_live));
        assert!(!t.town_has_hall(1, 1, &all_live));
        assert!(
            !t.town_has_hall(2, 0, &all_live),
            "another player has no slots"
        );
    }

    #[test]
    fn add_slot_reuses_then_fills_then_stops_at_four() {
        let mut t = Towns::default();
        assert_eq!(t.add_slot(0, 7), 0);
        assert_eq!(t.add_slot(0, 7), 0);
        assert_eq!(t.add_slot(0, 8), 1);
        assert_eq!(t.add_slot(0, 9), 2);
        assert_eq!(t.add_slot(0, 10), 3);
        assert_eq!(t.add_slot(0, 11), 4);
        assert_eq!(t.slots(0), [7, 8, 9, 10]);
    }

    #[test]
    fn second_mine_takes_the_second_reference_and_removed_mines_do_not_count() {
        let mut t = Towns::default();
        t.register(
            &[
                mine(1, 0.0, 0.0),
                mine(2, 10.0, 0.0),
                hall(3, 1, 200.0, 0.0),
            ],
            &ai(1),
            &all_live,
        );
        assert_eq!(t.mines_owned(1, &all_live), 2);
        let gold = |k: ObjKey| if k == ObjKey(1) { 12500 } else { 300 };
        assert_eq!(t.gold_owned(1, &all_live, &gold), 12800);
        let live = |k: ObjKey| k != ObjKey(1);
        assert_eq!(t.mines_owned(1, &live), 1);
        assert_eq!(
            t.town_with_mine(1, 1, &live, &|_| GOLD_MINE, &|_| false),
            -1,
            "first mine gone"
        );
    }

    #[test]
    fn race_rules_of_town_with_mine() {
        let mut t = Towns::default();
        t.register(
            &[mine(1, 0.0, 0.0), hall(3, 1, 200.0, 0.0)],
            &ai(1),
            &all_live,
        );
        let no_hall = |k: ObjKey| k != ObjKey(3);
        assert_eq!(
            t.town_with_mine(1, 2, &no_hall, &|_| GOLD_MINE, &|_| false),
            -1
        );
        assert_eq!(
            t.town_with_mine(1, 3, &no_hall, &|_| HAUNTED_MINE, &|_| false),
            0,
            "undead: the mine alone"
        );
        assert_eq!(
            t.town_with_mine(1, 4, &all_live, &|_| GOLD_MINE, &|_| false),
            -1,
            "elf: needs the entangled mine"
        );
        assert_eq!(
            t.town_with_mine(1, 4, &all_live, &|_| ENTANGLED_MINE, &|_| false),
            0
        );
    }

    #[test]
    fn halls_of_a_non_ai_player_are_not_registered() {
        let mut t = Towns::default();
        t.register(
            &[mine(1, 0.0, 0.0), hall(3, 2, 200.0, 0.0)],
            &ai(1),
            &all_live,
        );
        assert_eq!(t.slots(2), [-1; 4]);
        assert!(t.sites[0].hall.is_none());
    }
}
