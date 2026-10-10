//! `w3boot`: boots a map and prints a short report.
//!
//! ```text
//! w3boot [--dll Game.dll] (--common common.j --blizzard blizzard.j | --mpq War3.mpq [--mpq War3x.mpq])
//!        [--steps N] map.w3x
//! ```
//!
//! `--mpq` reads `Scripts\common.j` and `Scripts\Blizzard.j` from the archives (later ones override).

use std::process::ExitCode;

use engine::{Engine, Inputs};

const USAGE: &str = "usage: w3boot [--dll Game.dll] (--common common.j --blizzard blizzard.j | \
--mpq War3.mpq [--mpq War3x.mpq]) [--steps N] map.w3x";

#[derive(Debug, Default)]
struct Args {
    dll: Option<String>,
    common: Option<String>,
    blizzard: Option<String>,
    mpqs: Vec<String>,
    steps: u64,
    map: Option<String>,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--dll" => args.dll = Some(value("--dll")?),
            "--common" => args.common = Some(value("--common")?),
            "--blizzard" => args.blizzard = Some(value("--blizzard")?),
            "--mpq" => args.mpqs.push(value("--mpq")?),
            "--steps" => {
                let v = value("--steps")?;
                args.steps = v
                    .parse()
                    .map_err(|_| format!("--steps: not a number: {v}"))?;
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path => {
                if args.map.replace(path.to_string()).is_some() {
                    return Err("more than one map file given".to_string());
                }
            }
        }
    }
    if args.map.is_none() {
        return Err("no map file given".to_string());
    }
    let have_paths = args.common.is_some() && args.blizzard.is_some();
    if !have_paths && args.mpqs.is_empty() {
        return Err("give --common and --blizzard, or --mpq".to_string());
    }
    if args.common.is_some() != args.blizzard.is_some() {
        return Err("--common and --blizzard go together".to_string());
    }
    Ok(args)
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn inputs(args: &Args) -> Result<Inputs, String> {
    let map = read(args.map.as_deref().unwrap_or_default())?;
    let game_dll = args.dll.as_deref().map(read).transpose()?;
    let (common_j, blizzard_j) = match (&args.common, &args.blizzard) {
        (Some(c), Some(b)) => (
            String::from_utf8_lossy(&read(c)?).into_owned(),
            String::from_utf8_lossy(&read(b)?).into_owned(),
        ),
        _ => {
            let archives = args
                .mpqs
                .iter()
                .map(|p| read(p))
                .collect::<Result<Vec<_>, _>>()?;
            let refs: Vec<&[u8]> = archives.iter().map(Vec::as_slice).collect();
            engine::scripts_from_archives(&refs).map_err(|e| e.to_string())?
        }
    };
    Ok(Inputs {
        map,
        game_dll,
        common_j,
        blizzard_j,
    })
}

fn run(argv: &[String]) -> Result<(), String> {
    let args = parse_args(argv)?;
    let mut engine = Engine::boot(inputs(&args)?).map_err(|e| e.to_string())?;
    for _ in 0..args.steps {
        engine.step();
    }
    print!("{}", engine.report());
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("w3boot: {e}");
            ExitCode::FAILURE
        }
    }
}
