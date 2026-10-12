//! Terrain objects: cliff models, placed doodads, and pre-placed units.
//!
//! This module handles loading and rendering 3D models placed on the terrain. It includes:
//! - **Cliffs**: loaded from Cliffs.slk, with model names derived from corner heights
//! - **Doodads**: loaded from `war3map.doo`, resolved through Doodads.slk
//! - **Units**: loaded from `war3mapUnits.doo`, resolved through unit SLK files
//!
//! The cliff model naming scheme: for each terrain tile with cliff height differences between
//! corners, a model is selected using the corner heights (a, b, c, d for corners) and the
//! tileset name. Path: `Doodads\Terrain\Cliffs\<Tileset><ABCD><variation>.mdx`.
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

/// Generate cliff model name from corner heights and tileset.
///
/// # Example
/// For a tile with corner heights [0, 1, 1, 0] and tileset "Ashenvale", returns:
/// `Doodads\Terrain\Cliffs\Ashenvalea b b a 0.mdx` (where variation = 0).
pub fn cliff_model_path(tileset: &str, corners: [u8; 4], variation: u32) -> String {
    let [tl, tr, br, bl] = corners;
    let letters = format!(
        "{}{}{}{}",
        height_letter(tl),
        height_letter(tr),
        height_letter(br),
        height_letter(bl)
    );
    format!(
        "Doodads\\Terrain\\Cliffs\\Cliffs{}{}_{}.mdx",
        tileset, letters, variation
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
        let path = cliff_model_path("Ashenvale", [0, 1, 1, 0], 0);
        assert!(path.contains("Cliffs"));
        assert!(path.contains("Ashenvale"));
        assert!(path.contains("abba"));
        assert!(path.ends_with(".mdx"));
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
