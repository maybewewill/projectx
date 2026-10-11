//! Lists every hash-table entry whose name hashes match `name` (all locales and platforms, in
//! table order), with the probe start and the block record. For protected maps that hide files.
//! Usage: `cargo run -p mpq --example mpqfind -- archive.mpq "Name\In\Archive"`
use mpq::crypto::{hash_string, HASH_NAME_A, HASH_NAME_B, HASH_TABLE_OFFSET};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [archive, name] = args.as_slice() else {
        eprintln!("usage: mpqfind <archive.mpq> <name>");
        std::process::exit(2);
    };
    let bytes = std::fs::read(archive).expect("read archive");
    let a = mpq::Archive::open(&bytes).expect("open archive");
    let n = a.hash_table.len();
    let (ha, hb) = (hash_string(name, HASH_NAME_A), hash_string(name, HASH_NAME_B));
    println!(
        "hash table {n} entries, block table {}, probe start {}",
        a.block_table.len(),
        hash_string(name, HASH_TABLE_OFFSET) as usize % n.max(1)
    );
    for (i, e) in a.hash_table.iter().enumerate() {
        if e.name_a == ha && e.name_b == hb {
            let b = a.block_table.get(e.block_index as usize);
            println!(
                "slot {i}: locale {:#x} platform {:#x} block {:#x} -> {b:?}",
                e.locale, e.platform, e.block_index
            );
        }
    }
    println!("find() picks block {:?}", a.find(name));
}
