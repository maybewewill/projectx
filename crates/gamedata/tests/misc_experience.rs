//! The three kill-experience flags of `[Misc]` (`BuildingKillsGiveExp`, `GlobalExperience`,
//! `MaxLevelHeroesDrainExp`; `docs/spec/heroes.md`, section 8.3). Read by the Misc loader `0x6F26E080` through the
//! integer reader `0x6F009F90` with default 0. The flag is on when the value is non-zero. Synthetic layers only.

use gamedata::{MiscData, Profile};

fn layer(text: &str) -> Profile {
    Profile::parse(text).expect("fixture parses")
}

#[test]
fn absent_flags_are_off() {
    let p = layer("[Misc]\nDefenseArmor=0.06\n");
    let m = MiscData::from_layers(&[&p], None);
    assert!(!m.hero.building_kills_give_exp);
    assert!(!m.hero.global_experience);
    assert!(!m.hero.max_level_heroes_drain_exp);
}

#[test]
fn zero_is_off_and_any_non_zero_is_on() {
    let p = layer(
        "[Misc]\n\
         BuildingKillsGiveExp=1\n\
         GlobalExperience=0\n\
         MaxLevelHeroesDrainExp=2\n",
    );
    let m = MiscData::from_layers(&[&p], None);
    assert!(m.hero.building_kills_give_exp);
    assert!(!m.hero.global_experience);
    assert!(m.hero.max_level_heroes_drain_exp);
}

#[test]
fn a_later_layer_overrides_the_flag() {
    let base = layer("[Misc]\nGlobalExperience=1\n");
    let map = layer("[Misc]\nGlobalExperience=0\n");
    let m = MiscData::from_layers(&[&base, &map], None);
    assert!(!m.hero.global_experience);
}
