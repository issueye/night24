//! Bytecode opcodes for the stack-based VM.
//!
//! The enum is laid out to cover the full instruction set described in
//! `docs/bytecode-vm-development-plan.md` §3.1, so later stages only need to
//! fill in interpreter arms rather than reshape the enum. Stage 0 implements
//! semantics for `Const` / `Add` / `Pop` / `Return` only; any other opcode
//! surfaced before its stage is a compiler bug and the interpreter reports it.

use std::fmt;

/// A single VM instruction. Operands are decoded inline by the interpreter:
/// `Const(u16)` reads two bytes for a constant-pool index, `Jump*(u32)` reads
/// four, etc. This keeps `Chunk.code` a flat `Vec<u8>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    /// Push `constants[idx]` onto the stack. Operand: u16.
    Const = 0,
    /// Discard the top of the stack.
    Pop = 1,
    /// Duplicate the top of the stack.
    Dup = 2,

    // —— arithmetic (binary, pop two, push one) ——
    Add = 10,
    Sub = 11,
    Mul = 12,
    Div = 13,
    Mod = 14,
    Pow = 15,
    /// String concatenation (specialised `+`).
    Concat = 16,
    BitAnd = 17,
    BitOr = 18,
    BitXor = 19,

    // —— comparison ——
    Eq = 20,
    Neq = 21,
    Lt = 22,
    Le = 23,
    Gt = 24,
    Ge = 25,
    InstanceOf = 26,
    In = 27,

    // —— logic (short-circuit forms are lowered to jumps by the compiler) ——
    And = 30,
    Or = 31,
    Not = 32,
    Neg = 33,
    BitNot = 34,
    Shl = 35,
    Shr = 36,
    UShr = 37,
    Identity = 38,

    // —— variables ——
    /// Operand: u16 name-table index.
    LoadGlobal = 40,
    StoreGlobal = 41,
    /// Operand: u8 slot index.
    LoadLocal = 42,
    StoreLocal = 43,
    /// Operand: u8 upvalue index.
    LoadUpvalue = 44,
    StoreUpvalue = 45,
    /// Dynamic name lookup (migration scaffold; operand u16).
    LoadName = 46,
    /// Store the top of the stack into a named binding. Operand u16; the high
    /// bit (0x8000) marks a const declaration so the binding is created
    /// const-tracked (assignment later raises TypeError).
    StoreName = 47,
    /// Assign to an *existing* binding (declaration already done). Pops the
    /// value; raises ReferenceError if unbound or TypeError if const. Operand
    /// u16 name index. The assigned value is also left on the stack (assignment
    /// is an expression).
    AssignName = 48,
    /// Push the current `this` binding.
    LoadThis = 49,
    /// Store a declared binding with a type annotation. Operands: u16 name
    /// index (high bit = const), u16 type-annotation index in `Chunk::types`.
    StoreTypedName = 55,

    // —— control flow ——
    /// Operand: u32 absolute ip.
    Jump = 50,
    JumpIfFalse = 51,
    JumpIfTrue = 52,
    /// Backwards jump (loop bottom). Operand: u32 absolute ip.
    Loop = 53,
    /// Operand: u16 name index; super method dispatch.
    SuperMethod = 54,

    // —— functions / closures ——
    /// Operand: u16 proto index in the constant pool; binds upvalues at runtime.
    Closure = 60,
    /// Operand: u8 arg_count.
    Call = 61,
    Return = 62,
    ReturnNull = 63,
    /// Append a single evaluated value to the packed call-args array.
    PushArg = 64,
    /// Call with arguments packed in a runtime array. Used for spread args.
    CallSpread = 65,

    // —— object model ——
    NewObject = 70,
    /// Operand: u16 element count.
    NewArray = 71,
    /// Operand: u16 name index.
    GetProperty = 72,
    SetProperty = 73,
    GetIndex = 74,
    SetIndex = 75,
    Spread = 76,
    /// Operand: u16 class proto index.
    NewClass = 77,
    /// Operand: u16 name index.
    DefineMethod = 78,
    /// Operand: u16 class name index.
    New = 79,
    /// Convert the top value to an array of for-in keys.
    IterKeys = 80,
    /// Convert the top value to a for-of iterator through Symbol.iterator.
    IterValues = 81,
    /// Push the length/size of the top collection-like value.
    Len = 82,
    /// Advance the top iterator and push its `{ value, done }` record.
    IterNext = 83,

    // —— errors / async ——
    Throw = 90,
    Await = 91,
    /// Pop a value, convert to its string representation (Object::inspect),
    /// push the resulting string. Used for template-literal interpolation.
    ToString = 92,
    /// Pop a match subject and throw `MatchError: no arm matched for <subject>`.
    ThrowMatchError = 93,
    /// Pop a value and push its `typeof` string.
    TypeOf = 94,
    /// Load a module through the VM importer callback. Operand: u16 source
    /// string index.
    ImportModule = 95,
    /// Pop a value and write it to the current module exports object. Operand:
    /// u16 export name string index.
    ExportName = 96,
    /// Pop a start index (number) then an array, push a new array containing
    /// the elements from `start` to the end. Used for destructuring rest
    /// (`let [a, ...rest] = arr`). No operand.
    ArraySliceFrom = 97,
    /// Pop a source module's exports object; copy every property into the
    /// current module's exports. Used for `export * from "..."`. No operand.
    ExportAll = 98,
    /// Pop a value and push a resolved Promise wrapping it. Used for dynamic
    /// `import(specifier)` (synchronous module load → resolved Promise).
    /// No operand.
    WrapResolvedPromise = 99,
}

impl Opcode {
    /// Decode a single byte into an opcode. Returns `None` for an unknown
    /// opcode byte (indicates bytecode corruption).
    pub fn from_byte(b: u8) -> Option<Opcode> {
        // Match every variant explicitly so adding a new opcode forces this
        // table to be revisited.
        Some(match b {
            0 => Opcode::Const,
            1 => Opcode::Pop,
            2 => Opcode::Dup,
            10 => Opcode::Add,
            11 => Opcode::Sub,
            12 => Opcode::Mul,
            13 => Opcode::Div,
            14 => Opcode::Mod,
            15 => Opcode::Pow,
            16 => Opcode::Concat,
            17 => Opcode::BitAnd,
            18 => Opcode::BitOr,
            19 => Opcode::BitXor,
            20 => Opcode::Eq,
            21 => Opcode::Neq,
            22 => Opcode::Lt,
            23 => Opcode::Le,
            24 => Opcode::Gt,
            25 => Opcode::Ge,
            26 => Opcode::InstanceOf,
            27 => Opcode::In,
            30 => Opcode::And,
            31 => Opcode::Or,
            32 => Opcode::Not,
            33 => Opcode::Neg,
            34 => Opcode::BitNot,
            35 => Opcode::Shl,
            36 => Opcode::Shr,
            37 => Opcode::UShr,
            38 => Opcode::Identity,
            40 => Opcode::LoadGlobal,
            41 => Opcode::StoreGlobal,
            42 => Opcode::LoadLocal,
            43 => Opcode::StoreLocal,
            44 => Opcode::LoadUpvalue,
            45 => Opcode::StoreUpvalue,
            46 => Opcode::LoadName,
            47 => Opcode::StoreName,
            48 => Opcode::AssignName,
            49 => Opcode::LoadThis,
            55 => Opcode::StoreTypedName,
            50 => Opcode::Jump,
            51 => Opcode::JumpIfFalse,
            52 => Opcode::JumpIfTrue,
            53 => Opcode::Loop,
            54 => Opcode::SuperMethod,
            60 => Opcode::Closure,
            61 => Opcode::Call,
            62 => Opcode::Return,
            63 => Opcode::ReturnNull,
            64 => Opcode::PushArg,
            65 => Opcode::CallSpread,
            70 => Opcode::NewObject,
            71 => Opcode::NewArray,
            72 => Opcode::GetProperty,
            73 => Opcode::SetProperty,
            74 => Opcode::GetIndex,
            75 => Opcode::SetIndex,
            76 => Opcode::Spread,
            77 => Opcode::NewClass,
            78 => Opcode::DefineMethod,
            79 => Opcode::New,
            80 => Opcode::IterKeys,
            81 => Opcode::IterValues,
            82 => Opcode::Len,
            83 => Opcode::IterNext,
            90 => Opcode::Throw,
            91 => Opcode::Await,
            92 => Opcode::ToString,
            93 => Opcode::ThrowMatchError,
            94 => Opcode::TypeOf,
            95 => Opcode::ImportModule,
            96 => Opcode::ExportName,
            97 => Opcode::ArraySliceFrom,
            98 => Opcode::ExportAll,
            99 => Opcode::WrapResolvedPromise,
            _ => return None,
        })
    }

    /// Human-readable name for disassembly.
    pub fn name(self) -> &'static str {
        match self {
            Opcode::Const => "CONST",
            Opcode::Pop => "POP",
            Opcode::Dup => "DUP",
            Opcode::Add => "ADD",
            Opcode::Sub => "SUB",
            Opcode::Mul => "MUL",
            Opcode::Div => "DIV",
            Opcode::Mod => "MOD",
            Opcode::Pow => "POW",
            Opcode::Concat => "CONCAT",
            Opcode::BitAnd => "BIT_AND",
            Opcode::BitOr => "BIT_OR",
            Opcode::BitXor => "BIT_XOR",
            Opcode::Eq => "EQ",
            Opcode::Neq => "NEQ",
            Opcode::Lt => "LT",
            Opcode::Le => "LE",
            Opcode::Gt => "GT",
            Opcode::Ge => "GE",
            Opcode::InstanceOf => "INSTANCEOF",
            Opcode::In => "IN",
            Opcode::And => "AND",
            Opcode::Or => "OR",
            Opcode::Not => "NOT",
            Opcode::Neg => "NEG",
            Opcode::BitNot => "BIT_NOT",
            Opcode::Shl => "SHL",
            Opcode::Shr => "SHR",
            Opcode::UShr => "USHR",
            Opcode::Identity => "IDENTITY",
            Opcode::LoadGlobal => "LOAD_GLOBAL",
            Opcode::StoreGlobal => "STORE_GLOBAL",
            Opcode::LoadLocal => "LOAD_LOCAL",
            Opcode::StoreLocal => "STORE_LOCAL",
            Opcode::LoadUpvalue => "LOAD_UPVALUE",
            Opcode::StoreUpvalue => "STORE_UPVALUE",
            Opcode::LoadName => "LOAD_NAME",
            Opcode::StoreName => "STORE_NAME",
            Opcode::AssignName => "ASSIGN_NAME",
            Opcode::LoadThis => "LOAD_THIS",
            Opcode::StoreTypedName => "STORE_TYPED_NAME",
            Opcode::Jump => "JUMP",
            Opcode::JumpIfFalse => "JUMP_IF_FALSE",
            Opcode::JumpIfTrue => "JUMP_IF_TRUE",
            Opcode::Loop => "LOOP",
            Opcode::SuperMethod => "SUPER_METHOD",
            Opcode::Closure => "CLOSURE",
            Opcode::Call => "CALL",
            Opcode::Return => "RETURN",
            Opcode::ReturnNull => "RETURN_NULL",
            Opcode::PushArg => "PUSH_ARG",
            Opcode::CallSpread => "CALL_SPREAD",
            Opcode::NewObject => "NEW_OBJECT",
            Opcode::NewArray => "NEW_ARRAY",
            Opcode::GetProperty => "GET_PROPERTY",
            Opcode::SetProperty => "SET_PROPERTY",
            Opcode::GetIndex => "GET_INDEX",
            Opcode::SetIndex => "SET_INDEX",
            Opcode::Spread => "SPREAD",
            Opcode::NewClass => "NEW_CLASS",
            Opcode::DefineMethod => "DEFINE_METHOD",
            Opcode::New => "NEW",
            Opcode::IterKeys => "ITER_KEYS",
            Opcode::IterValues => "ITER_VALUES",
            Opcode::Len => "LEN",
            Opcode::IterNext => "ITER_NEXT",
            Opcode::Throw => "THROW",
            Opcode::Await => "AWAIT",
            Opcode::ToString => "TO_STRING",
            Opcode::ThrowMatchError => "THROW_MATCH_ERROR",
            Opcode::TypeOf => "TYPEOF",
            Opcode::ImportModule => "IMPORT_MODULE",
            Opcode::ExportName => "EXPORT_NAME",
            Opcode::ArraySliceFrom => "ARRAY_SLICE_FROM",
            Opcode::ExportAll => "EXPORT_ALL",
            Opcode::WrapResolvedPromise => "WRAP_RESOLVED_PROMISE",
        }
    }
}

impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}
