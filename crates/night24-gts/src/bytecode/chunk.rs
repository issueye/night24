//! Compiled bytecode container: a flat instruction stream plus a constant
//! pool, a per-instruction source-position table, and (later) protected
//! regions for try/catch.
//!
//! Encoding is deliberately simple: `code` is a flat `Vec<u8>` where each
//! instruction is `<opcode byte> <operand bytes…>`. Operand widths are fixed
//! per opcode (see `Opcode`). Position tracking is line-anchored via
//! `line_offsets` to keep memory low; `position_at(ip)` does the lookup.

use crate::ast::{ClassDecl, Position, TypeAnnotation};
use crate::object::Object;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use super::closure::FunctionProto;
use super::opcode::Opcode;

/// A try/catch protected region. Used from stage 6 onwards; declared here so
/// `Chunk` has a stable shape across stages.
#[derive(Debug, Clone)]
pub struct ProtectedRegion {
    /// Inclusive start ip of the try body.
    pub try_start: u32,
    /// Exclusive end ip of the try body (first byte after it).
    pub try_end: u32,
    /// Catch handler ip. Points past the try body.
    pub handler_ip: u32,
    /// Optional finally block ip.
    pub finally_ip: Option<u32>,
    /// Local slot that receives the caught value, if any.
    pub catch_binding_slot: Option<u8>,
}

/// A compiled function body / top-level program.
#[derive(Default)]
pub struct Chunk {
    /// Flat instruction stream.
    pub code: Vec<u8>,
    /// Constant pool. Referred to by `Const(u16)` etc.
    pub constants: Vec<Object>,
    /// Dedup index over `constants`, keyed by a hashable view of each value
    /// (see `ConstKey`). Covers the common primitive constants (numbers,
    /// strings, bools, null, undefined) so `add_constant` is O(1) amortized
    /// instead of O(n) per insertion. Reference-type constants (rare in the
    /// pool) fall back to a linear scan because they compare by `Rc` identity,
    /// which can't be hashed cheaply.
    const_index: HashMap<ConstKey, u16>,
    /// Function prototypes referenced by `OpClosure(u16)`. Stored separately
    /// from `constants` because `FunctionProto` is not an `Object`.
    pub protos: Vec<Rc<FunctionProto>>,
    /// Class declarations referenced by `OpNewClass(u16)`. Stage 5 bridges
    /// to the shared evaluator class builder before method bodies are lowered
    /// into bytecode prototypes.
    pub classes: Vec<Rc<ClassDecl>>,
    /// Type annotations referenced by typed declaration opcodes.
    pub types: Vec<TypeAnnotation>,
    /// Source-position table, stored at **instruction granularity** (one entry
    /// per opcode byte, not per byte) and sorted by code offset. `position_at`
    /// binary-searches this table, so a query for any byte — opcode or operand
    /// — resolves to the Position of the instruction that owns it.
    ///
    /// This is the run-length-encoded replacement for the old per-byte
    /// `Vec<Position>` (which stored N entries for an N-byte chunk, ~32 bytes
    /// each). Operand bytes inherit their instruction's Position, which is the
    /// same value the per-byte table stored anyway, so error positions are
    /// unchanged while memory shrinks to one entry per instruction.
    pub lines: Vec<LineEntry>,
    /// Protected regions, sorted by `try_start`. Empty until stage 6.
    pub protected_regions: Vec<ProtectedRegion>,
}

/// A single entry in the instruction-granularity position table.
#[derive(Clone)]
pub struct LineEntry {
    /// Code offset (byte index into `code`) of the instruction this entry
    /// describes. The entry covers every byte from this offset up to (but not
    /// including) the next entry's offset.
    pub ip: u32,
    pub pos: Position,
}

/// A hashable view of a constant-pool value, covering the primitive cases that
/// dominate the pool. Consistent with `Object`'s `PartialEq` for those variants:
/// equal `ConstKey`s imply equal `Object`s (and vice versa for primitives).
///
/// Reference-type `Object`s (Array/Hash/Function/...) intentionally have no
/// key here — they compare by `Rc` identity, which can't be hashed without
/// pointer-stable storage, so they fall back to a linear scan in
/// `add_constant`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConstKey {
    NumberBits(u64),
    String(Rc<String>),
    Bool(bool),
    Null,
    Undefined,
}

impl ConstKey {
    /// Map a value to its dedup key, or `None` for reference types.
    fn from_object(value: &Object) -> Option<ConstKey> {
        Some(match value {
            // Numbers are hashed via their bits so that `==`-equal f64 values
            // share a key. Three cases need care vs. raw `to_bits()`:
            //  - `0.0 == -0.0` is true, so both must map to the same key.
            //  - NaN never equals NaN (even with identical bits), so NaN must
            //    NOT be indexed — returning `None` forces the linear-scan path,
            //    which correctly never dedups NaN (matching `==`).
            Object::Number(n) => {
                if n.is_nan() {
                    return None;
                }
                let bits = n.to_bits();
                // Collapse -0.0 onto 0.0.
                let canonical = if bits == 0x8000_0000_0000_0000 {
                    0
                } else {
                    bits
                };
                ConstKey::NumberBits(canonical)
            }
            Object::String(s) => ConstKey::String(s.clone()),
            Object::Boolean(b) => ConstKey::Bool(*b),
            Object::Null => ConstKey::Null,
            Object::Undefined => ConstKey::Undefined,
            _ => return None,
        })
    }
}

impl Chunk {
    pub fn new() -> Chunk {
        Chunk::default()
    }

    /// Append a raw opcode byte (no operand) at the current offset, recording
    /// `pos` for error reporting. This is the only writer that creates a
    /// position-table entry; operand bytes (`write_byte`/`write_u16`/`write_u32`)
    /// inherit this instruction's Position.
    pub fn write_op(&mut self, op: Opcode, pos: Position) -> u32 {
        let offset = self.code.len() as u32;
        self.code.push(op as u8);
        self.lines.push(LineEntry { ip: offset, pos });
        offset
    }

    /// Append a single operand byte. The `pos` argument is accepted for API
    /// stability but ignored — operand bytes resolve to their owning
    /// instruction's Position via `position_at`.
    pub fn write_byte(&mut self, b: u8, _pos: Position) {
        self.code.push(b);
    }

    /// Append a u16 operand (big-endian, two bytes).
    pub fn write_u16(&mut self, v: u16, _pos: Position) {
        self.code.push((v >> 8) as u8);
        self.code.push((v & 0xff) as u8);
    }

    /// Append a u32 operand (big-endian, four bytes). Used by jump targets.
    pub fn write_u32(&mut self, v: u32, _pos: Position) {
        self.code.push(((v >> 24) & 0xff) as u8);
        self.code.push(((v >> 16) & 0xff) as u8);
        self.code.push(((v >> 8) & 0xff) as u8);
        self.code.push((v & 0xff) as u8);
    }

    /// Push a constant onto the pool and return its index. Deduplicates via
    /// `PartialEq` (numbers/strings/bools compare by value; reference types by
    /// shared `Rc` pointer).
    ///
    /// The primitive fast path is O(1) amortized through `const_index`; the
    /// rare reference-type path (and the legacy undefined-fallback) is a linear
    /// scan, preserving the original `PartialEq` semantics exactly.
    pub fn add_constant(&mut self, value: Object) -> u16 {
        // Fast path: primitive constants are hashed and looked up in O(1).
        if let Some(key) = ConstKey::from_object(&value) {
            if let Some(&idx) = self.const_index.get(&key) {
                // The hashed key is consistent with PartialEq for primitives,
                // so a hit is a true duplicate. Verify defensively to keep the
                // invariant self-evident (cheap: it's an Rc ptr_eq / value eq).
                debug_assert_eq!(self.constants[idx as usize], value);
                return idx;
            }
            let idx = self.constants.len() as u16;
            self.const_index.insert(key, idx);
            self.constants.push(value);
            return idx;
        }
        // Slow path: reference-type constants (Array/Hash/Function/...) compare
        // by `Rc` identity, which has no cheap hash. These are rare in the
        // constant pool, so a linear scan is acceptable.
        for (i, existing) in self.constants.iter().enumerate() {
            if existing == &value {
                return i as u16;
            }
        }
        let idx = self.constants.len() as u16;
        self.constants.push(value);
        idx
    }

    /// Read a u16 operand at the given byte offset (no bounds check beyond the
    /// slice indexing; callers ensure the offset is valid).
    pub fn read_u16(&self, ip: usize) -> u16 {
        let hi = self.code[ip] as u16;
        let lo = self.code[ip + 1] as u16;
        (hi << 8) | lo
    }

    /// Read a u32 operand at the given byte offset.
    pub fn read_u32(&self, ip: usize) -> u32 {
        let b0 = self.code[ip] as u32;
        let b1 = self.code[ip + 1] as u32;
        let b2 = self.code[ip + 2] as u32;
        let b3 = self.code[ip + 3] as u32;
        (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
    }

    /// Source position of the instruction that owns byte offset `ip`.
    ///
    /// `ip` may point at an opcode byte or any operand byte within an
    /// instruction; the result is the Position recorded for that instruction
    /// (i.e. the entry with the greatest `ip` ≤ the query). Returns the default
    /// Position when `ip` precedes the first recorded instruction.
    ///
    /// Binary-searches the instruction-granularity table (`O(log n)`); this
    /// matters because every error/timeout path in the interpreter funnels
    /// through here.
    pub fn position_at(&self, ip: usize) -> Position {
        // Find the rightmost entry whose `ip` <= the query. The table is sorted
        // ascending by construction (write_op appends at increasing offsets).
        let target = ip as u32;
        match self.lines.binary_search_by_key(&target, |e| e.ip) {
            Ok(idx) => self.lines[idx].pos.clone(),
            Err(insertion) => {
                // `insertion` is where `target` would go; the owning entry is
                // the one just before it.
                if insertion == 0 {
                    Position::default()
                } else {
                    self.lines[insertion - 1].pos.clone()
                }
            }
        }
    }

    /// Readable disassembly, primarily for debugging and stage-0 unit tests.
    pub fn disassemble(&self) -> String {
        let mut out = String::new();
        out.push_str("== constants ==\n");
        for (i, c) in self.constants.iter().enumerate() {
            out.push_str(&format!("  {:4} {:?}\n", i, c));
        }
        out.push_str("== code ==\n");
        let mut ip = 0;
        while ip < self.code.len() {
            let b = self.code[ip];
            let op = Opcode::from_byte(b);
            let pos = self.position_at(ip);
            let start = ip;
            ip += 1;
            match op {
                Some(op) => {
                    // Read the operand (if any) so we advance past its bytes.
                    let operand_str = match op {
                        Opcode::Const
                        | Opcode::LoadName
                        | Opcode::StoreName
                        | Opcode::AssignName
                        | Opcode::GetProperty
                        | Opcode::SetProperty
                        | Opcode::DefineMethod
                        | Opcode::NewClass
                        | Opcode::SuperMethod
                        | Opcode::NewArray
                        | Opcode::New
                        | Opcode::Call
                        | Opcode::Closure
                        | Opcode::ImportModule
                        | Opcode::ExportName => {
                            let v = self.read_u16(ip);
                            ip += 2;
                            format!(" {}", v)
                        }
                        Opcode::StoreTypedName => {
                            let name = self.read_u16(ip);
                            ip += 2;
                            let type_idx = self.read_u16(ip);
                            ip += 2;
                            format!(" {} {}", name, type_idx)
                        }
                        Opcode::LoadLocal
                        | Opcode::StoreLocal
                        | Opcode::LoadUpvalue
                        | Opcode::StoreUpvalue => {
                            let v = self.code[ip];
                            ip += 1;
                            format!(" {}", v)
                        }
                        Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue | Opcode::Loop => {
                            let v = self.read_u32(ip);
                            ip += 4;
                            format!(" ->{}", v)
                        }
                        _ => String::new(),
                    };
                    out.push_str(&format!(
                        "  {:4} {:<16}{} ; {}:{}:{}\n",
                        start,
                        op.name(),
                        operand_str,
                        pos.file,
                        pos.line,
                        pos.col
                    ));
                }
                None => out.push_str(&format!("  {:4} <bad opcode 0x{:02x}>\n", start, b)),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::num_obj;

    fn pos() -> Position {
        Position::new("t.gs", 1, 1, 0)
    }

    #[test]
    fn chunk_roundtrips_const_and_add() {
        let mut c = Chunk::new();
        let three = c.add_constant(num_obj(3.0));
        let four = c.add_constant(num_obj(4.0));
        // Same value pushed again dedups to the existing index.
        let three_again = c.add_constant(num_obj(3.0));
        assert_eq!(three_again, three);
        // CONST three ; CONST four ; ADD ; POP ; RETURN
        c.write_op(Opcode::Const, pos());
        c.write_u16(three, pos());
        c.write_op(Opcode::Const, pos());
        c.write_u16(four, pos());
        c.write_op(Opcode::Add, pos());
        c.write_op(Opcode::Pop, pos());
        c.write_op(Opcode::Return, pos());

        assert_eq!(c.constants.len(), 2); // 3.0 and 4.0 only
        assert_eq!(c.read_u16(1), 0);
        assert_eq!(c.read_u16(4), 1);
        assert_eq!(c.code[0], Opcode::Const as u8);
        assert_eq!(c.code[3], Opcode::Const as u8);
        assert_eq!(c.code[6], Opcode::Add as u8);
    }

    #[test]
    fn disassemble_is_readable() {
        let mut c = Chunk::new();
        let v = c.add_constant(num_obj(1.0));
        c.write_op(Opcode::Const, pos());
        c.write_u16(v, pos());
        c.write_op(Opcode::Return, pos());
        let s = c.disassemble();
        assert!(s.contains("CONST"));
        assert!(s.contains("RETURN"));
    }

    /// The position table stores one entry per instruction, not per byte.
    /// `CONST <u16>` is a 3-byte instruction but produces exactly one entry.
    #[test]
    fn position_table_is_instruction_granular() {
        let mut c = Chunk::new();
        let v = c.add_constant(num_obj(1.0));
        c.write_op(Opcode::Const, Position::new("t.gs", 7, 1, 0));
        c.write_u16(v, Position::new("t.gs", 7, 1, 0));
        c.write_op(Opcode::Return, Position::new("t.gs", 9, 1, 0));
        // Two instructions -> two entries, despite a 4-byte code stream.
        // CONST(1) + u16 operand(2) + RETURN(1) = 4 bytes.
        assert_eq!(c.code.len(), 4);
        assert_eq!(c.lines.len(), 2);
        assert_eq!(c.lines[0].ip, 0);
        assert_eq!(c.lines[1].ip, 3);
    }

    /// `position_at` resolves an operand byte to its owning instruction's
    /// Position, and never panics for any in-range offset.
    #[test]
    fn position_at_resolves_operand_bytes_to_owning_instruction() {
        let mut c = Chunk::new();
        let v = c.add_constant(num_obj(1.0));
        c.write_op(Opcode::Const, Position::new("t.gs", 7, 3, 0));
        c.write_u16(v, Position::new("t.gs", 7, 3, 0));
        c.write_op(Opcode::Return, Position::new("t.gs", 9, 5, 0));
        // Byte 0 (opcode) and bytes 1,2 (operand) all belong to line 7.
        for ip in 0..=2 {
            assert_eq!(
                c.position_at(ip).line,
                7,
                "byte {} should map to line 7",
                ip
            );
        }
        // Byte 3 (RETURN opcode) belongs to line 9.
        assert_eq!(c.position_at(3).line, 9);
        assert_eq!(c.position_at(4).line, 9); // past end -> last instruction
    }

    /// `position_at` returns the default Position for offsets before the first
    /// recorded instruction (e.g. ip 0 when the table is empty).
    #[test]
    fn position_at_empty_table_returns_default() {
        let c = Chunk::new();
        assert_eq!(c.position_at(0), Position::default());
    }

    /// `add_constant` dedups every primitive kind, not just numbers.
    #[test]
    fn add_constant_dedups_strings_bools_and_nulls() {
        let mut c = Chunk::new();
        use crate::object::{bool_obj, str_obj};
        let s1 = c.add_constant(str_obj("hello"));
        let s2 = c.add_constant(str_obj("hello"));
        assert_eq!(s1, s2);
        let b1 = c.add_constant(bool_obj(true));
        let b2 = c.add_constant(bool_obj(true));
        assert_eq!(b1, b2);
        let n1 = c.add_constant(Object::Null);
        let n2 = c.add_constant(Object::Null);
        assert_eq!(n1, n2);
        let u1 = c.add_constant(Object::Undefined);
        let u2 = c.add_constant(Object::Undefined);
        assert_eq!(u1, u2);
        // Four distinct primitive values.
        assert_eq!(c.constants.len(), 4);
    }

    /// `0.0 == -0.0` is true, so they must share one pool slot (matching the
    /// original `PartialEq`-based dedup).
    #[test]
    fn add_constant_treats_signed_zero_as_equal() {
        let mut c = Chunk::new();
        let pos_zero = c.add_constant(num_obj(0.0));
        let neg_zero = c.add_constant(num_obj(-0.0));
        assert_eq!(pos_zero, neg_zero);
        assert_eq!(c.constants.len(), 1);
    }

    /// Distinct numeric values never collide, and NaN never dedups with itself
    /// (NaN != NaN, matching `==`).
    #[test]
    fn add_constant_keeps_distinct_numbers_and_nan_separate() {
        let mut c = Chunk::new();
        let a = c.add_constant(num_obj(1.0));
        let b = c.add_constant(num_obj(2.0));
        assert_ne!(a, b);
        let nan1 = c.add_constant(num_obj(f64::NAN));
        let nan2 = c.add_constant(num_obj(f64::NAN));
        assert_ne!(nan1, nan2); // NaN != NaN -> separate slots
    }
}
