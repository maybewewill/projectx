//! Synthetic map files built in the documented layouts. No game data is used.

use w3map::{Doodads, Error, MapInfo, PathMap, ShadowMap, Terrain};

/// Little-endian byte builder for test fixtures.
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
    fn i16(self, v: i16) -> Self {
        self.raw(&v.to_le_bytes())
    }
    fn u32(self, v: u32) -> Self {
        self.raw(&v.to_le_bytes())
    }
    fn i32(self, v: i32) -> Self {
        self.raw(&v.to_le_bytes())
    }
    fn f32(self, v: f32) -> Self {
        self.raw(&v.to_le_bytes())
    }
    /// Null-terminated string.
    fn s(self, v: &str) -> Self {
        self.raw(v.as_bytes()).raw(&[0])
    }
    fn done(self) -> Vec<u8> {
        self.0
    }
}

fn w3e_bytes() -> Vec<u8> {
    B::default()
        .raw(b"W3E!")
        .u32(11)
        .u8(b'L')
        .u32(1) // custom tileset
        .u32(2)
        .raw(b"Ldrt")
        .raw(b"Lgrs")
        .u32(1)
        .raw(b"Xbtt") // cliff tilesets
        .u32(2) // width in points
        .u32(2) // height in points
        .f32(1.5)
        .f32(-2.25)
        // (0,0): ground -5, water 0x1234 with map-edge bit, flags 0xA5, detail 0xB2, cliff 0xA3
        .i16(-5)
        .u16(0x4000 | 0x1234)
        .u8(0xA5)
        .u8(0xB2)
        .u8(0xA3)
        // (1,0)
        .i16(10)
        .u16(0)
        .u8(0)
        .u8(0)
        .u8(0)
        // (0,1): water flag and ramp flag set, ground texture 1
        .i16(-300)
        .u16(0x0040)
        .u8(0x51)
        .u8(0x01)
        .u8(0x10)
        // (1,1)
        .i16(7)
        .u16(0)
        .u8(0)
        .u8(0)
        .u8(0)
        .done()
}

#[test]
fn terrain_header_and_tile_fields() {
    let t = Terrain::parse(&w3e_bytes()).unwrap();
    assert_eq!(t.main_tileset, b'L');
    assert!(t.custom_tileset);
    assert_eq!(t.ground_tilesets, vec![*b"Ldrt", *b"Lgrs"]);
    assert_eq!(t.cliff_tilesets, vec![*b"Xbtt"]);
    assert_eq!((t.width, t.height), (2, 2));
    assert_eq!(t.center, [1.5, -2.25]);
    assert_eq!(t.points.len(), 4);

    let p = t.point(0, 0).unwrap();
    assert_eq!(p.ground_height, -5);
    assert_eq!(p.water_level, 0x1234);
    assert!(p.map_edge);
    assert_eq!(p.ground_texture, 5);
    assert!(p.boundary);
    assert!(p.blight);
    assert!(!p.water);
    assert!(!p.ramp);
    assert_eq!(p.ground_variation, 0x12);
    assert_eq!(p.cliff_variation, 5);
    assert_eq!(p.cliff_texture, 0xA);
    assert_eq!(p.layer_height, 3);

    // Row-major: (x=1, y=0) is the second point, (x=0, y=1) the third.
    assert_eq!(t.point(1, 0).unwrap().ground_height, 10);
    let q = t.point(0, 1).unwrap();
    assert_eq!(q.ground_height, -300);
    assert_eq!(q.water_level, 0x40);
    assert!(!q.map_edge);
    assert!(q.water);
    assert_eq!(q.ground_texture, 1);
    assert!(q.ramp);
    assert_eq!(t.point(2, 0), None);
    assert_eq!(t.point(0, 2), None);
}

#[test]
fn terrain_rejects_bad_magic_and_version() {
    let mut bad_magic = w3e_bytes();
    bad_magic[0] = b'X';
    assert!(matches!(
        Terrain::parse(&bad_magic),
        Err(Error::BadMagic {
            file: "war3map.w3e",
            ..
        })
    ));

    let mut bad_version = w3e_bytes();
    bad_version[4] = 12;
    assert!(matches!(
        Terrain::parse(&bad_version),
        Err(Error::UnsupportedVersion {
            found: 12,
            expected: 11,
            ..
        })
    ));
}

#[test]
fn terrain_rejects_bad_custom_flag() {
    let mut data = w3e_bytes();
    data[9] = 2; // custom tileset flag is the u32 at offset 9
    assert!(matches!(
        Terrain::parse(&data),
        Err(Error::InvalidValue { .. })
    ));
}

fn w3i_bytes() -> Vec<u8> {
    let mut b = B::default()
        .u32(25) // format
        .u32(3) // saves
        .u32(6059) // editor version
        .s("Test Map")
        .s("Author")
        .s("Description text")
        .s("2-4");
    for v in [
        -1000.0f32, 1000.0, -2000.0, 2000.0, -1100.0, 1100.0, -2100.0, 2100.0,
    ] {
        b = b.f32(v);
    }
    b = b
        .i32(10)
        .i32(20)
        .i32(30)
        .i32(40)
        .u32(96) // playable width
        .u32(64) // playable height
        .u32(0x04 | 0x40) // melee | use custom forces
        .u8(b'L')
        .u32(0) // campaign background
        // loading screen
        .s("")
        .s("Loading text")
        .s("Loading title")
        .s("Loading subtitle")
        .u32(0) // game data set
        // prologue screen
        .s("war3mapPrologue.mdx")
        .s("")
        .s("")
        .s("")
        // fog and environment
        .u32(1) // fog style
        .f32(100.0)
        .f32(2000.0)
        .f32(0.5)
        .raw(&[0x10, 0x20, 0x30, 0xFF])
        .raw(b"RAhr") // weather
        .s("SoundEnv")
        .u8(3) // light environment tileset
        .raw(&[0x01, 0x02, 0x03, 0x04]); // water color
                                         // players
    b = b
        .u32(1)
        .u32(0)
        .u32(1) // human
        .u32(2) // race
        .u32(0)
        .s("Player 1")
        .f32(-64.0)
        .f32(32.5)
        .u32(0)
        .u32(0);
    // forces
    b = b.u32(1).u32(0x1).u32(0x1).s("Force 1");
    b.done()
}

#[test]
fn map_info_fields() {
    let data = w3i_bytes();
    let info = MapInfo::parse(&data).unwrap();
    assert_eq!(info.file_version, 25);
    assert_eq!(info.saves, 3);
    assert_eq!(info.editor_version, 6059);
    assert_eq!(info.name, "Test Map");
    assert_eq!(info.author, "Author");
    assert_eq!(info.description, "Description text");
    assert_eq!(info.recommended_players, "2-4");
    assert_eq!(info.camera_bounds[0], -1000.0);
    assert_eq!(info.camera_bounds[7], 2100.0);
    assert_eq!(info.camera_complements, [10, 20, 30, 40]);
    assert_eq!((info.playable_width, info.playable_height), (96, 64));
    assert_eq!(info.flags, 0x44);
    assert_eq!(info.main_tileset, b'L');
    assert_eq!(info.loading_screen.text, "Loading text");
    assert_eq!(info.loading_screen.subtitle, "Loading subtitle");
    assert_eq!(info.prologue_screen.model, "war3mapPrologue.mdx");
    assert_eq!(info.fog.style, 1);
    assert_eq!(info.fog.end_z, 2000.0);
    assert_eq!(info.fog.color, [0x10, 0x20, 0x30, 0xFF]);
    assert_eq!(info.weather_id, *b"RAhr");
    assert_eq!(info.sound_environment, "SoundEnv");
    assert_eq!(info.light_environment_tileset, 3);
    assert_eq!(info.water_color, [1, 2, 3, 4]);

    assert_eq!(info.players.len(), 1);
    let p = &info.players[0];
    assert_eq!(p.number, 0);
    assert_eq!(p.player_type, 1);
    assert_eq!(p.race, 2);
    assert_eq!(p.name, "Player 1");
    assert_eq!((p.start_x, p.start_y), (-64.0, 32.5));

    assert_eq!(info.forces.len(), 1);
    assert_eq!(info.forces[0].name, "Force 1");
    assert_eq!(info.forces[0].player_masks, 1);
    assert!(info.rest.is_empty());
}

#[test]
fn map_info_keeps_trailing_bytes_raw() {
    let mut data = w3i_bytes();
    data.extend_from_slice(&[9, 8, 7]);
    let info = MapInfo::parse(&data).unwrap();
    assert_eq!(info.rest, vec![9, 8, 7]);
}

#[test]
fn map_info_rejects_wrong_version() {
    let mut data = w3i_bytes();
    data[0] = 18; // ROC format
    assert!(matches!(
        MapInfo::parse(&data),
        Err(Error::UnsupportedVersion {
            found: 18,
            expected: 25,
            ..
        })
    ));
}

fn doo_bytes() -> Vec<u8> {
    let b = B::default().raw(b"W3do").u32(8).u32(11).u32(2);
    // doodad 0: no dropped item sets
    let b = b
        .raw(b"ATtr")
        .u32(3)
        .f32(1.0)
        .f32(2.0)
        .f32(3.0)
        .f32(0.5)
        .f32(1.0)
        .f32(1.0)
        .f32(1.0)
        .u8(0x02)
        .u8(75)
        .u32(0xFFFF_FFFF)
        .u32(0)
        .u32(42);
    // doodad 1: two dropped item sets, the first with one item and the second with two
    let b = b
        .raw(b"LTlt")
        .u32(0)
        .f32(-4.0)
        .f32(8.0)
        .f32(0.0)
        .f32(0.0)
        .f32(2.0)
        .f32(2.0)
        .f32(2.0)
        .u8(0x06)
        .u8(100)
        .u32(7)
        .u32(2)
        .u32(1)
        .raw(b"ckhy")
        .u32(50)
        .u32(2)
        .raw(b"I001")
        .u32(10)
        .raw(b"I002")
        .u32(20)
        .u32(43);
    // special doodads
    b.u32(0).u32(1).raw(b"ATtr").i32(10).i32(-4).f32(1.5).done()
}

#[test]
fn doodads_fields() {
    let d = Doodads::parse(&doo_bytes()).unwrap();
    assert_eq!(d.doodads.len(), 2);

    let a = &d.doodads[0];
    assert_eq!(a.id, *b"ATtr");
    assert_eq!(a.variation, 3);
    assert_eq!(a.position, [1.0, 2.0, 3.0]);
    assert_eq!(a.angle, 0.5);
    assert_eq!(a.scale, [1.0, 1.0, 1.0]);
    assert_eq!(a.flags, 0x02);
    assert_eq!(a.life, 75);
    assert_eq!(a.item_table_ptr, 0xFFFF_FFFF);
    assert!(a.dropped_item_sets.is_empty());
    assert_eq!(a.editor_id, 42);

    let b = &d.doodads[1];
    assert_eq!(b.id, *b"LTlt");
    assert_eq!(b.position, [-4.0, 8.0, 0.0]);
    assert_eq!(b.scale, [2.0, 2.0, 2.0]);
    assert_eq!(b.life, 100);
    assert_eq!(b.dropped_item_sets.len(), 2);
    assert_eq!(b.dropped_item_sets[0].len(), 1);
    assert_eq!(b.dropped_item_sets[0][0].id, *b"ckhy");
    assert_eq!(b.dropped_item_sets[0][0].chance, 50);
    assert_eq!(b.dropped_item_sets[1].len(), 2);
    assert_eq!(b.dropped_item_sets[1][1].id, *b"I002");
    assert_eq!(b.dropped_item_sets[1][1].chance, 20);
    assert_eq!(b.editor_id, 43);

    assert_eq!(d.special.len(), 1);
    assert_eq!(d.special[0].id, *b"ATtr");
    assert_eq!((d.special[0].x, d.special[0].y), (10, -4));
    assert_eq!(d.special[0].z, 1.5);
}

#[test]
fn doodads_reject_bad_header() {
    let mut magic = doo_bytes();
    magic[0] = b'X';
    assert!(matches!(
        Doodads::parse(&magic),
        Err(Error::BadMagic { .. })
    ));

    let mut version = doo_bytes();
    version[4] = 7; // ROC
    assert!(matches!(
        Doodads::parse(&version),
        Err(Error::UnsupportedVersion { found: 7, .. })
    ));

    let mut subversion = doo_bytes();
    subversion[8] = 10;
    assert!(matches!(
        Doodads::parse(&subversion),
        Err(Error::UnsupportedVersion { found: 10, .. })
    ));
}

#[test]
fn doodads_huge_count_is_an_error() {
    let data = B::default()
        .raw(b"W3do")
        .u32(8)
        .u32(11)
        .u32(u32::MAX)
        .done();
    assert!(matches!(
        Doodads::parse(&data),
        Err(Error::CountTooLarge { .. })
    ));
}

#[test]
fn shadow_map_is_exact_byte_grid() {
    let data = [1u8, 2, 3, 4, 5, 6];
    let s = ShadowMap::parse(&data, 3, 2).unwrap();
    assert_eq!(s.get(0, 0), Some(1));
    assert_eq!(s.get(2, 1), Some(6));
    assert_eq!(s.get(3, 0), None);

    assert!(matches!(
        ShadowMap::parse(&data, 3, 3),
        Err(Error::SizeMismatch {
            expected: 9,
            found: 6,
            ..
        })
    ));
}

fn wpm_bytes() -> Vec<u8> {
    B::default()
        .raw(b"MP3W")
        .u32(0)
        .u32(3)
        .u32(2)
        .raw(&[0, 1, 2, 3, 4, 5])
        .done()
}

#[test]
fn path_map_fields() {
    let p = PathMap::parse(&wpm_bytes()).unwrap();
    assert_eq!((p.width, p.height), (3, 2));
    assert_eq!(p.cells, vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(p.get(1, 1), Some(4));
}

#[test]
fn path_map_rejects_bad_magic_version_and_size() {
    let mut magic = wpm_bytes();
    magic[3] = b'X';
    assert!(matches!(
        PathMap::parse(&magic),
        Err(Error::BadMagic { .. })
    ));

    let mut version = wpm_bytes();
    version[4] = 1;
    assert!(matches!(
        PathMap::parse(&version),
        Err(Error::UnsupportedVersion { found: 1, .. })
    ));

    let mut short = wpm_bytes();
    short.pop();
    assert!(matches!(
        PathMap::parse(&short),
        Err(Error::SizeMismatch { .. })
    ));
}

/// Every strict prefix of a complete file must fail with an error, never panic or succeed.
#[test]
fn truncated_input_is_an_error() {
    let full = w3e_bytes();
    for n in 0..full.len() {
        assert!(Terrain::parse(&full[..n]).is_err(), "w3e prefix {n}");
    }
    let full = w3i_bytes();
    for n in 0..full.len() {
        assert!(MapInfo::parse(&full[..n]).is_err(), "w3i prefix {n}");
    }
    let full = doo_bytes();
    for n in 0..full.len() {
        assert!(Doodads::parse(&full[..n]).is_err(), "doo prefix {n}");
    }
    let full = wpm_bytes();
    for n in 0..full.len() {
        assert!(PathMap::parse(&full[..n]).is_err(), "wpm prefix {n}");
    }
    let full = [1u8, 2, 3, 4, 5, 6];
    for n in 0..full.len() {
        assert!(
            ShadowMap::parse(&full[..n], 3, 2).is_err(),
            "shd prefix {n}"
        );
    }
}

/// Overwriting any single byte with 0xFF or 0x00 must never panic.
#[test]
fn corrupted_input_never_panics() {
    let files: [(&str, Vec<u8>); 4] = [
        ("w3e", w3e_bytes()),
        ("w3i", w3i_bytes()),
        ("doo", doo_bytes()),
        ("wpm", wpm_bytes()),
    ];
    for (name, original) in files {
        for i in 0..original.len() {
            for value in [0x00u8, 0xFF] {
                let mut data = original.clone();
                data[i] = value;
                let _ = match name {
                    "w3e" => Terrain::parse(&data).map(|_| ()),
                    "w3i" => MapInfo::parse(&data).map(|_| ()),
                    "doo" => Doodads::parse(&data).map(|_| ()),
                    _ => PathMap::parse(&data).map(|_| ()),
                };
            }
        }
    }
}

#[test]
fn terrain_huge_dimensions_are_an_error() {
    let data = B::default()
        .raw(b"W3E!")
        .u32(11)
        .u8(b'L')
        .u32(0)
        .u32(0)
        .u32(0xFFFF)
        .u32(0xFFFF)
        .f32(0.0)
        .f32(0.0)
        .done();
    assert!(matches!(
        Terrain::parse(&data),
        Err(Error::CountTooLarge { .. })
    ));
}
