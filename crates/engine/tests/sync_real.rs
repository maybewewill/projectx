//! The 0x22 sync records of a real 1.26a replay (never committed): `W3G_REPLAY=/path/file.w3g` runs it, otherwise
//! it returns. The net checksum and the tick-clock model must explain the record differences wherever the hidden
//! words (presence/birth tags, generator, data slots) did not change (docs/spec/lockstep.md section 15.3).

use engine::sync_check::check_replay;

#[test]
fn real_replay_pairs_are_predicted_by_clock_and_net_checksum() {
    let Ok(path) = std::env::var("W3G_REPLAY") else {
        return;
    };
    let bytes = std::fs::read(&path).expect("read W3G_REPLAY");
    let replay = w3g::Replay::parse(&bytes).expect("real replay parses");
    let report = check_replay(&replay);
    println!("{}", report.render());
    assert!(report.computed > 0);
    assert_eq!(report.net_rotation, 21);
    // With a wrong net checksum, clock model or rotation no pair of a real replay has a zero residual (chance
    // about 2^-32 per pair). The 1.26a DotA replay used for the decode has several hundred.
    assert!(
        report.exact_pairs > 0,
        "no record pair is explained by the net checksum and the clock\n{}",
        report.render()
    );
    // The absolute prediction needs the hidden words; the start state must not match by accident.
    assert!(report.first_absolute_mismatch.is_some());
}
