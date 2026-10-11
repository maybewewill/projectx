//! Extracts one file from an archive. Usage: `cargo run -p mpq --example mpqx -- archive.mpq "Name\\In\\Archive" out_file`
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [archive, name, out] = args.as_slice() else {
        eprintln!("usage: mpqx <archive.mpq> <name> <out_file>");
        std::process::exit(2);
    };
    let bytes = std::fs::read(archive).expect("read archive");
    let archive = mpq::Archive::open(&bytes).expect("open archive");
    let data = archive.read(name).expect("read file");
    std::fs::write(out, &data).expect("write output");
    println!("{name}: {} bytes -> {out}", data.len());
}
