//! Typed player actions of a 1.26a time slot (the command sub-stream of one player inside a 0x1E/0x1F block).
//!
//! Layouts are the field reads of the original's command readers, traced under Unicorn: each reader is
//! run on a known buffer and every stream primitive it calls is logged with its width and destination
//! (`tools/emu/` harness, scratch copy; see `docs/spec/replay-actions.md`, section 2). The dispatch is
//! `0x6F550730` (applier; CONFIRMED), whose `switch` maps the action byte to a handler
//! (`0x6F5402B0`..`0x6F5431E0`, `0x6F54BE30`..`0x6F54C1C0`, `0x6F540790`..`0x6F541250` and so on).
//! Each handler = reader (`ECX` = stream, `EDX` = payload object), `consumed == length` check
//! (`0x6F5378A0` on mismatch), then the apply template (`0x6F53Bxxx`), which dispatches the payload
//! as the command event `0x000A0000 | action` (CONFIRMED from the sender objects, which set `+0x14`
//! to `0xA0000 | action` and `+0x15` to the player id).
//!
//! Byte order is little endian. Strings are NUL terminated (the reader reads up to the NUL).
//!
//! Actions the original applies but this decoder does not model (`0x6E`, `0x81`..`0x83`) and every
//! id the original does not handle (its `switch` default) are errors with their absolute offset.
//! Nothing is skipped silently.
//!
//! Names of fields are the public replay names where the layout matches; the rest are named by wire
//! position (`unknown_*`) and marked as such in the spec.

use crate::Error;

/// An object reference as written in a replay: two 32-bit words. `NO_OBJECT` (both `0xFFFFFFFF`) means
/// "no target" (CONFIRMED: the sender writes `-1, -1` when it has no target, `0x6F2CBAD0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId {
    pub id1: u32,
    pub id2: u32,
}

/// The "no object" pair.
pub const NO_OBJECT: ObjectId = ObjectId {
    id1: 0xFFFF_FFFF,
    id2: 0xFFFF_FFFF,
};

impl ObjectId {
    pub fn is_none(self) -> bool {
        self == NO_OBJECT
    }
}

/// A world point as two single-precision floats (the bits are kept exactly).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// The ability-with-target fields shared by 0x10..0x15.
#[derive(Clone, Debug, PartialEq)]
pub struct AbilityHeader {
    /// Order flags (u16). Bit meanings: see `docs/spec/replay-actions.md`, section 4.
    pub flags: u16,
    /// Ability rawcode or order id (u32). Order ids are `0x000D_xxxx` (`0xD0003` smart, `0xD0012` move, ...).
    pub ability: u32,
    /// Two words at `+0x20`/`+0x24` of the payload (zero in normal play; meaning UNSURE).
    pub unknown: [u32; 2],
}

/// Fields of the 0x14 and 0x15 layouts (an ability with two points and one object).
#[derive(Clone, Debug, PartialEq)]
pub struct AbilityPointObject {
    pub header: AbilityHeader,
    pub point: Point,
    pub target: ObjectId,
    /// u32 at `+0x38` (UNSURE).
    pub unknown_38: u32,
    /// u8 at `+0x3C` (UNSURE).
    pub unknown_3c: u8,
    /// The second point (`+0x40`, `+0x44`).
    pub point2: Point,
}

/// One decoded action. The variant carries the wire fields; the sim-side meaning is in the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// 0x01 pause game (no payload).
    PauseGame,
    /// 0x02 resume game (no payload).
    ResumeGame,
    /// 0x03 set game speed; `setting` is the raw byte (0 slow, 1 normal, 2 fast; larger values are
    /// clamped to fast by the original, `0x6F53E890`).
    SetGameSpeed { setting: u8 },
    /// 0x04 increase game speed (no payload).
    IncreaseGameSpeed,
    /// 0x05 decrease game speed (no payload).
    DecreaseGameSpeed,
    /// 0x06 save game; the name is a NUL-terminated string (max 0x103 bytes, `0x6F4C2E10`).
    SaveGame { name: Vec<u8> },
    /// 0x07 save game finished; one u32 (UNSURE meaning).
    SaveGameFinished { value: u32 },
    /// 0x10 ability / order without a target (14 bytes).
    Ability(AbilityHeader),
    /// 0x11 ability / order with a point target (22 bytes).
    AbilityPoint { header: AbilityHeader, point: Point },
    /// 0x12 ability / order with a point and an object (30 bytes).
    AbilityPointObject {
        header: AbilityHeader,
        point: Point,
        target: ObjectId,
    },
    /// 0x13 ability / order with a point and two objects (38 bytes).
    AbilityPointTwoObjects {
        header: AbilityHeader,
        point: Point,
        first: ObjectId,
        second: ObjectId,
    },
    /// 0x14 ability with point, object and second point (43 bytes).
    AbilityPointObjectPoint(AbilityPointObject),
    /// 0x15 as 0x14 plus two words at `+0x48`/`+0x4C` (51 bytes).
    AbilityPointObjectPointEx {
        base: AbilityPointObject,
        extra: [u32; 2],
    },
    /// 0x16 change selection: `mode` 1 = add, 2 = remove (public names; the applier only dispatches the event).
    SelectionChange { mode: u8, units: Vec<ObjectId> },
    /// 0x17 assign group hotkey: group number and the units (`count` u16, then 8 bytes per unit).
    AssignGroup { group: u8, units: Vec<ObjectId> },
    /// 0x18 select group hotkey. Wire order: first byte (`+0x19`), then second byte (`+0x18`).
    /// The first byte is the group number by the public format (UNSURE for this build).
    SelectGroup { group: u8, unknown: u8 },
    /// 0x19 select subgroup: item rawcode and the unit (12 bytes).
    SelectSubgroup { item: u32, unit: ObjectId },
    /// 0x1A pre-subselection (no payload).
    PreSubselection,
    /// 0x1B: byte and two words (9 bytes). Meaning UNSURE.
    Variant1B { byte: u8, a: u32, b: u32 },
    /// 0x1C: same layout as 0x1B (9 bytes). Meaning UNSURE.
    Variant1C { byte: u8, a: u32, b: u32 },
    /// 0x1D: two words (8 bytes). Meaning UNSURE.
    Variant1D { a: u32, b: u32 },
    /// 0x1E: slot byte and a unit word (5 bytes). Meaning UNSURE (public: remove unit from queue).
    RemoveFromQueue { slot: u8, unit: u32 },
    /// 0x20, 0x22..0x26, 0x29..0x2C, 0x2F..0x32, 0x66, 0x67, 0x84, 0x85: no payload. The names are not
    /// known for this build; the id is kept.
    NoPayload { id: u8 },
    /// 0x21, 0x63, 0x64, 0x65: two words (8 bytes). Meaning UNSURE.
    Pair { id: u8, a: u32, b: u32 },
    /// 0x27, 0x28, 0x2D: byte and word (5 bytes). Meaning UNSURE.
    ByteWord { id: u8, byte: u8, value: u32 },
    /// 0x2E: one float (4 bytes). Meaning UNSURE.
    Real { id: u8, value: f32 },
    /// 0x50 change ally options: slot byte and flags word (5 bytes).
    AllyOptions { slot: u8, flags: u32 },
    /// 0x51 transfer resources: slot byte, gold and lumber (9 bytes; raw amounts as the game writes them).
    TransferResources { slot: u8, gold: u32, lumber: u32 },
    /// 0x60 chat trigger: two words, then a NUL-terminated string. Meaning UNSURE.
    ChatTrigger { words: [u32; 2], text: Vec<u8> },
    /// 0x61 ESC key pressed (no payload).
    EscPressed,
    /// 0x62: three words (12 bytes). Meaning UNSURE.
    Triple { id: u8, values: [u32; 3] },
    /// 0x68 minimap signal (ping): x, y and a third float (12 bytes; public: the duration).
    MinimapSignal { x: f32, y: f32, extra: f32 },
    /// 0x69 / 0x6A continue game blocks: four words (16 bytes).
    ContinueBlock { id: u8, words: [u32; 4] },
    /// 0x6B sync integer: three strings and a word.
    SyncInteger { strings: [Vec<u8>; 3], value: u32 },
    /// 0x6C: three strings and a float (cheat-style record; UNSURE which cheat).
    StringsReal { id: u8, strings: [Vec<u8>; 3], value: f32 },
    /// 0x6D: three strings and a word (cheat-style record; UNSURE).
    StringsWord { id: u8, strings: [Vec<u8>; 3], value: u32 },
    /// 0x70..0x73: three strings (meaning UNSURE).
    ThreeStrings { id: u8, strings: [Vec<u8>; 3] },
    /// 0x75: one byte (public: unknown, 1 byte).
    Byte { id: u8, value: u8 },
}

impl Action {
    /// The wire action byte of this action.
    pub fn id(&self) -> u8 {
        match self {
            Action::PauseGame => 0x01,
            Action::ResumeGame => 0x02,
            Action::SetGameSpeed { .. } => 0x03,
            Action::IncreaseGameSpeed => 0x04,
            Action::DecreaseGameSpeed => 0x05,
            Action::SaveGame { .. } => 0x06,
            Action::SaveGameFinished { .. } => 0x07,
            Action::Ability(_) => 0x10,
            Action::AbilityPoint { .. } => 0x11,
            Action::AbilityPointObject { .. } => 0x12,
            Action::AbilityPointTwoObjects { .. } => 0x13,
            Action::AbilityPointObjectPoint(_) => 0x14,
            Action::AbilityPointObjectPointEx { .. } => 0x15,
            Action::SelectionChange { .. } => 0x16,
            Action::AssignGroup { .. } => 0x17,
            Action::SelectGroup { .. } => 0x18,
            Action::SelectSubgroup { .. } => 0x19,
            Action::PreSubselection => 0x1A,
            Action::Variant1B { .. } => 0x1B,
            Action::Variant1C { .. } => 0x1C,
            Action::Variant1D { .. } => 0x1D,
            Action::RemoveFromQueue { .. } => 0x1E,
            Action::NoPayload { id }
            | Action::Pair { id, .. }
            | Action::ByteWord { id, .. }
            | Action::Real { id, .. }
            | Action::Triple { id, .. }
            | Action::ContinueBlock { id, .. }
            | Action::StringsReal { id, .. }
            | Action::StringsWord { id, .. }
            | Action::ThreeStrings { id, .. }
            | Action::Byte { id, .. } => *id,
            Action::AllyOptions { .. } => 0x50,
            Action::TransferResources { .. } => 0x51,
            Action::ChatTrigger { .. } => 0x60,
            Action::EscPressed => 0x61,
            Action::MinimapSignal { .. } => 0x68,
            Action::SyncInteger { .. } => 0x6B,
        }
    }
}

/// Byte cursor over one command sub-stream that reports absolute offsets.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    base: usize,
}

impl<'a> Cursor<'a> {
    fn fail(&self, wanted: usize) -> Error {
        Error::UnexpectedEof {
            offset: self.base + self.pos,
            wanted,
        }
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.data.len() - self.pos < n {
            return Err(self.fail(n));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn object(&mut self) -> Result<ObjectId, Error> {
        Ok(ObjectId {
            id1: self.u32()?,
            id2: self.u32()?,
        })
    }

    fn point(&mut self) -> Result<Point, Error> {
        Ok(Point {
            x: self.f32()?,
            y: self.f32()?,
        })
    }

    /// NUL-terminated string; the terminator is consumed and not returned.
    fn cstr(&mut self) -> Result<Vec<u8>, Error> {
        let rest = &self.data[self.pos..];
        match rest.iter().position(|&b| b == 0) {
            Some(n) => {
                let s = rest[..n].to_vec();
                self.pos += n + 1;
                Ok(s)
            }
            None => Err(Error::UnterminatedString {
                offset: self.base + self.pos,
            }),
        }
    }

    fn strings3(&mut self) -> Result<[Vec<u8>; 3], Error> {
        Ok([self.cstr()?, self.cstr()?, self.cstr()?])
    }

    fn header(&mut self) -> Result<AbilityHeader, Error> {
        Ok(AbilityHeader {
            flags: self.u16()?,
            ability: self.u32()?,
            unknown: [self.u32()?, self.u32()?],
        })
    }

    fn ability_point_object(&mut self) -> Result<AbilityPointObject, Error> {
        Ok(AbilityPointObject {
            header: self.header()?,
            point: self.point()?,
            target: self.object()?,
            unknown_38: self.u32()?,
            unknown_3c: self.u8()?,
            point2: self.point()?,
        })
    }

    /// Selection-style list: a count word, then one object per entry.
    fn object_list(&mut self) -> Result<Vec<ObjectId>, Error> {
        let n = self.u16()? as usize;
        let mut v = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            v.push(self.object()?);
        }
        Ok(v)
    }
}

/// Decodes the command sub-stream of one player (the bytes of one `CommandBlock`).
///
/// `base` is the absolute offset of `data` in the decompressed replay, used in errors. Every action is
/// read to its end; an unknown id, an unmodelled id or a short payload is an error with its offset.
pub fn decode_actions(data: &[u8], base: usize) -> Result<Vec<Action>, Error> {
    let mut c = Cursor { data, pos: 0, base };
    let mut out = Vec::new();
    while c.pos < data.len() {
        let at = base + c.pos;
        let id = c.u8()?;
        let action = match id {
            0x01 => Action::PauseGame,
            0x02 => Action::ResumeGame,
            0x03 => Action::SetGameSpeed { setting: c.u8()? },
            0x04 => Action::IncreaseGameSpeed,
            0x05 => Action::DecreaseGameSpeed,
            0x06 => Action::SaveGame { name: c.cstr()? },
            0x07 => Action::SaveGameFinished { value: c.u32()? },
            0x10 => Action::Ability(c.header()?),
            0x11 => Action::AbilityPoint {
                header: c.header()?,
                point: c.point()?,
            },
            0x12 => Action::AbilityPointObject {
                header: c.header()?,
                point: c.point()?,
                target: c.object()?,
            },
            0x13 => Action::AbilityPointTwoObjects {
                header: c.header()?,
                point: c.point()?,
                first: c.object()?,
                second: c.object()?,
            },
            0x14 => Action::AbilityPointObjectPoint(c.ability_point_object()?),
            0x15 => Action::AbilityPointObjectPointEx {
                base: c.ability_point_object()?,
                extra: [c.u32()?, c.u32()?],
            },
            0x16 => {
                let mode = c.u8()?;
                Action::SelectionChange {
                    mode,
                    units: c.object_list()?,
                }
            }
            0x17 => {
                let group = c.u8()?;
                Action::AssignGroup {
                    group,
                    units: c.object_list()?,
                }
            }
            0x18 => {
                let first = c.u8()?;
                let second = c.u8()?;
                Action::SelectGroup {
                    group: first,
                    unknown: second,
                }
            }
            0x19 => Action::SelectSubgroup {
                item: c.u32()?,
                unit: c.object()?,
            },
            0x1A => Action::PreSubselection,
            0x1B => Action::Variant1B {
                byte: c.u8()?,
                a: c.u32()?,
                b: c.u32()?,
            },
            0x1C => Action::Variant1C {
                byte: c.u8()?,
                a: c.u32()?,
                b: c.u32()?,
            },
            0x1D => Action::Variant1D {
                a: c.u32()?,
                b: c.u32()?,
            },
            0x1E => Action::RemoveFromQueue {
                slot: c.u8()?,
                unit: c.u32()?,
            },
            0x20 | 0x22..=0x26 | 0x29..=0x2C | 0x2F..=0x32 | 0x66 | 0x67 | 0x84 | 0x85 => {
                Action::NoPayload { id }
            }
            0x21 | 0x63 | 0x64 | 0x65 => Action::Pair {
                id,
                a: c.u32()?,
                b: c.u32()?,
            },
            0x27 | 0x28 | 0x2D => Action::ByteWord {
                id,
                byte: c.u8()?,
                value: c.u32()?,
            },
            0x2E => Action::Real {
                id,
                value: c.f32()?,
            },
            0x50 => Action::AllyOptions {
                slot: c.u8()?,
                flags: c.u32()?,
            },
            0x51 => Action::TransferResources {
                slot: c.u8()?,
                gold: c.u32()?,
                lumber: c.u32()?,
            },
            0x60 => {
                // The two words are read first (`0x6F555010`), then the string.
                let words = [c.u32()?, c.u32()?];
                Action::ChatTrigger {
                    words,
                    text: c.cstr()?,
                }
            }
            0x61 => Action::EscPressed,
            0x62 => Action::Triple {
                id,
                values: [c.u32()?, c.u32()?, c.u32()?],
            },
            0x68 => Action::MinimapSignal {
                x: c.f32()?,
                y: c.f32()?,
                extra: c.f32()?,
            },
            0x69 | 0x6A => Action::ContinueBlock {
                id,
                words: [c.u32()?, c.u32()?, c.u32()?, c.u32()?],
            },
            0x6B => Action::SyncInteger {
                strings: c.strings3()?,
                value: c.u32()?,
            },
            0x6C => Action::StringsReal {
                id,
                strings: c.strings3()?,
                value: c.f32()?,
            },
            0x6D => Action::StringsWord {
                id,
                strings: c.strings3()?,
                value: c.u32()?,
            },
            0x70..=0x73 => Action::ThreeStrings {
                id,
                strings: c.strings3()?,
            },
            0x75 => Action::Byte {
                id,
                value: c.u8()?,
            },
            // Read by the original (`0x6F554680`..`0x6F5552E0`) but not modelled here: the 0x6E layout
            // has loops whose count is not traced yet, and 0x81..0x83 are replay-file markers.
            0x6E | 0x81..=0x83 => return Err(Error::UnsupportedAction { offset: at, id }),
            _ => return Err(Error::UnknownAction { offset: at, id }),
        };
        out.push(action);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le32(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    #[test]
    fn pause_and_speed_have_their_payloads() {
        // 0x03 takes one byte (the setting), so the second 0x02 is the resume.
        let a = decode_actions(&[0x01, 0x03, 0x02, 0x02, 0x04, 0x05], 0).unwrap();
        assert_eq!(
            a,
            vec![
                Action::PauseGame,
                Action::SetGameSpeed { setting: 2 },
                Action::ResumeGame,
                Action::IncreaseGameSpeed,
                Action::DecreaseGameSpeed,
            ]
        );
    }

    #[test]
    fn move_order_with_no_target_is_14_bytes() {
        let mut b = vec![0x10, 0x00, 0x00];
        b.extend_from_slice(&le32(0x000D_0012));
        b.extend_from_slice(&le32(0));
        b.extend_from_slice(&le32(0));
        let a = decode_actions(&b, 100).unwrap();
        assert_eq!(b.len(), 15);
        match &a[0] {
            Action::Ability(h) => {
                assert_eq!(h.flags, 0);
                assert_eq!(h.ability, 0x000D_0012);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn point_order_reads_two_float_bits() {
        let mut b = vec![0x11, 0x01, 0x00];
        b.extend_from_slice(&le32(0x000D_0003));
        b.extend_from_slice(&le32(0));
        b.extend_from_slice(&le32(0));
        b.extend_from_slice(&le32(1000.0f32.to_bits()));
        b.extend_from_slice(&le32((-250.5f32).to_bits()));
        let a = decode_actions(&b, 0).unwrap();
        assert_eq!(
            a,
            vec![Action::AbilityPoint {
                header: AbilityHeader {
                    flags: 1,
                    ability: 0xD0003,
                    unknown: [0, 0]
                },
                point: Point {
                    x: 1000.0,
                    y: -250.5
                },
            }]
        );
    }

    #[test]
    fn object_target_keeps_the_pair_and_none_is_recognised() {
        let mut b = vec![0x12, 0x00, 0x00];
        b.extend_from_slice(&le32(0xD000F));
        b.extend_from_slice(&le32(0));
        b.extend_from_slice(&le32(0));
        b.extend_from_slice(&le32(0));
        b.extend_from_slice(&le32(0));
        b.extend_from_slice(&le32(7));
        b.extend_from_slice(&le32(9));
        let a = decode_actions(&b, 0).unwrap();
        match &a[0] {
            Action::AbilityPointObject { target, .. } => {
                assert_eq!(*target, ObjectId { id1: 7, id2: 9 });
                assert!(!target.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(NO_OBJECT.is_none());
    }

    #[test]
    fn selection_reads_count_then_pairs() {
        let mut b = vec![0x16, 0x01];
        b.extend_from_slice(&2u16.to_le_bytes());
        for id in [(1u32, 2u32), (3, 4)] {
            b.extend_from_slice(&le32(id.0));
            b.extend_from_slice(&le32(id.1));
        }
        let a = decode_actions(&b, 0).unwrap();
        assert_eq!(
            a,
            vec![Action::SelectionChange {
                mode: 1,
                units: vec![ObjectId { id1: 1, id2: 2 }, ObjectId { id1: 3, id2: 4 }],
            }]
        );
    }

    #[test]
    fn strings_are_nul_terminated_and_short_payloads_fail_with_offset() {
        let mut b = vec![0x06];
        b.extend_from_slice(b"save\0");
        let a = decode_actions(&b, 0).unwrap();
        assert_eq!(
            a,
            vec![Action::SaveGame {
                name: b"save".to_vec()
            }]
        );
        // 0x19 needs 12 bytes; only 4 are there. The error names the offset of the payload.
        let err = decode_actions(&[0x01, 0x19, 0, 0, 0], 40).unwrap_err();
        match err {
            Error::UnexpectedEof { offset, .. } => assert_eq!(offset, 42),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn unknown_and_unmodelled_ids_fail_loudly_with_their_offset() {
        let err = decode_actions(&[0x01, 0x6F], 7).unwrap_err();
        assert!(matches!(err, Error::UnknownAction { offset: 8, id: 0x6F }));
        let err = decode_actions(&[0x6E], 0).unwrap_err();
        assert!(matches!(err, Error::UnsupportedAction { offset: 0, id: 0x6E }));
        let err = decode_actions(&[0x81], 3).unwrap_err();
        assert!(matches!(err, Error::UnsupportedAction { offset: 3, id: 0x81 }));
    }

    #[test]
    fn unterminated_string_is_an_error() {
        let err = decode_actions(&[0x06, b'a', b'b'], 0).unwrap_err();
        assert!(matches!(err, Error::UnterminatedString { .. }));
    }

    #[test]
    fn every_payload_size_matches_the_traced_reader() {
        // Sizes traced under Unicorn (payload bytes after the action byte).
        let cases: &[(u8, usize)] = &[
            (0x10, 14),
            (0x11, 22),
            (0x12, 30),
            (0x13, 38),
            (0x14, 43),
            (0x15, 51),
            (0x18, 2),
            (0x19, 12),
            (0x1B, 9),
            (0x1C, 9),
            (0x1D, 8),
            (0x1E, 5),
            (0x21, 8),
            (0x27, 5),
            (0x2E, 4),
            (0x50, 5),
            (0x51, 9),
            (0x62, 12),
            (0x63, 8),
            (0x68, 12),
            (0x69, 16),
            (0x6A, 16),
            (0x75, 1),
        ];
        for &(id, n) in cases {
            let mut b = vec![id];
            b.extend(std::iter::repeat_n(0x11u8, n));
            let a = decode_actions(&b, 0).unwrap_or_else(|e| panic!("{id:#x}: {e}"));
            assert_eq!(a.len(), 1, "id {id:#x}");
            assert_eq!(a[0].id(), id);
        }
    }

    #[test]
    fn no_payload_ids_take_no_bytes() {
        for id in [0x1A, 0x20, 0x22, 0x26, 0x29, 0x2C, 0x2F, 0x32, 0x61, 0x66, 0x67, 0x84, 0x85] {
            let a = decode_actions(&[id], 0).unwrap();
            assert_eq!(a[0].id(), id);
        }
    }

    #[test]
    fn sync_integer_reads_three_strings_and_a_word() {
        let mut b = vec![0x6B];
        b.extend_from_slice(b"a\0bb\0\0");
        b.extend_from_slice(&le32(77));
        let a = decode_actions(&b, 0).unwrap();
        assert_eq!(
            a,
            vec![Action::SyncInteger {
                strings: [b"a".to_vec(), b"bb".to_vec(), Vec::new()],
                value: 77,
            }]
        );
    }
}
