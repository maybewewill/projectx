//! The economy keys of `[Misc]` (`UpkeepUsage`, `UpkeepGoldTax`, `UpkeepLumberTax`, the refund rates, `LowGoldAmount`)
//! and the `atoi` rule of the integer list. Synthetic layers only; the S2R check needs `GAME_DLL` and is in `misc.rs`.

use gamedata::misc::atoi;
use gamedata::{MiscData, Profile};
use softfloat::F32;

const ECONOMY: [&str; 7] = [
    "UpkeepUsage",
    "UpkeepGoldTax",
    "UpkeepLumberTax",
    "TrainRefundRate",
    "ResearchRefundRate",
    "ReviveRefundRate",
    "LowGoldAmount",
];

fn layer(text: &str) -> Profile {
    Profile::parse(text).expect("fixture parses")
}

fn f(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

#[test]
fn economy_keys_are_read_as_lists_and_reals() {
    let p = layer(
        "[Misc]\n\
         UpkeepUsage=40,70,10000,10000,10000,10000,10000,10000,10000,10000\n\
         UpkeepGoldTax=0,0.3,0.6,0.6,0.6,0.6,0.6,0.6,0.6,0.6\n\
         UpkeepLumberTax=0,0,0.1\n\
         TrainRefundRate=0.5\n\
         ResearchRefundRate=0.25\n\
         ReviveRefundRate=0.75\n\
         LowGoldAmount=100\n",
    );
    let m = MiscData::from_layers(&[&p], None);
    assert_eq!(
        m.upkeep_usage,
        [40, 70, 10000, 10000, 10000, 10000, 10000, 10000, 10000, 10000]
    );
    assert_eq!(m.upkeep_gold_tax[1], f(0.3));
    assert_eq!(m.upkeep_gold_tax[9], f(0.6));
    assert_eq!(m.upkeep_lumber_tax[2], f(0.1));
    assert_eq!(m.upkeep_lumber_tax[3], F32::ZERO, "a short list leaves the tail at 0.0");
    assert_eq!(m.train_refund_rate, f(0.5));
    assert_eq!(m.research_refund_rate, f(0.25));
    assert_eq!(m.revive_refund_rate, f(0.75));
    assert_eq!(m.low_gold_amount, f(100.0));
    for key in ECONOMY {
        assert!(!m.defaulted.contains(&key), "{key} is set, not defaulted");
    }
}

#[test]
fn absent_economy_keys_read_as_zero_and_are_recorded() {
    let p = layer("[Misc]\nDefenseArmor=0.06\n");
    let m = MiscData::from_layers(&[&p], None);
    assert_eq!(m.upkeep_usage, [0; 10]);
    assert!(m.upkeep_gold_tax.iter().all(|&t| t == F32::ZERO));
    assert!(m.upkeep_lumber_tax.iter().all(|&t| t == F32::ZERO));
    assert_eq!(m.train_refund_rate, F32::ZERO);
    assert_eq!(m.research_refund_rate, F32::ZERO);
    assert_eq!(m.revive_refund_rate, F32::ZERO);
    assert_eq!(m.low_gold_amount, F32::ZERO);
    for key in ECONOMY {
        assert!(m.defaulted.contains(&key), "{key} is absent");
    }
    let none = MiscData::from_layers(&[], None);
    assert_eq!(none.upkeep_usage, [0; 10]);
    assert_eq!(none.layers_with_misc, 0);
}

#[test]
fn integers_follow_atoi_and_a_long_list_is_noted() {
    assert_eq!(atoi("12"), 12);
    assert_eq!(atoi("  7"), 7);
    assert_eq!(atoi("-3"), -3);
    assert_eq!(atoi("+5"), 5);
    assert_eq!(atoi("12abc"), 12);
    assert_eq!(atoi("abc"), 0);
    assert_eq!(atoi(""), 0);

    let p = layer("[Misc]\nUpkeepUsage=40,x,7\n");
    let m = MiscData::from_layers(&[&p], None);
    assert_eq!(m.upkeep_usage, [40, 0, 7, 0, 0, 0, 0, 0, 0, 0]);

    let long = layer("[Misc]\nUpkeepUsage=1,2,3,4,5,6,7,8,9,10,11,12\n");
    let m = MiscData::from_layers(&[&long], None);
    assert_eq!(m.upkeep_usage, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    assert!(m.notes.iter().any(|n| n.starts_with("UpkeepUsage")));
}

#[test]
fn a_later_layer_overrides_an_economy_key() {
    let game = layer("[Misc]\nTrainRefundRate=0.5\n");
    let map = layer("[Misc]\nTrainRefundRate=0.25\n");
    let m = MiscData::from_layers(&[&game, &map], None);
    assert_eq!(m.train_refund_rate, f(0.25));
}
