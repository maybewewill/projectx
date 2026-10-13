//! The item abilities read from `ItemData.slk` and `AbilityData.slk` at boot (`engine::objdata::load_item_abilities`,
//! `sim::item_abilities`). Synthetic files only.

mod common;

use common::mpq;
use engine::objdata;

const ITEM_SLK: &str = "ID;PWXL;N;E\n\
B;X7;Y3;D0\n\
C;X1;Y1;K\"itemID\"\n\
C;X2;K\"abilList\"\n\
C;X3;K\"uses\"\n\
C;X4;K\"perishable\"\n\
C;X5;K\"cooldownID\"\n\
C;X6;K\"powerup\"\n\
C;Y2;X1;K\"phea\"\n\
C;X2;K\"AIh1\"\n\
C;X3;K3\n\
C;X4;K1\n\
C;X5;K\"AIh1\"\n\
C;X6;K0\n\
C;Y3;X1;K\"tstr\"\n\
C;X2;K\"AIsm\"\n\
C;X3;K0\n\
C;X4;K0\n\
C;X5;K\"\"\n\
C;X6;K1\n\
E\n";

const ABILITY_SLK: &str = "ID;PWXL;N;E\n\
B;X5;Y3;D0\n\
C;X1;Y1;K\"alias\"\n\
C;X2;K\"code\"\n\
C;X3;K\"levels\"\n\
C;X4;K\"DataA1\"\n\
C;X5;K\"Cool1\"\n\
C;Y2;X1;K\"AIh1\"\n\
C;X2;K\"AIhe\"\n\
C;X3;K1\n\
C;X4;K150\n\
C;X5;K5\n\
C;Y3;X1;K\"AIsm\"\n\
C;X2;K\"AIsm\"\n\
C;X3;K1\n\
C;X4;K3\n\
C;X5;K0\n\
E\n";

#[test]
fn item_abilities_are_read_from_the_item_and_ability_tables() {
    let archive = mpq(
        &[
            ("Units\\ItemData.slk", ITEM_SLK.as_bytes().to_vec()),
            ("Units\\AbilityData.slk", ABILITY_SLK.as_bytes().to_vec()),
        ],
        0,
    );
    let text = objdata::load(&[archive.as_slice()]).expect("loads");
    let phea = u32::from_be_bytes(*b"phea");
    let aih1 = u32::from_be_bytes(*b"AIh1");
    let spec = &text.item_abilities.specs[&phea];
    assert_eq!(spec.abilities, vec![aih1]);
    assert_eq!((spec.uses, spec.perishable, spec.powerup, spec.cooldown_id), (3, true, false, aih1));
    assert_eq!(text.item_abilities.bases[&aih1], u32::from_be_bytes(*b"AIhe"));
    let levels = &text.item_abilities.levels[&aih1];
    assert_eq!(levels[0].data[0].to_f32(), 150.0);
    assert_eq!(levels[0].cool.to_f32(), 5.0);
    let tstr = u32::from_be_bytes(*b"tstr");
    assert!(text.item_abilities.specs[&tstr].powerup);

    // Installed into a game: the item type is registered with its charges and flags.
    let mut game = sim::natives::game::Game::new();
    objdata::install(&mut game, &text, &[]);
    let ty = game.item_types().get(phea).expect("type registered");
    assert_eq!(ty.charges, 3);
    assert!(ty.flags & sim::natives::game::item_flags::PERISHABLE != 0);
    assert!(game.item_types().get(tstr).expect("tome").powerup);
}

/// The real 1.26a tables, when `W3_ARCHIVES` names the game archives (`;` separated, later ones override): the item
/// rows read through the same loader. Skipped when the variable is not set.
#[test]
fn real_item_tables_load() {
    let Ok(list) = std::env::var("W3_ARCHIVES") else {
        eprintln!("W3_ARCHIVES not set: real item table check skipped");
        return;
    };
    let archives: Vec<Vec<u8>> = list
        .split(';')
        .filter(|p| !p.is_empty())
        .map(|p| std::fs::read(p).unwrap_or_else(|e| panic!("reading {p}: {e}")))
        .collect();
    let refs: Vec<&[u8]> = archives.iter().map(Vec::as_slice).collect();
    let text = objdata::load(&refs).expect("loads");
    let code = |s: &[u8; 4]| u32::from_be_bytes(*s);
    let ia = &text.item_abilities;
    assert!(ia.specs.len() > 100, "{} item rows", ia.specs.len());
    // Potion of Healing: ability AIh1 of the AIhe class, shares the AIhe cooldown, one perishable charge.
    let phea = &ia.specs[&code(b"phea")];
    assert_eq!(phea.abilities, vec![code(b"AIh1")]);
    assert!(phea.perishable && phea.usable && !phea.powerup);
    assert_eq!(phea.uses, 1);
    assert_eq!(phea.cooldown_id, code(b"AIhe"));
    assert_eq!(ia.bases[&code(b"AIh1")], code(b"AIhe"));
    assert_eq!(ia.levels[&code(b"AIh1")][0].data[0].to_f32(), 250.0);
    assert_eq!(ia.levels[&code(b"AIh1")][0].cool.to_f32(), 20.0);
    // Tome of Strength: AIsm class, strength is DataC.
    let tstr = &ia.specs[&code(b"tstr")];
    assert!(tstr.powerup);
    let lv = &ia.levels[&code(b"AIsm")][0];
    assert_eq!((lv.data[0].to_f32(), lv.data[1].to_f32(), lv.data[2].to_f32()), (0.0, 0.0, 1.0));
    // Agility bonus +3 reads DataA.
    assert_eq!(ia.levels[&code(b"AIa3")][0].data[0].to_f32(), 3.0);
    // Figurine of a Skeleton: DataC / DataD name the two summoned unit types, DataA / DataB their counts.
    assert_eq!(ia.unit_codes[&code(b"AIfs")], [code(b"nsce"), code(b"nsca")]);
    assert_eq!(ia.levels[&code(b"AIfs")][0].data[0].to_f32(), 4.0);
    assert_eq!(ia.levels[&code(b"AIfr")].len(), 1);
    // The Endurance Aura item (`AIae`, class `AOae`): the aura record has the area, the strengths and the buff.
    let aura = &ia.auras[&code(b"AIae")];
    assert!(aura[0].area.to_f32() > 0.0 && aura[0].data[0].to_f32() > 0.0, "{:?}", aura[0]);
    assert_eq!(ia.bases[&code(b"AIae")], code(b"AOae"));
}
