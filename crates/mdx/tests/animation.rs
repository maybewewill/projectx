//! Animation evaluation, skeleton, and skinning tests on synthetic models. Known answers are
//! worked out by hand in the comments. No game data is used.

use mdx::glam::{Mat4, Vec3};
use mdx::skin::{average, matrix_groups, skin_geoset, skin_vertex};
use mdx::{
    BillboardCamera, Extent, Geoset, Interpolation, Key, Model, ModelInfo, Node, Sequence,
    Skeleton, SkeletonError, Track, BILLBOARD, BILLBOARD_LOCK_Z,
};

const EPS: f32 = 1e-4;

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < EPS, "{a} != {b}");
}

fn close3(a: [f32; 3], b: [f32; 3]) {
    for (x, y) in a.iter().zip(b) {
        close(*x, y);
    }
}

fn key<T>(time: u32, value: T) -> Key<T> {
    Key {
        time,
        value,
        in_tangent: None,
        out_tangent: None,
    }
}

fn key_t<T: Copy>(time: u32, value: T, inn: T, out: T) -> Key<T> {
    Key {
        time,
        value,
        in_tangent: Some(inn),
        out_tangent: Some(out),
    }
}

fn track<T>(interpolation: Interpolation, keys: Vec<Key<T>>) -> Track<T> {
    Track {
        interpolation,
        global_sequence: None,
        keys,
    }
}

fn empty_model() -> Model {
    Model {
        version: 800,
        info: ModelInfo {
            name: "test".into(),
            extent: Extent {
                radius: 0.0,
                min: [0.0; 3],
                max: [0.0; 3],
            },
            blend_time: 150,
        },
        sequences: Vec::new(),
        global_sequences: Vec::new(),
        textures: Vec::new(),
        materials: Vec::new(),
        geosets: Vec::new(),
        bones: Vec::new(),
        helpers: Vec::new(),
        pivots: Vec::new(),
        geoset_anims: Vec::new(),
        unknown_chunks: Vec::new(),
    }
}

fn node(name: &str, object_id: i32, parent_id: i32) -> Node {
    Node {
        name: name.into(),
        object_id,
        parent_id,
        flags: 0x100,
        translation: None,
        rotation: None,
        scaling: None,
        visibility: None,
    }
}

fn bone(n: Node) -> mdx::Bone {
    mdx::Bone {
        node: n,
        geoset_id: -1,
        geoset_anim_id: -1,
    }
}

fn sequence(name: &str, start: u32, end: u32, flags: u32) -> Sequence {
    Sequence {
        name: name.into(),
        start,
        end,
        move_speed: 0.0,
        flags,
        rarity: 0.0,
        sync_point: 0,
        extent: Extent {
            radius: 0.0,
            min: [0.0; 3],
            max: [0.0; 3],
        },
    }
}

// ---------- sampling ----------

#[test]
fn linear_vector_interpolates_and_holds_at_the_ends() {
    let t = track(
        Interpolation::Linear,
        vec![key(0, [0.0, 0.0, 0.0]), key(1000, [10.0, 0.0, -4.0])],
    );
    let g: [u32; 0] = [];
    close3(t.sample(500, &g).unwrap(), [5.0, 0.0, -2.0]);
    // Before the first key and after the last key, the end values hold.
    close3(t.sample(0, &g).unwrap(), [0.0, 0.0, 0.0]);
    close3(t.sample(5000, &g).unwrap(), [10.0, 0.0, -4.0]);
}

#[test]
fn none_interpolation_steps_at_the_key_time() {
    let t = track(
        Interpolation::None,
        vec![key(0, 1.0f32), key(100, 2.0), key(200, 3.0)],
    );
    let g: [u32; 0] = [];
    close(t.sample(99, &g).unwrap(), 1.0);
    close(t.sample(100, &g).unwrap(), 2.0);
    close(t.sample(199, &g).unwrap(), 2.0);
}

#[test]
fn empty_track_samples_to_none() {
    let t: Track<f32> = track(Interpolation::Linear, Vec::new());
    assert!(t.sample(0, &[]).is_none());
}

#[test]
fn global_sequence_wraps_time() {
    // Global sequence 0 is 1000 ms long. Time 1250 maps to 250 ms, which is halfway to 500.
    let mut t = track(Interpolation::Linear, vec![key(0, 0.0f32), key(500, 1.0)]);
    t.global_sequence = Some(0);
    close(t.sample(1250, &[1000]).unwrap(), 0.5);
    // Without the GLBS entry the time is used as is, so 1250 is past the last key.
    close(t.sample(1250, &[]).unwrap(), 1.0);
}

#[test]
fn hermite_with_one_slope_matches_the_basis_functions() {
    // p0 = 0 with slope 1 out, p1 = 1 with slope 0 in. At s = 0.5:
    // h00 = 0.5, h10 = 0.125, h01 = 0.5, h11 = -0.125 -> 0.5 * 0 + 0.125 * 1 + 0.5 * 1 = 0.625.
    let t = track(
        Interpolation::Hermite,
        vec![key_t(0, 0.0f32, 0.0, 1.0), key_t(1000, 1.0, 0.0, 0.0)],
    );
    close(t.sample(500, &[]).unwrap(), 0.625);
}

#[test]
fn bezier_with_evenly_spaced_controls_is_linear() {
    // Control values one third and two thirds of the way make the cubic a straight line.
    let t = track(
        Interpolation::Bezier,
        vec![key_t(0, 0.0f32, 0.0, 1.0 / 3.0), key_t(1000, 1.0, 2.0 / 3.0, 0.0)],
    );
    close(t.sample(500, &[]).unwrap(), 0.5);
    close(t.sample(250, &[]).unwrap(), 0.25);
}

#[test]
fn rotation_slerp_halfway_is_45_degrees() {
    // Identity to 90 degrees about Z. Halfway is 45 degrees about Z, so X maps to (c, s, 0).
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let t = track(
        Interpolation::Linear,
        vec![key(0, [0.0, 0.0, 0.0, 1.0]), key(1000, [0.0, 0.0, s, s])],
    );
    let q = mdx::glam::Quat::from_array(t.sample(500, &[]).unwrap());
    let v = q * Vec3::X;
    close3(v.to_array(), [s, s, 0.0]);
}

#[test]
fn rotation_takes_the_shorter_arc() {
    // The end quaternion is the negation of identity, the same rotation. The midpoint must be
    // the identity rotation (|w| = 1), not a spin of 180 degrees.
    let t = track(
        Interpolation::Linear,
        vec![key(0, [0.0, 0.0, 0.0, 1.0]), key(1000, [0.0, 0.0, 0.0, -1.0])],
    );
    let q = mdx::glam::Quat::from_array(t.sample(500, &[]).unwrap());
    close(q.w.abs(), 1.0);
}

#[test]
fn squad_with_endpoint_tangents_equals_slerp() {
    // With the tangents equal to the endpoints, squad reduces to slerp.
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let q0 = [0.0, 0.0, 0.0, 1.0];
    let q1 = [0.0, 0.0, s, s];
    let linear = track(Interpolation::Linear, vec![key(0, q0), key(1000, q1)]);
    let squad = track(
        Interpolation::Hermite,
        vec![key_t(0, q0, q0, q0), key_t(1000, q1, q1, q1)],
    );
    let a = linear.sample(300, &[]).unwrap();
    let b = squad.sample(300, &[]).unwrap();
    for (x, y) in a.iter().zip(b) {
        close(*x, y);
    }
}

#[test]
fn integer_track_holds_the_earlier_value() {
    let t = track(Interpolation::Linear, vec![key(0, 3u32), key(100, 7)]);
    assert_eq!(t.sample(50, &[]), Some(3));
    assert_eq!(t.sample(100, &[]), Some(7));
}

#[test]
fn sequence_time_loops_or_clamps() {
    let looping = sequence("Walk", 1000, 2000, 0);
    close(looping.time_at(500.0) as f32, 1500.0);
    close(looping.time_at(1500.0) as f32, 1500.0); // wraps: 1000 + (1500 % 1000) = 1500
    assert_eq!(looping.time_at(2500.0), 1500);
    let once = sequence("Death", 1000, 2000, 1);
    assert!(!once.is_looping());
    assert_eq!(once.time_at(500.0), 1500);
    assert_eq!(once.time_at(5000.0), 2000);
    assert_eq!(once.duration(), 1000);
}

#[test]
fn find_sequence_prefers_exact_then_prefix() {
    let mut m = empty_model();
    m.sequences = vec![
        sequence("Stand Victory", 0, 1, 0),
        sequence("Stand", 0, 1, 0),
        sequence("Attack - 1", 0, 1, 0),
    ];
    assert_eq!(m.find_sequence("stand"), Some(1));
    assert_eq!(m.find_sequence("attack"), Some(2));
    assert_eq!(m.find_sequence("walk"), None);
    assert_eq!(m.find_sequence("  "), None);
}

// ---------- skeleton ----------

fn chain_model() -> Model {
    // Parent (object 0) pivot (10,0,0), child (object 1) pivot (20,0,0).
    let mut m = empty_model();
    m.bones = vec![bone(node("parent", 0, -1)), bone(node("child", 1, 0))];
    m.pivots = vec![[10.0, 0.0, 0.0], [20.0, 0.0, 0.0]];
    m
}

#[test]
fn bind_pose_places_each_pivot_at_its_rest_position() {
    let m = chain_model();
    let sk = Skeleton::new(&m).unwrap();
    let pose = sk.bind_pose(&m);
    let child = pose.nodes.iter().find(|n| n.object_id == 1).unwrap();
    // Origin of the child at rest is its pivot.
    close3(child.world.transform_point3(Vec3::ZERO).to_array(), [20.0, 0.0, 0.0]);
    // At rest the skinning matrix is the identity.
    for n in &pose.nodes {
        for (a, b) in n.skin.to_cols_array().iter().zip(Mat4::IDENTITY.to_cols_array()) {
            close(*a, b);
        }
    }
}

#[test]
fn rotating_the_parent_moves_the_child_pivot() {
    // Parent rotates 90 degrees about Z around (10,0,0). The child's pivot (20,0,0) is offset
    // (10,0,0) from it, which rotates to (0,10,0). The child pivot ends at (10,10,0).
    let mut m = chain_model();
    let s = std::f32::consts::FRAC_1_SQRT_2;
    m.bones[0].node.rotation = Some(track(
        Interpolation::Linear,
        vec![key(0, [0.0, 0.0, s, s]), key(1000, [0.0, 0.0, s, s])],
    ));
    m.sequences = vec![sequence("Stand", 0, 1000, 0)];
    let sk = Skeleton::new(&m).unwrap();
    let pose = sk.evaluate_sequence(&m, 0, 0.0, None).unwrap();
    let child = pose.nodes.iter().find(|n| n.object_id == 1).unwrap();
    let pivot_now = child.skin.transform_point3(Vec3::new(20.0, 0.0, 0.0));
    close3(pivot_now.to_array(), [10.0, 10.0, 0.0]);
}

#[test]
fn child_translation_track_moves_only_the_child() {
    let mut m = chain_model();
    m.bones[1].node.translation = Some(track(
        Interpolation::Linear,
        vec![key(0, [0.0, 0.0, 0.0]), key(1000, [0.0, 0.0, 4.0])],
    ));
    let sk = Skeleton::new(&m).unwrap();
    let pose = sk.evaluate_at(&m, Some(500), None);
    let parent = pose.nodes.iter().find(|n| n.object_id == 0).unwrap();
    let child = pose.nodes.iter().find(|n| n.object_id == 1).unwrap();
    close3(parent.world.transform_point3(Vec3::ZERO).to_array(), [10.0, 0.0, 0.0]);
    close3(child.world.transform_point3(Vec3::ZERO).to_array(), [20.0, 0.0, 2.0]);
}

#[test]
fn parents_are_placed_before_children_regardless_of_file_order() {
    // The child comes first in the file. The hierarchy must still resolve.
    let mut m = empty_model();
    m.bones = vec![bone(node("child", 1, 0)), bone(node("parent", 0, -1))];
    m.pivots = vec![[0.0; 3], [5.0, 0.0, 0.0]];
    let sk = Skeleton::new(&m).unwrap();
    let pose = sk.bind_pose(&m);
    let child = pose.nodes.iter().find(|n| n.object_id == 1).unwrap();
    close3(child.world.transform_point3(Vec3::ZERO).to_array(), [5.0, 0.0, 0.0]);
}

#[test]
fn cycles_and_duplicate_ids_are_errors() {
    let mut cyc = empty_model();
    cyc.bones = vec![bone(node("a", 0, 1)), bone(node("b", 1, 0))];
    assert!(matches!(Skeleton::new(&cyc), Err(SkeletonError::Cycle(_))));

    let mut dup = empty_model();
    dup.bones = vec![bone(node("a", 0, -1)), bone(node("b", 0, -1))];
    assert_eq!(
        Skeleton::new(&dup).unwrap_err(),
        SkeletonError::DuplicateObject(0)
    );
}

#[test]
fn billboard_faces_the_camera_and_keeps_translation() {
    // Camera looks along +Y with +Z up, so it is at -Y. The node's +Y becomes camera up (+Z)
    // and its +Z points at the viewer (-Y).
    let mut m = empty_model();
    let mut n = node("bb", 0, -1);
    n.flags |= BILLBOARD;
    m.bones = vec![bone(n)];
    m.pivots = vec![[3.0, 4.0, 0.0]];
    let sk = Skeleton::new(&m).unwrap();
    let cam = BillboardCamera::from_view(Vec3::Y, Vec3::Z).unwrap();
    let pose = sk.bind_pose(&m);
    assert!(pose.nodes[0].world.w_axis.truncate().length() > 0.0);
    let pose = sk.evaluate_at(&m, None, Some(&cam));
    let w = pose.nodes[0].world;
    close3(w.transform_vector3(Vec3::Y).to_array(), [0.0, 0.0, 1.0]);
    close3(w.transform_vector3(Vec3::Z).to_array(), [0.0, -1.0, 0.0]);
    close3(w.transform_point3(Vec3::ZERO).to_array(), [3.0, 4.0, 0.0]);
}

#[test]
fn billboard_lock_z_stays_upright() {
    // Lock Z keeps the node's Z on world Z and turns Y toward the viewer (-Y here).
    let mut m = empty_model();
    let mut n = node("upright", 0, -1);
    n.flags |= BILLBOARD | BILLBOARD_LOCK_Z;
    m.bones = vec![bone(n)];
    let sk = Skeleton::new(&m).unwrap();
    let cam = BillboardCamera::from_view(Vec3::Y, Vec3::Z).unwrap();
    let pose = sk.evaluate_at(&m, None, Some(&cam));
    let w = pose.nodes[0].world;
    close3(w.transform_vector3(Vec3::Z).to_array(), [0.0, 0.0, 1.0]);
    close3(w.transform_vector3(Vec3::Y).to_array(), [0.0, -1.0, 0.0]);
}

#[test]
fn billboard_camera_rejects_a_view_along_up() {
    assert!(BillboardCamera::from_view(Vec3::Z, Vec3::Z).is_none());
}

#[test]
fn visibility_and_material_and_geoset_tracks_are_sampled() {
    use mdx::{GeosetAnim, Layer, Material};

    let mut m = empty_model();
    let mut n = node("vis", 0, -1);
    n.visibility = Some(track(Interpolation::Linear, vec![key(0, 0.0f32), key(1000, 1.0)]));
    m.bones = vec![bone(n)];
    m.sequences = vec![sequence("Stand", 0, 1000, 0)];
    m.materials = vec![Material {
        priority_plane: 0,
        flags: 0,
        layers: vec![Layer {
            filter_mode: 2,
            shading_flags: 0,
            texture_id: 0,
            texture_anim_id: None,
            coord_id: 0,
            alpha: 1.0,
            alpha_track: Some(track(Interpolation::Linear, vec![key(0, 0.0f32), key(1000, 0.5)])),
            texture_track: Some(track(Interpolation::None, vec![key(0, 0u32), key(500, 2)])),
        }],
    }];
    m.geoset_anims = vec![GeosetAnim {
        alpha: 1.0,
        flags: 0,
        color: [1.0, 1.0, 1.0],
        geoset_id: 0,
        alpha_track: None,
        color_track: Some(track(
            Interpolation::Linear,
            vec![key(0, [1.0, 1.0, 1.0]), key(1000, [0.0, 0.0, 0.0])],
        )),
    }];
    let sk = Skeleton::new(&m).unwrap();
    let pose = sk.evaluate_sequence(&m, 0, 500.0, None).unwrap();
    close(pose.nodes[0].visibility, 0.5);
    close(pose.materials[0][0].alpha, 0.25);
    assert_eq!(pose.materials[0][0].texture_id, 2);
    let g = pose.geoset_state(0).unwrap();
    close(g.alpha, 1.0);
    close3(g.color, [0.5, 0.5, 0.5]);
    assert!(pose.geoset_state(3).is_none());
}

// ---------- skinning ----------

fn geoset_with_groups() -> Geoset {
    // Two matrix groups: group 0 = bones [7], group 1 = bones [7, 8]. MATS is the flat list.
    Geoset {
        vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        primitives: Vec::new(),
        vertex_groups: vec![0, 1, 9],
        matrix_groups: vec![1, 2],
        matrices: vec![7, 7, 8],
        material_id: 0,
        selection_group: 0,
        selection_flags: 0,
        extent: Extent {
            radius: 0.0,
            min: [0.0; 3],
            max: [0.0; 3],
        },
        sequence_extents: Vec::new(),
        uv_sets: Vec::new(),
    }
}

#[test]
fn matrix_groups_split_the_flat_matrix_list() {
    let g = geoset_with_groups();
    assert_eq!(matrix_groups(&g), vec![vec![7], vec![7, 8]]);
}

#[test]
fn matrix_groups_survive_a_truncated_list() {
    let mut g = geoset_with_groups();
    g.matrices.truncate(1);
    assert_eq!(matrix_groups(&g), vec![vec![7], vec![]]);
}

#[test]
fn average_of_translations_is_the_midpoint() {
    // Bone A moves the point by +2 in x, bone B by 0. The average moves it by +1.
    let a = Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0));
    let b = Mat4::IDENTITY;
    let m = average(&[a, b]);
    close3(m.transform_point3(Vec3::ZERO).to_array(), [1.0, 0.0, 0.0]);
    assert_eq!(average(&[]), Mat4::IDENTITY);
}

#[test]
fn skin_vertex_applies_the_matrix_to_position_and_normal() {
    // 90 degree rotation about Z: (1,0,0) -> (0,1,0), normal (0,0,1) stays.
    let m = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let (p, n) = skin_vertex(m, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    close3(p, [0.0, 1.0, 0.0]);
    close3(n, [0.0, 0.0, 1.0]);
}

#[test]
fn skin_geoset_uses_each_vertex_group_and_falls_back_to_identity() {
    let g = geoset_with_groups();
    let groups = [
        Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)),
        average(&[
            Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0)),
            Mat4::IDENTITY,
        ]),
    ];
    let out = skin_geoset(&g, &groups);
    close3(out[0].0, [10.0, 0.0, 0.0]); // group 0
    close3(out[1].0, [2.0, 0.0, 0.0]); // group 1: (1,0,0) + 1
    close3(out[2].0, [2.0, 0.0, 0.0]); // GNDX 9 is out of range: identity
}

// ---------- parsing GEOA end to end ----------

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(tag);
    put_u32(out, body.len() as u32);
    out.extend_from_slice(body);
}

#[test]
fn parses_geoset_animation_chunk() {
    let mut file = b"MDLX".to_vec();
    let mut vers = Vec::new();
    put_u32(&mut vers, 800);
    chunk(&mut file, b"VERS", &vers);
    let mut modl = vec![0u8; 336];
    modl[..4].copy_from_slice(b"Tst\0");
    put_u32(&mut modl, 0); // unknown field
    modl.extend_from_slice(&[0u8; 28]); // extent
    put_u32(&mut modl, 150); // blend time
    chunk(&mut file, b"MODL", &modl);

    // One GEOA record: static alpha 0.75, flags 0x10, color (0.2, 0.4, 0.6), geoset 2,
    // and a KGAO track with two float keys (0 -> 0.0, 1000 -> 1.0).
    let mut rec = Vec::new();
    put_f32(&mut rec, 0.75);
    put_u32(&mut rec, 0x10);
    for c in [0.2f32, 0.4, 0.6] {
        put_f32(&mut rec, c);
    }
    rec.extend_from_slice(&2i32.to_le_bytes());
    rec.extend_from_slice(b"KGAO");
    put_u32(&mut rec, 2);
    put_u32(&mut rec, 1); // linear
    rec.extend_from_slice(&(-1i32).to_le_bytes());
    put_u32(&mut rec, 0);
    put_f32(&mut rec, 0.0);
    put_u32(&mut rec, 1000);
    put_f32(&mut rec, 1.0);
    let mut geoa = Vec::new();
    put_u32(&mut geoa, rec.len() as u32 + 4);
    geoa.extend_from_slice(&rec);
    chunk(&mut file, b"GEOA", &geoa);

    let m = Model::parse(&file).unwrap();
    assert_eq!(m.geoset_anims.len(), 1);
    let g = &m.geoset_anims[0];
    assert_eq!(g.geoset_id, 2);
    close(g.alpha, 0.75);
    close3(g.color, [0.2, 0.4, 0.6]);
    close(g.alpha_track.as_ref().unwrap().sample(250, &[]).unwrap(), 0.25);
    assert!(g.color_track.is_none());
}

// ---------- geoset visibility ----------

fn geoa(geoset_id: i32, alpha: f32, keys: Option<Vec<Key<f32>>>) -> mdx::GeosetAnim {
    mdx::GeosetAnim {
        alpha,
        flags: 0,
        color: [1.0; 3],
        geoset_id,
        alpha_track: keys.map(|k| track(Interpolation::None, k)),
        color_track: None,
    }
}

#[test]
fn track_sample_in_uses_only_keys_inside_the_interval() {
    let t = track(Interpolation::None, vec![key(100, 0.0f32), key(500, 1.0), key(900, 0.0)]);
    // Interval 400..=600 holds only the key at 500.
    assert_eq!(t.sample_in(400, 400, 600, &[]), Some(1.0));
    // Before the first in-interval key the first holds (the key at 100 does not leak in).
    assert_eq!(t.sample_in(450, 450, 600, &[]), Some(1.0));
    // Interval with no key at all: None, the caller uses the static value.
    assert_eq!(t.sample_in(200, 150, 300, &[]), None);
    // Plain sampling would have returned the earlier sequence's key.
    assert_eq!(t.sample(200, &[]), Some(0.0));
}

#[test]
fn death_only_geosets_are_hidden_in_the_stand_pose() {
    let mut m = empty_model();
    m.sequences = vec![
        sequence("Birth", 0, 1000, 1),
        sequence("Stand - 1", 2000, 3000, 0),
        sequence("Death", 4000, 5000, 1),
    ];
    for _ in 0..4 {
        m.geosets.push(geoset_with_groups());
    }
    m.geoset_anims = vec![
        // Geoset 0: visible only during Death. Key at Stand start is 0 -> hidden.
        geoa(0, 1.0, Some(vec![key(0, 0.0), key(2000, 0.0), key(4000, 1.0)])),
        // Geoset 1: keys exist only for Death (alpha 0 there); Stand has none -> static 1.
        geoa(1, 1.0, Some(vec![key(4000, 0.0)])),
        // Geoset 2: no track, static alpha 0 -> hidden.
        geoa(2, 0.0, None),
        // Geoset 3 has no GEOA at all.
    ];
    assert_eq!(m.stand_sequence(), Some(1));
    assert_eq!(m.idle_geoset_visibility(), vec![false, true, false, true]);
    // During Death geoset 0 shows and geoset 1 is gone.
    assert_eq!(m.geoset_visibility(Some(2)), vec![true, false, false, true]);
}
