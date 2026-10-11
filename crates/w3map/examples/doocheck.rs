//! Parses `war3map.doo` and `war3mapUnits.doo` of every map given and reports counts and failures.
//! Usage: `cargo run -p w3map --example doocheck -- <map-or-dir>...`
//! Directories are scanned for `*.w3m` and `*.w3x` (not recursive). Maps are only read.

use std::path::{Path, PathBuf};

fn collect(arg: &str, out: &mut Vec<PathBuf>) {
    let path = Path::new(arg);
    if path.is_dir() {
        let mut found: Vec<PathBuf> = std::fs::read_dir(path)
            .expect("read dir")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("w3m") || e.eq_ignore_ascii_case("w3x"))
            })
            .collect();
        found.sort();
        out.extend(found);
    } else {
        out.push(path.to_path_buf());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: doocheck <map-or-dir>...");
        std::process::exit(2);
    }
    let mut maps = Vec::new();
    for a in &args {
        collect(a, &mut maps);
    }

    let (mut ok_doo, mut ok_units, mut failed_maps) = (0usize, 0usize, 0usize);
    let (mut doodads, mut special, mut units, mut items, mut abilities) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for path in &maps {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                println!("{name}: read error {e}");
                failed_maps += 1;
                continue;
            }
        };
        let archive = match mpq::Archive::open(&bytes) {
            Ok(a) => a,
            Err(e) => {
                println!("{name}: not an archive: {e}");
                failed_maps += 1;
                continue;
            }
        };

        let doo = match archive.read("war3map.doo") {
            Ok(data) => match w3map::Doodads::parse(&data) {
                Ok(d) => {
                    ok_doo += 1;
                    doodads += d.doodads.len();
                    special += d.special.len();
                    format!("doodads {} special {}", d.doodads.len(), d.special.len())
                }
                Err(e) => format!("doodads FAILED: {e}"),
            },
            Err(_) => "no war3map.doo".to_string(),
        };

        let unit_status = match archive.read("war3mapUnits.doo") {
            Ok(data) => match w3map::PlacedUnits::parse(&data) {
                Ok(u) => {
                    ok_units += 1;
                    units += u.units.len();
                    items += u.units.iter().map(|x| x.items.len()).sum::<usize>();
                    abilities += u.units.iter().map(|x| x.abilities.len()).sum::<usize>();
                    format!("units {}", u.units.len())
                }
                Err(e) => format!("units FAILED: {e}"),
            },
            Err(_) => "no war3mapUnits.doo".to_string(),
        };

        if doo.contains("FAILED") || unit_status.contains("FAILED") {
            failed_maps += 1;
        }
        println!("{name}: {doo} | {unit_status}");
    }

    println!(
        "maps {} | doo ok {} | units ok {} | failed maps {} | doodads {} | special {} | units {} | unit items {} | unit abilities {}",
        maps.len(),
        ok_doo,
        ok_units,
        failed_maps,
        doodads,
        special,
        units,
        items,
        abilities
    );
}
