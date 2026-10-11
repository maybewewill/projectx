//! Building the pathing data of a map: the cell words from `war3map.wpm`, and the pyramid levels
//! from the cell words.
//!
//! Spec: `docs/spec/pathing-build.md` (sections 1.3, 3, 4.1). The original: the wpm byte mapping
//! at `0x6F46BAD7..0x6F46BB22` (in the reader `0x6F46B850`), the rect rebuild `0x6F488330`, the
//! level-A builder `0x6F485150` with its cell step `0x6F483160`, and the upper builder `0x6F4881C0`
//! with its child step `0x6F488010`.
//!
//! Verified by `tools/emu/pyramid_build_vectors.py` (Unicorn runs of the rebuild `0x6F488330` on
//! 300 generated grids, 44,842 records, 0 mismatches; the vectors are checked again by
//! `crates/sim/tests/pathing/build_cases.rs`). The vectors cover the whole grid as the rebuild
//! rect, cells without occupant lists, and level dimensions `ceil(W / 2^k)`.
//!
//! Level mapping (the open item of pathing-build.md, section 9.1, resolved here as UNSURE):
//! * The search's level 0 is one record per cell. No code that writes such records was found, so
//!   [`build_pyramid`] derives it from the cell words with the level-A rule applied to one cell
//!   (state 01 when the cell is blocked for the type, 00 otherwise).
//! * Search levels 1, 2 and 3 are the builder's levels A, B and C (2, 4 and 8 cells per side). This
//!   is consistent with the search: state 10 (a mixed 2x2 block) makes the lookup refine to the
//!   cell records, and the search's largest region is 8 cells (level 3). The builder's level D
//!   (16 cells) has no search level and is not used by [`build_pyramid`]; [`build_levels`] builds
//!   it so the original's output can still be compared.

use super::grid::NO_OCCUPANT;
use super::pyramid::{Level, Pyramid};

/// Path-type shifts of the four state fields built by the original (types 0, 2, 4, 6).
pub const TYPE_SHIFTS: [u32; 4] = [0, 2, 4, 6];
/// Per-type test masks of the level builders (`0x6FA84E84..0x6FA84E90`). Only the top byte of a
/// cell word is tested, so the low bytes of these masks do not matter for the cell words.
pub const TYPE_MASKS: [u32; 4] = [0x0600_0006, 0x8000_0080, 0x4000_0040, 0x0400_0004];

/// Errors of the builders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// The map's pathing grid has no cells.
    Empty,
    /// The byte count does not equal `width * height`.
    Size { expected: usize, found: usize },
    /// The grid is too large for the 32-bit index arithmetic of the original.
    TooLarge,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Empty => write!(f, "pathing grid has no cells"),
            BuildError::Size { expected, found } => {
                write!(
                    f,
                    "pathing grid has {found} cell bytes, expected {expected}"
                )
            }
            BuildError::TooLarge => write!(f, "pathing grid is too large"),
        }
    }
}

impl std::error::Error for BuildError {}

/// Top byte of the cell word for one byte of the `wpm` file (`0x6F46BAD7..0x6F46BB22`).
///
/// Each file bit ORs its own constant (`0x02 -> 0x13`, `0x04 -> 0x05`, `0x08 -> 0x09`,
/// `0x20 -> 0x20`, `0x40 -> 0x41`). The summary value `0x81` is ORed in addition when bit `0x80`
/// is set, or when both `0x02` and `0x40` are set. Bits `0x01` and `0x10` of the file byte have no
/// effect. There is no inversion: a set bit means blocked.
pub fn wpm_top_byte(b: u8) -> u8 {
    let mut v = 0u8;
    if b & 0x02 != 0 {
        v |= 0x13;
    }
    if b & 0x04 != 0 {
        v |= 0x05;
    }
    if b & 0x08 != 0 {
        v |= 0x09;
    }
    if b & 0x20 != 0 {
        v |= 0x20;
    }
    if b & 0x40 != 0 {
        v |= 0x41;
    }
    if b & 0x80 != 0 || b & 0x42 == 0x42 {
        v |= 0x81;
    }
    v
}

/// Cell words of a `width` by `height` grid from the file's cell bytes (row-major).
///
/// The top byte is [`wpm_top_byte`] of the file byte and the low 24 bits are [`NO_OCCUPANT`]. The
/// original leaves the low bits of the cell words to the occupant code; with no units placed the
/// value is the "no occupant" sentinel, which is what this assumes (UNSURE for the initial value).
pub fn cell_words(width: u32, height: u32, bytes: &[u8]) -> Result<Vec<u32>, BuildError> {
    let expected = cell_count(width, height)?;
    if bytes.len() != expected {
        return Err(BuildError::Size {
            expected,
            found: bytes.len(),
        });
    }
    Ok(bytes
        .iter()
        .map(|&b| (u32::from(wpm_top_byte(b)) << 24) | NO_OCCUPANT)
        .collect())
}

fn cell_count(width: u32, height: u32) -> Result<usize, BuildError> {
    if width == 0 || height == 0 {
        return Err(BuildError::Empty);
    }
    (width as usize)
        .checked_mul(height as usize)
        .filter(|&n| n <= u32::MAX as usize)
        .ok_or(BuildError::TooLarge)
}

/// Dimensions of the builder levels A, B, C, D: `ceil(width / 2^k)` for `k = 1..=4` (LIKELY: the
/// out-of-range rule of the builders only matters for partial blocks, which needs the rounding up;
/// the original's own level dimensions were not located).
pub fn level_dims(width: u32, height: u32) -> [(u32, u32); 4] {
    let mut out = [(0, 0); 4];
    let (mut w, mut h) = (width, height);
    for slot in &mut out {
        w = w.div_ceil(2);
        h = h.div_ceil(2);
        *slot = (w, h);
    }
    out
}

/// Whether the cell `(x, y)` blocks path type `mask`. Outside the grid counts as blocked.
fn cell_blocked(words: &[u32], width: u32, height: u32, x: u64, y: u64, mask: u32) -> bool {
    if x >= u64::from(width) || y >= u64::from(height) {
        return true;
    }
    let word = words[(y * u64::from(width) + x) as usize];
    word & mask & 0xFF00_0000 != 0
}

/// Sets the 2-bit state `value` (1 = `01`, 2 = `10`) of path type `t` in a record word.
fn set_state(word: u32, t: u32, value: u32) -> u32 {
    word | ((value << 30) >> t)
}

/// The state of one 2x2 block (or one cell) from its number of blocked members.
fn state_of(blocked: u32) -> u32 {
    match blocked {
        0 => 0,
        4 => 1,
        _ => 2,
    }
}

/// Level-A records (`0x6F485150` with the cell step `0x6F483160`): for each record, the state of
/// each type from the 2x2 cells `(2x, 2y)`, `(2x+1, 2y)`, `(2x+1, 2y+1)`, `(2x, 2y+1)`.
fn level_a(words: &[u32], width: u32, height: u32, dims: (u32, u32)) -> Vec<u32> {
    let (dw, dh) = dims;
    let mut out = Vec::with_capacity(dw as usize * dh as usize);
    for y in 0..u64::from(dh) {
        for x in 0..u64::from(dw) {
            let mut word = 0u32;
            for (t, mask) in TYPE_SHIFTS.iter().zip(TYPE_MASKS) {
                let blocked = [
                    (2 * x, 2 * y),
                    (2 * x + 1, 2 * y),
                    (2 * x + 1, 2 * y + 1),
                    (2 * x, 2 * y + 1),
                ]
                .iter()
                .filter(|&&(cx, cy)| cell_blocked(words, width, height, cx, cy, mask))
                .count() as u32;
                let state = state_of(blocked);
                if state != 0 {
                    word = set_state(word, *t, state);
                }
            }
            out.push(word);
        }
    }
    out
}

/// How one child record counts for the upper-level rule (`0x6F488010`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Child {
    Bad,
    Block,
    Good,
}

fn classify(child: &[u32], dims: (u32, u32), t: u32, x: u64, y: u64) -> Child {
    let (cw, ch) = dims;
    if x >= u64::from(cw) || y >= u64::from(ch) {
        return Child::Bad;
    }
    let word = child[(y * u64::from(cw) + x) as usize];
    let state = (word << t) & 0xC000_0000;
    let tag = (word >> 16) & 0xFF;
    if state == 0x4000_0000 {
        Child::Bad
    } else if state == 0x8000_0000 || tag != 0 {
        Child::Block
    } else {
        Child::Good
    }
}

/// Upper-level records (`0x6F4881C0` with the child step `0x6F488010`): the state of each type from
/// the four child records of the 2x2 block. Any blocking child makes the state `10`; otherwise the
/// bad children decide (4 gives `01`, 1 to 3 give `10`, 0 gives `00`).
fn upper(child: &[u32], child_dims: (u32, u32), dims: (u32, u32)) -> Vec<u32> {
    let (dw, dh) = dims;
    let mut out = Vec::with_capacity(dw as usize * dh as usize);
    for y in 0..u64::from(dh) {
        for x in 0..u64::from(dw) {
            let mut word = 0u32;
            for &t in &TYPE_SHIFTS {
                let cells = [
                    (2 * x, 2 * y),
                    (2 * x + 1, 2 * y),
                    (2 * x + 1, 2 * y + 1),
                    (2 * x, 2 * y + 1),
                ];
                let kinds = cells.map(|(cx, cy)| classify(child, child_dims, t, cx, cy));
                let state = if kinds.contains(&Child::Block) {
                    2
                } else {
                    state_of(kinds.iter().filter(|&&k| k == Child::Bad).count() as u32)
                };
                if state != 0 {
                    word = set_state(word, t, state);
                }
            }
            out.push(word);
        }
    }
    out
}

/// The builder levels A, B, C and D from the cell words (the rect rebuild over the whole grid).
///
/// `words` are the cell words of a `width` by `height` grid. The records hold only the state
/// fields and zero tag and region bits, as a freshly built pyramid does.
pub fn build_levels(words: &[u32], width: u32, height: u32) -> Result<[Level; 4], BuildError> {
    let expected = cell_count(width, height)?;
    if words.len() != expected {
        return Err(BuildError::Size {
            expected,
            found: words.len(),
        });
    }
    let dims = level_dims(width, height);
    let a = level_a(words, width, height, dims[0]);
    let b = upper(&a, dims[0], dims[1]);
    let c = upper(&b, dims[1], dims[2]);
    let d = upper(&c, dims[2], dims[3]);
    Ok([
        Level::new(dims[0].0, dims[0].1, &a),
        Level::new(dims[1].0, dims[1].1, &b),
        Level::new(dims[2].0, dims[2].1, &c),
        Level::new(dims[3].0, dims[3].1, &d),
    ])
}

/// The search's pyramid for a grid: level 0 is one record per cell (state `01` where the cell is
/// blocked for the type, `00` otherwise), levels 1 to 3 are the builder levels A, B and C.
/// See the module documentation for why this mapping was chosen (UNSURE).
pub fn build_pyramid(words: &[u32], width: u32, height: u32) -> Result<Pyramid, BuildError> {
    let expected = cell_count(width, height)?;
    if words.len() != expected {
        return Err(BuildError::Size {
            expected,
            found: words.len(),
        });
    }
    let cell_words: Vec<u32> = words
        .iter()
        .map(|&w| {
            let mut word = 0u32;
            for (t, mask) in TYPE_SHIFTS.iter().zip(TYPE_MASKS) {
                if w & mask & 0xFF00_0000 != 0 {
                    word = set_state(word, *t, 1);
                }
            }
            word
        })
        .collect();
    let [a, b, c, _d] = build_levels(words, width, height)?;
    Ok(Pyramid::new([
        Level::new(width, height, &cell_words),
        a,
        b,
        c,
    ]))
}

/// The record words of levels A to D, row-major, one list per level (for comparisons).
pub fn level_words(levels: &[Level; 4]) -> [Vec<u32>; 4] {
    levels
        .each_ref()
        .map(|l| l.records().iter().map(|r| r.word1).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpm_bytes_map_to_the_original_top_bytes() {
        assert_eq!(wpm_top_byte(0x00), 0x00);
        assert_eq!(wpm_top_byte(0x02), 0x13);
        assert_eq!(wpm_top_byte(0x04), 0x05);
        assert_eq!(wpm_top_byte(0x08), 0x09);
        assert_eq!(wpm_top_byte(0x20), 0x20);
        assert_eq!(wpm_top_byte(0x40), 0x41);
        assert_eq!(wpm_top_byte(0x80), 0x81);
        // 0x02 and 0x40 together also set the summary value.
        assert_eq!(wpm_top_byte(0x42), 0x13 | 0x41 | 0x81);
        // Bits 0x01 and 0x10 do nothing.
        assert_eq!(wpm_top_byte(0x11), 0x00);
        // Several bits OR together.
        assert_eq!(wpm_top_byte(0x0C), 0x05 | 0x09);
    }

    #[test]
    fn cell_words_keep_the_sentinel_low_bits_and_check_the_size() {
        let words = cell_words(2, 1, &[0x02, 0x00]).unwrap();
        assert_eq!(words, vec![(0x13 << 24) | NO_OCCUPANT, NO_OCCUPANT]);
        assert_eq!(
            cell_words(2, 2, &[0; 3]),
            Err(BuildError::Size {
                expected: 4,
                found: 3
            })
        );
        assert_eq!(cell_words(0, 2, &[]), Err(BuildError::Empty));
    }

    #[test]
    fn level_dims_round_up() {
        assert_eq!(
            level_dims(17, 9),
            [(9, 5), (5, 3), (3, 2), (2, 1)],
            "odd sizes keep their partial block"
        );
        assert_eq!(level_dims(8, 8)[0], (4, 4));
    }

    #[test]
    fn level_a_states_follow_the_block_count() {
        // 4x2 cells. Blocked for type 0: (0,0), (1,1), (2,1), (3,1). Level A is 2x1 records.
        // Record 0 covers (0,0),(1,0),(0,1),(1,1): two blocked, state 10.
        // Record 1 covers (2,0),(3,0),(2,1),(3,1): two blocked, state 10.
        let blocked = (0x02 << 24) | NO_OCCUPANT;
        let mut words = vec![NO_OCCUPANT; 8];
        for i in [0, 5, 6, 7] {
            words[i] = blocked;
        }
        let levels = build_levels(&words, 4, 2).unwrap();
        let a = level_words(&levels)[0].clone();
        assert_eq!(a, vec![0x8000_0000, 0x8000_0000]);
        // Type 2 is only blocked by the top bit 0x80, which these cells do not have: state 00.
        assert_eq!((a[0] << 2) >> 30, 0);
    }

    #[test]
    fn out_of_range_cells_count_as_blocked_in_partial_blocks() {
        // A 1x1 grid: the record's three missing cells are blocked for every type, so an open
        // cell gives 10 for every type (0xAA000000).
        let open = build_levels(&[NO_OCCUPANT], 1, 1).unwrap();
        assert_eq!(level_words(&open)[0], vec![0xAA00_0000]);
        // Blocked for type 0 as well: the four cells are blocked (01), the other types see three
        // blocked cells (10): 0x6A000000.
        let blocked = build_levels(&[(0x02 << 24) | NO_OCCUPANT], 1, 1).unwrap();
        assert_eq!(level_words(&blocked)[0], vec![0x6A00_0000]);
    }

    #[test]
    fn upper_block_wins_over_bad_children() {
        // Level A of 2x2 cells: child (0,0) blocked (mixed), the rest fully blocked. Level B is one
        // record: any block child gives 10 for type 0, whatever the others are.
        let mut child = vec![0u32; 4];
        child[0] = 2 << 30;
        child[1] = 1 << 30;
        child[2] = 1 << 30;
        child[3] = 1 << 30;
        let b = upper(&child, (2, 2), (1, 1));
        assert_eq!(b, vec![2 << 30]);
    }

    #[test]
    fn search_on_a_pyramid_built_from_a_wall_routes_through_the_gap() {
        use super::super::search::{FlagLink, Request, Search, Status};
        use softfloat::F32;
        // 16 by 8 cells from file bytes: a wall (bit 0x02, unwalkable) on column 8 for rows 0..6,
        // open at row 7.
        let (w, h) = (16u32, 8u32);
        let mut bytes = vec![0u8; (w * h) as usize];
        for y in 0..7 {
            bytes[(y * w + 8) as usize] = 0x02;
        }
        let words = cell_words(w, h, &bytes).unwrap();
        let pyramid = build_pyramid(&words, w, h).unwrap();
        let mut search = Search::new(pyramid, vec![FlagLink::default(); 256], 8, 0);
        let at = |x: f32, y: f32| (F32::from_bits(x.to_bits()), F32::from_bits(y.to_bits()));
        let req = Request {
            path_type: 0,
            flag_on: false,
            start: at(2.5, 2.5),
            goal: at(13.5, 2.5),
            budget: super::super::search::POP_BUDGET,
            size_class: 0,
        };
        let path = search.find_path(&req);
        assert_eq!(path.status, Status::Reached);
        assert!(
            path.cells.iter().any(|&(_, y)| y >= 7),
            "the route goes down to the gap at row 7: {:?}",
            path.cells
        );
        assert!(
            path.cells.iter().all(|&(x, y)| !(x == 8 && y < 7)),
            "no route cell on the wall"
        );
    }

    #[test]
    fn pyramid_level_zero_is_per_cell_and_stops_refinement_at_mixed_blocks() {
        // One blocked cell in a 2x2 grid: level 0 blocks it, level 1 is mixed (10), so a query at
        // level 1 asks for a finer look and the cell query gives its own record.
        let words = vec![
            (0x02 << 24) | NO_OCCUPANT,
            NO_OCCUPANT,
            NO_OCCUPANT,
            NO_OCCUPANT,
        ];
        let p = build_pyramid(&words, 2, 2).unwrap();
        assert_eq!(p.level(0).records()[0].word1 >> 30, 1);
        assert_eq!(p.level(0).records()[1].word1, 0);
        assert_eq!(p.level(1).records()[0].word1 >> 30, 2);
        assert_eq!(p.level(3).width(), 1);
    }
}
