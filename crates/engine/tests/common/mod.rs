//! Synthetic archives and scripts for the engine tests. Nothing here comes from the game.

#![allow(dead_code)]

use mpq::crypto::{encrypt_bytes, hash_string};

/// Builds a store-only MPQ (no compression, no encryption) with `files`, after `prefix` bytes
/// (512 for a map file's header). Hash table 16 entries, 512-byte sectors, format version 0.
pub fn mpq(files: &[(&str, Vec<u8>)], prefix: usize) -> Vec<u8> {
    let hash_n = 16usize;
    let mut body = Vec::new();
    let mut blocks = Vec::new();
    let mut hashes = vec![[0xFFFF_FFFFu32; 4]; hash_n];
    for (bi, (name, data)) in files.iter().enumerate() {
        let offset = (32 + body.len()) as u32;
        blocks.push([offset, data.len() as u32, data.len() as u32, 0x8000_0000]);
        body.extend_from_slice(data);
        let mut i = hash_string(name, 0) as usize % hash_n;
        while hashes[i][3] != 0xFFFF_FFFF {
            i = (i + 1) % hash_n;
        }
        hashes[i] = [hash_string(name, 1), hash_string(name, 2), 0, bi as u32];
    }
    let mut ht: Vec<u8> = hashes
        .iter()
        .flat_map(|h| h.iter().flat_map(|v| v.to_le_bytes()))
        .collect();
    let mut bt: Vec<u8> = blocks
        .iter()
        .flat_map(|b| b.iter().flat_map(|v| v.to_le_bytes()))
        .collect();
    encrypt_bytes(&mut ht, hash_string("(hash table)", 3));
    encrypt_bytes(&mut bt, hash_string("(block table)", 3));
    let ht_pos = 32 + body.len();
    let bt_pos = ht_pos + ht.len();
    let mut f = vec![0u8; prefix];
    f.extend(b"MPQ\x1a");
    f.extend(32u32.to_le_bytes());
    f.extend(((bt_pos + bt.len()) as u32).to_le_bytes());
    f.extend(0u16.to_le_bytes());
    f.extend(0u16.to_le_bytes());
    f.extend((ht_pos as u32).to_le_bytes());
    f.extend((bt_pos as u32).to_le_bytes());
    f.extend((hash_n as u32).to_le_bytes());
    f.extend((blocks.len() as u32).to_le_bytes());
    f.extend(body);
    f.extend(ht);
    f.extend(bt);
    f
}

/// A player record for [`w3i`]: slot, raw type (1 human, 2 computer, 4 rescuable, 0 empty), raw
/// race, name, start x and y.
pub struct PlayerRec {
    pub number: u32,
    pub player_type: u32,
    pub race: u32,
    pub name: &'static str,
    pub start: (f32, f32),
}

/// Writes a format-25 `war3map.w3i` with the given players, forces (`(player mask, name)`) and
/// playable rectangle `bounds` (left, right, bottom, top, then four unused values; tile-aligned,
/// centered). The camera bounds are written as the same values; the engine derives the playable
/// area from the size in tiles and the (zero) complements, with the map centered on the origin.
pub fn w3i(
    map_name: &str,
    bounds: [f32; 8],
    players: &[PlayerRec],
    forces: &[(u32, &str)],
) -> Vec<u8> {
    fn cstr(o: &mut Vec<u8>, s: &str) {
        o.extend(s.as_bytes());
        o.push(0);
    }
    fn screen(o: &mut Vec<u8>) {
        for _ in 0..4 {
            cstr(o, "");
        }
    }
    let mut o = Vec::new();
    o.extend(25u32.to_le_bytes()); // file version
    o.extend(0u32.to_le_bytes()); // saves
    o.extend(0u32.to_le_bytes()); // editor version
    cstr(&mut o, map_name);
    cstr(&mut o, "engine test");
    cstr(&mut o, "synthetic");
    cstr(&mut o, "");
    for b in bounds {
        o.extend(b.to_le_bytes());
    }
    for _ in 0..4 {
        o.extend(0i32.to_le_bytes()); // complements
    }
    // Playable size in tiles (128 units), as a real map stores it.
    let tiles = |lo: f32, hi: f32| {
        let t = (hi - lo) / 128.0;
        assert!(
            t.fract() == 0.0 && lo == -hi,
            "test bounds must be tile-aligned and centered"
        );
        t as u32
    };
    o.extend(tiles(bounds[0], bounds[1]).to_le_bytes()); // playable width
    o.extend(tiles(bounds[2], bounds[3]).to_le_bytes()); // playable height
    o.extend(0u32.to_le_bytes()); // flags
    o.push(0); // main tileset
    o.extend(0u32.to_le_bytes()); // campaign background
    screen(&mut o); // loading screen
    o.extend(0u32.to_le_bytes()); // game data set
    screen(&mut o); // prologue
    o.extend(0u32.to_le_bytes()); // fog style
    for _ in 0..3 {
        o.extend(0f32.to_le_bytes()); // fog start, end, density
    }
    o.extend([0u8; 4]); // fog color
    o.extend([0u8; 4]); // weather
    cstr(&mut o, ""); // sound environment
    o.push(0); // light environment tileset
    o.extend([0u8; 4]); // water color
    o.extend((players.len() as u32).to_le_bytes());
    for p in players {
        o.extend(p.number.to_le_bytes());
        o.extend(p.player_type.to_le_bytes());
        o.extend(p.race.to_le_bytes());
        o.extend(0u32.to_le_bytes()); // flags
        cstr(&mut o, p.name);
        o.extend(p.start.0.to_le_bytes());
        o.extend(p.start.1.to_le_bytes());
        o.extend(0u32.to_le_bytes()); // ally low
        o.extend(0u32.to_le_bytes()); // ally high
    }
    o.extend((forces.len() as u32).to_le_bytes());
    for (mask, name) in forces {
        o.extend(0u32.to_le_bytes()); // flags
        o.extend(mask.to_le_bytes());
        cstr(&mut o, name);
    }
    o
}

/// A map file: the 512-byte header prefix, then the archive with `war3map.w3i` and `war3map.j`.
pub fn map_file(w3i_bytes: Vec<u8>, war3map_j: &str) -> Vec<u8> {
    mpq(
        &[
            ("war3map.w3i", w3i_bytes),
            ("war3map.j", war3map_j.as_bytes().to_vec()),
        ],
        512,
    )
}

/// Shared type and native declarations, and a native that no host implements.
pub const COMMON_J: &str = r#"
type player extends handle
type timer extends handle

native Player takes integer i returns player
native CreateTimer takes nothing returns timer
native TimerStart takes timer t, real timeout, boolean periodic, code handlerFunc returns nothing
native NoSuchNative takes integer x returns integer
"#;

/// Blizzard helpers: an init function that `main` calls, and a function that uses a missing native.
pub const BLIZZARD_J: &str = r#"
globals
    integer gBlizzardRan = 0
endglobals

function InitBlizzard takes nothing returns nothing
    set gBlizzardRan = gBlizzardRan + 1
endfunction

function UsesMissing takes nothing returns integer
    return NoSuchNative(3)
endfunction
"#;

/// The map script: config and main, a timer that main starts, and a real literal (for the tables).
pub const WAR3MAP_J: &str = r#"
globals
    timer gT = null
    integer gFired = 0
    integer gConfig = 0
    integer gMain = 0
    real gR = 0.1
endglobals

function OnExpire takes nothing returns nothing
    set gFired = gFired + 1
endfunction

function config takes nothing returns nothing
    set gConfig = 1
endfunction

function main takes nothing returns nothing
    set gMain = 1
    set gT = CreateTimer()
    call TimerStart(gT, 0.5, false, function OnExpire)
    call InitBlizzard()
endfunction
"#;
