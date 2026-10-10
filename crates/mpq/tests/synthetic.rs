//! Writes small MPQ archives in memory with the format's own rules and reads them back.

use std::io::Write;

use mpq::crypto::{encrypt_bytes, file_key, hash_string};
use mpq::*;

struct Entry {
    name: &'static str,
    data: Vec<u8>,
    flags: u32,
    /// 0x02 zlib, 0x10 bzip2, 0 = store.
    method: u8,
}

fn compress(method: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![method];
    match method {
        0x02 => {
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
            z.write_all(data).unwrap();
            out.extend(z.finish().unwrap());
        }
        0x10 => {
            let mut z = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
            z.write_all(data).unwrap();
            out.extend(z.finish().unwrap());
        }
        _ => unreachable!(),
    }
    // The format stores a sector raw when compression does not help.
    if out.len() >= data.len() {
        data.to_vec()
    } else {
        out
    }
}

/// Encodes a file body (sector table + sectors) as stored at `offset`.
fn encode_file(e: &Entry, offset: u32, sector: usize) -> Vec<u8> {
    let key = (e.flags & FLAG_ENCRYPTED != 0).then(|| {
        file_key(
            e.name,
            offset,
            e.data.len() as u32,
            e.flags & FLAG_FIX_KEY != 0,
        )
    });
    if e.flags & FLAG_SINGLE_UNIT != 0 {
        let mut body = if e.method == 0 {
            e.data.clone()
        } else {
            compress(e.method, &e.data)
        };
        if let Some(k) = key {
            encrypt_bytes(&mut body, k);
        }
        return body;
    }
    let chunks: Vec<&[u8]> = e.data.chunks(sector).collect();
    let mut sectors: Vec<Vec<u8>> = chunks
        .iter()
        .map(|c| {
            if e.method == 0 {
                c.to_vec()
            } else {
                compress(e.method, c)
            }
        })
        .collect();
    if let Some(k) = key {
        for (i, s) in sectors.iter_mut().enumerate() {
            encrypt_bytes(s, k.wrapping_add(i as u32));
        }
    }
    let mut out = Vec::new();
    if e.flags & FLAG_COMPRESS != 0 {
        let mut table = Vec::new();
        let mut pos = (sectors.len() + 1) * 4;
        table.extend((pos as u32).to_le_bytes());
        for s in &sectors {
            pos += s.len();
            table.extend((pos as u32).to_le_bytes());
        }
        if let Some(k) = key {
            encrypt_bytes(&mut table, k.wrapping_sub(1));
        }
        out.extend(table);
    }
    for s in sectors {
        out.extend(s);
    }
    out
}

fn build(entries: &[Entry], prefix: usize) -> Vec<u8> {
    let shift = 0u16; // 512-byte sectors keep test files multi-sector
    let sector = 512usize << shift;
    let hash_n = 16usize;
    let mut body = Vec::new();
    let mut blocks = Vec::new();
    let mut hashes = vec![[0xFFFF_FFFFu32; 4]; hash_n];
    for (bi, e) in entries.iter().enumerate() {
        let offset = (32 + body.len()) as u32;
        let enc = encode_file(e, offset, sector);
        blocks.push([
            offset,
            enc.len() as u32,
            e.data.len() as u32,
            e.flags | FLAG_EXISTS,
        ]);
        body.extend(enc);
        let mut i = hash_string(e.name, 0) as usize % hash_n;
        while hashes[i][3] != 0xFFFF_FFFF {
            i = (i + 1) % hash_n;
        }
        hashes[i] = [hash_string(e.name, 1), hash_string(e.name, 2), 0, bi as u32];
    }
    let mut ht: Vec<u8> = hashes
        .iter()
        .flat_map(|h| h.iter().flat_map(|v| v.to_le_bytes()))
        .collect();
    let mut bt: Vec<u8> = blocks
        .iter()
        .flat_map(|b| b.iter().flat_map(|v| v.to_le_bytes()))
        .collect();
    encrypt_bytes(&mut ht, hash_string("(hash table)", 3));
    encrypt_bytes(&mut bt, hash_string("(block table)", 3));
    let ht_pos = 32 + body.len();
    let bt_pos = ht_pos + ht.len();
    let mut f = vec![0u8; prefix];
    f.extend(b"MPQ\x1a");
    f.extend(32u32.to_le_bytes());
    f.extend(((bt_pos + bt.len()) as u32).to_le_bytes());
    f.extend(0u16.to_le_bytes());
    f.extend(shift.to_le_bytes());
    f.extend((ht_pos as u32).to_le_bytes());
    f.extend((bt_pos as u32).to_le_bytes());
    f.extend((hash_n as u32).to_le_bytes());
    f.extend((blocks.len() as u32).to_le_bytes());
    f.extend(body);
    f.extend(ht);
    f.extend(bt);
    f
}

fn sample(len: usize, seed: u32) -> Vec<u8> {
    // Compressible but not trivially uniform.
    (0..len)
        .map(|i| ((i as u32 / 7).wrapping_mul(seed) >> 3) as u8)
        .collect()
}

#[test]
fn reads_all_storage_variants() {
    let entries = vec![
        Entry {
            name: "(listfile)",
            data: b"war3map.j\r\nUnits\\UnitData.slk\r\nraw.bin\r\nsingle.txt\r\n".to_vec(),
            flags: FLAG_COMPRESS,
            method: 0x02,
        },
        Entry {
            name: "war3map.j",
            data: sample(3000, 31),
            flags: FLAG_COMPRESS | FLAG_ENCRYPTED | FLAG_FIX_KEY,
            method: 0x02,
        },
        Entry {
            name: "Units\\UnitData.slk",
            data: sample(1500, 17),
            flags: FLAG_COMPRESS | FLAG_ENCRYPTED,
            method: 0x10,
        },
        Entry {
            name: "raw.bin",
            data: sample(1100, 5),
            flags: 0,
            method: 0,
        },
        Entry {
            name: "single.txt",
            data: sample(900, 3),
            flags: FLAG_COMPRESS | FLAG_SINGLE_UNIT | FLAG_ENCRYPTED,
            method: 0x02,
        },
    ];
    // A map file puts a 512-byte HM3W header before the archive.
    for prefix in [0, 512] {
        let file = build(&entries, prefix);
        let a = Archive::open(&file).unwrap();
        for e in &entries {
            assert_eq!(
                a.read(e.name).unwrap(),
                e.data,
                "{} (prefix {prefix})",
                e.name
            );
        }
        assert_eq!(a.read("units/unitdata.SLK").unwrap(), entries[2].data);
        assert!(matches!(a.read("missing.txt"), Err(Error::NotFound(_))));
        assert_eq!(a.listfile().unwrap().len(), 4);
    }
}

#[test]
fn rejects_non_mpq() {
    assert!(matches!(Archive::open(&[0u8; 2048]), Err(Error::NoHeader)));
}
