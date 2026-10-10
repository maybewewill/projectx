//! Real literals of the JASS scanner. The original converts them with its soft-float helpers
//! (`0x6F466690`, spec jass-frontend 1.5), so `0.1` is `0x3DCCCCCE`, not the correctly rounded
//! `0x3DCCCCCD`. With the lookup tables (`GAME_DLL`) the lexer must give those bits.

use std::sync::OnceLock;

use jass::lexer::{lex, lex_with, Tok};
use softfloat::Tables;

fn tables() -> Option<Tables> {
    static TABLES: OnceLock<Option<Tables>> = OnceLock::new();
    TABLES
        .get_or_init(|| {
            let path = std::env::var("GAME_DLL").ok()?;
            let bytes = std::fs::read(path).ok()?;
            Tables::from_game_dll(&bytes).ok()
        })
        .clone()
}

fn real_bits(src: &str, t: Option<&Tables>) -> Vec<u32> {
    lex_with(src, t)
        .unwrap()
        .into_iter()
        .filter_map(|t| match t.tok {
            Tok::Real(f) => Some(f.to_bits()),
            _ => None,
        })
        .collect()
}

#[test]
fn real_literals_use_the_soft_float_path() {
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: skipping the soft-float real literal check");
        return;
    };
    assert_eq!(
        real_bits("0.1 1.5 3.25 5. .25 1000000.0", Some(&t)),
        vec![
            0x3DCC_CCCE,
            0x3FC0_0000,
            0x4050_0000,
            0x40A0_0000,
            0x3E80_0000,
            0x4974_2400
        ]
    );
}

#[test]
fn real_literals_without_tables_are_correctly_rounded() {
    // No table: the lexer falls back to Rust's parser (documented in `lex`).
    assert_eq!(real_bits("0.1", None), vec![0.1f32.to_bits()]);
    assert_eq!(lex("0.5").unwrap()[0].tok, Tok::Real(0.5));
}
