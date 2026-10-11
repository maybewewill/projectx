//! Reads every file named in an archive's `(listfile)` and reports failures by error kind.
//! Usage: `cargo run -p mpq --example mpqcheck -- archive.mpq [more.mpq ...]`
use std::collections::BTreeMap;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: mpqcheck <archive.mpq>...");
        std::process::exit(2);
    }
    for path in paths {
        let bytes = std::fs::read(&path).expect("read archive");
        let archive = match mpq::Archive::open(&bytes) {
            Ok(a) => a,
            Err(e) => {
                println!("{path}: open failed: {e}");
                continue;
            }
        };
        let names = match archive.listfile() {
            Ok(n) => n,
            Err(e) => {
                println!("{path}: no listfile: {e}");
                continue;
            }
        };
        let (mut ok, mut missing, mut bytes_out) = (0usize, 0usize, 0u64);
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for name in &names {
            if !archive.contains(name) {
                missing += 1;
                continue;
            }
            match archive.read(name) {
                Ok(data) => {
                    ok += 1;
                    bytes_out += data.len() as u64;
                }
                Err(e) => {
                    let key = e.to_string();
                    let key = key.split(':').next().unwrap_or(&key).to_string();
                    errors.entry(key).or_default().push(name.clone());
                }
            }
        }
        let failed: usize = errors.values().map(Vec::len).sum();
        println!(
            "{path}: {} names, {ok} read ({bytes_out} bytes), {failed} failed, {missing} not in hash table",
            names.len()
        );
        for (kind, files) in &errors {
            println!(
                "  {kind}: {} (e.g. {})",
                files.len(),
                files[..files.len().min(3)].join(", ")
            );
        }
    }
}
