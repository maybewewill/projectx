//! Object text and data at boot (`engine::objdata`) and the loader fixes found on the real maps: a Reign of Chaos
//! (format 18) `war3map.w3i`, FDF float suffixes, stray profile lines. Synthetic files only.

mod common;

use common::{mpq, BLIZZARD_J, COMMON_J, WAR3MAP_J};
use engine::{objdata, Engine, Inputs};

const FOOT: u32 = 0x6866_6F6F;
const AHHB: u32 = 0x4148_6862;

fn cstr(o: &mut Vec<u8>, s: &str) {
    o.extend(s.as_bytes());
    o.push(0);
}

/// A format-18 `war3map.w3i` with two players and one force, 32 x 32 playable tiles.
fn roc_w3i() -> Vec<u8> {
    let mut o = Vec::new();
    o.extend(18u32.to_le_bytes());
    o.extend(0u32.to_le_bytes()); // saves
    o.extend(0u32.to_le_bytes()); // editor version
    for s in ["Roc Test", "author", "desc", ""] {
        cstr(&mut o, s);
    }
    for b in [-2048.0f32, 2048.0, -2048.0, 2048.0, 0.0, 0.0, 0.0, 0.0] {
        o.extend(b.to_le_bytes());
    }
    for _ in 0..4 {
        o.extend(0i32.to_le_bytes());
    }
    o.extend(32u32.to_le_bytes());
    o.extend(32u32.to_le_bytes());
    o.extend(0u32.to_le_bytes()); // flags
    o.push(b'L');
    o.extend(0xFFFF_FFFFu32.to_le_bytes()); // loading screen number
    for _ in 0..3 {
        cstr(&mut o, "");
    }
    o.extend(0xFFFF_FFFFu32.to_le_bytes()); // prologue number
    for _ in 0..3 {
        cstr(&mut o, "");
    }
    o.extend(2u32.to_le_bytes());
    for (n, race) in [(0u32, 1u32), (1, 2)] {
        for v in [n, 1, race, 0] {
            o.extend(v.to_le_bytes());
        }
        cstr(&mut o, "P");
        o.extend(0f32.to_le_bytes());
        o.extend(0f32.to_le_bytes());
        o.extend(0u32.to_le_bytes());
        o.extend(0u32.to_le_bytes());
    }
    o.extend(1u32.to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend(3u32.to_le_bytes());
    cstr(&mut o, "Force");
    o
}

const ITEM_SLK: &str = "ID;PWXL;N;E\n\
B;X4;Y3;D0\n\
C;X1;Y1;K\"itemID\"\n\
C;X2;K\"stockMax\"\n\
C;X3;K\"stockRegen\"\n\
C;Y2;X1;K\"rde1\"\n\
C;X2;K3\n\
C;X3;K30.5\n\
C;Y3;X1;K\"rst1\"\n\
C;X2;K1\n\
C;X3;K120\n\
E\n";

const ABILITY_SLK: &str = "ID;PWXL;N;E\n\
B;X4;Y2;D0\n\
C;X1;Y1;K\"alias\"\n\
C;X2;K\"levels\"\n\
C;X3;K\"DataA1\"\n\
C;Y2;X1;K\"Agld\"\n\
C;X2;K1\n\
C;X3;K12500\n\
E\n";

const GLOBAL_FDF: &str = "StringList {\n  COLON_TEST \"Test:\",\n  HEROES \"Heroes\",\n}\n";
// A sibling file with a float suffix, as in the shipped Glue\CampaignMenu.fdf, listed after the strings.
const MENU_FDF: &str = "Frame \"BACKDROP\" \"Menu\" {\n  Height 0.090f,\n}\nStringList {\n  AFTER_FLOAT \"kept\",\n}\n";

const FUNC_TXT: &str = "[AHhb]\nTargetart=Abilities\\a.mdl,Abilities\\b.mdl\nEffectsound=HolyLight\nEffectsoundlooped=HolyLoop\nstray line\n[hfoo]\nArt=x.blp\n";
const STRINGS_TXT: &str = "// comment\n[hfoo]\nName=Footman\n[AHhb]\nName=Holy Light\nTip=Holy Light\n";

fn game_archive() -> Vec<u8> {
    mpq(
        &[
            ("UI\\FrameDef\\FrameDef.toc", b"UI\\FrameDef\\GlobalStrings.fdf\nUI\\FrameDef\\Menu.fdf\n".to_vec()),
            ("UI\\FrameDef\\GlobalStrings.fdf", GLOBAL_FDF.as_bytes().to_vec()),
            ("UI\\FrameDef\\Menu.fdf", MENU_FDF.as_bytes().to_vec()),
            ("Units\\HumanAbilityFunc.txt", FUNC_TXT.as_bytes().to_vec()),
            ("Units\\HumanUnitStrings.txt", STRINGS_TXT.as_bytes().to_vec()),
            ("Units\\ItemData.slk", ITEM_SLK.as_bytes().to_vec()),
            ("Units\\AbilityData.slk", ABILITY_SLK.as_bytes().to_vec()),
        ],
        0,
    )
}

#[test]
fn object_text_is_read_from_the_archives() {
    let archive = game_archive();
    let text = objdata::load(&[archive.as_slice()]).expect("loads");

    // The global string table: every FDF listed in the TOC, case-insensitive keys, strings after a float suffix kept.
    assert_eq!(text.localized.get("colon_test").map(String::as_str), Some("Test:"));
    assert_eq!(text.localized.get("after_float").map(String::as_str), Some("kept"));

    // Names come from the Strings profiles, art and sound lists from the Func profiles (lenient about stray lines).
    assert_eq!(text.names.get(&FOOT).map(String::as_str), Some("Footman"));
    assert_eq!(text.names.get(&AHHB).map(String::as_str), Some("Holy Light"));
    assert_eq!(
        text.art.get(&(AHHB, 1)),
        Some(&vec!["Abilities\\a.mdl".to_string(), "Abilities\\b.mdl".to_string()])
    );
    assert_eq!(text.sound.get(&(AHHB, 0)), Some(&vec!["HolyLight".to_string()]));
    assert_eq!(text.sound.get(&(AHHB, 1)), Some(&vec!["HolyLoop".to_string()]));

    // Item stock rows and the gold mine amount.
    let rde1 = text.item_stock[&u32::from_be_bytes(*b"rde1")];
    assert_eq!(rde1.stock_max, 3);
    assert_eq!(rde1.regen.0, 30.5f32.to_bits());
    assert_eq!(text.gold_mine_amount, Some(12500));
}

#[test]
fn a_reign_of_chaos_map_boots_and_gets_the_object_text() {
    let map = mpq(
        &[
            ("war3map.w3i", roc_w3i()),
            ("war3map.j", WAR3MAP_J.as_bytes().to_vec()),
        ],
        512,
    );
    let engine = Engine::boot(Inputs {
        map,
        game_dll: None,
        common_j: COMMON_J.to_string(),
        blizzard_j: BLIZZARD_J.to_string(),
        rng_seed: 0,
        ai_common: None,
        ai_scripts: Vec::new(),
        game_archives: vec![game_archive()],
        check_natives: false,
    })
    .expect("a format-18 map boots");
    let report = engine.report();
    assert_eq!(report.map_name, "Roc Test");
    assert_eq!(report.players.len(), 2);

    let game = engine.host().game();
    assert_eq!(game.lane.localized.get("heroes").map(String::as_str), Some("Heroes"));
    assert_eq!(game.lane.object_names.get(&FOOT).map(String::as_str), Some("Footman"));
    assert_eq!(game.shops.item_stock[&u32::from_be_bytes(*b"rst1")].stock_max, 1);
}
