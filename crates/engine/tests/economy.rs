//! The economy constants at boot (`engine::data::economy_constants` and `install_economy`): the built-in defaults when no
//! `[Misc]` layer exists, the Misc values otherwise, and the report of the absent keys. Synthetic layers only.

use engine::data::{economy_constants, install_economy};
use gamedata::{MiscData, Profile};
use sim::economy::EconomyConstants;
use sim::Game;

#[test]
fn no_misc_layer_means_the_builtin_defaults() {
    let misc = MiscData::from_layers(&[], None);
    let (constants, setup) = economy_constants(&misc);
    assert_eq!(constants, EconomyConstants::default());
    assert!(!setup.from_misc);
    assert_eq!(setup.layers, 0);
    assert!(setup.absent.is_empty());

    let mut game = Game::new();
    let setup = install_economy(&mut game, &misc);
    assert!(!setup.from_misc);
    assert_eq!(game.economy_constants(), &EconomyConstants::default());
    // The default table gives tier 0 (no tax) at food 0.
    assert_eq!(game.player_economy(0).unwrap().tax_percent, [0, 0]);
}

#[test]
fn a_misc_layer_sets_the_tables_and_lists_the_absent_keys() {
    let p = Profile::parse("[Misc]\nUpkeepGoldTax=0,0.25\nTrainRefundRate=0.5\nLowGoldAmount=80.6\n")
        .expect("fixture parses");
    let misc = MiscData::from_layers(&[&p], None);

    let mut game = Game::new();
    let setup = install_economy(&mut game, &misc);
    assert!(setup.from_misc);
    assert_eq!(setup.layers, 1);
    for key in [
        "UpkeepUsage",
        "UpkeepLumberTax",
        "ResearchRefundRate",
        "ReviveRefundRate",
    ] {
        assert!(setup.absent.contains(&key), "{key} is absent");
    }
    assert!(!setup.absent.contains(&"TrainRefundRate"));

    let c = game.economy_constants();
    assert_eq!(c.low_gold_amount, 80, "F2I of the real, truncated");
    assert_eq!(c.upkeep.thresholds, [0; 10], "an absent UpkeepUsage is an all-zero row");
    assert_eq!(c.upkeep.gold_tax[1].0, 0.25f32.to_bits());
    assert_eq!(c.train_refund_rate.0, 0.5f32.to_bits());
    assert_eq!(c.research_refund_rate.0, 0, "absent rate is 0.0");
    // Tier 0 at food 0: the Misc tax for tier 0 is 0.
    assert_eq!(game.player_economy(0).unwrap().tax_percent, [0, 0]);
}
