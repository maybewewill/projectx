//! Parses every `.mdx` in each archive, evaluates the first sequence of each model, and
//! reports failures grouped by error kind.
//!
//! Names come from the archive's own `(listfile)`. An archive without one (War3Patch.mpq)
//! uses the union of all listfiles given on the command line and only counts names it holds.
//! Usage: `cargo run -p mdx --example mdxcheck -- archive.mpq [more.mpq ...]`
use std::collections::{BTreeMap, BTreeSet};

use mdx::{Model, Skeleton};

/// Error kind: the message with hex offsets and digits replaced by `#`, so offsets and sizes
/// group together. Set `MDXCHECK_VERBOSE=1` to print the full message of every failure.
fn kind_of(message: &str) -> String {
    let mut out = String::new();
    let mut in_hex = false;
    for c in message.chars() {
        if c == 'x' && out.ends_with('0') {
            out.pop();
            out.push_str("0x");
            in_hex = true;
        } else if in_hex && c.is_ascii_hexdigit() {
            if !out.ends_with('#') {
                out.push('#');
            }
        } else {
            in_hex = false;
            out.push(if c.is_ascii_digit() { '#' } else { c });
        }
    }
    out
}

/// Parses one model and evaluates its first sequence. Returns the failure kind, if any.
fn check(bytes: &[u8]) -> Result<(), String> {
    let model = Model::parse(bytes).map_err(|e| kind_of(&e.to_string()))?;
    if std::env::var_os("MDXCHECK_STATS").is_some() {
        // One line per raw value, for `sort | uniq -c` over the whole run.
        for s in &model.sequences {
            let base = s.name.split(' ').next().unwrap_or("");
            eprintln!("STAT seq_flags={} base={}", s.flags, base);
        }
        for t in &model.textures {
            eprintln!("STAT tex_flags={} replaceable={} empty_path={}", t.flags, t.replaceable_id, t.path.is_empty());
        }
        for g in &model.geosets {
            eprintln!("STAT geoset_sel_flags={} sel_group={}", g.selection_flags, g.selection_group);
        }
        for m in &model.materials {
            eprintln!("STAT mat_flags={}", m.flags);
            for l in &m.layers {
                eprintln!("STAT layer_filter={} shading={}", l.filter_mode, l.shading_flags);
            }
        }
    }
    let skeleton = Skeleton::new(&model)
        .map_err(|e| format!("skeleton: {}", kind_of(&e.to_string())))?;
    if let Some(seq) = model.sequences.first() {
        let duration = seq.duration();
        for t in [0, duration / 2, duration] {
            if skeleton.evaluate_sequence(&model, 0, t as f32, None).is_none() {
                return Err("evaluate: no pose".to_string());
            }
        }
    }
    Ok(())
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: mdxcheck <archive.mpq>...");
        std::process::exit(2);
    }

    // Union of every listfile, used for archives that have none.
    let mut union = BTreeSet::new();
    for path in &paths {
        let bytes = std::fs::read(path).expect("read archive");
        if let Ok(archive) = mpq::Archive::open(&bytes) {
            if let Ok(names) = archive.listfile() {
                union.extend(names);
            }
        }
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
        let own = archive.listfile();
        let (names, from_union) = match own {
            Ok(n) => (n, false),
            Err(_) => (union.iter().cloned().collect(), true),
        };
        let (mut ok, mut total, mut missing) = (0usize, 0usize, 0usize);
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for name in names.iter().filter(|n| n.to_ascii_lowercase().ends_with(".mdx")) {
            if !archive.contains(name) {
                missing += 1;
                continue;
            }
            total += 1;
            let data = match archive.read(name) {
                Ok(d) => d,
                Err(e) => {
                    let kind = format!("mpq: {}", kind_of(&e.to_string()));
                    errors.entry(kind).or_default().push(name.clone());
                    continue;
                }
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&data)))
                .unwrap_or_else(|_| Err("PANIC".to_string()));
            match outcome {
                Ok(()) => ok += 1,
                Err(message) => {
                    if std::env::var_os("MDXCHECK_VERBOSE").is_some() {
                        eprintln!("{name}: {message}");
                    }
                    errors.entry(kind_of(&message)).or_default().push(name.clone())
                }
            }
        }
        let failed: usize = errors.values().map(Vec::len).sum();
        let source = if from_union { "union listfile" } else { "own listfile" };
        println!(
            "{path}: {total} .mdx files ({source}), {ok} ok, {failed} failed, {missing} names not in archive"
        );
        for (kind, files) in &errors {
            println!("  {kind}: {} (e.g. {})", files.len(), files[..files.len().min(3)].join(", "));
        }
    }
}
