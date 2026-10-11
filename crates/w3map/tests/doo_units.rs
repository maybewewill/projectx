//! Synthetic fixtures for the Reign of Chaos doodad layout and the TFT unit layout.
//! No game data is used.

use w3map::{Doodads, Error, PlacedUnits, UnitItem};

#[derive(Default)]
struct B(Vec<u8>);

impl B {
    fn raw(mut self, b: &[u8]) -> Self {
        self.0.extend_from_slice(b);
        self
    }
    fn u8(self, v: u8) -> Self {
        self.raw(&[v])
    }
    fn u16(self, v: u16) -> Self {
        self.raw(&v.to_le_bytes())
    }
    fn i32(self, v: i32) -> Self {
        self.raw(&v.to_le_bytes())
    }
    fn u32(self, v: u32) -> Self {
        self.raw(&v.to_le_bytes())
    }
    fn f32(self, v: f32) -> Self {
        self.raw(&v.to_le_bytes())
    }
    fn done(self) -> Vec<u8> {
        self.0
    }
}

/// One Reign of Chaos doodad record (42 bytes, no dropped item sets, no editor id).
#[test]
fn doodad_reign_of_chaos_record() {
    let bytes = B::default()
        .raw(b"W3do")
        .u32(7)
        .u32(9)
        .u32(1)
        .raw(b"LTlt")
        .u32(4)
        .f32(-9088.0)
        .f32(-4160.0)
        .f32(292.0)
        .f32(4.7)
        .f32(1.0)
        .f32(1.0)
        .f32(1.0)
        .u8(0x02)
        .u8(100)
        .u32(0xFFFF_FFFF)
        .u32(0)
        .u32(0)
        .done();
    let d = Doodads::parse(&bytes).unwrap();
    assert_eq!(d.doodads.len(), 1);
    let a = &d.doodads[0];
    assert_eq!(a.id, *b"LTlt");
    assert_eq!(a.variation, 4);
    assert_eq!(a.position, [-9088.0, -4160.0, 292.0]);
    assert_eq!(a.life, 100);
    assert_eq!(a.item_table_ptr, 0xFFFF_FFFF);
    assert!(a.dropped_item_sets.is_empty());
    assert_eq!(a.editor_id, 0);
    assert!(d.special.is_empty());
}

/// Units: TFT record (111 fixed bytes) with one item and one ability.
fn unit_bytes(random: u32, version: u32) -> Vec<u8> {
    B::default()
        .raw(b"W3do")
        .u32(version)
        .u32(11)
        .u32(1)
        .raw(b"ngol")
        .u32(0)
        .f32(-768.0)
        .f32(4544.0)
        .f32(514.0)
        .f32(4.71)
        .f32(1.0)
        .f32(1.0)
        .f32(1.0)
        .u8(2)
        .u32(15)
        .u16(0)
        .i32(-1)
        .i32(-1)
        .i32(-1)
        .u32(0)
        .u32(12500)
        .f32(-1.0)
        .u32(1)
        .u32(3)
        .u32(4)
        .u32(5)
        .u32(1)
        .u32(2)
        .raw(b"ratf")
        .u32(1)
        .raw(b"AHhb")
        .u32(1)
        .u32(3)
        .u32(random)
        .u32(1)
        .u32(0xFFFF_FFFF)
        .u32(0xFFFF_FFFF)
        .u32(7)
        .done()
}

#[test]
fn unit_tft_record_fields() {
    let u = PlacedUnits::parse(&unit_bytes(0, 8)).unwrap();
    assert_eq!(u.units.len(), 1);
    let x = &u.units[0];
    assert_eq!(x.id, *b"ngol");
    assert_eq!(x.position, [-768.0, 4544.0, 514.0]);
    assert_eq!(x.flags, 2);
    assert_eq!(x.player, 15);
    assert_eq!((x.hit_points, x.mana, x.item_table_ptr), (-1, -1, -1));
    assert_eq!(x.gold, 12500);
    assert_eq!(x.target_acquisition, -1.0);
    assert_eq!((x.hero_level, x.strength, x.agility, x.intelligence), (1, 3, 4, 5));
    assert_eq!(x.items, vec![UnitItem { slot: 2, id: *b"ratf" }]);
    assert_eq!(x.abilities.len(), 1);
    assert_eq!(x.abilities[0].id, *b"AHhb");
    assert_eq!((x.abilities[0].active, x.abilities[0].level), (1, 3));
    assert_eq!(x.unknown_tail, [1, 0xFFFF_FFFF, 0xFFFF_FFFF]);
    assert_eq!(x.editor_id, 7);
}

#[test]
fn unit_rejects_random_flag_and_reign_of_chaos() {
    assert!(matches!(
        PlacedUnits::parse(&unit_bytes(1, 8)),
        Err(Error::InvalidValue { .. })
    ));
    assert!(matches!(
        PlacedUnits::parse(&unit_bytes(0, 7)),
        Err(Error::UnsupportedVersion { found: 7, .. })
    ));
}

#[test]
fn unit_rejects_truncated_record() {
    let bytes = unit_bytes(0, 8);
    assert!(PlacedUnits::parse(&bytes[..bytes.len() - 3]).is_err());
}
