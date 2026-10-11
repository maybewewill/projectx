//! Game data at boot: the Misc layers, the unit rows and the damage model choice. Synthetic archives only; the
//! `SpecDamage` check runs only with `GAME_DLL` set to a 1.26a `Game.dll`.

mod common;

use common::{mpq, w3i, PlayerRec, BLIZZARD_J, COMMON_J, WAR3MAP_J};
use engine::data;
use engine::{Engine, Inputs};
use mpq::Archive;

const BOUNDS: [f32; 8] = [-1000.0, 1000.0, -800.0, 800.0, 0.0, 0.0, 0.0, 0.0];
const HFOO: u32 = 0x6866_6F6F;

fn map_with(extra: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let info = w3i(
        "Combat Test",
        BOUNDS,
        &[PlayerRec {
            number: 0,
            player_type: 1,
            race: 4,
            name: "Alice",
            start: (0.0, 0.0),
        }],
        &[(0b1, "Force A")],
    );
    let mut files = vec![
        ("war3map.w3i", info),
        ("war3map.j", WAR3MAP_J.as_bytes().to_vec()),
    ];
    files.extend(extra.iter().map(|(name, data)| (*name, data.clone())));
    mpq(&files, 512)
}

const MISC: &str =
    "[Misc]\nDefenseArmor=0.05\nDamageBonusNormal=1,1,1,1,1,1,1,1\nReactionDelay=0.3\n";
const BALANCE: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitID\"\nC;X2;Y1;K\"def\"\nC;X3;Y1;K\"defType\"\n\
C;X1;Y2;K\"hfoo\"\nC;X2;Y2;K2\nC;X3;Y2;K\"medium\"\nE\n";
const WEAPONS: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitID\"\nC;X2;Y1;K\"dice1\"\nC;X3;Y1;K\"sides1\"\nC;X4;Y1;K\"dmgplus1\"\n\
C;X5;Y1;K\"cool1\"\nC;X6;Y1;K\"rangeN1\"\nC;X7;Y1;K\"atkType1\"\n\
C;X1;Y2;K\"hfoo\"\nC;X2;Y2;K2\nC;X3;Y2;K6\nC;X4;Y2;K3\nC;X5;Y2;K1.35\nC;X6;Y2;K60\nC;X7;Y2;K\"Normal\"\n\
C;X1;Y3;K\"hfo\"\nC;X2;Y3;K1\nE\n";

fn game_archive() -> Vec<u8> {
    mpq(
        &[
            ("Units\\MiscData.txt", MISC.as_bytes().to_vec()),
            ("Units\\UnitBalance.slk", BALANCE.as_bytes().to_vec()),
            ("Units\\UnitWeapons.slk", WEAPONS.as_bytes().to_vec()),
        ],
        0,
    )
}

fn inputs(map: Vec<u8>, game_archives: Vec<Vec<u8>>, game_dll: Option<Vec<u8>>) -> Inputs {
    Inputs {
        map,
        game_dll,
        common_j: COMMON_J.to_string(),
        blizzard_j: BLIZZARD_J.to_string(),
        rng_seed: 0,
        ai_common: None,
        ai_scripts: Vec::new(),
        game_archives,
    }
}

#[test]
fn without_game_archives_the_placeholder_stays_and_the_report_says_why() {
    let engine = Engine::boot(inputs(map_with(&[]), Vec::new(), None)).expect("boot");
    let report = engine.report();
    assert!(
        report
            .combat
            .damage_model
            .starts_with("PlaceholderDamage ("),
        "{}",
        report.combat.damage_model
    );
    assert!(report.combat.damage_model.contains("no Game.dll tables"));
    assert!(report.combat.damage_model.contains("no [Misc] data"));
    assert_eq!(report.combat.unit_types, 0);
    assert!(report.notes.iter().any(|n| n.contains("no game archives")));
    assert!(report
        .to_string()
        .contains("damage model: PlaceholderDamage"));
}

#[test]
fn unit_rows_reach_the_combat_registries_without_the_game_dll() {
    let engine = Engine::boot(inputs(map_with(&[]), vec![game_archive()], None)).expect("boot");
    let report = engine.report();
    // `hfo` is not a four-character code: it is skipped and noted.
    assert_eq!(report.combat.unit_types, 1);
    assert!(
        report.notes.iter().any(|n| n.contains("\"hfo\"")),
        "{:?}",
        report.notes
    );
    assert!(report
        .combat
        .damage_model
        .starts_with("PlaceholderDamage ("));
    assert!(!report.combat.damage_model.contains("no unit balance"));

    let game = engine.host().game();
    let attack = game.attack_type(HFOO);
    assert_eq!((attack.dice, attack.sides, attack.dice_bonus), (2, 6, 3));
    assert_eq!(attack.cooldown.to_bits(), 1.35f32.to_bits());
    assert_eq!(attack.range.to_bits(), 60f32.to_bits());
    assert_eq!(attack.damage_class, 0x10, "a basic attack is class 0x10");
    let defense = game.defense_of_type(HFOO);
    assert_eq!(defense.armor.to_bits(), 2f32.to_bits());
    assert_eq!(defense.armor_type, 1, "medium is column 1");
}

#[test]
fn the_map_layer_overrides_the_game_layer_and_the_map_can_stand_alone() {
    let game = [game_archive()];
    let map_misc = b"[Misc]\nDefenseArmor=0.1\n".to_vec();
    let map_bytes = map_with(&[("war3mapMisc.txt", map_misc)]);
    let archive = Archive::open(&map_bytes).expect("map archive");
    let refs: Vec<&[u8]> = game.iter().map(Vec::as_slice).collect();
    let data = data::load(&refs, &archive, None).expect("game data");
    assert_eq!(data.misc.layers_with_misc, 2);
    assert_eq!(
        data.misc.defense_armor.to_bits(),
        0.1f32.to_bits(),
        "map wins"
    );
    assert_eq!(
        data.misc.reaction_delay.to_bits(),
        0.3f32.to_bits(),
        "game value kept"
    );
    assert!(data.has_balance && data.has_weapons);

    let plain = map_with(&[]);
    let archive = Archive::open(&plain).expect("map archive");
    let data = data::load(&refs, &archive, None).expect("game data");
    assert_eq!(data.misc.defense_armor.to_bits(), 0.05f32.to_bits());
}

/// With the Game.dll tables, Misc and the unit data, the model is `SpecDamage`.
#[test]
fn spec_damage_is_the_default_with_the_game_dll_and_the_data() {
    let Ok(path) = std::env::var("GAME_DLL") else {
        eprintln!("GAME_DLL not set: skipping the SpecDamage boot check");
        return;
    };
    let dll = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    let engine =
        Engine::boot(inputs(map_with(&[]), vec![game_archive()], Some(dll))).expect("boot");
    let report = engine.report();
    assert_eq!(report.combat.damage_model, "SpecDamage");
    assert!(report.to_string().contains("damage model: SpecDamage"));
}
