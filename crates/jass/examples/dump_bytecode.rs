//! Compiles one JASS file and prints the instruction stream as decimal word pairs, one instruction
//! per line: `w0 w1` where `w0` packs `b0 b1 b2 op` (little-endian) and `w1` is the immediate.
//! This is the format `tools/emu/jasscomp/parity.py` compares with the original compiler.
//!
//! Usage: `cargo run -p jass --example dump_bytecode -- file.j`
//! With `GAME_DLL=/path/Game.dll` the soft-float tables are loaded, so real literals get the
//! original's bits. Compile errors print `ERROR line=<n>` (or `ERROR parse line=<n>`) and exit 1.

use softfloat::Tables;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: dump_bytecode FILE.j");
    let src = std::fs::read_to_string(&path).expect("read source");
    let tables = std::env::var("GAME_DLL").ok().map(|p| {
        let bytes = std::fs::read(&p).expect("read GAME_DLL");
        Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL")
    });
    let prog = match jass::parse_with(&src, tables.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            let line = match e {
                jass::Error::Lex { line, .. } | jass::Error::Parse { line, .. } => line,
            };
            println!("ERROR parse line={line}");
            std::process::exit(1);
        }
    };
    match jass::compile::compile(&[prog]) {
        Ok(m) => {
            for ins in m.stream() {
                let b = ins.to_bytes();
                let w0 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                let w1 = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
                println!("{w0} {w1}");
            }
        }
        Err(jass::compile::CompileError::Check(d)) => {
            println!("ERROR line={}", d[0].line);
            std::process::exit(1);
        }
        Err(e) => {
            println!("ERROR {e}");
            std::process::exit(1);
        }
    }
}
