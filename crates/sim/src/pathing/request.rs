//! Path output of a request: the cell centre helper (`0x6F497D30`) and the waypoint output
//! (`0x6F49A470`, which turns the parent chain of the search into the caller's point list).
//!
//! Verified by `tools/emu/request_vectors.py` (Unicorn): the waypoint arrays of 400 random
//! requests per run, modes 1 and 2, with flag markers and fractional inputs. The float operations
//! are the software float helpers (`crates/softfloat`): integer to real (`0x6F6EF350`) and add
//! (`0x6F6EF000`). Every value they produce here is exactly representable, so the result does not
//! depend on the rounding of the helpers, but the operations are still the helpers'.
//!
//! The points are in the request's own coordinates (cells): the output has no origin and no
//! scale. Each node gives the point `(x + off, y + off)` of its seed, `off` being 0.75 in mode 1
//! and 1.25 in mode 2. A node with a non-zero via-flag byte also gives the marker point
//! `(-128000.0, flag)`. The first and last entries are replaced by the exact goal (or the closest
//! node's centre) and start points.

use super::pyramid::{Node, NO_NODE};
use softfloat::{arith, conv, F32};

/// `0.25`, the constant at `0x6FAAE4B8`.
const QUARTER: F32 = F32::from_bits(0x3E80_0000);
/// `0.5`, the constants at `0x6FAAE4D0` and `0x6FAAE4D4`.
const HALF: F32 = F32::from_bits(0x3F00_0000);
/// `1.0`, the constant at `0x6FAAE4F4` (mode 2).
const ONE: F32 = F32::from_bits(0x3F80_0000);
/// The marker's x: `-128000.0` (`0x6FAB7748`, written at start-up by a real parse of its text).
const FLAG_MARK_X: F32 = F32::from_bits(0xC7FA_0001);

/// The cell centre of a node's seed, `(x + 0.5, y + 0.5)` (`0x6F497D30`).
pub(super) fn centre(node: &Node) -> (F32, F32) {
    (
        arith::add(conv::i2r(node.seed.0), HALF),
        arith::add(conv::i2r(node.seed.1), HALF),
    )
}

/// Appends the waypoints of the parent chain from `node` to `out` (which must be empty), and
/// returns how many flag markers were written (added to `S+0xA0`).
///
/// The chain is walked from `node` towards the root. For mode 2, a block-level node whose seed is
/// on the last column (row) of its block is moved one cell back. Then the list is `out`, with
/// `out[0] = end` and `out[last] = start` replacing the first and last computed points.
pub(super) fn waypoints(
    nodes: &[Node],
    mode: u32,
    node: u32,
    start: (F32, F32),
    end: (F32, F32),
    out: &mut Vec<(F32, F32)>,
) -> u32 {
    let mode2 = mode == 2;
    let off = arith::add(if mode2 { ONE } else { HALF }, QUARTER);
    let mut flags = 0;
    let mut cur = node;
    loop {
        let n = &nodes[cur as usize];
        let (mut x, mut y) = n.seed;
        let lvl = u32::from(n.level);
        if mode2 && lvl != 0 {
            let size = 1i32 << lvl;
            let ox = (x >> lvl) << lvl;
            let oy = (y >> lvl) << lvl;
            if x == ox + size - 1 {
                x -= 1;
            }
            if y == oy + size - 1 {
                y -= 1;
            }
        }
        out.push((arith::add(conv::i2r(x), off), arith::add(conv::i2r(y), off)));
        if n.via_flag != 0 {
            out.push((FLAG_MARK_X, conv::i2r(i32::from(n.via_flag))));
            flags += 1;
        }
        if n.parent == NO_NODE {
            break;
        }
        cur = n.parent;
    }
    let last = out.len() - 1;
    out[last] = start;
    out[0] = end;
    flags
}
