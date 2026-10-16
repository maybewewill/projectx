//! The full-engine 0x22 oracle on a real replay (never committed). Needs `W3G_REPLAY=file.w3g`, `W3G_MAP=map.w3x`,
//! `GAME_DLL=Game.dll` and `W3G_MPQS=a.mpq;b.mpq;...` (later archives override); without them the test returns.
//! `docs/spec/lockstep.md` section 18.

use engine::cli::{inputs, BootArgs};

#[test]
fn the_simulation_runs_a_real_replay_and_the_report_is_consistent() {
    let (Ok(replay), Ok(map), Ok(dll), Ok(mpqs)) = (
        std::env::var("W3G_REPLAY"),
        std::env::var("W3G_MAP"),
        std::env::var("GAME_DLL"),
        std::env::var("W3G_MPQS"),
    ) else {
        return;
    };
    let args = BootArgs {
        map: Some(map),
        dll: Some(dll),
        mpqs: mpqs.split(';').filter(|s| !s.is_empty()).map(str::to_string).collect(),
        replay: Some(replay.clone()),
        ..BootArgs::default()
    };
    let bytes = std::fs::read(&replay).expect("read W3G_REPLAY");
    let parsed = w3g::Replay::parse(&bytes).expect("real replay parses");
    let (mut engine, _setup) =
        engine::Engine::boot_from_replay(&bytes, inputs(&args).expect("inputs")).expect("boot");
    let opts = engine::sim_sync::Options { max_slots: 300, ..Default::default() };
    let report = engine::sim_sync::run(&mut engine, &parsed, &opts);
    println!("{}", report.render());
    assert_eq!(report.time_slots, 300);
    assert!(report.computed > 0);
    assert!(report.command_errors.is_empty(), "{:?}", report.command_errors);
    assert!(report.game_seconds > 0.0);
    // The timeline rows are always the four classes, and the matched counts cannot exceed the event counts.
    assert_eq!(report.timeline.len(), 4);
    for t in &report.timeline {
        assert!(t.real_matched <= t.real && t.sim_matched <= t.sim);
    }
    // The stream-only part (clock with the pause rules + net checksum) must explain some pairs exactly.
    let stream = engine::sync_check::check_replay(&parsed);
    assert!(stream.exact_pairs > 0, "{}", stream.render());
}
