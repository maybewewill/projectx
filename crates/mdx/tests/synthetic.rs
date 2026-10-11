//! Tests on small synthetic MDX files built in memory. No game data is used.

use mdx::{Error, Interpolation, Model, RawChunk};

/// Little-endian byte writer for building test files.
#[derive(Default, Clone)]
struct W(Vec<u8>);

impl W {
    fn tag(mut self, t: &[u8; 4]) -> Self {
        self.0.extend_from_slice(t);
        self
    }
    fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    fn u16(mut self, v: u16) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i32(mut self, v: i32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f32(mut self, v: f32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn floats(mut self, vs: &[f32]) -> Self {
        for v in vs {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        self
    }
    fn fixed(mut self, s: &str, n: usize) -> Self {
        let mut bytes = s.as_bytes().to_vec();
        bytes.resize(n, 0);
        self.0.extend(bytes);
        self
    }
    fn append(mut self, other: W) -> Self {
        self.0.extend(other.0);
        self
    }
    fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

/// Wraps a body in an inclusive-size record.
fn record(body: W) -> W {
    W::default().u32(body.0.len() as u32 + 4).append(body)
}

/// Wraps a payload in a `[tag][size][payload]` chunk.
fn chunk(tag: &[u8; 4], body: W) -> W {
    W::default().tag(tag).u32(body.0.len() as u32).append(body)
}

/// A track: tag, key count, interpolation, global sequence id, then keys.
fn track(tag: &[u8; 4], interp: u32, gseq: i32, count: u32, keys: W) -> W {
    W::default()
        .tag(tag)
        .u32(count)
        .u32(interp)
        .i32(gseq)
        .append(keys)
}

/// The synthetic model as separate top-level chunks. The first part is the magic.
fn sample_parts() -> Vec<Vec<u8>> {
    let magic = W::default().tag(b"MDLX");
    let vers = chunk(b"VERS", W::default().u32(800));
    let modl = chunk(
        b"MODL",
        W::default()
            .fixed("Footman", 336)
            .u32(0)
            .floats(&[10.0, -1.0, -1.0, 0.0, 1.0, 1.0, 2.0])
            .u32(30),
    );
    let seqs = chunk(
        b"SEQS",
        W::default()
            .fixed("Stand", 80)
            .u32(0)
            .u32(100)
            .f32(270.0)
            .u32(0)
            .f32(0.0)
            .u32(0)
            .floats(&[5.0, -1.0, -1.0, 0.0, 1.0, 1.0, 2.0]),
    );
    let glbs = chunk(b"GLBS", W::default().u32(1000));
    let texs = chunk(
        b"TEXS",
        W::default()
            .u32(0)
            .fixed("Textures\\white.blp", 260)
            .u32(1),
    );

    // Material 0: one transparent layer with a linear animated alpha track.
    let alpha_keys = W::default()
        .u32(0)
        .f32(0.0)
        .u32(50)
        .f32(1.0)
        .u32(100)
        .f32(0.5);
    let layer = record(
        W::default()
            .u32(1)
            .u32(0)
            .u32(0)
            .i32(-1)
            .u32(0)
            .f32(1.0)
            .append(track(b"KMTA", 1, -1, 3, alpha_keys)),
    );
    let material = record(W::default().i32(0).u32(0).tag(b"LAYS").u32(1).append(layer));
    let mtls = chunk(b"MTLS", W::default().append(material));

    // Geoset 0: one triangle, one UV set, one sequence extent.
    let geoset = record(
        W::default()
            .tag(b"VRTX")
            .u32(3)
            .floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])
            .tag(b"NRMS")
            .u32(3)
            .floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0])
            .tag(b"PTYP")
            .u32(1)
            .u32(4)
            .tag(b"PCNT")
            .u32(1)
            .u32(3)
            .tag(b"PVTX")
            .u32(3)
            .u16(0)
            .u16(1)
            .u16(2)
            .tag(b"GNDX")
            .u32(3)
            .u8(0)
            .u8(0)
            .u8(0)
            .tag(b"MTGC")
            .u32(1)
            .u32(1)
            .tag(b"MATS")
            .u32(1)
            .u32(0)
            .u32(0)
            .u32(0)
            .u32(0)
            .floats(&[1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0])
            .u32(1)
            .floats(&[1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0])
            .tag(b"UVAS")
            .u32(1)
            .tag(b"UVBS")
            .u32(3)
            .floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
    );
    let geos = chunk(b"GEOS", W::default().append(geoset));

    // Bone 0: linear translation. Bone 1: Hermite translation with tangents, global sequence 0.
    // Real layout: header and tracks inside the record, then the geoset ids outside it.
    let bone0 = record(
        W::default()
            .fixed("Root", 80)
            .i32(0)
            .i32(-1)
            .u32(0x100)
            .append(track(
                b"KGTR",
                1,
                -1,
                2,
                W::default()
                    .u32(0)
                    .floats(&[0.0, 0.0, 0.0])
                    .u32(10)
                    .floats(&[1.0, 2.0, 3.0]),
            )),
    )
    .i32(-1)
    .i32(-1);
    let bone1 = record(
        W::default()
            .fixed("Arm", 80)
            .i32(1)
            .i32(0)
            .u32(0x100)
            .append(track(
                b"KGTR",
                2,
                0,
                2,
                W::default()
                    .u32(0)
                    .floats(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0])
                    .u32(10)
                    .floats(&[1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            )),
    )
    .i32(7)
    .i32(-1);
    let bones = chunk(b"BONE", W::default().append(bone0).append(bone1));

    let helpers = chunk(
        b"HELP",
        W::default().append(record(
            W::default().fixed("Helper", 80).i32(2).i32(0).u32(0),
        )),
    );
    let pivt = chunk(
        b"PIVT",
        W::default().floats(&[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
    );
    let atch = chunk(b"ATCH", W::default().u8(1).u8(2).u8(3).u8(4).u8(5));

    vec![
        magic.into_vec(),
        vers.into_vec(),
        modl.into_vec(),
        seqs.into_vec(),
        glbs.into_vec(),
        texs.into_vec(),
        mtls.into_vec(),
        geos.into_vec(),
        bones.into_vec(),
        helpers.into_vec(),
        pivt.into_vec(),
        atch.into_vec(),
    ]
}

fn sample() -> Vec<u8> {
    sample_parts().concat()
}

#[test]
fn parses_header_sequences_and_textures() {
    let m = Model::parse(&sample()).expect("sample parses");
    assert_eq!(m.version, 800);
    assert_eq!(m.info.name, "Footman");
    assert_eq!(m.info.blend_time, 30);
    assert_eq!(m.info.extent.radius, 10.0);
    assert_eq!(m.info.extent.max, [1.0, 1.0, 2.0]);

    assert_eq!(m.sequences.len(), 1);
    let s = &m.sequences[0];
    assert_eq!(s.name, "Stand");
    assert_eq!((s.start, s.end), (0, 100));
    assert_eq!(s.move_speed, 270.0);
    assert!(s.is_looping());
    assert_eq!(s.extent.radius, 5.0);

    assert_eq!(m.global_sequences, vec![1000]);

    assert_eq!(m.textures.len(), 1);
    assert_eq!(m.textures[0].path, "Textures\\white.blp");
    assert_eq!(m.textures[0].replaceable_id, 0);
    assert_eq!(m.textures[0].flags, 1);
}

#[test]
fn parses_material_layer_with_animated_alpha() {
    let m = Model::parse(&sample()).unwrap();
    assert_eq!(m.materials.len(), 1);
    let mat = &m.materials[0];
    assert_eq!(mat.layers.len(), 1);
    let layer = &mat.layers[0];
    assert_eq!(layer.filter_mode, 1);
    assert_eq!(layer.texture_id, 0);
    assert_eq!(layer.texture_anim_id, None);
    assert_eq!(layer.alpha, 1.0);
    assert!(layer.texture_track.is_none());

    let track = layer.alpha_track.as_ref().expect("KMTA present");
    assert_eq!(track.interpolation, Interpolation::Linear);
    assert_eq!(track.global_sequence, None);
    let times: Vec<u32> = track.keys.iter().map(|k| k.time).collect();
    let values: Vec<f32> = track.keys.iter().map(|k| k.value).collect();
    assert_eq!(times, vec![0, 50, 100]);
    assert_eq!(values, vec![0.0, 1.0, 0.5]);
    assert!(track
        .keys
        .iter()
        .all(|k| k.in_tangent.is_none() && k.out_tangent.is_none()));
}

#[test]
fn parses_geoset_and_triangle() {
    let m = Model::parse(&sample()).unwrap();
    assert_eq!(m.geosets.len(), 1);
    let g = &m.geosets[0];
    assert_eq!(g.vertices.len(), 3);
    assert_eq!(g.vertices[1], [1.0, 0.0, 0.0]);
    assert_eq!(g.normals.len(), 3);
    assert_eq!(g.primitives.len(), 1);
    assert_eq!(g.primitives[0].kind, 4);
    assert_eq!(g.primitives[0].indices, vec![0, 1, 2]);
    assert_eq!(g.triangles(), vec![[0, 1, 2]]);
    assert_eq!(g.vertex_groups, vec![0, 0, 0]);
    assert_eq!(g.matrix_groups, vec![1]);
    assert_eq!(g.matrices, vec![0]);
    assert_eq!(g.material_id, 0);
    assert_eq!(g.sequence_extents.len(), 1);
    assert_eq!(g.uv_sets.len(), 1);
    assert_eq!(g.uv_sets[0], vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
}

#[test]
fn parses_bones_with_linear_and_hermite_tracks() {
    let m = Model::parse(&sample()).unwrap();
    assert_eq!(m.bones.len(), 2);

    let root = &m.bones[0];
    assert_eq!(root.node.name, "Root");
    assert_eq!(root.node.parent_id, -1);
    assert_eq!(root.node.flags, 0x100);
    assert_eq!((root.geoset_id, root.geoset_anim_id), (-1, -1));
    let t = root.node.translation.as_ref().expect("KGTR");
    assert_eq!(t.interpolation, Interpolation::Linear);
    assert_eq!(t.keys[1].time, 10);
    assert_eq!(t.keys[1].value, [1.0, 2.0, 3.0]);
    assert!(t.keys[1].in_tangent.is_none());

    let arm = &m.bones[1];
    assert_eq!(arm.node.name, "Arm");
    assert_eq!(arm.node.object_id, 1);
    assert_eq!(arm.node.parent_id, 0);
    assert_eq!((arm.geoset_id, arm.geoset_anim_id), (7, -1));
    let t = arm.node.translation.as_ref().expect("KGTR");
    assert_eq!(t.interpolation, Interpolation::Hermite);
    assert_eq!(t.global_sequence, Some(0));
    assert_eq!(t.keys.len(), 2);
    assert_eq!(t.keys[0].in_tangent, Some([0.0, 0.0, 0.0]));
    assert_eq!(t.keys[0].out_tangent, Some([1.0, 0.0, 0.0]));
    assert_eq!(t.keys[1].value, [1.0, 0.0, 0.0]);
    assert_eq!(t.keys[1].in_tangent, Some([1.0, 0.0, 0.0]));
    assert_eq!(t.keys[1].out_tangent, Some([0.0, 0.0, 0.0]));
}

#[test]
fn parses_helpers_pivots_and_keeps_unknown_chunks() {
    let m = Model::parse(&sample()).unwrap();
    assert_eq!(m.helpers.len(), 1);
    assert_eq!(m.helpers[0].name, "Helper");
    assert!(m.helpers[0].translation.is_none());
    assert_eq!(
        m.pivots,
        vec![[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]
    );
    assert_eq!(
        m.unknown_chunks,
        vec![RawChunk {
            tag: *b"ATCH",
            data: vec![1, 2, 3, 4, 5],
        }]
    );
}

#[test]
fn rejects_bad_magic_and_version() {
    let mut data = sample();
    data[0] = b'X';
    assert!(matches!(Model::parse(&data), Err(Error::BadMagic { .. })));

    let mut parts = sample_parts();
    parts[1] = chunk(b"VERS", W::default().u32(900)).into_vec();
    assert_eq!(
        Model::parse(&parts.concat()),
        Err(Error::UnsupportedVersion { found: 900 })
    );
}

#[test]
fn rejects_chunk_larger_than_file() {
    let mut data = sample();
    // Bump the size of the first chunk (VERS) past the end of the file.
    data[8..12].copy_from_slice(&1000u32.to_le_bytes());
    assert!(matches!(
        Model::parse(&data),
        Err(Error::ChunkOverrun { .. })
    ));
}

#[test]
fn rejects_missing_required_chunk() {
    let parts = sample_parts();
    let without_modl: Vec<u8> = parts
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 2)
        .flat_map(|(_, p)| p.clone())
        .collect();
    assert_eq!(
        Model::parse(&without_modl),
        Err(Error::MissingChunk {
            tag: "MODL".to_string()
        })
    );
}

#[test]
fn rejects_invalid_interpolation_and_huge_count() {
    // Bone track with interpolation 9.
    let bad_interp = chunk(
        b"BONE",
        W::default()
            .append(record(
                W::default()
                    .fixed("Bad", 80)
                    .i32(0)
                    .i32(-1)
                    .u32(0)
                    .append(track(b"KGTR", 9, -1, 0, W::default())),
            ))
            .i32(-1)
            .i32(-1),
    );
    let mut file = vec![b"MDLX".to_vec()];
    file.push(chunk(b"VERS", W::default().u32(800)).into_vec());
    file.push(
        chunk(
            b"MODL",
            W::default().fixed("X", 336).u32(0).floats(&[0.0; 7]).u32(0),
        )
        .into_vec(),
    );
    file.push(bad_interp.into_vec());
    assert!(matches!(
        Model::parse(&file.concat()),
        Err(Error::InvalidValue {
            what: "interpolation type",
            ..
        })
    ));

    // Sequence-count style corruption: a geoset with a huge vertex count.
    let huge = chunk(
        b"GEOS",
        W::default().append(record(W::default().tag(b"VRTX").u32(u32::MAX))),
    );
    let mut file = vec![b"MDLX".to_vec()];
    file.push(chunk(b"VERS", W::default().u32(800)).into_vec());
    file.push(
        chunk(
            b"MODL",
            W::default().fixed("X", 336).u32(0).floats(&[0.0; 7]).u32(0),
        )
        .into_vec(),
    );
    file.push(huge.into_vec());
    assert!(matches!(
        Model::parse(&file.concat()),
        Err(Error::CountTooLarge {
            what: "vertices",
            ..
        })
    ));
}

#[test]
fn rejects_triangle_index_out_of_range() {
    let geoset = record(
        W::default()
            .tag(b"VRTX")
            .u32(1)
            .floats(&[0.0, 0.0, 0.0])
            .tag(b"NRMS")
            .u32(0)
            .tag(b"PTYP")
            .u32(1)
            .u32(4)
            .tag(b"PCNT")
            .u32(1)
            .u32(3)
            .tag(b"PVTX")
            .u32(3)
            .u16(0)
            .u16(0)
            .u16(5)
            .tag(b"GNDX")
            .u32(0)
            .tag(b"MTGC")
            .u32(0)
            .tag(b"MATS")
            .u32(0)
            .u32(0)
            .u32(0)
            .u32(0)
            .floats(&[0.0; 7])
            .u32(0),
    );
    let mut file = vec![b"MDLX".to_vec()];
    file.push(chunk(b"VERS", W::default().u32(800)).into_vec());
    file.push(
        chunk(
            b"MODL",
            W::default().fixed("X", 336).u32(0).floats(&[0.0; 7]).u32(0),
        )
        .into_vec(),
    );
    file.push(chunk(b"GEOS", W::default().append(geoset)).into_vec());
    assert!(matches!(
        Model::parse(&file.concat()),
        Err(Error::InvalidValue {
            what: "primitive vertex index",
            value: 5,
            ..
        })
    ));
}

#[test]
fn every_truncation_is_an_error_and_never_panics() {
    let parts = sample_parts();
    let full = parts.concat();
    assert!(Model::parse(&full).is_ok());

    // Chunk boundaries (prefix lengths that end exactly after a chunk).
    let mut boundaries = Vec::new();
    let mut acc = 0;
    for p in &parts {
        acc += p.len();
        boundaries.push(acc);
    }
    // Everything up to the end of MODL is required by the model, so those prefixes must fail.
    let modl_end = boundaries[2];

    for len in 0..full.len() {
        let result = Model::parse(&full[..len]);
        if len < modl_end {
            assert!(result.is_err(), "prefix of {len} bytes parsed");
        }
        // A prefix that cuts a chunk in the middle must fail. A prefix that ends on a chunk
        // boundary may still parse when only optional trailing chunks are missing.
        if !boundaries.contains(&len) {
            assert!(result.is_err(), "mid-chunk prefix of {len} bytes parsed");
        }
    }
}

#[test]
fn corrupted_bytes_never_panic() {
    let full = sample();
    for i in 0..full.len() {
        for value in [0x00u8, 0x01, 0x7F, 0xFF] {
            let mut data = full.clone();
            data[i] = value;
            let _ = Model::parse(&data);
        }
    }
}

/// Replaces one chunk of the sample (by its index in `sample_parts`) and parses the result.
fn parse_with_chunk(index: usize, chunk: W) -> Result<Model, Error> {
    let mut parts = sample_parts();
    parts[index] = chunk.into_vec();
    Model::parse(&parts.concat())
}

#[test]
fn texs_records_are_268_bytes() {
    // Two entries of 268 bytes each: replaceable id, 260-byte path, flags.
    let texs = chunk(
        b"TEXS",
        W::default()
            .u32(0)
            .fixed("Textures\\a.blp", 260)
            .u32(3)
            .u32(1)
            .fixed("ReplaceableTextures\\b.blp", 260)
            .u32(2),
    );
    let m = parse_with_chunk(5, texs).unwrap();
    assert_eq!(m.textures.len(), 2);
    assert_eq!(m.textures[0].path, "Textures\\a.blp");
    assert_eq!(m.textures[0].flags, 3);
    assert_eq!(m.textures[1].replaceable_id, 1);
    assert_eq!(m.textures[1].flags, 2);
}

#[test]
fn texs_with_272_byte_records_is_rejected() {
    // A 272-byte record (the old, wrong layout) leaves 4 bytes over.
    let texs = chunk(
        b"TEXS",
        W::default().u32(0).fixed("Textures\\a.blp", 260).u32(0).u32(1),
    );
    assert!(matches!(
        parse_with_chunk(5, texs),
        Err(Error::SizeMismatch { elem: 268, .. })
    ));
}

#[test]
fn unknown_track_inside_a_bone_is_an_error() {
    // The track tag KGXX is not one we read, and its size is unknown: the bone cannot be
    // parsed to its end, so this must be reported instead of silently dropping the rest.
    let bone = record(
        W::default()
            .fixed("Root", 80)
            .i32(0)
            .i32(-1)
            .u32(0x100)
            .tag(b"KGXX")
            .u32(0),
    )
    .i32(-1)
    .i32(-1);
    assert!(matches!(
        parse_with_chunk(8, chunk(b"BONE", bone)),
        Err(Error::Malformed { what: "bone", .. })
    ));
}

#[test]
fn unknown_track_inside_a_layer_is_an_error() {
    let layer = record(
        W::default()
            .u32(1)
            .u32(0)
            .u32(0)
            .i32(-1)
            .u32(0)
            .f32(1.0)
            .tag(b"KMTX")
            .u32(0),
    );
    let material = record(W::default().i32(0).u32(0).tag(b"LAYS").u32(1).append(layer));
    assert!(matches!(
        parse_with_chunk(6, chunk(b"MTLS", W::default().append(material))),
        Err(Error::Malformed { what: "layer", .. })
    ));
}
