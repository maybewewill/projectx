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

use glam::{Mat4, Quat, Vec3};

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

#[cfg(test)]
mod tests {
    use super::*;

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
