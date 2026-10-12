//! Prints the sequences and GEOA entries of one MDX file, and the idle geoset visibility.
//! Usage: `cargo run -p mdx --example geoa -- model.mdx`
use mdx::Model;

fn main() {
    let path = std::env::args().nth(1).expect("usage: geoa model.mdx");
    let model = Model::parse(&std::fs::read(&path).expect("read")).expect("parse");
    for (i, s) in model.sequences.iter().enumerate() {
        println!("seq {i}: {:?} {}..{} flags {}", s.name, s.start, s.end, s.flags);
    }
    println!("geosets: {}", model.geosets.len());
    for (i, g) in model.geoset_anims.iter().enumerate() {
        println!("geoa {i}: geoset {} static alpha {} flags {}", g.geoset_id, g.alpha, g.flags);
        if let Some(t) = &g.alpha_track {
            println!("  KGAO interp {:?} global {:?}", t.interpolation, t.global_sequence);
            for k in &t.keys {
                println!("    t={} v={}", k.time, k.value);
            }
        }
    }
    let vis = model.idle_geoset_visibility();
    println!("idle visibility: {vis:?}");
    for (i, g) in model.geosets.iter().enumerate().filter(|(i, _)| vis[*i]) {
        let layers = model.materials.get(g.material_id as usize).map(|m| m.layers.as_slice()).unwrap_or(&[]);
        for l in layers {
            let t = model.textures.get(l.texture_id as usize);
            println!(
                "  visible geoset {i}: filter {} tex {:?} replaceable {:?} verts {}",
                l.filter_mode,
                t.map(|t| t.path.as_str()),
                t.map(|t| t.replaceable_id),
                g.vertices.len()
            );
        }
    }
}
