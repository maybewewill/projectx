//! JASS bytecode: fixed 8-byte instructions.
//!
//! Layout follows the field order of the original (docs/spec/jass-vm.md, section 1.1):
//!
//! | byte | field                                   |
//! |------|-----------------------------------------|
//! | 0    | `right` slot (right operand)            |
//! | 1    | `left` slot (left operand)              |
//! | 2    | `dest` slot (result / destination)      |
//! | 3    | opcode                                  |
//! | 4-7  | `imm`, little-endian `u32`              |
//!
//! The opcode numbers below are OUR OWN. The original's numbering is different and was not
//! reproduced; only the behaviour of the operations is meant to match. Opcode numbers are not
//! stable across crate versions and must not be persisted.
//!
//! Slot operands index the frame's slot array. `NO_SLOT` (0xFF) means "no slot": it is the
//! discard destination of a call whose result is unused, and the "no value" return operand.
//!
//! `imm` meaning per opcode is documented on each variant.

/// Marker for an unused slot operand.
pub const NO_SLOT: u8 = 0xFF;

/// Opcodes. Operand conventions are documented per variant; "slot[x]" means the value in slot x.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    /// dest = Integer(imm as i32)
    LoadInt = 0x01,
    /// dest = Real(f32::from_bits(imm))
    LoadReal = 0x02,
    /// dest = Boolean(imm != 0)
    LoadBool = 0x03,
    /// dest = String(imm), imm is an index into the module string table
    LoadString = 0x04,
    /// dest = Null
    LoadNull = 0x05,
    /// dest = Code(imm), imm is a script function index
    LoadCode = 0x06,
    /// dest = slot[left]
    Move = 0x07,
    /// dest = global[imm] (scalar global)
    GlobalGet = 0x08,
    /// global[imm] = slot[right] (scalar global)
    GlobalSet = 0x09,
    /// dest = global_array[imm][slot[left]]
    GlobalArrayGet = 0x0A,
    /// global_array[imm][slot[left]] = slot[right]
    GlobalArraySet = 0x0B,
    /// dest = local_array[imm][slot[left]] (array local of the current frame)
    LocalArrayGet = 0x0C,
    /// local_array[imm][slot[left]] = slot[right]
    LocalArraySet = 0x0D,

    /// dest = slot[left] + slot[right] (wrapping)
    AddI = 0x10,
    /// dest = slot[left] - slot[right] (wrapping)
    SubI = 0x11,
    /// dest = slot[left] * slot[right] (wrapping)
    MulI = 0x12,
    /// dest = slot[left] / slot[right], truncating. Zero divisor aborts the thread.
    DivI = 0x13,
    /// dest = slot[left] % slot[right], C remainder (sign of dividend). Zero divisor gives 0.
    ModI = 0x14,

    /// dest = slot[left] + slot[right] in f32 (see `vm::real`)
    AddR = 0x18,
    /// dest = slot[left] - slot[right] in f32
    SubR = 0x19,
    /// dest = slot[left] * slot[right] in f32
    MulR = 0x1A,
    /// dest = slot[left] / slot[right] in f32. Zero divisor (either sign) aborts the thread.
    DivR = 0x1B,

    /// dest = String(concat(slot[left], slot[right]))
    Concat = 0x1C,

    /// dest = -slot[left] (wrapping)
    NegI = 0x20,
    /// dest = -slot[left] (sign flip)
    NegR = 0x21,
    /// dest = !slot[left] (boolean)
    Not = 0x22,
    /// dest = Real(slot[left] as f32), exact for |x| < 2^24, rounded otherwise
    IntToReal = 0x23,
    /// dest = slot[left] && slot[right]. Both operands are already evaluated (no short circuit).
    And = 0x24,
    /// dest = slot[left] || slot[right]. Both operands are already evaluated (no short circuit).
    Or = 0x25,

    /// dest = (slot[left] == slot[right]) with JASS equality (see `vm::jass_equal`)
    Eq = 0x30,
    /// dest = (slot[left] != slot[right])
    Ne = 0x31,
    /// dest = (slot[left] < slot[right]), integer or real operands
    Lt = 0x32,
    /// dest = (slot[left] <= slot[right])
    Le = 0x33,
    /// dest = (slot[left] > slot[right])
    Gt = 0x34,
    /// dest = (slot[left] >= slot[right])
    Ge = 0x35,

    /// Unconditional jump; imm is an instruction index in the current function.
    Jump = 0x40,
    /// Jump to imm if slot[left] is false.
    JumpIfFalse = 0x41,
    /// Jump to imm if slot[left] is true.
    JumpIfTrue = 0x42,

    /// Call script function imm. Arguments are `right` consecutive slots starting at `left`
    /// (`right` = argument count); the callee sees them in its slots 0..right.
    /// The return value is written to `dest` (NO_SLOT to discard).
    CallScript = 0x50,
    /// Call native imm. Same argument convention as `CallScript`. Result to `dest`.
    CallNative = 0x51,
    /// Return slot[right] to the caller (NO_SLOT returns no value). Returning from the
    /// outermost frame finishes the thread.
    Return = 0x52,
}

impl Op {
    /// Decodes an opcode byte. Returns `None` for numbers that are not assigned.
    pub fn from_u8(b: u8) -> Option<Op> {
        use Op::*;
        Some(match b {
            0x01 => LoadInt,
            0x02 => LoadReal,
            0x03 => LoadBool,
            0x04 => LoadString,
            0x05 => LoadNull,
            0x06 => LoadCode,
            0x07 => Move,
            0x08 => GlobalGet,
            0x09 => GlobalSet,
            0x0A => GlobalArrayGet,
            0x0B => GlobalArraySet,
            0x0C => LocalArrayGet,
            0x0D => LocalArraySet,
            0x10 => AddI,
            0x11 => SubI,
            0x12 => MulI,
            0x13 => DivI,
            0x14 => ModI,
            0x18 => AddR,
            0x19 => SubR,
            0x1A => MulR,
            0x1B => DivR,
            0x1C => Concat,
            0x20 => NegI,
            0x21 => NegR,
            0x22 => Not,
            0x23 => IntToReal,
            0x24 => And,
            0x25 => Or,
            0x30 => Eq,
            0x31 => Ne,
            0x32 => Lt,
            0x33 => Le,
            0x34 => Gt,
            0x35 => Ge,
            0x40 => Jump,
            0x41 => JumpIfFalse,
            0x42 => JumpIfTrue,
            0x50 => CallScript,
            0x51 => CallNative,
            0x52 => Return,
            _ => return None,
        })
    }
}

/// One decoded instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instr {
    pub right: u8,
    pub left: u8,
    pub dest: u8,
    pub op: Op,
    pub imm: u32,
}

impl Instr {
    /// An instruction with all slots unused and `imm` 0.
    pub const fn new(op: Op) -> Instr {
        Instr {
            right: NO_SLOT,
            left: NO_SLOT,
            dest: NO_SLOT,
            op,
            imm: 0,
        }
    }

    pub const fn right(mut self, slot: u8) -> Instr {
        self.right = slot;
        self
    }

    pub const fn left(mut self, slot: u8) -> Instr {
        self.left = slot;
        self
    }

    pub const fn dest(mut self, slot: u8) -> Instr {
        self.dest = slot;
        self
    }

    pub const fn imm(mut self, imm: u32) -> Instr {
        self.imm = imm;
        self
    }

    /// Encodes to the 8-byte wire format.
    pub fn to_bytes(self) -> [u8; 8] {
        let imm = self.imm.to_le_bytes();
        [
            self.right,
            self.left,
            self.dest,
            self.op as u8,
            imm[0],
            imm[1],
            imm[2],
            imm[3],
        ]
    }

    /// Decodes the 8-byte wire format. `None` if the opcode byte is unknown.
    pub fn from_bytes(b: [u8; 8]) -> Option<Instr> {
        Some(Instr {
            right: b[0],
            left: b[1],
            dest: b[2],
            op: Op::from_u8(b[3])?,
            imm: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_round_trip_and_layout() {
        let i = Instr {
            right: 3,
            left: 2,
            dest: 1,
            op: Op::SubI,
            imm: 0x0102_0304,
        };
        let b = i.to_bytes();
        assert_eq!(b, [3, 2, 1, 0x11, 4, 3, 2, 1]);
        assert_eq!(Instr::from_bytes(b), Some(i));
        assert_eq!(Instr::from_bytes([0, 0, 0, 0xEE, 0, 0, 0, 0]), None);
    }

    #[test]
    fn every_opcode_round_trips() {
        for b in 0..=255u8 {
            if let Some(op) = Op::from_u8(b) {
                assert_eq!(op as u8, b);
            }
        }
    }
}
