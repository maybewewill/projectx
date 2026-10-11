//! Loads the game's FDF frame files from MPQ archives and prints frame rects.
//!
//! ```text
//! fdfdump --mpq war3.mpq --mpq War3x.mpq --mpq War3Patch.mpq [--toc PATH]
//!         [--list] [--root NAME]... [--size WxH]...
//! ```
//!
//! Archives are searched last-to-first (a later `--mpq` wins, as the patch must).
//! Only the archives named on the command line are read.

use std::process::ExitCode;

use fdf::layout::{solve, ExternalFrames, Screen};
use fdf::{parse_toc, Library, Scene};

const DEFAULT_TOC: &str = "UI\\FrameDef\\FrameDef.toc";

struct Args {
    mpqs: Vec<String>,
    toc: String,
    /// Print one file from the archives and exit.
    cat: Option<String>,
    /// Write every TOC file under this directory (for reading the sources).
    dump: Option<String>,
    list: bool,
    roots: Vec<String>,
    sizes: Vec<(u32, u32)>,
}

fn parse_size(v: &str) -> Option<(u32, u32)> {
    let (w, h) = v.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        mpqs: Vec::new(),
        toc: DEFAULT_TOC.to_string(),
        cat: None,
        dump: None,
        list: false,
        roots: Vec::new(),
        sizes: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--list" => a.list = true,
            "--mpq" | "--toc" | "--root" | "--size" | "--cat" | "--dump" => {
                let v = it.next().ok_or_else(|| format!("{arg} needs a value"))?;
                match arg.as_str() {
                    "--mpq" => a.mpqs.push(v),
                    "--toc" => a.toc = v,
                    "--cat" => a.cat = Some(v),
                    "--dump" => a.dump = Some(v),
                    "--root" => a.roots.push(v),
                    _ => a
                        .sizes
                        .push(parse_size(&v).ok_or(format!("bad --size {v}, want WxH"))?),
                }
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if a.mpqs.is_empty() {
        return Err("give at least one --mpq".into());
    }
    if a.sizes.is_empty() {
        a.sizes.push((1280, 720));
    }
    Ok(a)
}

/// Reads a file from the archives, last archive first.
fn read_mpq(archives: &[mpq::Archive<'_>], name: &str) -> Option<Vec<u8>> {
    let name = name.replace('/', "\\");
    archives
        .iter()
        .rev()
        .find(|a| a.contains(&name))
        .and_then(|a| a.read(&name).ok())
}

fn depth_of(scene: &Scene, mut node: usize) -> usize {
    let mut d = 0;
    while let Some(p) = scene.nodes[node].parent {
        d += 1;
        node = p;
    }
    d
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("fdfdump: {e}");
            return ExitCode::from(2);
        }
    };
    let blobs: Vec<Vec<u8>> = args
        .mpqs
        .iter()
        .map(|p| std::fs::read(p).unwrap_or_else(|e| panic!("read {p}: {e}")))
        .collect();
    let archives: Vec<mpq::Archive<'_>> = blobs
        .iter()
        .zip(&args.mpqs)
        .map(|(b, p)| mpq::Archive::open(b).unwrap_or_else(|e| panic!("open {p}: {e}")))
        .collect();

    if let Some(name) = &args.cat {
        match read_mpq(&archives, name) {
            Some(b) => print!("{}", String::from_utf8_lossy(&b)),
            None => println!("not found: {name}"),
        }
        return ExitCode::SUCCESS;
    }

    let toc_bytes = read_mpq(&archives, &args.toc).unwrap_or_else(|| panic!("no {}", args.toc));
    let files = parse_toc(&String::from_utf8_lossy(&toc_bytes));
    println!("TOC {}: {} files", args.toc, files.len());

    if let Some(dir) = &args.dump {
        for f in &files {
            let Some(bytes) = read_mpq(&archives, f) else {
                println!("missing {f}");
                continue;
            };
            let rel = f.replace('\\', "/");
            let out = std::path::Path::new(dir).join(&rel);
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).expect("create dump directory");
            }
            std::fs::write(&out, bytes).expect("write dump file");
        }
        println!("dumped {} files to {dir}", files.len());
    }

    let mut lib = Library::new();
    let mut failures = 0;
    for f in &files {
        let before = lib.warnings.len();
        let mut loader =
            |p: &str| read_mpq(&archives, p).map(|b| String::from_utf8_lossy(&b).into_owned());
        if let Err(e) = lib.load(f, &mut loader) {
            failures += 1;
            println!("LOAD ERROR {f}: {e}");
        }
        for w in &lib.warnings[before..] {
            println!("WARN {f}: {w}");
        }
    }
    println!(
        "loaded {} files, {} load errors, {} top-level frames, {} strings",
        files.len(),
        failures,
        lib.frame_names().count(),
        lib.strings().count()
    );

    if args.list {
        for name in lib.frame_names() {
            println!("frame {name}");
        }
    }

    for root in &args.roots {
        for &(w, h) in &args.sizes {
            println!("== {root} at {w}x{h}");
            let scene = match Scene::build(&lib, &[root.as_str()]) {
                Ok(s) => s,
                Err(e) => {
                    println!("SCENE ERROR {root}: {e}");
                    failures += 1;
                    continue;
                }
            };
            for w in &scene.warnings {
                println!("SCENE WARN {w}");
            }
            let screen = Screen::for_window(w, h);
            let layout = match solve(&scene, &screen, &ExternalFrames::new()) {
                Ok(l) => l,
                Err(e) => {
                    println!("LAYOUT ERROR {root}: {e}");
                    failures += 1;
                    continue;
                }
            };
            for (i, node) in scene.nodes.iter().enumerate() {
                let indent = depth_of(&scene, i) * 2;
                match layout.rect(i) {
                    Some(r) => println!(
                        "{:indent$}{} {} [{:.4}, {:.4}, {:.4}, {:.4}] {:.4}x{:.4}",
                        "",
                        node.type_name,
                        node.name,
                        r.left,
                        r.bottom,
                        r.right,
                        r.top,
                        r.width(),
                        r.height(),
                    ),
                    None => println!(
                        "{:indent$}{} {} (unpositioned)",
                        "", node.type_name, node.name
                    ),
                }
            }
            for w in &layout.warnings {
                println!("LAYOUT WARN {w:?}");
            }
        }
    }
    if failures > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
