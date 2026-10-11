//! Seed link on a real 1.26a replay (never committed): `W3G_REPLAY=/path/file.w3g` runs it, otherwise it returns.
//! The real file's settings flags have neither fixed-seed bit (0x8000, 0x80000), so the setup seed must be the
//! `0x19` record seed unchanged (docs/spec/game-start.md section 2, docs/spec/replay-load.md section 5).

use engine::replay_setup::setup_of;

#[test]
fn real_replay_seed_is_the_record_seed_when_no_fixed_seed_bit_is_set() {
    let Ok(path) = std::env::var("W3G_REPLAY") else {
        return;
    };
    let bytes = std::fs::read(&path).expect("read W3G_REPLAY");
    let replay = w3g::Replay::parse(&bytes).expect("real replay parses");
    let setup = setup_of(&replay).expect("real replay sets up");
    let flags = setup.settings_flags;
    assert_eq!(setup.record_seed, replay.game.random_seed);
    if flags & (0x8000 | 0x80000) == 0 {
        assert_eq!(setup.seed, setup.record_seed, "flags {flags:#010x}");
    }
    assert_eq!(setup.slots.len(), replay.game.slots.len());
}
