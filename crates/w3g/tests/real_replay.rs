//! Checks on a real 1.26a replay supplied by the user. The file is never committed:
//! set `W3G_REPLAY=/path/file.w3g` to run (the test returns early without it).

#[test]
fn real_replay_parses_and_every_action_decodes() {
    let Ok(path) = std::env::var("W3G_REPLAY") else {
        return;
    };
    let bytes = std::fs::read(&path).expect("read replay");
    let replay = w3g::Replay::parse(&bytes).expect("parse replay");
    let (mut blocks, mut actions) = (0usize, 0usize);
    for (_, cmds) in replay.ticks() {
        for c in cmds {
            blocks += 1;
            actions += c
                .decode()
                .unwrap_or_else(|e| panic!("player {}: {e}", c.player_id))
                .len();
        }
    }
    eprintln!("real replay: {blocks} command blocks, {actions} actions decoded");
    assert!(blocks > 0);
}
