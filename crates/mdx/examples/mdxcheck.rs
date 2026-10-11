//! Parses every `.mdx` named in the `(listfile)` of the given archives and reports failures by error kind.
//! Usage: `cargo run -p mdx --example mdxcheck -- war3.mpq [War3x.mpq ...]` (later archives override earlier ones).
use std::collections::BTreeMap;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: mdxcheck <archive.mpq>...");
        std::process::exit(2);
    }
    let datas: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).expect("read archive")).collect();
    let archives: Vec<mpq::Archive> = datas
        .iter()
        .map(|d| mpq::Archive::open(d).expect("open archive"))
        .collect();
    let mut names = std::collections::BTreeSet::new();
    for a in &archives {
        if let Ok(list) = a.listfile() {
            for n in list {
                if n.to_ascii_lowercase().ends_with(".mdx") {
                    names.insert(n);
                }
            }
        }
    }
    let (mut ok, mut bad) = (0usize, 0usize);
    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in &names {
        let Some(data) = archives.iter().rev().find_map(|a| a.contains(name).then(|| a.read(name).ok()).flatten())
        else {
            continue;
        };
        match mdx::Model::parse(&data) {
            Ok(m) => {
                ok += 1;
                // sanity: parents are -1 or the object id of another node, and bone geoset ids are -1 or in range
                let ids: std::collections::BTreeSet<i32> = m
                    .bones
                    .iter()
                    .map(|b| b.node.object_id)
                    .chain(m.helpers.iter().map(|h| h.object_id))
                    .collect();
                let bad_parent = m
                    .bones
                    .iter()
                    .map(|b| b.node.parent_id)
                    .chain(m.helpers.iter().map(|h| h.parent_id))
                    .any(|p| p != -1 && p != 0xFFFF && !ids.contains(&p));
                let bad_geoset = m
                    .bones
                    .iter()
                    .any(|b| b.geoset_id < -1 || b.geoset_id > m.geosets.len() as i32 + 1);
                if bad_parent || bad_geoset {
                    errors
                        .entry(format!("suspicious skeleton (parent {bad_parent}, geoset {bad_geoset})"))
                        .or_default()
                        .push(name.clone());
                }
            }
            Err(e) => {
                bad += 1;
                let key = e.to_string();
                let key = key.split(" at ").next().unwrap_or(&key).to_string();
                errors.entry(key).or_default().push(name.clone());
            }
        }
    }
    println!("parsed {ok} models, {bad} failed");
    for (k, v) in &errors {
        println!("{k}: {} (e.g. {})", v.len(), v.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
    }
}
