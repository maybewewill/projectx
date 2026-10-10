//! JASS bytecode in the layout and numbering of the original compiler (Game.dll 1.26a).
//!
//! Every instruction is 8 bytes: word 0 packs `b0 b1 b2 op` (b0 = byte 0, op = byte 3), word 1 is
//! `imm` (little-endian `u32`). The field meaning depends on the opcode; the table below is the
//! one used by `compile.rs` and `vm.rs`, and was confirmed by compiling test texts with the
//! original compiler in emulation (`tools/emu/jasscomp`, `parity.py`).
//!
//! | op   | name              | operands (b0 / b1 / b2 / imm)                                    |
//! |------|-------------------|--------------------------------------------------------------------|
//! | 0x01 | End               | end of the whole script (last instruction of the stream)           |
//! | 0x02 | DebugSkip         | `debug` prefix: imm = label to jump to (release build: skipped) UNVERIFIED semantics |
//! | 0x03 | FuncHdr           | b2 = return type code (0 nothing), imm = function symbol           |
//! | 0x04 | FuncEnd           | end of a function                                                  |
//! | 0x05 | DeclLocal         | b2 = type code (array = base + 5), imm = local symbol              |
//! | 0x06 | DeclGlobal        | b2 = type code, imm = global symbol                                |
//! | 0x07 | DeclConstGlobal   | as DeclGlobal, for `constant` globals                              |
//! | 0x08 | ParamBind         | b1 = depth from the value-stack top (1 = top, peek), b2 = type, imm = param symbol |
//! | 0x09 | TypeParent        | type declaration: imm = parent type symbol (emitted before all code) |
//! | 0x0A | TypeName          | type declaration: imm = declared type symbol                       |
//! | 0x0B | PopArgs           | b2 = argument count: pops that many values after a call            |
//! | 0x0C | LoadConst         | b1 = type code (0/2/7 null, 4 int, 5 real, 6 string, 8 bool), b2 = dest, imm = value (real: f32 bits, string: symbol) |
//! | 0x0D | Move              | b1 = source, b2 = dest                                             |
//! | 0x0E | LoadVar           | b1 = type code of the variable, b2 = dest, imm = symbol (local or global) |
//! | 0x0F | LoadFuncRef       | b1 = 3 (code), b2 = dest, imm = function symbol                    |
//! | 0x10 | ArrayLoad         | b0 = element type code, b1 = index slot, b2 = dest, imm = array symbol |
//! | 0x11 | StoreVar          | b2 = source, imm = symbol (local or global)                        |
//! | 0x12 | ArrayStore        | b1 = value slot, b2 = index slot, imm = array symbol               |
//! | 0x13 | Push              | b2 = slot pushed onto the value stack                              |
//! | 0x14 | Pop               | b2 = dest slot, popped from the value stack                        |
//! | 0x15 | CallNative        | imm = index into `Module::natives` (UNVERIFIED encoding, see below) |
//! | 0x16 | CallScript        | imm = function symbol; result in slot 0 of the caller              |
//! | 0x17 | IntToReal         | in place on b2 (integer -> real)                                   |
//! | 0x1A..0x1F | Eq, Ne, Le, Ge, Lt, Gt | b0 = right, b1 = left, b2 = dest                  |
//! | 0x20..0x23 | Add, Sub, Mul, Div | b0 = right, b1 = left, b2 = dest (Add also concatenates strings) |
//! | 0x25 | Neg               | in place on b2                                                     |
//! | 0x26 | Not               | in place on b2                                                     |
//! | 0x27 | Return            | returns slot 0 of the current frame                                |
//! | 0x28 | Label             | imm = label id (numbered per script, from 1)                       |
//! | 0x29 | JumpIfTrue        | b2 = condition slot, imm = label                                   |
//! | 0x2A | JumpIfFalse       | b2 = condition slot, imm = label                                   |
//! | 0x2B | Jump              | imm = label                                                        |
//!
//! Slots are 8-bit. Temporaries are allocated from a counter that runs over the whole script
//! (1..=255, wrapping to 1, slot 0 is the return slot), so there is no per-statement reuse.
//!
//! UNVERIFIED: the native call opcode (0x15) and its immediate. The original's `native` keyword is
//! rejected by the entry the harness uses, so no native call was ever observed. The encoding here
//! (imm = native index, arguments on the value stack, POP_ARGS afterwards like a script call) is
//! the most plausible reading of the spec.

/// Type codes as the original writes them into `LoadConst`/`LoadVar`/declarations.
pub mod ty {
    /// Null / no value (default return and `null` in some contexts).
    pub const NOTHING: u8 = 0;
    /// `null` literal in an assignment or argument (observed).
    pub const NULL: u8 = 2;
    pub const CODE: u8 = 3;
    pub const INTEGER: u8 = 4;
    pub const REAL: u8 = 5;
    pub const STRING: u8 = 6;
    /// `handle` and every user type that extends it.
    pub const HANDLE: u8 = 7;
    pub const BOOLEAN: u8 = 8;
    /// Array types are the base code plus this offset (integer array = 9).
    pub const ARRAY_OFFSET: u8 = 5;
}

/// Opcodes, with the original's numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    End = 0x01,
    DebugSkip = 0x02,
    FuncHdr = 0x03,
    FuncEnd = 0x04,
    DeclLocal = 0x05,
    DeclGlobal = 0x06,
    DeclConstGlobal = 0x07,
    ParamBind = 0x08,
    TypeParent = 0x09,
    TypeName = 0x0A,
    PopArgs = 0x0B,
    LoadConst = 0x0C,
    Move = 0x0D,
    LoadVar = 0x0E,
    LoadFuncRef = 0x0F,
    ArrayLoad = 0x10,
    StoreVar = 0x11,
    ArrayStore = 0x12,
    Push = 0x13,
    Pop = 0x14,
    CallNative = 0x15,
    CallScript = 0x16,
    IntToReal = 0x17,
    Eq = 0x1A,
    Ne = 0x1B,
    Le = 0x1C,
    Ge = 0x1D,
    Lt = 0x1E,
    Gt = 0x1F,
    Add = 0x20,
    Sub = 0x21,
    Mul = 0x22,
    Div = 0x23,
    Neg = 0x25,
    Not = 0x26,
    Return = 0x27,
    Label = 0x28,
    JumpIfTrue = 0x29,
    JumpIfFalse = 0x2A,
    Jump = 0x2B,
}

impl Op {
    /// Decodes an opcode byte. `None` for numbers the original does not use here.
    pub fn from_u8(b: u8) -> Option<Op> {
        use Op::*;
        Some(match b {
            0x01 => End,
            0x02 => DebugSkip,
            0x03 => FuncHdr,
            0x04 => FuncEnd,
            0x05 => DeclLocal,
            0x06 => DeclGlobal,
            0x07 => DeclConstGlobal,
            0x08 => ParamBind,
            0x09 => TypeParent,
            0x0A => TypeName,
            0x0B => PopArgs,
            0x0C => LoadConst,
            0x0D => Move,
            0x0E => LoadVar,
            0x0F => LoadFuncRef,
            0x10 => ArrayLoad,
            0x11 => StoreVar,
            0x12 => ArrayStore,
            0x13 => Push,
            0x14 => Pop,
            0x15 => CallNative,
            0x16 => CallScript,
            0x17 => IntToReal,
            0x1A => Eq,
            0x1B => Ne,
            0x1C => Le,
            0x1D => Ge,
            0x1E => Lt,
            0x1F => Gt,
            0x20 => Add,
            0x21 => Sub,
            0x22 => Mul,
            0x23 => Div,
            0x25 => Neg,
            0x26 => Not,
            0x27 => Return,
            0x28 => Label,
            0x29 => JumpIfTrue,
            0x2A => JumpIfFalse,
            0x2B => Jump,
            _ => return None,
        })
    }
}

/// One decoded instruction. Unused operand fields are 0, as in the original's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instr {
    /// Byte 0 (right operand for binary ops, array element type for `ArrayLoad`).
    pub right: u8,
    /// Byte 1 (left operand, or the type code for `LoadConst`/`LoadVar`).
    pub left: u8,
    /// Byte 2 (destination, or the in-place operand, or the condition slot).
    pub dest: u8,
    pub op: Op,
    pub imm: u32,
}

impl Instr {
    /// An instruction with all operand fields 0 and `imm` 0.
    pub const fn new(op: Op) -> Instr {
        Instr {
            right: 0,
            left: 0,
            dest: 0,
            op,
            imm: 0,
        }
    }

    pub const fn right(mut self, v: u8) -> Instr {
        self.right = v;
        self
    }

    pub const fn left(mut self, v: u8) -> Instr {
        self.left = v;
        self
    }

    pub const fn dest(mut self, v: u8) -> Instr {
        self.dest = v;
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
            op: Op::Sub,
            imm: 0x0102_0304,
        };
        let b = i.to_bytes();
        assert_eq!(b, [3, 2, 1, 0x21, 4, 3, 2, 1]);
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
