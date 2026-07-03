//! Bytecode VM: AST → Chunk → interpretation.
//!
//! This is the default execution backend: the AST is compiled to a flat
//! [`Chunk`] of opcodes (see [`compile`]) and executed by a stack machine
//! ([`interpret`]). It covers the full language — statements, expressions,
//! closures/upvalues, classes, try/catch, modules, and async/await — and is
//! selected by default (`EXEC_MODE_BYTECODE`). The tree-walking evaluator
//! remains available as an opt-in fallback via `--exec-mode=tree`.
//!
//! All value-level semantics (operators, property/index access, calls,
//! iteration) are delegated to `crate::evaluator` so the VM and the
//! tree-walker stay byte-for-byte identical; the VM owns only the
//! control-flow representation (jumps, loop frames, protected regions) and
//! the call-frame/upvalue machinery.

pub mod call;
pub mod chunk;
pub mod class;
pub mod closure;
pub mod compiler;
pub mod frame;
pub mod interp;
pub mod opcode;
pub mod resolve;
pub mod upvalue;

pub use chunk::{Chunk, ProtectedRegion};
pub use compiler::compile;
pub use interp::interpret;
pub use opcode::Opcode;
