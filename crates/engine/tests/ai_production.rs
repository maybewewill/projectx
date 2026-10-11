//! A computer player on a synthetic map: its AI script harvests gold and trains peons through the production model,
//! with the tech data (`Trains`, `Builds`, harvest abilities, build times) read from synthetic game files by
//! `engine::data` and `engine::objdata`. Nothing here comes from the game. Without the soft-float tables the units
//! cannot walk, so the hall, the mine and the peons stand within reach of each other.

mod common;

use common::{map_file, mpq, w3i, PlayerRec};
use engine::{Engine, Inputs};
use sim::economy::Res;

const BOUNDS: [f32; 8] = [-1024.0, 1024.0, -896.0, 896.0, 0.0, 0.0, 0.0, 0.0];

const COMMON_J: &str = r#"
type player extends handle
type unit extends handle
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native SetPlayerState takes player p, integer s, integer v returns nothing
native SetResourceAmount takes unit u, integer a returns nothing
native StartMeleeAI takes player whichPlayer, string scriptName returns nothing
"#;

const COMMON_AI: &str = r#"
native Sleep takes real duration returns nothing
native TownWithMine takes nothing returns integer
native GetUnitCount takes integer t returns integer
native SetProduce takes integer n, integer t, integer x returns boolean
native ClearHarvestAI takes nothing returns nothing
native HarvestGold takes integer town, integer peons returns nothing
"#;

const AI: &str = r#"
function main takes nothing returns nothing
    loop
        call ClearHarvestAI()
        call HarvestGold(TownWithMine(), 3)
        if GetUnitCount('opeo') < 5 then
            call SetProduce(1, 'opeo', -1)
        endif
        call Sleep(2.0)
    endloop
endfunction
"#;

const UI: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitUIID\"\nC;X2;Y1;K\"name\"\n\
C;X1;Y2;K\"ogre\"\nC;X2;Y2;K\"Hall\"\n\
C;X1;Y3;K\"opeo\"\nC;X2;Y3;K\"Peon\"\n\
C;X1;Y4;K\"ngol\"\nC;X2;Y4;K\"Mine\"\nE\n";
const DATA: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitID\"\nC;X2;Y1;K\"movetp\"\n\
C;X1;Y2;K\"ogre\"\nC;X2;Y2;K\"\"\n\
C;X1;Y3;K\"opeo\"\nC;X2;Y3;K\"foot\"\n\
C;X1;Y4;K\"ngol\"\nC;X2;Y4;K\"\"\nE\n";
const BALANCE: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitID\"\nC;X2;Y1;K\"HP\"\nC;X3;Y1;K\"spd\"\nC;X4;Y1;K\"bldtm\"\nC;X5;Y1;K\"goldcost\"\n\
C;X6;Y1;K\"lumbercost\"\nC;X7;Y1;K\"fmade\"\nC;X8;Y1;K\"fused\"\nC;X9;Y1;K\"isbldg\"\nC;X10;Y1;K\"type\"\n\
C;X1;Y2;K\"ogre\"\nC;X2;Y2;K1400\nC;X3;Y2;K0\nC;X4;Y2;K150\nC;X5;Y2;K450\nC;X6;Y2;K150\nC;X7;Y2;K10\nC;X8;Y2;K0\nC;X9;Y2;K1\nC;X10;Y2;K\"townhall\"\n\
C;X1;Y3;K\"opeo\"\nC;X2;Y3;K250\nC;X3;Y3;K190\nC;X4;Y3;K15\nC;X5;Y3;K75\nC;X6;Y3;K0\nC;X7;Y3;K0\nC;X8;Y3;K1\nC;X9;Y3;K0\nC;X10;Y3;K\"peon\"\n\
C;X1;Y4;K\"ngol\"\nC;X2;Y4;K4000\nC;X3;Y4;K0\nC;X4;Y4;K0\nC;X5;Y4;K0\nC;X6;Y4;K0\nC;X7;Y4;K0\nC;X8;Y4;K0\nC;X9;Y4;K1\nC;X10;Y4;K\"\"\nE\n";
const ABILITIES: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitAbilID\"\nC;X2;Y1;K\"abilList\"\n\
C;X1;Y2;K\"opeo\"\nC;X2;Y2;K\"Amov,Ahar\"\nE\n";
const FUNC: &str = "[ogre]\nTrains=opeo\n[opeo]\nBuilds=ogre\n";

fn game_archive() -> Vec<u8> {
    mpq(
        &[
            ("Units\\UnitUI.slk", UI.as_bytes().to_vec()),
            ("Units\\UnitData.slk", DATA.as_bytes().to_vec()),
            ("Units\\UnitBalance.slk", BALANCE.as_bytes().to_vec()),
            ("Units\\UnitAbilities.slk", ABILITIES.as_bytes().to_vec()),
            ("Units\\OrcUnitFunc.txt", FUNC.as_bytes().to_vec()),
        ],
        0,
    )
}

fn map() -> Vec<u8> {
    let players = [
        PlayerRec { number: 0, player_type: 1, race: 4, name: "Alice", start: (-100.0, 50.0) },
        PlayerRec { number: 1, player_type: 2, race: 2, name: "Computer", start: (300.0, -20.0) },
    ];
    let info = w3i("Production", BOUNDS, &players, &[(0b11, "Force")]);
    let script = r#"
function main takes nothing returns nothing
    local unit mine = CreateUnit(Player(15), 'ngol', 0.0, 150.0, 0.0)
    call SetResourceAmount(mine, 4000)
    call CreateUnit(Player(1), 'ogre', 0.0, 0.0, 0.0)
    call CreateUnit(Player(1), 'opeo', 0.0, 70.0, 0.0)
    call CreateUnit(Player(1), 'opeo', 25.0, 70.0, 0.0)
    call CreateUnit(Player(1), 'opeo', -25.0, 70.0, 0.0)
    call SetPlayerState(Player(1), 1, 500)
    call StartMeleeAI(Player(1), "prod.ai")
endfunction
"#;
    map_file(info, script)
}

fn boot() -> Engine {
    Engine::boot(Inputs {
        map: map(),
        game_dll: None,
        common_j: COMMON_J.to_string(),
        blizzard_j: String::new(),
        rng_seed: 0,
        ai_common: Some(COMMON_AI.to_string()),
        ai_scripts: vec![("prod.ai".to_string(), AI.to_string())],
        game_archives: vec![game_archive()],
    })
    .expect("boot")
}

#[test]
fn tech_data_reaches_the_production_model() {
    let engine = boot();
    let game = engine.host().game();
    let p = game.production();
    let hall = u32::from_be_bytes(*b"ogre");
    let peon = u32::from_be_bytes(*b"opeo");
    assert_eq!(p.tech[&hall].trains, vec![peon]);
    assert_eq!(p.tech[&peon].builds, vec![hall]);
    assert!(p.tech[&peon].harvest_gold && p.tech[&peon].harvest_wood);
    assert_eq!(p.build_seconds[&peon], 15);
}

#[test]
fn the_computer_harvests_gold_and_trains_peons() {
    let mut engine = boot();
    for _ in 0..1200 {
        engine.step();
    }
    let game = engine.host().game();
    let p = game.production();
    assert!(p.stats.gold_deposited > 0, "no gold deposited: {:?}", p.stats);
    assert!(p.stats.trained >= 2, "trained {}", p.stats.trained);
    let peon = u32::from_be_bytes(*b"opeo");
    let peons = game
        .unit_objects()
        .filter(|(_, u)| u.owner == 1 && u.type_id == peon && !u.removed && !u.dead)
        .count();
    assert_eq!(peons, 5);
    // The mine lost what the workers carried away.
    let mine = game.unit_objects().find(|(_, u)| u.type_id == u32::from_be_bytes(*b"ngol")).map(|(k, _)| k).unwrap();
    assert!(game.resource_amount(mine).unwrap() < 4000);
    assert!(game.player_economy(1).unwrap().stored(Res::Gold) > 0);
}
