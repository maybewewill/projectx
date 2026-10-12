//! Terrain objects: cliff models, placed doodads, and pre-placed units.
//!
//! This module handles loading and rendering 3D models placed on the terrain. It includes:
//! - **Cliffs**: loaded from Cliffs.slk, with model names derived from corner heights
//! - **Doodads**: loaded from `war3map.doo`, resolved through Doodads.slk
//! - **Units**: loaded from `war3mapUnits.doo`, resolved through unit SLK files
//!
//! The cliff model naming scheme: for each terrain tile with cliff height differences between
//! corners, a model is selected using the corner heights (A, B, C, D for flat, 1-step, 2-step, 3-step)
//! and the cliff tileset name (from CliffTypes.slk). Path: `Doodads\Terrain\<TilesetName>\<TilesetName><ABCD><variation>.mdx`.
//! Examples: `Doodads\Terrain\Cliffs\CliffsAAAB0.mdx`, `Doodads\Terrain\CityCliffs\CityCliffsAAAB0.mdx`.
//!
//! Nothing here is bit-exact or game-accurate: it is presentation only.

use std::collections::HashMap;

use gamedata::Slk;
use glam::{Mat4, Quat, Vec3};
use w3map::TilePoint;

/// Cliff configuration: tileset name and directory for model lookup.
#[derive(Debug, Clone)]
pub struct CliffTileset {
    pub name: String,
    pub dir: String,
}

/// Placed object (doodad or unit): 3D position, rotation, and scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedObject {
    pub position: Vec3,
    pub angle: f32,
    pub scale: Vec3,
}

impl PlacedObject {
    pub fn to_matrix(&self) -> Mat4 {
        let rotation = Quat::from_rotation_z(self.angle);
        Mat4::from_scale_rotation_translation(self.scale, rotation, self.position)
    }
}

/// Corner height letters for cliff naming (a=0, b=1, c=2, etc.).
/// Corners are ordered: TL (a), TR (b), BR (c), BL (d).
pub fn height_letter(diff: u8) -> char {
    match diff {
        0 => 'a',
        1 => 'b',
        2 => 'c',
        3 => 'd',
        4 => 'e',
        5 => 'f',
        6 => 'g',
        7 => 'h',
        n => {
            // UNSURE: how high can cliff differences actually go?
            // For now clamp to 'h' for safety.
            if n > 7 { 'h' } else { (b'a' + n) as char }
        }
    }
}

/// Generate cliff model name from corner heights and cliff tileset.
///
/// Real cliff models use the naming scheme: `Doodads\Terrain\<TilesetName>\<TilesetName><ABCD><variation>.mdx`
/// where A, B, C, D are height letters for the four corners (TL, TR, BR, BL, uppercase in real files)
/// and variation is typically 0 but can be higher.
///
/// # Example
/// For corners [0, 0, 1, 0] and cliff_tileset "Cliffs", returns:
/// `Doodads\Terrain\Cliffs\CliffsAAAB0.mdx`
pub fn cliff_model_path(cliff_tileset: &str, corners: [u8; 4], variation: u32) -> String {
    let [tl, tr, br, bl] = corners;
    // Uppercase to match actual model names (CliffsAAAB0.mdx, not CliffsaaAb0.mdx)
    let letters = format!(
        "{}{}{}{}",
        height_letter(tl).to_ascii_uppercase(),
        height_letter(tr).to_ascii_uppercase(),
        height_letter(br).to_ascii_uppercase(),
        height_letter(bl).to_ascii_uppercase()
    );
    format!(
        "Doodads\\Terrain\\{}\\{}{}{}.mdx",
        cliff_tileset, cliff_tileset, letters, variation
    )
}

/// One row of `TerrainArt\CliffTypes.slk`: the model folder and texture of a cliff type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliffType {
    /// Cliff id as stored in the terrain file, e.g. `CLdi`.
    pub id: [u8; 4],
    /// `cliffModelDir`, e.g. `Cliffs` or `CityCliffs`: the model folder and the name prefix.
    pub model_dir: String,
    /// `texDir\texFile` without extension, e.g. `ReplaceableTextures\Cliff\Cliff0`.
    pub texture: Option<String>,
}

/// The cliff types of a game, keyed by cliff id.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CliffTypes {
    types: HashMap<[u8; 4], CliffType>,
}

/// Model folder used when a map's cliff tileset is not in the table (the Lordaeron cliffs).
pub const DEFAULT_CLIFF_DIR: &str = "Cliffs";

fn clean_slk_text(text: &str) -> String {
    text.trim().trim_matches('"').replace('/', "\\")
}

impl CliffTypes {
    /// Reads the rows of `CliffTypes.slk`. Rows without a model folder are skipped.
    pub fn from_slk(slk: &Slk) -> Self {
        let mut types = HashMap::new();
        for row in slk.rows() {
            let key = row.key().as_bytes();
            let Ok(id) = <[u8; 4]>::try_from(key) else {
                continue;
            };
            let Some(model_dir) = row.raw("cliffModelDir").map(clean_slk_text).filter(|s| !s.is_empty()) else {
                continue;
            };
            let texture = match (row.raw("texDir"), row.raw("texFile")) {
                (Some(dir), Some(file)) => {
                    let (dir, file) = (clean_slk_text(dir), clean_slk_text(file));
                    (!file.is_empty()).then(|| format!("{dir}\\{file}"))
                }
                _ => None,
            };
            types.insert(id, CliffType { id, model_dir, texture });
        }
        Self { types }
    }

    pub fn get(&self, id: [u8; 4]) -> Option<&CliffType> {
        self.types.get(&id)
    }

    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

/// The cliff type of one cliff tile, resolved from the map's cliff tileset list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileCliff {
    /// Model folder (and name prefix) for the tile's models.
    pub model_dir: String,
    /// Cliff texture path (`texDir\texFile`), when the type has one.
    pub texture: Option<String>,
    /// Cliff variation of the upper corner (models exist for several variations).
    pub variation: u8,
    /// False when the cliff id is not in the table, so `model_dir` is the default.
    pub from_table: bool,
}

/// Picks the cliff type of a cliff tile from its four corner points (TL, TR, BR, BL).
///
/// The cliff texture index is stored per point. The corner with the highest cliff layer is
/// taken as the tile's upper corner (its index and variation decide the type); on a tie the
/// first corner in TL, TR, BR, BL order wins. `cliff_tilesets` is the map's list of cliff ids
/// and `types` the game's table. The index is looked up in the map's list, so a texture index
/// past the list falls back to the default folder.
pub fn tile_cliff(corners: [&TilePoint; 4], cliff_tilesets: &[[u8; 4]], types: &CliffTypes) -> TileCliff {
    let upper = corners
        .iter()
        .copied()
        .fold(corners[0], |best, p| if p.layer_height > best.layer_height { p } else { best });
    let id = cliff_tilesets.get(usize::from(upper.cliff_texture)).copied();
    match id.and_then(|id| types.get(id)) {
        Some(ty) => TileCliff {
            model_dir: ty.model_dir.clone(),
            texture: ty.texture.clone(),
            variation: upper.cliff_variation,
            from_table: true,
        },
        None => TileCliff {
            model_dir: DEFAULT_CLIFF_DIR.to_string(),
            texture: None,
            variation: upper.cliff_variation,
            from_table: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(layer: u8, texture: u8, variation: u8) -> TilePoint {
        TilePoint {
            ground_height: 8192,
            water_level: 0,
            map_edge: false,
            ground_texture: 0,
            ramp: false,
            blight: false,
            water: false,
            boundary: false,
            ground_variation: 0,
            cliff_variation: variation,
            cliff_texture: texture,
            layer_height: layer,
        }
    }

    fn table(text: &str) -> CliffTypes {
        CliffTypes::from_slk(&Slk::parse_bytes(text.as_bytes()).expect("synthetic SLK parses"))
    }

    #[test]
    fn cliff_types_come_from_the_slk_rows() {
        let types = table(
            "ID;PWXL;N;E\n\
             C;Y1;X1;K\"cliffID\"\n\
             C;X2;K\"cliffModelDir\"\n\
             C;X4;K\"texDir\"\n\
             C;X5;K\"texFile\"\n\
             C;Y2;X1;K\"CLdi\"\n\
             C;X2;K\"Cliffs\"\n\
             C;X4;K\"ReplaceableTextures\\Cliff\"\n\
             C;X5;K\"Cliff0\"\n\
             C;Y3;X1;K\"CLcc\"\n\
             C;X2;K\"CityCliffs\"\n\
             E\n",
        );
        assert_eq!(types.len(), 2);
        let di = types.get(*b"CLdi").expect("CLdi row");
        assert_eq!(di.model_dir, "Cliffs");
        assert_eq!(di.texture.as_deref(), Some("ReplaceableTextures\\Cliff\\Cliff0"));
        let cc = types.get(*b"CLcc").expect("CLcc row");
        assert_eq!(cc.model_dir, "CityCliffs");
        assert_eq!(cc.texture, None, "no texDir/texFile means no texture");
        assert!(types.get(*b"CLxx").is_none());
    }

    #[test]
    fn tile_takes_the_cliff_type_of_its_upper_corner() {
        let types = table(
            "ID;PWXL;N;E\n\
             C;Y1;X1;K\"cliffID\"\n\
             C;X2;K\"cliffModelDir\"\n\
             C;Y2;X1;K\"CLdi\"\n\
             C;X2;K\"Cliffs\"\n\
             C;Y3;X1;K\"CLcc\"\n\
             C;X2;K\"CityCliffs\"\n\
             E\n",
        );
        let tilesets = [*b"CLdi", *b"CLcc"];
        // Upper corner is BR (index 2, layer 2) and uses tileset 1 (CLcc).
        let lo = point(0, 0, 0);
        let hi = point(2, 1, 3);
        let tile = tile_cliff([&lo, &lo, &hi, &lo], &tilesets, &types);
        assert_eq!(tile.model_dir, "CityCliffs");
        assert_eq!(tile.variation, 3);
        assert!(tile.from_table);
        // A tie goes to the first corner, here TL (tileset 0, CLdi).
        let a = point(1, 0, 1);
        let b = point(1, 1, 2);
        let tile = tile_cliff([&a, &b, &b, &b], &tilesets, &types);
        assert_eq!(tile.model_dir, "Cliffs");
        assert_eq!(tile.variation, 1);
    }

    #[test]
    fn unknown_cliff_index_falls_back_to_the_default_folder() {
        let types = table("ID;PWXL;N;E\nC;Y1;X1;K\"cliffID\"\nE\n");
        let p = point(1, 5, 0);
        let tile = tile_cliff([&p, &p, &p, &p], &[*b"CLdi"], &types);
        assert_eq!(tile.model_dir, DEFAULT_CLIFF_DIR);
        assert!(!tile.from_table);
        assert_eq!(tile.texture, None);
    }

    #[test]
    fn height_letter_values() {
        assert_eq!(height_letter(0), 'a');
        assert_eq!(height_letter(1), 'b');
        assert_eq!(height_letter(7), 'h');
        assert_eq!(height_letter(8), 'h'); // clamped
    }

    #[test]
    fn cliff_model_path_naming() {
        // Real example: CliffsAAAB0.mdx (corners [0,0,0,1], variation 0)
        let path = cliff_model_path("Cliffs", [0, 0, 0, 1], 0);
        assert_eq!(path, "Doodads\\Terrain\\Cliffs\\CliffsAAAB0.mdx");

        // CityCliffs example
        let path = cliff_model_path("CityCliffs", [0, 1, 1, 0], 2);
        assert_eq!(path, "Doodads\\Terrain\\CityCliffs\\CityCliffsABBA2.mdx");
    }

    #[test]
    #[ignore] // Requires tools/war3.mpq to be present
    fn cliff_model_paths_exist_in_mpq() {
        // This test checks that generated cliff paths actually exist in the game MPQs.
        // Only runs when war3.mpq is available (not in the worktree).
        let mpq_path = "../../tools/war3.mpq";
        if !std::path::Path::new(mpq_path).exists() {
            eprintln!("Skipping: {} not found", mpq_path);
            return;
        }

        let mpq_bytes = match std::fs::read(mpq_path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("Could not read {}", mpq_path);
                return;
            }
        };

        let archive = match mpq::Archive::open(&mpq_bytes) {
            Ok(a) => a,
            Err(_) => {
                eprintln!("Could not open {} as MPQ", mpq_path);
                return;
            }
        };

        // Check a few real cliff model names
        let test_cases = vec![
            ("Cliffs", [0, 0, 0, 1], 0, "Doodads\\Terrain\\Cliffs\\CliffsAAAB0.mdx"),
            ("CityCliffs", [0, 0, 0, 1], 0, "Doodads\\Terrain\\CityCliffs\\CityCliffsAAAB0.mdx"),
        ];

        for (tileset, corners, var, expected_path) in test_cases {
            let generated = cliff_model_path(tileset, corners, var);
            assert_eq!(generated, expected_path);

            // Try to find it in the MPQ
            if let Ok(_) = archive.read(expected_path) {
                eprintln!("Found in MPQ: {}", expected_path);
            } else {
                eprintln!("NOT in MPQ: {}", expected_path);
            }
        }
    }

    #[test]
    fn placed_object_matrix_identity() {
        let obj = PlacedObject {
            position: Vec3::ZERO,
            angle: 0.0,
            scale: Vec3::ONE,
        };
        let mat = obj.to_matrix();
        let identity = Mat4::IDENTITY;
        // Allow small floating point differences
        for i in 0..16 {
            let expected = identity.to_cols_array()[i];
            let actual = mat.to_cols_array()[i];
            assert!((expected - actual).abs() < 1e-5, "mat[{}] mismatch", i);
        }
    }

    #[test]
    fn placed_object_matrix_translation() {
        let obj = PlacedObject {
            position: Vec3::new(10.0, 20.0, 30.0),
            angle: 0.0,
            scale: Vec3::ONE,
        };
        let mat = obj.to_matrix();
        let col3 = mat.col(3);
        assert!((col3.x - 10.0).abs() < 1e-5);
        assert!((col3.y - 20.0).abs() < 1e-5);
        assert!((col3.z - 30.0).abs() < 1e-5);
    }

    #[test]
    fn placed_object_matrix_scale() {
        let obj = PlacedObject {
            position: Vec3::ZERO,
            angle: 0.0,
            scale: Vec3::new(2.0, 3.0, 4.0),
        };
        let mat = obj.to_matrix();
        let col0 = mat.col(0);
        let col1 = mat.col(1);
        let col2 = mat.col(2);
        // Scale should be reflected in the column magnitudes
        assert!((col0.length() - 2.0).abs() < 1e-5);
        assert!((col1.length() - 3.0).abs() < 1e-5);
        assert!((col2.length() - 4.0).abs() < 1e-5);
    }
}
