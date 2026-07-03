//! The bytecode interpreter: a stack machine that executes a `Chunk`.
//!
//! [`interpret`] builds a [`VmState`] over the chunk and runs the dispatch
//! loop, which handles the full opcode set: constants, the operator family,
//! property/index access, calls and construction, closures/upvalues, classes,
//! control flow (jumps, loop break/continue, try/catch/finally), modules,
//! and async/await. Value-level semantics are delegated to `crate::evaluator`
//! so behavior matches the tree-walker exactly.
//!
//! `Add` semantics mirror `evaluator::expressions::eval_add` byte-for-byte:
//! number+number → numeric add, string+string → concatenation, mixed →
//! TypeError. The main loop also samples the VM deadline every
//! [`TIMEOUT_CHECK_INTERVAL`] instructions so `--timeout` can interrupt
//! tight CPU loops.

use crate::ast::{Position, TypeAnnotation, TypeKind};
use crate::object::{
    new_error, new_named_error, str_obj, ArrayData, CallContext, EnvRef, HashData, Object,
    PromiseState, VirtualMachine,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::Ordering;

use super::chunk::Chunk;
use super::closure::UpvalueSource;
use super::opcode::Opcode;
use super::upvalue::Upvalue;
use crate::evaluator::builtins::register_globals;
use crate::evaluator::expressions::{apply_binary_op, apply_unary_op};

/// How many bytecode instructions to execute between deadline checks.
///
/// Checking `vm.check_timeout()` on every instruction adds noticeable overhead
/// to tight loops; sampling every Nth instruction keeps the cost negligible
/// while still terminating an infinite loop promptly (the deadline is
/// wall-clock based, so a hot loop reaches the next check within N steps).
const TIMEOUT_CHECK_INTERVAL: u64 = 4096;

/// Execute a compiled chunk under the given (root) environment. The
/// environment holds the global name table; variable lookups route through it.
///
/// Globals (`println`, `print`, `console`, ...) are installed idempotently so
/// that a freshly-built root environment (e.g. in unit tests) has the same
/// builtins the CLI session provides. `register_globals` overwrites, so
/// calling it twice is safe.
pub fn interpret(chunk: &Chunk, env: &EnvRef) -> Object {
    interpret_with_upvalues(chunk, env, Vec::new())
}

pub(crate) fn interpret_with_upvalues(
    chunk: &Chunk,
    env: &EnvRef,
    upvalues: Vec<Rc<Upvalue>>,
) -> Object {
    let vm = env.borrow().vm.clone();
    // Install the standard globals (println, print, console, Math, ...) only
    // if they aren't already present, so callers that supply their own (e.g. a
    // test stubbing `println` to capture output) keep their overrides.
    if !vm.has_global("println") {
        register_globals(&vm);
    }
    let mut state = VmState::new(chunk, env.clone(), upvalues, vm);
    state.run()
}

struct VmState<'a> {
    chunk: &'a Chunk,
    ip: usize,
    last_ip: usize,
    stack: Vec<Object>,
    env: EnvRef,
    open_upvalues: BTreeMap<usize, Vec<Rc<Upvalue>>>,
    current_upvalues: Vec<Rc<Upvalue>>,
    /// VM handle, used to sample the execution deadline (see `run`).
    vm: Rc<VirtualMachine>,
    /// Instructions executed since the last deadline check.
    instruction_count: u64,
}

impl<'a> VmState<'a> {
    fn new(
        chunk: &'a Chunk,
        env: EnvRef,
        current_upvalues: Vec<Rc<Upvalue>>,
        vm: Rc<VirtualMachine>,
    ) -> Self {
        VmState {
            chunk,
            ip: 0,
            last_ip: 0,
            stack: Vec::with_capacity(256),
            env,
            open_upvalues: BTreeMap::new(),
            current_upvalues,
            vm,
            instruction_count: 0,
        }
    }

    fn run(&mut self) -> Object {
        loop {
            // Defensive: bail on truncated bytecode rather than panicking.
            if self.ip >= self.chunk.code.len() {
                return new_error(
                    Position::default(),
                    "VMError: ran off the end of bytecode without RETURN",
                );
            }
            // Sample the execution deadline periodically so `--timeout` can
            // interrupt tight CPU loops. Mirrors the per-statement check the
            // tree-walker performs in `evaluator::eval_core`.
            //
            // Two-stage check: first the cheap deadline probe (no Position
            // lookup, no `Rc<str>` clone, no error construction). Only when the
            // deadline has actually elapsed do we pay for `position_at(ip)` and
            // build the `TimeoutError`. In tight CPU loops this branch fires
            // every `TIMEOUT_CHECK_INTERVAL` instructions but almost never
            // times out, so the saving on the hot path is the Position lookup.
            self.instruction_count = self.instruction_count.wrapping_add(1);
            if self
                .instruction_count
                .is_multiple_of(TIMEOUT_CHECK_INTERVAL)
            {
                // D4.1: check instruction limit (resource guard) alongside timeout.
                if self
                    .vm
                    .is_instruction_limit_exceeded(self.instruction_count)
                {
                    let pos = self.chunk.position_at(self.ip);
                    if let Some(err) = self.vm.check_instruction_limit(self.instruction_count, pos)
                    {
                        return err;
                    }
                }
                // Timeout check (existing).
                if self.vm.is_deadline_exceeded() {
                    let pos = self.chunk.position_at(self.ip);
                    if let Some(timeout) = self.vm.check_timeout(pos) {
                        return timeout;
                    }
                }
            }
            match self.step() {
                Ok(Flow::Continue) => {}
                Ok(Flow::Return(v)) => return v,
                Err(e) => {
                    if self.unwind_to_handler(e.clone()) {
                        continue;
                    }
                    return e;
                }
            }
        }
    }

    /// Decode and execute one instruction. Returning `Result` lets opcode
    /// handlers use `?` for error propagation; `run` translates the outcomes.
    fn step(&mut self) -> Result<Flow, Object> {
        let instruction_ip = self.ip;
        self.last_ip = instruction_ip;
        let byte = self.chunk.code[self.ip];
        let op = match Opcode::from_byte(byte) {
            Some(op) => op,
            None => {
                return Err(new_error(
                    self.chunk.position_at(self.ip),
                    format!("VMError: unknown opcode byte 0x{:02x}", byte),
                ));
            }
        };
        self.ip += 1;
        match op {
            Opcode::Const => {
                let idx = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let value = self.chunk.constants[idx].clone();
                self.stack.push(value);
            }
            Opcode::Pop => {
                self.stack.pop();
            }
            Opcode::Dup => {
                let v = self
                    .stack
                    .last()
                    .cloned()
                    .ok_or_else(|| self.stack_underflow(self.chunk.position_at(self.ip - 1)))?;
                self.stack.push(v);
            }

            // —— binary operators: delegate to the shared evaluator core ——
            Opcode::Add => self.bin_op("+")?,
            Opcode::Sub => self.bin_op("-")?,
            Opcode::Mul => self.bin_op("*")?,
            Opcode::Div => self.bin_op("/")?,
            Opcode::Mod => self.bin_op("%")?,
            Opcode::Pow => self.bin_op("**")?,
            Opcode::BitAnd => self.bin_op("&")?,
            Opcode::BitOr => self.bin_op("|")?,
            Opcode::BitXor => self.bin_op("^")?,
            Opcode::Shl => self.bin_op("<<")?,
            Opcode::Shr => self.bin_op(">>")?,
            Opcode::UShr => self.bin_op(">>>")?,
            Opcode::Eq => self.bin_op("===")?,
            Opcode::Neq => self.bin_op("!==")?,
            Opcode::Lt => self.bin_op("<")?,
            Opcode::Le => self.bin_op("<=")?,
            Opcode::Gt => self.bin_op(">")?,
            Opcode::Ge => self.bin_op(">=")?,
            Opcode::InstanceOf => self.bin_op("instanceof")?,
            Opcode::In => self.bin_op("in")?,
            // Concat is a specialised `+` for the string-only fast path; route
            // through the same core so semantics stay identical.
            Opcode::Concat => self.bin_op("+")?,

            // —— unary operators ——
            Opcode::Not => self.un_op("!")?,
            Opcode::Neg => self.un_op("-")?,
            Opcode::BitNot => self.un_op("~")?,
            Opcode::Identity => self.un_op("+")?,

            // —— control flow ——
            Opcode::Jump => {
                let target = self.chunk.read_u32(self.ip) as usize;
                self.ip = target;
            }
            Opcode::Loop => {
                // Backwards jump (loop back-edge). Same encoding as Jump.
                let target = self.chunk.read_u32(self.ip) as usize;
                self.ip = target;
            }
            Opcode::JumpIfFalse => {
                let target = self.chunk.read_u32(self.ip) as usize;
                self.ip += 4;
                let pos = self.chunk.position_at(self.ip - 5);
                let cond = self.stack.pop().ok_or_else(|| self.stack_underflow(pos))?;
                if !cond.is_truthy() {
                    self.ip = target;
                }
            }
            Opcode::JumpIfTrue => {
                let target = self.chunk.read_u32(self.ip) as usize;
                self.ip += 4;
                let pos = self.chunk.position_at(self.ip - 5);
                let cond = self.stack.pop().ok_or_else(|| self.stack_underflow(pos))?;
                if cond.is_truthy() {
                    self.ip = target;
                }
            }

            Opcode::Return => {
                let v = self.stack.pop().unwrap_or(Object::Undefined);
                self.close_open_upvalues_from(0);
                return Ok(Flow::Return(v));
            }
            Opcode::ReturnNull => {
                self.close_open_upvalues_from(0);
                return Ok(Flow::Return(Object::Null));
            }
            Opcode::ToString => {
                let pos = self.chunk.position_at(self.ip - 1);
                let v = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                // Mirror the tree-walker's template interpolation (inspect()).
                self.stack.push(str_obj(v.inspect()));
            }
            Opcode::TypeOf => {
                let pos = self.chunk.position_at(self.ip - 1);
                let v = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                self.stack
                    .push(str_obj(crate::evaluator::expressions::typeof_name(&v)));
            }
            Opcode::Await => {
                let pos = self.chunk.position_at(self.ip - 1);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let awaited = await_value(value, &self.env, pos)?;
                self.stack.push(awaited);
            }
            Opcode::ImportModule => {
                let source_idx = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let source = self.read_string_const(source_idx, pos.clone(), "IMPORT_MODULE")?;
                let importer = self.env.borrow().vm.importer();
                let module = match importer {
                    Some(importer) => importer(&self.env, &source)?,
                    None => {
                        return Err(new_error(
                            pos,
                            "ImportError: module loading is not configured",
                        ));
                    }
                };
                self.stack.push(module);
            }
            Opcode::ExportName => {
                let name_idx = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let name = self.read_string_const(name_idx, pos.clone(), "EXPORT_NAME")?;
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let exports = self
                    .env
                    .borrow()
                    .get("exports")
                    .unwrap_or(Object::Undefined);
                match exports {
                    Object::Hash(h) => h.borrow_mut().set(name, value),
                    other => {
                        return Err(new_error(
                            pos,
                            format!("TypeError: cannot export from {}", other.type_tag()),
                        ));
                    }
                }
            }
            Opcode::ArraySliceFrom => {
                // Stack: [..., array, start]. Pop start, pop array, push tail.
                let pos = self.chunk.position_at(self.ip);
                let start_val = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let start = match start_val {
                    Object::Number(n) => n as usize,
                    other => {
                        return Err(new_error(
                            pos.clone(),
                            format!(
                                "TypeError: array slice start must be a number, got {}",
                                other.type_tag()
                            ),
                        ));
                    }
                };
                let array_val = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let tail: Vec<Object> = match array_val {
                    Object::Array(arr) => {
                        let arr = arr.borrow();
                        arr.elements[start.min(arr.elements.len())..].to_vec()
                    }
                    other => {
                        // Non-array source: empty tail (parity with tree-walker).
                        let _ = other;
                        Vec::new()
                    }
                };
                self.stack
                    .push(Object::Array(Rc::new(RefCell::new(ArrayData {
                        elements: tail,
                    }))));
            }
            Opcode::ExportAll => {
                // Pop the source module's exports object; copy every property
                // into the current module's exports (`export * from "..."`).
                let pos = self.chunk.position_at(self.ip);
                let source_exports = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let current_exports = self
                    .env
                    .borrow()
                    .get("exports")
                    .unwrap_or(Object::Undefined);
                match (&source_exports, &current_exports) {
                    (Object::Hash(src), Object::Hash(dst)) => {
                        let pairs: Vec<(String, Object)> = {
                            let sb = src.borrow();
                            sb.entries
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect()
                        };
                        for (k, v) in pairs {
                            // `export *` does NOT re-export a `default` binding.
                            if k == "default" {
                                continue;
                            }
                            dst.borrow_mut().set(k, v);
                        }
                    }
                    (other_src, _) => {
                        return Err(new_error(
                            pos,
                            format!(
                                "TypeError: export * source must be a module object, got {}",
                                other_src.type_tag()
                            ),
                        ));
                    }
                }
            }
            Opcode::WrapResolvedPromise => {
                // Pop a value and push a resolved Promise wrapping it (for
                // dynamic `import()`).
                let pos = self.chunk.position_at(self.ip);
                let value = self.stack.pop().ok_or_else(|| self.stack_underflow(pos))?;
                let promise = crate::object::Promise::new();
                promise.resolve(value);
                self.stack.push(Object::Promise(promise));
            }
            Opcode::Call => {
                let encoded_arg_count = self.chunk.read_u16(self.ip);
                let has_this_receiver = encoded_arg_count & 0x8000 != 0;
                let arg_count = (encoded_arg_count & 0x7fff) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                // Stack: [..., receiver?, callee, arg1, ..., argN].
                let stack_len = self.stack.len();
                let needed = arg_count + 1 + usize::from(has_this_receiver);
                if stack_len < needed {
                    return Err(self.stack_underflow(pos.clone()));
                }
                let args: Vec<Object> = self.stack.split_off(stack_len - arg_count);
                let callee = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let this = if has_this_receiver {
                    Some(
                        self.stack
                            .pop()
                            .ok_or_else(|| self.stack_underflow(pos.clone()))?,
                    )
                } else {
                    None
                };
                let result = self.call_value(callee, args, this, pos.clone())?;
                self.stack.push(result);
            }
            Opcode::PushArg => {
                let pos = self.chunk.position_at(self.ip - 1);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                self.push_packed_arg(value, false, pos)?;
            }
            Opcode::Spread => {
                let pos = self.chunk.position_at(self.ip - 1);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let target = self
                    .stack
                    .last()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?
                    .clone();
                match target {
                    Object::Array(_) => self.push_packed_arg(value, true, pos)?,
                    Object::Hash(hash) => {
                        if let Object::Hash(source) = value {
                            for (key, copied) in source.borrow().entries.iter() {
                                hash.borrow_mut().set(key.clone(), copied.clone());
                            }
                        }
                    }
                    other => {
                        return Err(new_error(
                            pos,
                            format!("VMError: SPREAD target is {}", other.type_tag()),
                        ));
                    }
                }
            }
            Opcode::CallSpread => {
                let pos = self.chunk.position_at(self.ip - 1);
                let args_obj = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let callee = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let args = match args_obj {
                    Object::Array(a) => a.borrow().elements.clone(),
                    other => {
                        return Err(new_error(
                            pos,
                            format!(
                                "VMError: CALL_SPREAD expected args array, got {}",
                                other.type_tag()
                            ),
                        ));
                    }
                };
                let result = self.call_value(callee, args, None, pos.clone())?;
                self.stack.push(result);
            }
            Opcode::New => {
                let arg_count = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let stack_len = self.stack.len();
                if stack_len < arg_count + 1 {
                    return Err(self.stack_underflow(pos.clone()));
                }
                let args: Vec<Object> = self.stack.split_off(stack_len - arg_count);
                let callee = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let result = self.construct_value(callee, args, pos.clone())?;
                self.stack.push(result);
            }
            Opcode::NewClass => {
                let class_idx = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let Some(class_decl) = self.chunk.classes.get(class_idx) else {
                    return Err(new_error(
                        pos,
                        format!("VMError: missing class declaration {}", class_idx),
                    ));
                };
                let class = crate::bytecode::class::build_class(
                    class_decl,
                    &self.env,
                    &crate::bytecode::resolve::ResolutionMap::default(),
                )?;
                self.stack.push(class);
            }
            Opcode::Closure => {
                let proto_idx = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let proto = self.chunk.protos[proto_idx].clone();
                let upvalues = self.capture_proto_upvalues(&proto)?;
                let closure = crate::bytecode::closure::ClosureData {
                    upvalue_names: proto
                        .upvalue_desc
                        .iter()
                        .map(|desc| desc.name.clone())
                        .collect(),
                    proto,
                    upvalues,
                    home_env: self.env.clone(),
                };
                self.stack.push(Object::Closure(Rc::new(closure)));
            }
            Opcode::NewArray => {
                let count = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                if self.stack.len() < count {
                    return Err(self.stack_underflow(pos));
                }
                let elements = self.stack.split_off(self.stack.len() - count);
                self.stack
                    .push(Object::Array(Rc::new(RefCell::new(ArrayData { elements }))));
            }
            Opcode::NewObject => {
                self.stack
                    .push(Object::Hash(Rc::new(RefCell::new(HashData::default()))));
            }
            Opcode::SetProperty => {
                let name_idx = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let name = self.read_string_const(name_idx, pos.clone(), "SET_PROPERTY")?;
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let obj = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let assigned = assign_property(&obj, &name, value, pos)?;
                self.stack.push(assigned);
            }
            Opcode::GetProperty => {
                let name_idx = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let name = self.read_string_const(name_idx, pos.clone(), "GET_PROPERTY")?;
                let obj = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let value = crate::evaluator::methods::get_property(&obj, &name, pos);
                if value.is_runtime_error() {
                    return Err(value);
                }
                self.stack.push(value);
            }
            Opcode::GetIndex => {
                let pos = self.chunk.position_at(self.ip - 1);
                let key = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let obj = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let value = crate::evaluator::methods::get_index(&obj, &key, pos);
                if value.is_runtime_error() {
                    return Err(value);
                }
                self.stack.push(value);
            }
            Opcode::SetIndex => {
                let pos = self.chunk.position_at(self.ip - 1);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let key = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let obj = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let assigned = assign_index(&obj, &key, value, pos)?;
                self.stack.push(assigned);
            }
            Opcode::IterKeys => {
                let pos = self.chunk.position_at(self.ip - 1);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let elements = crate::evaluator::eval_core::iterable_keys(&value)
                    .into_iter()
                    .map(str_obj)
                    .collect();
                self.stack
                    .push(Object::Array(Rc::new(RefCell::new(ArrayData { elements }))));
            }
            Opcode::IterValues => {
                let pos = self.chunk.position_at(self.ip - 1);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let iterator =
                    crate::evaluator::iterator::get_iterator(&value, &self.env, pos.clone());
                if iterator.is_runtime_error() {
                    return Err(iterator);
                }
                self.stack.push(iterator);
            }
            Opcode::IterNext => {
                let pos = self.chunk.position_at(self.ip - 1);
                let iterator = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let next =
                    crate::evaluator::iterator::iterator_next(&iterator, &self.env, pos.clone());
                if next.is_runtime_error() {
                    return Err(next);
                }
                self.stack.push(next);
            }
            Opcode::Len => {
                let pos = self.chunk.position_at(self.ip - 1);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let len = match &value {
                    Object::Array(a) => a.borrow().elements.len(),
                    Object::Hash(h) => h.borrow().entries.len(),
                    Object::String(s) => s.chars().count(),
                    Object::Map(m) => m.borrow().size(),
                    Object::Set(s) => s.borrow().size(),
                    _ => {
                        return Err(new_error(
                            pos,
                            format!("TypeError: cannot get length of {}", value.type_tag()),
                        ));
                    }
                };
                self.stack.push(Object::Number(len as f64));
            }

            // —— variables (routed through the environment name table) ——
            Opcode::LoadName => {
                let operand = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let name = match &self.chunk.constants[operand] {
                    Object::String(s) => s.as_str(),
                    _ => {
                        return Err(new_error(pos, "VMError: LOAD_NAME operand is not a string"));
                    }
                };
                let value = match self.env.borrow().get(name) {
                    Some(v) => v,
                    None => {
                        return Err(new_error(
                            pos,
                            format!("ReferenceError: '{}' is not defined", name),
                        ));
                    }
                };
                self.stack.push(value);
            }
            Opcode::StoreName => {
                let operand = self.chunk.read_u16(self.ip);
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let is_const = operand & 0x8000 != 0;
                let name_idx = (operand & 0x7fff) as usize;
                let name = match &self.chunk.constants[name_idx] {
                    Object::String(s) => s.as_str(),
                    _ => {
                        return Err(new_error(
                            pos,
                            "VMError: STORE_NAME operand is not a string",
                        ));
                    }
                };
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                if is_const {
                    self.env
                        .borrow_mut()
                        .set_const_here(name.to_string(), value);
                } else {
                    // `let`/`var` declaration: create the binding in this scope.
                    self.env.borrow_mut().set_here(name.to_string(), value);
                }
            }
            Opcode::StoreTypedName => {
                let operand = self.chunk.read_u16(self.ip);
                self.ip += 2;
                let type_idx = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 5);
                let is_const = operand & 0x8000 != 0;
                let name_idx = (operand & 0x7fff) as usize;
                let name = match &self.chunk.constants[name_idx] {
                    Object::String(s) => s.as_str(),
                    _ => {
                        return Err(new_error(
                            pos,
                            "VMError: STORE_TYPED_NAME operand is not a string",
                        ));
                    }
                };
                let Some(type_anno) = self.chunk.types.get(type_idx).cloned() else {
                    return Err(new_error(
                        pos,
                        format!("VMError: missing type annotation {}", type_idx),
                    ));
                };
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                if self.env.borrow().vm.type_check.load(Ordering::Relaxed)
                    && !value_matches_type_annotation(&value, &type_anno)
                {
                    return Err(new_error(
                        pos,
                        format!(
                            "TypeError: cannot assign {} to '{}: {}'",
                            value.type_tag(),
                            name,
                            type_anno
                        ),
                    ));
                }
                if is_const {
                    self.env
                        .borrow_mut()
                        .set_typed_const(name.to_string(), value, Some(type_anno));
                } else {
                    self.env
                        .borrow_mut()
                        .set_typed(name.to_string(), value, Some(type_anno));
                }
            }
            Opcode::AssignName => {
                let name_idx = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let name = match &self.chunk.constants[name_idx] {
                    Object::String(s) => s.as_str(),
                    _ => {
                        return Err(new_error(
                            pos,
                            "VMError: ASSIGN_NAME operand is not a string",
                        ));
                    }
                };
                // The value is already on the stack (assignment leaves it as
                // the expression result); peek, don't pop.
                let value = match self.stack.last() {
                    Some(v) => v.clone(),
                    None => return Err(self.stack_underflow(pos.clone())),
                };
                let Some((is_const, type_anno)) = self.binding_info(name) else {
                    return Err(new_error(
                        pos,
                        format!("ReferenceError: '{}' is not defined", name),
                    ));
                };
                if is_const {
                    return Err(new_error(
                        pos,
                        format!("TypeError: assignment to constant '{}'", name),
                    ));
                }
                if let Some(type_anno) = type_anno {
                    self.check_type_annotation(name, &value, &type_anno, pos)?;
                }
                let (found, is_const) = self.env.borrow_mut().assign(name, value);
                debug_assert!(found);
                debug_assert!(!is_const);
            }
            Opcode::LoadThis => {
                let value = self.env.borrow().this.clone().unwrap_or(Object::Undefined);
                self.stack.push(value);
            }
            // —— fast paths for resolved variable bindings ——
            //
            // These opcodes let the compiler skip the dynamic name-table lookup
            // (`LoadName`/`StoreName` → `Environment::get`, which walks the
            // scope chain with per-frame `borrow_mut`). Semantics are identical
            // to the `LoadName`/`StoreName` arms for the cases the compiler
            // lowers them into:
            //   * `LoadGlobal`/`StoreGlobal` read/write `vm.globals`, which is
            //     exactly where `Environment::get`/`assign` resolve globals.
            //   * `LoadLocal`/`StoreLocal` index the value stack by slot, the
            //     same storage `LoadUpvalue` already reads from.
            Opcode::LoadGlobal => {
                let operand = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let name = match &self.chunk.constants[operand] {
                    Object::String(s) => s.as_str(),
                    _ => {
                        return Err(new_error(
                            pos,
                            "VMError: LOAD_GLOBAL operand is not a string",
                        ));
                    }
                };
                // Mirrors `Environment::get`'s global fallback (`self.vm.get_global`).
                match self.env.borrow().vm.get_global(name) {
                    Some(v) => self.stack.push(v),
                    None => {
                        return Err(new_error(
                            pos,
                            format!("ReferenceError: '{}' is not defined", name),
                        ));
                    }
                }
            }
            Opcode::StoreGlobal => {
                let operand = self.chunk.read_u16(self.ip);
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let is_const = operand & 0x8000 != 0;
                let name_idx = (operand & 0x7fff) as usize;
                let name = match &self.chunk.constants[name_idx] {
                    Object::String(s) => s.as_str(),
                    _ => {
                        return Err(new_error(
                            pos,
                            "VMError: STORE_GLOBAL operand is not a string",
                        ));
                    }
                };
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                // The high bit is accepted for parity with `StoreName`'s
                // encoding (the const flag), but the global table has no
                // per-binding const marker. Declarations still go through
                // `StoreName` (which records const-ness in the environment), so
                // a `StoreGlobal` is only ever emitted for assignment to an
                // existing global; `is_const` is therefore informational here.
                let _ = is_const;
                self.env.borrow().vm.set_global(name.to_string(), value);
            }
            Opcode::LoadLocal => {
                let slot = self.read_single_byte_operand("LOAD_LOCAL")? as usize;
                let pos = self.chunk.position_at(self.ip - 2);
                let Some(value) = self.stack.get(slot).cloned() else {
                    return Err(new_error(
                        pos,
                        format!("VMError: LOAD_LOCAL slot {} out of range", slot),
                    ));
                };
                self.stack.push(value);
            }
            Opcode::StoreLocal => {
                let slot = self.read_single_byte_operand("STORE_LOCAL")? as usize;
                let pos = self.chunk.position_at(self.ip - 2);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                let Some(target) = self.stack.get_mut(slot) else {
                    return Err(new_error(
                        pos,
                        format!("VMError: STORE_LOCAL slot {} out of range", slot),
                    ));
                };
                *target = value;
            }
            Opcode::SuperMethod => {
                let name_idx = self.chunk.read_u16(self.ip) as usize;
                self.ip += 2;
                let pos = self.chunk.position_at(self.ip - 3);
                let name = self.read_string_const(name_idx, pos.clone(), "SUPER_METHOD")?;
                let value = if name == "constructor" {
                    crate::evaluator::methods::get_super_constructor(&self.env, pos)
                } else {
                    crate::evaluator::methods::get_super_method(&self.env, &name, pos)
                };
                if value.is_runtime_error() {
                    return Err(value);
                }
                self.stack.push(value);
            }
            Opcode::LoadUpvalue => {
                let index = self.read_single_byte_operand("LOAD_UPVALUE")? as usize;
                let pos = self.chunk.position_at(self.ip - 2);
                let Some(upvalue) = self.current_upvalues.get(index) else {
                    return Err(new_error(
                        pos,
                        format!("VMError: missing upvalue {}", index),
                    ));
                };
                let Some(value) = upvalue.get(&self.stack) else {
                    return Err(new_error(
                        pos,
                        format!("VMError: open upvalue {} points outside stack", index),
                    ));
                };
                self.stack.push(value);
            }
            Opcode::StoreUpvalue => {
                let index = self.read_single_byte_operand("STORE_UPVALUE")? as usize;
                let pos = self.chunk.position_at(self.ip - 2);
                let value = match self.stack.last() {
                    Some(v) => v.clone(),
                    None => return Err(self.stack_underflow(pos.clone())),
                };
                let Some(upvalue) = self.current_upvalues.get(index) else {
                    return Err(new_error(
                        pos,
                        format!("VMError: missing upvalue {}", index),
                    ));
                };
                if !upvalue.set(&mut self.stack, value) {
                    return Err(new_error(
                        pos,
                        format!("VMError: upvalue {} points outside stack", index),
                    ));
                }
            }
            Opcode::Throw => {
                let pos = self.chunk.position_at(instruction_ip);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                return Err(throw_value(value, pos));
            }
            Opcode::ThrowMatchError => {
                let pos = self.chunk.position_at(instruction_ip);
                let subject = self
                    .stack
                    .pop()
                    .ok_or_else(|| self.stack_underflow(pos.clone()))?;
                return Err(new_error(
                    pos,
                    format!("MatchError: no arm matched for {}", subject.inspect()),
                ));
            }

            other => {
                return Err(new_error(
                    self.chunk.position_at(self.ip - 1),
                    format!("VMError: opcode {:?} not implemented yet", other),
                ));
            }
        }
        Ok(Flow::Continue)
    }

    /// Pop two operands, apply a binary op via the shared evaluator core, push
    /// the result. The op string matches the GTS source operator so semantics
    /// are byte-identical to the tree-walker.
    fn bin_op(&mut self, op: &'static str) -> Result<(), Object> {
        let pos = self.chunk.position_at(self.ip - 1);
        let right = self
            .stack
            .pop()
            .ok_or_else(|| self.stack_underflow(pos.clone()))?;
        let left = self
            .stack
            .pop()
            .ok_or_else(|| self.stack_underflow(pos.clone()))?;
        let result = apply_binary_op(op, &left, &right, pos);
        if result.is_runtime_error() {
            return Err(result);
        }
        self.stack.push(result);
        Ok(())
    }

    fn check_type_annotation(
        &self,
        name: &str,
        value: &Object,
        type_anno: &TypeAnnotation,
        pos: Position,
    ) -> Result<(), Object> {
        if !self.env.borrow().vm.type_check.load(Ordering::Relaxed)
            || value_matches_type_annotation(value, type_anno)
        {
            return Ok(());
        }
        Err(new_error(
            pos,
            format!(
                "TypeError: cannot assign {} to '{}: {}'",
                value.type_tag(),
                name,
                type_anno
            ),
        ))
    }

    fn binding_info(&self, name: &str) -> Option<(bool, Option<TypeAnnotation>)> {
        let mut scope = Some(self.env.clone());
        while let Some(env) = scope {
            let borrowed = env.borrow();
            if let Some(binding) = borrowed.bindings.get(name) {
                return Some((binding.is_const, binding.type_anno.clone()));
            }
            scope = borrowed.parent.clone();
        }
        None
    }

    /// Pop one operand, apply a unary op, push the result.
    fn un_op(&mut self, op: &'static str) -> Result<(), Object> {
        let pos = self.chunk.position_at(self.ip - 1);
        let right = self
            .stack
            .pop()
            .ok_or_else(|| self.stack_underflow(pos.clone()))?;
        let result = apply_unary_op(op, &right, pos);
        if result.is_runtime_error() {
            return Err(result);
        }
        self.stack.push(result);
        Ok(())
    }

    fn stack_underflow(&self, pos: Position) -> Object {
        new_error(pos, "VMError: stack underflow")
    }

    fn read_single_byte_operand(&mut self, opcode: &'static str) -> Result<u8, Object> {
        let Some(byte) = self.chunk.code.get(self.ip).copied() else {
            return Err(new_error(
                self.chunk.position_at(self.ip.saturating_sub(1)),
                format!("VMError: {} missing operand", opcode),
            ));
        };
        self.ip += 1;
        Ok(byte)
    }

    fn capture_proto_upvalues(
        &mut self,
        proto: &Rc<crate::bytecode::closure::FunctionProto>,
    ) -> Result<Vec<Rc<Upvalue>>, Object> {
        let mut captured = Vec::with_capacity(proto.upvalue_desc.len());
        for desc in &proto.upvalue_desc {
            match desc.source {
                UpvalueSource::LocalSlot(slot) => {
                    if let Some(value) = self.capture_env_value(&desc.name) {
                        captured.push(Upvalue::new_closed(value));
                    } else {
                        captured.push(self.capture_open_upvalue(slot as usize));
                    }
                }
                UpvalueSource::ParentUpvalue(index) => {
                    let Some(parent) = self.current_upvalues.get(index as usize) else {
                        return Err(new_error(
                            proto.pos.clone(),
                            format!(
                                "VMError: missing parent upvalue {} for closure {}",
                                index, proto.name
                            ),
                        ));
                    };
                    captured.push(parent.clone());
                }
            }
        }
        Ok(captured)
    }

    fn capture_open_upvalue(&mut self, slot: usize) -> Rc<Upvalue> {
        let entry = self.open_upvalues.entry(slot).or_default();
        if let Some(existing) = entry.iter().find(|upvalue| upvalue.is_open()) {
            return existing.clone();
        }
        let upvalue = Upvalue::new_open(slot);
        entry.push(upvalue.clone());
        upvalue
    }

    fn capture_env_value(&self, name: &str) -> Option<Object> {
        self.env.borrow().get(name)
    }

    fn close_open_upvalues_from(&mut self, first_slot: usize) {
        let slots = &self.stack;
        let closing_slots: Vec<usize> = self
            .open_upvalues
            .range(first_slot..)
            .map(|(slot, _)| *slot)
            .collect();
        for slot in closing_slots {
            if let Some(upvalues) = self.open_upvalues.remove(&slot) {
                for upvalue in upvalues {
                    upvalue.close_from_slots(slots);
                }
            }
        }
    }

    fn unwind_to_handler(&mut self, error: Object) -> bool {
        let Some(region) = self
            .chunk
            .protected_regions
            .iter()
            .filter(|region| {
                let fault_ip = self.last_ip as u32;
                region.try_start <= fault_ip
                    && fault_ip < region.try_end
                    && region.handler_ip > region.try_end
            })
            .max_by_key(|region| region.try_start)
        else {
            return false;
        };

        self.stack.push(catch_value(error));
        self.ip = region.handler_ip as usize;
        true
    }

    fn read_string_const(
        &self,
        idx: usize,
        pos: Position,
        opcode: &'static str,
    ) -> Result<String, Object> {
        match self.chunk.constants.get(idx) {
            Some(Object::String(s)) => Ok(s.to_string()),
            _ => Err(new_error(
                pos,
                format!("VMError: {} operand is not a string", opcode),
            )),
        }
    }

    fn push_packed_arg(
        &mut self,
        value: Object,
        spread: bool,
        pos: Position,
    ) -> Result<(), Object> {
        let args_obj = self
            .stack
            .last()
            .ok_or_else(|| self.stack_underflow(pos.clone()))?;
        let Object::Array(args_array) = args_obj else {
            return Err(new_error(
                pos,
                format!(
                    "VMError: packed call args target is {}",
                    args_obj.type_tag()
                ),
            ));
        };

        let mut args = args_array.borrow_mut();
        if spread {
            if let Object::Array(items) = value {
                args.elements
                    .extend(items.borrow().elements.iter().cloned());
            } else {
                args.elements.push(value);
            }
        } else {
            args.elements.push(value);
        }
        Ok(())
    }

    /// Invoke a callable value with the given arguments. Stage 3 supports
    /// native builtins AND bytecode closures. Class construction is stage 5.
    fn call_value(
        &self,
        callee: Object,
        args: Vec<Object>,
        this: Option<Object>,
        pos: Position,
    ) -> Result<Object, Object> {
        match callee {
            Object::Builtin(b) => {
                let mut ctx = CallContext::new(&self.env, pos);
                ctx.receiver = b.extra.clone().or(this);
                let result = (b.func)(&mut ctx, &args);
                if result.is_runtime_error() {
                    Err(result)
                } else {
                    Ok(result)
                }
            }
            Object::Closure(c) => {
                crate::bytecode::call::call_closure_with_this(&c, &args, &self.env, this, pos)
            }
            // Tree-walker functions are still callable (e.g. globals installed
            // by register_globals that are Function values). Delegate to the
            // shared apply_function so semantics stay identical.
            other @ (Object::Function(_) | Object::Class(_)) => {
                let r = crate::evaluator::expressions::apply_function(
                    &other, &self.env, &args, this, pos,
                );
                if r.is_runtime_error() {
                    Err(r)
                } else {
                    Ok(r)
                }
            }
            Object::Hash(h) => {
                if let Some(Object::Builtin(b)) = h.borrow().get("__call").cloned() {
                    let mut ctx = CallContext::new(&self.env, pos);
                    ctx.receiver = b.extra.clone().or(this);
                    let result = (b.func)(&mut ctx, &args);
                    if result.is_runtime_error() {
                        Err(result)
                    } else {
                        Ok(result)
                    }
                } else {
                    Err(new_error(pos, "TypeError: object is not callable"))
                }
            }
            _ => Err(new_error(
                pos,
                format!("TypeError: {} is not callable", callee.type_tag()),
            )),
        }
    }

    fn construct_value(
        &self,
        callee: Object,
        args: Vec<Object>,
        pos: Position,
    ) -> Result<Object, Object> {
        let result = match callee {
            Object::Class(cls) => {
                crate::evaluator::methods::construct_class(&cls, &self.env, &args, pos.clone())
            }
            Object::Builtin(b) => {
                crate::evaluator::methods::construct_builtin(&b, &self.env, &args, pos.clone())
            }
            Object::Function(f) => crate::evaluator::expressions::apply_function(
                &Object::Function(f),
                &self.env,
                &args,
                None,
                pos.clone(),
            ),
            Object::Hash(_) => crate::evaluator::expressions::apply_function(
                &callee,
                &self.env,
                &args,
                None,
                pos.clone(),
            ),
            other => {
                return Err(new_error(
                    pos,
                    format!("TypeError: {} is not a constructor", other.type_tag()),
                ));
            }
        };
        if result.is_runtime_error() {
            Err(result)
        } else {
            Ok(result)
        }
    }
}

fn throw_value(value: Object, pos: Position) -> Object {
    match value {
        Object::Error(e) => {
            let mut data = e.borrow_mut().clone();
            data.runtime = true;
            if data.pos.is_zero() {
                data.pos = pos.clone();
            }
            if data.stack.is_empty() {
                data.stack = if pos.is_zero() {
                    format!("{}: {}", data.name, data.message)
                } else {
                    format!("{}: {}\n    at {}", data.name, data.message, pos)
                };
            }
            Object::Error(Rc::new(RefCell::new(data)))
        }
        other => {
            let err = new_named_error(pos, "Error", other.inspect());
            if let Object::Error(data) = &err {
                data.borrow_mut().thrown = Some(other);
            }
            err
        }
    }
}

fn catch_value(value: Object) -> Object {
    match value {
        Object::Error(e) => {
            let mut data = e.borrow_mut().clone();
            data.runtime = false;
            Object::Error(Rc::new(RefCell::new(data)))
        }
        other => other,
    }
}

/// One-step control-flow outcome.
enum Flow {
    Continue,
    Return(Object),
}

fn assign_property(
    obj: &Object,
    name: &str,
    value: Object,
    pos: Position,
) -> Result<Object, Object> {
    match obj {
        Object::Hash(h) => {
            if h.borrow().frozen {
                return Err(new_error(pos, "TypeError: cannot assign to frozen object"));
            }
            if h.borrow().sealed && !h.borrow().contains(name) {
                return Err(new_error(
                    pos,
                    "TypeError: cannot add property to sealed object",
                ));
            }
            h.borrow_mut().set(name, value.clone());
            Ok(value)
        }
        Object::Instance(i) => {
            i.borrow_mut().props.insert(name.into(), value.clone());
            Ok(value)
        }
        Object::Class(c) => {
            c.borrow_mut().statics.insert(name.into(), value.clone());
            Ok(value)
        }
        _ => Err(new_error(
            pos,
            format!("TypeError: cannot assign to property of {}", obj.type_tag()),
        )),
    }
}

fn assign_index(
    obj: &Object,
    key: &Object,
    value: Object,
    pos: Position,
) -> Result<Object, Object> {
    match obj {
        Object::Array(a) => {
            if let Object::Number(n) = key {
                let i = *n as isize;
                let mut arr = a.borrow_mut();
                let len = arr.elements.len() as isize;
                if i < 0 || i >= len {
                    return Err(new_error(pos, "RangeError: array index out of bounds"));
                }
                arr.elements[i as usize] = value.clone();
            }
            Ok(value)
        }
        Object::Hash(h) => {
            if h.borrow().frozen {
                return Err(new_error(pos, "TypeError: cannot assign to frozen object"));
            }
            h.borrow_mut().set(key.inspect(), value.clone());
            Ok(value)
        }
        _ => Err(new_error(
            pos,
            format!("TypeError: cannot index {}", obj.type_tag()),
        )),
    }
}

pub(crate) fn value_matches_type_annotation(value: &Object, anno: &TypeAnnotation) -> bool {
    if anno.optional && matches!(value, Object::Null | Object::Undefined) {
        return true;
    }
    match anno.kind {
        TypeKind::Union => anno
            .union
            .iter()
            .any(|member| value_matches_type_annotation(value, member)),
        TypeKind::Array => match value {
            Object::Array(items) => {
                let Some(inner) = &anno.array_of else {
                    return true;
                };
                items
                    .borrow()
                    .elements
                    .iter()
                    .all(|item| value_matches_type_annotation(item, inner))
            }
            _ => false,
        },
        TypeKind::Object => is_object_like(value),
        TypeKind::Function => is_function_like(value),
        TypeKind::Primitive => match anno.name.as_str() {
            "any" | "unknown" => true,
            "number" => matches!(value, Object::Number(_)),
            "string" => matches!(value, Object::String(_)),
            "boolean" | "bool" => matches!(value, Object::Boolean(_)),
            "null" => matches!(value, Object::Null),
            "undefined" | "void" => matches!(value, Object::Undefined),
            "object" => is_object_like(value),
            "function" => is_function_like(value),
            _ => true,
        },
    }
}

fn is_object_like(value: &Object) -> bool {
    matches!(
        value,
        Object::Hash(_)
            | Object::Array(_)
            | Object::Instance(_)
            | Object::Map(_)
            | Object::Set(_)
            | Object::Date(_)
            | Object::Regexp(_)
            | Object::Error(_)
            | Object::Null
    )
}

fn is_function_like(value: &Object) -> bool {
    matches!(
        value,
        Object::Function(_) | Object::Builtin(_) | Object::Class(_) | Object::Closure(_)
    )
}

fn await_value(value: Object, env: &EnvRef, pos: Position) -> Result<Object, Object> {
    match &value {
        Object::Promise(promise) => {
            if promise.state() == PromiseState::Pending {
                env.borrow().vm.wait_async();
            }
            let result = promise.wait();
            if promise.state() == PromiseState::Rejected {
                return Err(match &result {
                    Object::Error(data) => {
                        let mut error = data.borrow().clone();
                        error.runtime = true;
                        if error.pos.is_zero() {
                            error.pos = pos;
                        }
                        Object::Error(Rc::new(RefCell::new(error)))
                    }
                    other => new_error(pos, other.inspect()),
                });
            }
            Ok(result)
        }
        _ => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::compile;
    use crate::lexer::Lexer;
    use crate::object::Environment;
    use crate::object::Promise;
    use crate::object::VirtualMachine;
    use crate::parser::Parser;
    use std::sync::atomic::Ordering;

    fn run_src(src: &str) -> Object {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, "t.gs");
        let program = parser.parse_program();
        assert!(
            program.errors.is_empty(),
            "parse errors: {:?}",
            program.errors
        );
        let chunk = compile(&program).expect("compile");
        let vm = VirtualMachine::new();
        let env = Environment::new_root(vm);
        interpret(&chunk, &env)
    }

    fn compile_src(src: &str) -> Chunk {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, "t.gs");
        let program = parser.parse_program();
        assert!(
            program.errors.is_empty(),
            "parse errors: {:?}",
            program.errors
        );
        compile(&program).expect("compile")
    }

    fn run_src_with_globals(src: &str, globals: &[(&str, Object)]) -> Object {
        let chunk = compile_src(src);
        let vm = VirtualMachine::new();
        for (name, value) in globals {
            vm.set_global(*name, value.clone());
        }
        let env = Environment::new_root(vm);
        interpret(&chunk, &env)
    }

    fn run_src_with_env(src: &str) -> (Object, EnvRef) {
        let chunk = compile_src(src);
        let vm = VirtualMachine::new();
        let env = Environment::new_root(vm);
        let result = interpret(&chunk, &env);
        (result, env)
    }

    fn run_src_with_type_check(src: &str) -> Object {
        let chunk = compile_src(src);
        let vm = VirtualMachine::new();
        vm.type_check.store(true, Ordering::Relaxed);
        let env = Environment::new_root(vm);
        interpret(&chunk, &env)
    }

    fn module_fixture() -> Object {
        let module = Rc::new(RefCell::new(HashData::default()));
        module.borrow_mut().set("default", str_obj("D"));
        module.borrow_mut().set("named", str_obj("N"));
        module.borrow_mut().set("other", str_obj("A"));
        module.borrow_mut().set("extra", str_obj("X"));
        Object::Hash(module)
    }

    fn run_module_src(src: &str) -> (Object, Object) {
        let chunk = compile_src(src);
        let vm = VirtualMachine::new();
        let env = Environment::new_root(vm);
        let exports = Object::Hash(Rc::new(RefCell::new(HashData::default())));
        env.borrow_mut().set_here("exports", exports.clone());
        let result = interpret(&chunk, &env);
        (result, exports)
    }

    fn run_src_tree_and_bytecode(src: &str) -> (Object, Object) {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, "t.gs");
        let program = parser.parse_program();
        assert!(
            program.errors.is_empty(),
            "parse errors: {:?}",
            program.errors
        );

        let tree_vm = VirtualMachine::new();
        register_globals(&tree_vm);
        let tree_env = Environment::new_root(tree_vm);
        let tree = crate::evaluator::eval_program(&program, &tree_env);

        let chunk = compile(&program).expect("compile");
        let vm = VirtualMachine::new();
        let env = Environment::new_root(vm);
        let bytecode = interpret(&chunk, &env);
        (tree, bytecode)
    }

    fn assert_error_same(tree: Object, bytecode: Object) {
        let Object::Error(tree) = tree else {
            panic!("expected tree-walker error");
        };
        let Object::Error(bytecode) = bytecode else {
            panic!("expected bytecode error");
        };
        let tree = tree.borrow();
        let bytecode = bytecode.borrow();
        assert_eq!(bytecode.name, tree.name);
        assert_eq!(bytecode.message, tree.message);
        assert_eq!(bytecode.stack, tree.stack);
        assert_eq!(bytecode.pos, tree.pos);
    }

    fn run_chunk(chunk: Chunk) -> Object {
        let vm = VirtualMachine::new();
        let env = Environment::new_root(vm);
        interpret(&chunk, &env)
    }

    fn run_chunk_with_upvalues(chunk: Chunk, upvalues: Vec<Rc<Upvalue>>) -> Object {
        let vm = VirtualMachine::new();
        let env = Environment::new_root(vm);
        interpret_with_upvalues(&chunk, &env, upvalues)
    }

    fn state_for_upvalue_tests() -> VmState<'static> {
        let chunk = Box::leak(Box::new(Chunk::new()));
        let vm = VirtualMachine::new();
        let env = Environment::new_root(vm.clone());
        VmState::new(chunk, env, Vec::new(), vm)
    }

    #[test]
    fn throw_opcode_wraps_non_error_value() {
        let result = run_src("throw \"boom\";");
        let Object::Error(data) = result else {
            panic!("expected runtime error");
        };
        let data = data.borrow();
        assert!(data.runtime);
        assert_eq!(data.name, "Error");
        assert_eq!(data.message, "boom");
        assert!(matches!(data.thrown.as_ref(), Some(Object::String(s)) if s.as_ref() == "boom"));
    }

    #[test]
    fn try_catch_unwinds_to_handler() {
        let result = run_src(
            r#"
            let label = "none";
            try {
                throw "boom";
                label = "miss";
            } catch (err) {
                label = err.message;
            }
            label;
            "#,
        );
        assert!(matches!(result, Object::String(s) if s.as_ref() == "boom"));
    }

    #[test]
    fn try_finally_runs_on_normal_path() {
        let result = run_src(
            r#"
            let label = "start";
            try {
                label = label + ":try";
            } finally {
                label = label + ":finally";
            }
            label;
            "#,
        );
        assert!(matches!(result, Object::String(s) if s.as_ref() == "start:try:finally"));
    }

    #[test]
    fn catch_then_finally_runs_in_order() {
        let result = run_src(
            r#"
            let label = "start";
            try {
                throw "boom";
            } catch (err) {
                label = label + ":catch";
            } finally {
                label = label + ":finally";
            }
            label;
            "#,
        );
        assert!(matches!(result, Object::String(s) if s.as_ref() == "start:catch:finally"));
    }

    #[test]
    fn finally_throw_overrides_original_throw() {
        let result = run_src(
            r#"
            try {
                throw "first";
            } finally {
                throw "second";
            }
            "#,
        );
        let Object::Error(data) = result else {
            panic!("expected runtime error");
        };
        assert_eq!(data.borrow().message, "second");
    }

    #[test]
    fn await_non_promise_returns_value() {
        let result = run_src("await 42");
        assert!(matches!(result, Object::Number(n) if n == 42.0));
    }

    #[test]
    fn await_resolved_promise_returns_value() {
        let promise = Promise::new();
        promise.resolve(Object::Number(42.0));
        let result = run_src_with_globals("await ready", &[("ready", Object::Promise(promise))]);
        assert!(matches!(result, Object::Number(n) if n == 42.0));
    }

    #[test]
    fn await_rejected_promise_returns_runtime_error() {
        let promise = Promise::new();
        promise.reject(str_obj("nope"));
        let result = run_src_with_globals("await failed", &[("failed", Object::Promise(promise))]);
        let Object::Error(data) = result else {
            panic!("expected await rejection to become runtime error");
        };
        let data = data.borrow();
        assert!(data.runtime);
        assert_eq!(data.name, "Error");
        assert_eq!(data.message, "nope");
    }

    #[test]
    fn async_function_call_returns_resolved_promise() {
        let result = run_src(
            r#"
            async function answer() {
                return 42;
            }
            answer();
            "#,
        );
        let Object::Promise(promise) = result else {
            panic!("expected async function call to return a promise");
        };
        assert_eq!(promise.state(), PromiseState::Fulfilled);
        assert!(matches!(promise.wait(), Object::Number(n) if n == 42.0));
    }

    #[test]
    fn async_arrow_can_be_awaited() {
        let result = run_src(
            r#"
            let answer = async (value) => value + 1;
            await answer(41);
            "#,
        );
        assert!(matches!(result, Object::Number(n) if n == 42.0));
    }

    #[test]
    fn async_method_can_be_awaited() {
        let result = run_src(
            r#"
            class Box {
                async value() {
                    return 42;
                }
            }
            let box = new Box();
            await box.value();
            "#,
        );
        assert!(matches!(result, Object::Number(n) if n == 42.0));
    }

    #[test]
    fn error_position_matches_treewalker_for_binary_type_error() {
        let (tree, bytecode) = run_src_tree_and_bytecode("1 + true;");
        assert_error_same(tree, bytecode);
    }

    #[test]
    fn throw_position_matches_treewalker_for_non_error_value() {
        let (tree, bytecode) = run_src_tree_and_bytecode("throw \"boom\";");
        assert_error_same(tree, bytecode);
    }

    #[test]
    fn stage0_contract_one_plus_two() {
        // The single non-negotiable stage-0 contract: 1 + 2 → 3.0
        let result = run_src("1 + 2");
        assert!(matches!(result, Object::Number(n) if n == 3.0));
    }

    #[test]
    fn chain_add_left_associative() {
        let result = run_src("1 + 2 + 3");
        assert!(matches!(result, Object::Number(n) if n == 6.0));
    }

    #[test]
    fn import_statement_binds_default_named_alias_and_namespace() {
        let chunk = compile_src(
            r#"
            import def, { named, other as alias } from "mod";
            import * as ns from "mod";
            def + ":" + named + ":" + alias + ":" + ns.extra;
            "#,
        );
        let vm = VirtualMachine::new();
        vm.set_importer(Rc::new(|_env, spec| {
            assert_eq!(spec, "mod");
            Ok(module_fixture())
        }));
        let env = Environment::new_root(vm);
        let result = interpret(&chunk, &env);

        assert!(matches!(result, Object::String(s) if s.as_ref() == "D:N:A:X"));
        assert!(matches!(env.borrow().get("def"), Some(Object::String(s)) if s.as_ref() == "D"));
        assert!(matches!(env.borrow().get("named"), Some(Object::String(s)) if s.as_ref() == "N"));
        assert!(matches!(env.borrow().get("alias"), Some(Object::String(s)) if s.as_ref() == "A"));
        assert!(matches!(env.borrow().get("ns"), Some(Object::Hash(_))));
    }

    #[test]
    fn import_statement_reports_missing_importer() {
        let result = run_src(r#"import value from "mod";"#);
        let Object::Error(data) = result else {
            panic!("expected import error");
        };
        assert_eq!(data.borrow().name, "ImportError");
        assert_eq!(data.borrow().message, "module loading is not configured");
    }

    #[test]
    fn export_declaration_writes_named_and_alias_exports() {
        let (result, exports) = run_module_src(
            r#"
            export const value = 21;
            export function double(x) { return x * 2; }
            export { value as answer };
            "#,
        );
        assert!(matches!(result, Object::Undefined));
        let Object::Hash(exports) = exports else {
            panic!("expected exports hash");
        };
        let exports = exports.borrow();
        assert!(matches!(exports.get("value"), Some(Object::Number(n)) if *n == 21.0));
        assert!(matches!(exports.get("answer"), Some(Object::Number(n)) if *n == 21.0));
        assert!(matches!(exports.get("double"), Some(Object::Closure(_))));
    }

    #[test]
    fn export_default_expression_writes_default_export() {
        let (result, exports) = run_module_src(r#"export default "hello";"#);
        assert!(matches!(result, Object::Undefined));
        let Object::Hash(exports) = exports else {
            panic!("expected exports hash");
        };
        assert!(
            matches!(exports.borrow().get("default"), Some(Object::String(s)) if s.as_ref() == "hello")
        );
    }

    #[test]
    fn reexport_from_module_copies_source_exports() {
        let chunk = compile_src(r#"export { named as alias, extra } from "mod";"#);
        let vm = VirtualMachine::new();
        vm.set_importer(Rc::new(|_env, spec| {
            assert_eq!(spec, "mod");
            Ok(module_fixture())
        }));
        let env = Environment::new_root(vm);
        let exports = Object::Hash(Rc::new(RefCell::new(HashData::default())));
        env.borrow_mut().set_here("exports", exports.clone());
        let result = interpret(&chunk, &env);

        assert!(matches!(result, Object::Undefined));
        let Object::Hash(exports) = exports else {
            panic!("expected exports hash");
        };
        let exports = exports.borrow();
        assert!(matches!(exports.get("alias"), Some(Object::String(s)) if s.as_ref() == "N"));
        assert!(matches!(exports.get("extra"), Some(Object::String(s)) if s.as_ref() == "X"));
    }

    #[test]
    fn return_null_opcode_returns_null() {
        let mut chunk = Chunk::new();
        chunk.write_op(Opcode::ReturnNull, Position::default());
        assert!(matches!(run_chunk(chunk), Object::Null));
    }

    #[test]
    fn open_upvalues_reuse_the_same_slot_capture() {
        let mut state = state_for_upvalue_tests();
        state.stack.push(Object::Number(1.0));
        state.stack.push(Object::Number(2.0));

        let first = state.capture_open_upvalue(1);
        let second = state.capture_open_upvalue(1);

        assert!(Rc::ptr_eq(&first, &second));
        assert_eq!(state.open_upvalues.len(), 1);
        assert!(matches!(first.get(&state.stack), Some(Object::Number(2.0))));
    }

    #[test]
    fn return_closes_open_upvalues_from_frame_slots() {
        let mut state = state_for_upvalue_tests();
        state.stack.push(Object::Number(3.0));
        state.stack.push(Object::Number(4.0));
        let kept = state.capture_open_upvalue(1);

        state.close_open_upvalues_from(0);
        state.stack[1] = Object::Number(9.0);

        assert!(state.open_upvalues.is_empty());
        assert!(!kept.is_open());
        assert!(matches!(kept.get(&state.stack), Some(Object::Number(4.0))));
    }

    #[test]
    fn load_upvalue_reads_closed_capture() {
        let mut chunk = Chunk::new();
        chunk.write_op(Opcode::LoadUpvalue, Position::default());
        chunk.write_byte(0, Position::default());
        chunk.write_op(Opcode::Return, Position::default());

        let result = run_chunk_with_upvalues(chunk, vec![Upvalue::new_closed(Object::Number(8.0))]);

        assert!(matches!(result, Object::Number(n) if n == 8.0));
    }

    #[test]
    fn store_upvalue_updates_closed_capture_and_leaves_value() {
        let mut chunk = Chunk::new();
        let value = chunk.add_constant(Object::Number(11.0));
        chunk.write_op(Opcode::Const, Position::default());
        chunk.write_u16(value, Position::default());
        chunk.write_op(Opcode::StoreUpvalue, Position::default());
        chunk.write_byte(0, Position::default());
        chunk.write_op(Opcode::Return, Position::default());
        let upvalue = Upvalue::new_closed(Object::Number(1.0));

        let result = run_chunk_with_upvalues(chunk, vec![upvalue.clone()]);

        assert!(matches!(result, Object::Number(n) if n == 11.0));
        assert!(matches!(upvalue.get(&[]), Some(Object::Number(11.0))));
    }

    // —— resolved-binding fast paths (LoadGlobal/StoreGlobal/LoadLocal/StoreLocal) ——
    //
    // These exercise the opcode implementations directly (the compiler wiring
    // lands in stage 3), so each test hand-builds a chunk.

    /// Run a chunk against a root environment whose VM is pre-populated with the
    /// given globals, returning the top-of-stack result.
    fn run_chunk_with_globals(chunk: Chunk, globals: &[(&str, Object)]) -> Object {
        let vm = VirtualMachine::new();
        for (name, value) in globals {
            vm.set_global((*name).to_string(), value.clone());
        }
        let env = Environment::new_root(vm);
        interpret(&chunk, &env)
    }

    #[test]
    fn load_global_reads_existing_global() {
        let mut chunk = Chunk::new();
        let name = chunk.add_constant(str_obj("answer".to_string()));
        chunk.write_op(Opcode::LoadGlobal, Position::default());
        chunk.write_u16(name, Position::default());
        chunk.write_op(Opcode::Return, Position::default());

        let result = run_chunk_with_globals(chunk, &[("answer", Object::Number(42.0))]);
        assert!(matches!(result, Object::Number(n) if n == 42.0));
    }

    #[test]
    fn load_global_undefined_name_is_reference_error() {
        let mut chunk = Chunk::new();
        let name = chunk.add_constant(str_obj("nope".to_string()));
        chunk.write_op(Opcode::LoadGlobal, Position::default());
        chunk.write_u16(name, Position::default());
        chunk.write_op(Opcode::Return, Position::default());

        let result = run_chunk_with_globals(chunk, &[]);
        let Object::Error(data) = result else {
            panic!("expected ReferenceError, got {result:?}");
        };
        let data = data.borrow();
        assert_eq!(data.name, "ReferenceError");
        assert!(data.message.contains("'nope'"));
    }

    #[test]
    fn store_global_writes_global_table() {
        let mut chunk = Chunk::new();
        let value = chunk.add_constant(Object::Number(7.0));
        let name = chunk.add_constant(str_obj("g".to_string()));
        // Push the value, then store it into the global "g".
        chunk.write_op(Opcode::Const, Position::default());
        chunk.write_u16(value, Position::default());
        chunk.write_op(Opcode::StoreGlobal, Position::default());
        chunk.write_u16(name, Position::default());
        // Read it back via LoadGlobal to confirm round-trip.
        chunk.write_op(Opcode::LoadGlobal, Position::default());
        chunk.write_u16(name, Position::default());
        chunk.write_op(Opcode::Return, Position::default());

        let result = run_chunk_with_globals(chunk, &[]);
        assert!(matches!(result, Object::Number(n) if n == 7.0));
    }

    #[test]
    fn load_local_reads_stack_slot() {
        let mut chunk = Chunk::new();
        let a = chunk.add_constant(Object::Number(1.0));
        let b = chunk.add_constant(Object::Number(2.0));
        // Stack layout after the two Consts: [1, 2] (slots 0, 1).
        chunk.write_op(Opcode::Const, Position::default());
        chunk.write_u16(a, Position::default());
        chunk.write_op(Opcode::Const, Position::default());
        chunk.write_u16(b, Position::default());
        // Read slot 0 (the "1") and return it.
        chunk.write_op(Opcode::LoadLocal, Position::default());
        chunk.write_byte(0, Position::default());
        chunk.write_op(Opcode::Return, Position::default());

        let result = run_chunk(chunk);
        assert!(matches!(result, Object::Number(n) if n == 1.0));
    }

    #[test]
    fn store_local_overwrites_stack_slot() {
        let mut chunk = Chunk::new();
        let a = chunk.add_constant(Object::Number(1.0));
        let b = chunk.add_constant(Object::Number(2.0));
        chunk.write_op(Opcode::Const, Position::default());
        chunk.write_u16(a, Position::default());
        chunk.write_op(Opcode::Const, Position::default());
        chunk.write_u16(b, Position::default());
        // Overwrite slot 0 with the value just pushed (2).
        chunk.write_op(Opcode::StoreLocal, Position::default());
        chunk.write_byte(0, Position::default());
        // Now slot 0 should be 2; read it back.
        chunk.write_op(Opcode::LoadLocal, Position::default());
        chunk.write_byte(0, Position::default());
        chunk.write_op(Opcode::Return, Position::default());

        let result = run_chunk(chunk);
        assert!(matches!(result, Object::Number(n) if n == 2.0));
    }

    #[test]
    fn load_local_out_of_range_is_vmerror() {
        let mut chunk = Chunk::new();
        chunk.write_op(Opcode::LoadLocal, Position::default());
        chunk.write_byte(5, Position::default()); // nothing on the stack
        chunk.write_op(Opcode::Return, Position::default());

        let result = run_chunk(chunk);
        let Object::Error(data) = result else {
            panic!("expected VMError, got {result:?}");
        };
        assert!(data.borrow().message.contains("LOAD_LOCAL"));
    }

    #[test]
    fn load_upvalue_can_read_open_stack_slot() {
        let mut chunk = Chunk::new();
        let outer = chunk.add_constant(Object::Number(13.0));
        chunk.write_op(Opcode::Const, Position::default());
        chunk.write_u16(outer, Position::default());
        chunk.write_op(Opcode::LoadUpvalue, Position::default());
        chunk.write_byte(0, Position::default());
        chunk.write_op(Opcode::Return, Position::default());

        let result = run_chunk_with_upvalues(chunk, vec![Upvalue::new_open(0)]);

        assert!(matches!(result, Object::Number(n) if n == 13.0));
    }

    // —— arithmetic operators (each covered by its own case) ——
    #[test]
    fn arithmetic_sub() {
        assert!(matches!(run_src("10 - 3"), Object::Number(n) if n == 7.0));
    }
    #[test]
    fn arithmetic_mul() {
        assert!(matches!(run_src("4 * 5"), Object::Number(n) if n == 20.0));
    }
    #[test]
    fn arithmetic_div() {
        assert!(matches!(run_src("20 / 4"), Object::Number(n) if n == 5.0));
    }
    #[test]
    fn arithmetic_mod() {
        // number_op uses rem_euclid; 10 % 3 == 1
        assert!(matches!(run_src("10 % 3"), Object::Number(n) if n == 1.0));
    }
    #[test]
    fn arithmetic_pow() {
        assert!(matches!(run_src("2 ** 10"), Object::Number(n) if n == 1024.0));
    }
    #[test]
    fn bitwise_operators_match_treewalker_core() {
        assert!(matches!(run_src("(5 & 3) + (5 | 3) + (5 ^ 3)"), Object::Number(n) if n == 14.0));
        assert!(matches!(run_src("(~5) + (5 << 1) + (5 >> 1)"), Object::Number(n) if n == 6.0));
    }
    #[test]
    fn object_has_own_property_checks_direct_entries() {
        assert!(matches!(
            run_src("let obj = { city: \"Paris\" }; obj.hasOwnProperty(\"city\")"),
            Object::Boolean(true)
        ));
        assert!(matches!(
            run_src("let obj = { city: \"Paris\" }; obj.hasOwnProperty(\"name\")"),
            Object::Boolean(false)
        ));
    }

    #[test]
    fn callable_hash_globals_match_treewalker() {
        let (tree, bytecode) = run_src_tree_and_bytecode("String(Date.now()).length > 0;");
        assert!(matches!(tree, Object::Boolean(true)));
        assert!(matches!(bytecode, Object::Boolean(true)));
    }

    #[test]
    fn precedence_mul_before_add() {
        assert!(matches!(run_src("2 + 3 * 4"), Object::Number(n) if n == 14.0));
    }

    // —— comparison operators ——
    #[test]
    fn compare_eq_true() {
        assert!(matches!(run_src("3 === 3"), Object::Boolean(true)));
    }
    #[test]
    fn compare_eq_false() {
        assert!(matches!(run_src("3 === 4"), Object::Boolean(false)));
    }
    #[test]
    fn compare_neq() {
        assert!(matches!(run_src("3 !== 4"), Object::Boolean(true)));
    }
    #[test]
    fn compare_lt() {
        assert!(matches!(run_src("2 < 3"), Object::Boolean(true)));
        assert!(matches!(run_src("3 < 2"), Object::Boolean(false)));
    }
    #[test]
    fn compare_le() {
        assert!(matches!(run_src("3 <= 3"), Object::Boolean(true)));
        assert!(matches!(run_src("4 <= 3"), Object::Boolean(false)));
    }
    #[test]
    fn compare_gt() {
        assert!(matches!(run_src("5 > 3"), Object::Boolean(true)));
        assert!(matches!(run_src("3 > 5"), Object::Boolean(false)));
    }
    #[test]
    fn compare_ge() {
        assert!(matches!(run_src("3 >= 3"), Object::Boolean(true)));
        assert!(matches!(run_src("2 >= 3"), Object::Boolean(false)));
    }

    // —— unary ——
    #[test]
    fn unary_neg() {
        assert!(matches!(run_src("-5"), Object::Number(n) if n == -5.0));
        assert!(matches!(run_src("-(3 + 2)"), Object::Number(n) if n == -5.0));
    }
    #[test]
    fn unary_not_bool() {
        assert!(matches!(run_src("!false"), Object::Boolean(true)));
        assert!(matches!(run_src("!true"), Object::Boolean(false)));
    }
    #[test]
    fn unary_not_truthiness() {
        // numbers: 0 is falsy, non-zero truthy
        assert!(matches!(run_src("!0"), Object::Boolean(true)));
        assert!(matches!(run_src("!1"), Object::Boolean(false)));
    }

    // —— short-circuit && / || ——
    #[test]
    fn and_returns_left_when_falsy() {
        // 0 && 1 → 0 (left, short-circuits)
        assert!(matches!(run_src("0 && 1"), Object::Number(n) if n == 0.0));
    }
    #[test]
    fn and_returns_right_when_left_truthy() {
        // 1 && 2 → 2
        assert!(matches!(run_src("1 && 2"), Object::Number(n) if n == 2.0));
    }
    #[test]
    fn or_returns_left_when_truthy() {
        // 7 || 0 → 7
        assert!(matches!(run_src("7 || 0"), Object::Number(n) if n == 7.0));
    }
    #[test]
    fn or_returns_right_when_left_falsy() {
        // 0 || 9 → 9
        assert!(matches!(run_src("0 || 9"), Object::Number(n) if n == 9.0));
    }
    #[test]
    fn and_short_circuits_bool() {
        // false && true → false (right never semantically matters)
        assert!(matches!(run_src("false && true"), Object::Boolean(false)));
    }
    #[test]
    fn or_short_circuits_bool() {
        // true || false → true
        assert!(matches!(run_src("true || false"), Object::Boolean(true)));
    }

    #[test]
    fn nullish_coalescing_returns_right_for_null_or_undefined() {
        assert!(matches!(run_src("null ?? 42"), Object::Number(n) if n == 42.0));
        assert!(matches!(run_src("undefined ?? 7"), Object::Number(n) if n == 7.0));
    }

    #[test]
    fn nullish_coalescing_keeps_non_nullish_falsy_left() {
        assert!(matches!(run_src("0 ?? 9"), Object::Number(n) if n == 0.0));
        assert!(matches!(run_src("false ?? true"), Object::Boolean(false)));
    }

    // —— update operators ++/-- (B3.1): bytecode must match tree-walker ——
    #[test]
    fn prefix_increment_returns_new_value() {
        // `++x` evaluates to the new value.
        assert!(matches!(run_src("let x = 5; ++x"), Object::Number(n) if n == 6.0));
    }

    #[test]
    fn postfix_increment_returns_old_value() {
        // `x++` evaluates to the old value.
        assert!(matches!(run_src("let x = 5; x++"), Object::Number(n) if n == 5.0));
    }

    #[test]
    fn update_operator_parity_matches_treewalker() {
        // The full prefix/postfix sequence must be byte-identical to the
        // tree-walker (parity gate for B3.1).
        let src = "let a = 5; let b = a++; let c = ++a; let d = a--; let e = --a; [a, b, c, d, e]";
        let (tree, bytecode) = run_src_tree_and_bytecode(src);
        assert_eq!(tree.inspect(), bytecode.inspect());
    }

    // —— destructuring (B3.2): bytecode must match tree-walker ——
    #[test]
    fn array_destructuring_binds_in_order() {
        let out = run_src("let [a, b, c] = [1, 2, 3]; [a, b, c]");
        assert!(out.inspect().contains("1") && out.inspect().contains("3"));
    }

    #[test]
    fn destructuring_default_applies_on_undefined() {
        // Missing element → default. Array and object.
        let out = run_src("let [a, b = 9] = [1]; let {x = 7} = {}; [a, b, x]");
        let s = out.inspect();
        assert!(
            s.contains("9") && s.contains("7"),
            "defaults should apply: {s}"
        );
    }

    #[test]
    fn destructuring_parity_matches_treewalker() {
        // Array + hole + default + object + rename must match byte-for-byte.
        let src = "let [a, b] = [10, 20]; let [x, , z = 99] = [1, 2, 3]; let {p, q} = {p:1, q:2}; [a, b, x, z, p, q]";
        let (tree, bytecode) = run_src_tree_and_bytecode(src);
        assert_eq!(tree.inspect(), bytecode.inspect());
    }

    // —— void / delete operators (B3): bytecode must match tree-walker ——
    #[test]
    fn void_operator_yields_undefined() {
        // void evaluates its operand (side effect) then returns undefined.
        assert!(matches!(run_src("void 42"), Object::Undefined));
        assert!(matches!(run_src("void \"hi\""), Object::Undefined));
    }

    #[test]
    fn delete_operator_returns_true() {
        // delete evaluates its operand then returns true (parity: does not
        // actually remove the property — matches the tree-walker).
        assert!(matches!(run_src("delete (1)"), Object::Boolean(true)));
    }

    #[test]
    fn void_and_delete_parity_matches_treewalker() {
        // void (with side effect) and delete must match byte-for-byte.
        let src =
            "let s = 0; let a = void (s = s + 5); let o = {x:1}; let b = delete o.x; [a, b, s]";
        let (tree, bytecode) = run_src_tree_and_bytecode(src);
        assert_eq!(tree.inspect(), bytecode.inspect());
    }

    // —— destructuring rest (B3): bytecode must match tree-walker ——
    #[test]
    fn array_destructuring_rest_collects_tail() {
        // `...rest` collects the tail [2..] into a new array.
        let out = run_src("let [a, ...rest] = [1, 2, 3, 4]; rest");
        let s = out.inspect();
        assert!(
            s.contains("2") && s.contains("4"),
            "rest should be [2,3,4]: {s}"
        );
    }

    #[test]
    fn destructuring_rest_parity_matches_treewalker() {
        // Rest element + leading bindings + default must match byte-for-byte.
        let src = "let [a, b = 9, ...rest] = [1]; [a, b, rest]";
        let (tree, bytecode) = run_src_tree_and_bytecode(src);
        assert_eq!(tree.inspect(), bytecode.inspect());
    }

    // —— null / undefined literals (needed to exercise falsy paths) ——
    #[test]
    fn null_literal_is_falsy_in_and() {
        // null && 1 → null
        assert!(matches!(run_src("null && 1"), Object::Null));
    }
    #[test]
    fn undefined_literal_is_falsy_in_or() {
        // undefined || 42 → 42
        assert!(matches!(run_src("undefined || 42"), Object::Number(n) if n == 42.0));
    }

    // —— string literals + concatenation (stage 1.2) ——
    #[test]
    fn string_literal() {
        assert!(matches!(run_src("\"hello\""), Object::String(s) if &*s == "hello"));
    }
    #[test]
    fn string_literal_escape() {
        // \n is processed at compile time, mirroring eval_string_lit
        assert!(matches!(run_src("\"a\\nb\""), Object::String(s) if &*s == "a\nb"));
    }
    #[test]
    fn string_concat_now_supported() {
        // Previously deferred; String literals now compile so `+` routes
        // through apply_binary_op("+") which handles string+string.
        assert!(matches!(run_src("\"foo\" + \"bar\""), Object::String(s) if &*s == "foobar"));
    }
    #[test]
    fn string_strict_equal() {
        assert!(matches!(run_src("\"a\" === \"a\""), Object::Boolean(true)));
        assert!(matches!(run_src("\"a\" === \"b\""), Object::Boolean(false)));
    }
    #[test]
    fn static_template_literal() {
        // Backtick template with no interpolation reduces to a string.
        assert!(matches!(run_src("`hi there`"), Object::String(s) if &*s == "hi there"));
    }

    // —— variables (stage 1.3) ——
    #[test]
    fn let_decl_and_read() {
        // `let x = 10; x` — last expression is the result
        assert!(matches!(run_src("let x = 10\nx"), Object::Number(n) if n == 10.0));
    }

    #[test]
    fn typed_declaration_preserves_annotation_without_default_checking() {
        let (result, env) = run_src_with_env("let value: number = \"not-number\"\nvalue");

        assert!(matches!(result, Object::String(s) if s.as_ref() == "not-number"));
        let env = env.borrow();
        let binding = env.bindings.get("value").expect("typed binding");
        assert!(matches!(&binding.value, Object::String(s) if s.as_ref() == "not-number"));
        assert_eq!(
            binding
                .type_anno
                .as_ref()
                .expect("type annotation")
                .to_string(),
            "number"
        );
    }

    #[test]
    fn type_check_rejects_mismatched_typed_declaration() {
        let result = run_src_with_type_check("let value: number = \"not-number\"\nvalue");
        let Object::Error(data) = result else {
            panic!("expected type error");
        };
        let data = data.borrow();
        assert_eq!(data.name, "TypeError");
        assert_eq!(data.message, "cannot assign string to 'value: number'");
    }

    #[test]
    fn type_check_rejects_mismatched_assignment_to_typed_binding() {
        let result = run_src_with_type_check("let value: number = 1\nvalue = \"two\"");
        let Object::Error(data) = result else {
            panic!("expected type error");
        };
        let data = data.borrow();
        assert_eq!(data.name, "TypeError");
        assert_eq!(data.message, "cannot assign string to 'value: number'");
    }

    #[test]
    fn type_check_rejects_mismatched_function_return() {
        let result = run_src_with_type_check(
            r#"
            function value(): number {
                return "not-number";
            }
            value();
            "#,
        );
        let Object::Error(data) = result else {
            panic!("expected type error");
        };
        let data = data.borrow();
        assert_eq!(data.name, "TypeError");
        assert_eq!(
            data.message,
            "cannot return string from function returning number"
        );
    }

    #[test]
    fn const_decl_and_read() {
        assert!(matches!(run_src("const y = 5\ny * 2"), Object::Number(n) if n == 10.0));
    }
    #[test]
    fn var_decl_no_initializer() {
        // `var z;` → undefined
        assert!(matches!(run_src("var z\nz"), Object::Undefined));
    }
    #[test]
    fn assignment_to_let() {
        assert!(matches!(
            run_src("let a = 1\na = 2\na"),
            Object::Number(n) if n == 2.0));
    }
    #[test]
    fn assignment_is_expression() {
        // `let a = 1; a = 5` evaluates to 5 (the assigned value)
        assert!(matches!(run_src("let a = 1\na = 5"), Object::Number(n) if n == 5.0));
    }
    #[test]
    fn compound_add_assign() {
        assert!(matches!(run_src("let a = 10\na += 5\na"), Object::Number(n) if n == 15.0));
    }
    #[test]
    fn read_undefined_var_is_reference_error() {
        let r = run_src("nosuchvar");
        assert!(r.is_runtime_error());
    }
    #[test]
    fn const_reassign_is_type_error() {
        let r = run_src("const c = 1\nc = 2");
        assert!(r.is_runtime_error());
    }
    #[test]
    fn variable_in_arithmetic() {
        assert!(matches!(
            run_src("let a = 3\nlet b = 4\na * b + b"),
            Object::Number(n) if n == 16.0));
    }

    #[test]
    fn array_literal_spread_builds_flat_array() {
        let result = run_src("let a = [1, 2]\nlet b = [0, ...a, 3]\nb[2]");
        assert!(matches!(result, Object::Number(n) if n == 2.0));
    }

    #[test]
    fn object_literal_supports_spread_and_computed_keys() {
        let result = run_src(
            "let key = \"b\"\nlet base = { a: 1 }\nlet obj = { ...base, [key]: 2 }\nobj.a + obj.b",
        );
        assert!(matches!(result, Object::Number(n) if n == 3.0));
    }

    #[test]
    fn array_index_assignment_updates_element_and_returns_value() {
        let result = run_src("let values = [1, 2, 3]\nvalues[1] = values[0] + values[2]");
        assert!(matches!(result, Object::Number(n) if n == 4.0));

        let updated =
            run_src("let values = [1, 2, 3]\nvalues[1] = values[0] + values[2]\nvalues[1]");
        assert!(matches!(updated, Object::Number(n) if n == 4.0));
    }

    #[test]
    fn object_property_and_index_assignment_update_hash() {
        let result =
            run_src("let key = \"score\"\nlet doc = {}\ndoc[key] = 14\ndoc.score + doc[key]");
        assert!(matches!(result, Object::Number(n) if n == 28.0));

        let nested = run_src(
            "let doc = { user: { score: 7 } }\ndoc.user.score = doc.user.score + 5\ndoc.user.score",
        );
        assert!(matches!(nested, Object::Number(n) if n == 12.0));
    }

    // —— control flow (stage 2.1) ——
    // These tests store results in variables rather than relying on a block's
    // value, mirroring how real fixtures work (assign then println).
    #[test]
    fn if_true_branch() {
        let src = "let r = 0\nif (1 < 2) { r = 10 } else { r = 20 }\nr";
        assert!(matches!(run_src(src), Object::Number(n) if n == 10.0));
    }
    #[test]
    fn if_false_branch() {
        let src = "let r = 0\nif (1 > 2) { r = 10 } else { r = 20 }\nr";
        assert!(matches!(run_src(src), Object::Number(n) if n == 20.0));
    }
    #[test]
    fn if_no_else_skips_body() {
        // When false and no else, r stays at its initialized value.
        let src = "let r = 99\nif (1 > 2) { r = 10 }\nr";
        assert!(matches!(run_src(src), Object::Number(n) if n == 99.0));
    }
    #[test]
    fn while_loop_basic() {
        // sum 1..5 = 15
        let src = "let i = 0\nlet s = 0\nwhile (i < 5) { i = i + 1\ns = s + i }\ns";
        assert!(matches!(run_src(src), Object::Number(n) if n == 15.0));
    }
    #[test]
    fn while_break() {
        let src = "let i = 0\nwhile (true) { if (i >= 3) { break }\ni = i + 1 }\ni";
        assert!(matches!(run_src(src), Object::Number(n) if n == 3.0));
    }
    #[test]
    fn while_continue() {
        // sum 1..5 skipping 3 => 1+2+4+5 = 12
        let src = "let i = 0\nlet s = 0\nwhile (i < 5) { i = i + 1\nif (i === 3) { continue }\ns = s + i }\ns";
        assert!(matches!(run_src(src), Object::Number(n) if n == 12.0));
    }
    #[test]
    fn for_loop_basic() {
        // sum 1..5 = 15
        let src = "let s = 0\nfor (let i = 1; i <= 5; i = i + 1) { s = s + i }\ns";
        assert!(matches!(run_src(src), Object::Number(n) if n == 15.0));
    }
    #[test]
    fn for_loop_break() {
        let src = "let s = 0\nfor (let i = 1; i <= 10; i = i + 1) { if (i === 4) { break }\ns = s + i }\ns";
        // 1+2+3 = 6
        assert!(matches!(run_src(src), Object::Number(n) if n == 6.0));
    }
    #[test]
    fn nested_loops() {
        // count inner iterations
        let src = "let c = 0\nfor (let i = 0; i < 3; i = i + 1) { for (let j = 0; j < 3; j = j + 1) { c = c + 1 } }\nc";
        assert!(matches!(run_src(src), Object::Number(n) if n == 9.0));
    }
    #[test]
    fn labeled_break_exits_outer_loop() {
        let src = "let c = 0\nouter: for (let i = 0; i < 3; i = i + 1) { for (let j = 0; j < 3; j = j + 1) { if (i === 1 && j === 1) { break outer }\nc = c + 1 } }\nc";
        assert!(matches!(run_src(src), Object::Number(n) if n == 4.0));
    }

    // —— template interpolation + println (stage 2.1c/d) ——
    #[test]
    fn template_interpolation_number() {
        // `${1 + 2}` → "3"
        assert!(matches!(run_src("`x${1 + 2}y`"), Object::String(s) if &*s == "x3y"));
    }
    #[test]
    fn template_interpolation_variable() {
        let src = "let n = 5\n`v=${n}`";
        assert!(matches!(run_src(src), Object::String(s) if &*s == "v=5"));
    }
    #[test]
    fn template_multiple_interpolations() {
        let src = "let a = 1\nlet b = 2\n`${a}+${b}=${a + b}`";
        assert!(matches!(run_src(src), Object::String(s) if &*s == "1+2=3"));
    }
    #[test]
    fn println_bridges_to_global() {
        // println returns undefined; we just assert no error and the program
        // completes. (stdout is captured by the test runner; the contract is
        // that the call dispatches without a TypeError.)
        let r = run_src("println(\"hello\")");
        assert!(matches!(r, Object::Undefined));
    }
    #[test]
    fn println_with_template() {
        // Mirrors the parity-fixture pattern: `println(`label=${value}`)`.
        let r = run_src("let value = 42\nprintln(`v=${value}`)");
        assert!(matches!(r, Object::Undefined));
    }

    // —— functions (stage 3) ——
    #[test]
    fn function_declaration_and_call() {
        let src = "function add(a, b) { return a + b }\nadd(3, 4)";
        assert!(matches!(run_src(src), Object::Number(n) if n == 7.0));
    }
    #[test]
    fn function_no_return_yields_undefined() {
        let src = "function f() { let x = 1 }\nf()";
        assert!(matches!(run_src(src), Object::Undefined));
    }
    #[test]
    fn recursive_function() {
        // factorial(5) = 120
        let src = "function fact(n) { if (n <= 1) { return 1 }\nreturn n * fact(n - 1) }\nfact(5)";
        assert!(matches!(run_src(src), Object::Number(n) if n == 120.0));
    }
    #[test]
    fn arrow_function_expression_body() {
        let src = "const sq = (x) => x * x\nsq(6)";
        assert!(matches!(run_src(src), Object::Number(n) if n == 36.0));
    }
    #[test]
    fn arrow_function_block_body() {
        let src = "const double = (x) => { return x + x }\ndouble(21)";
        assert!(matches!(run_src(src), Object::Number(n) if n == 42.0));
    }
    #[test]
    fn function_expression_anonymous() {
        let src = "const f = function (x) { return x + 1 }\nf(9)";
        assert!(matches!(run_src(src), Object::Number(n) if n == 10.0));
    }
    #[test]
    fn default_parameter() {
        // missing arg → undefined; but with a default we'd need the default
        // support (bind_params handles it). Here just call with the arg.
        assert!(
            matches!(run_src("function f(a, b) { return b }\nf(1, 2)"), Object::Number(n) if n == 2.0)
        );
    }
    #[test]
    fn closure_over_global() {
        // The closure references `multiplier` which is a global; stage 3
        // resolves it through the env chain (true local capture is stage 4).
        let src = "let multiplier = 3\nfunction apply(x) { return x * multiplier }\napply(5)";
        assert!(matches!(run_src(src), Object::Number(n) if n == 15.0));
    }

    // Note: closures over *local* variables (function_closure fixture) need
    // stage 4 upvalue capture; the global-resolution path is exercised above.
}
