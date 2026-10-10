//! Usage: cargo run -p mpq --example mpqls -- archive.mpq [file-to-extract out-path]
use mpq::Archive;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = std::fs::read(&args[0]).expect("read archive");
    let a = Archive::open(&data).expect("open archive");
    if let [_, name, out] = args.as_slice() {
        std::fs::write(out, a.read(name).expect("read file")).expect("write");
        return;
    }
    println!("{:?}", a.header);
    match a.listfile() {
        Ok(names) => {
            let mut bad = 0;
            for n in &names {
                match a.read(n) {
                    Ok(d) => println!("{:>10}  {n}", d.len()),
                    Err(e) => {
                        bad += 1;
                        println!("{:>10}  {n}  ({e})", "ERR");
                    }
                }
            }
            eprintln!("{} files, {bad} failed", names.len());
        }
        Err(e) => eprintln!("no (listfile): {e}"),
    }
}
