//! Type-checks JASS files as one program set (in the order given) and prints every diagnostic.
//! Usage: `cargo run -p jass --example jcheck -- common.j Blizzard.j [war3map.j]`
fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: jcheck <file.j>...");
        std::process::exit(2);
    }
    let mut programs = Vec::new();
    for path in &paths {
        let bytes = std::fs::read(path).expect("read script");
        let src = String::from_utf8_lossy(&bytes);
        match jass::parse(&src) {
            Ok(p) => programs.push(p),
            Err(e) => {
                println!("{path}: parse error: {e}");
                std::process::exit(1);
            }
        }
    }
    let diags = jass::check::check(&programs);
    for d in &diags {
        println!("line {}: {}", d.line, d.message);
    }
    println!("{} diagnostic(s)", diags.len());
}
