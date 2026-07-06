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

use crate::ast::Position;
use crate::object::{new_error, EnvRef, Object, VirtualMachine};
use std::collections::BTreeMap;
use std::rc::Rc;

use super::chunk::Chunk;
use super::closure::UpvalueSource;
use super::interp_helpers::{
    apply_binary_stack_op, apply_unary_stack_op, array_slice_from_stack, assign_name, await_stack,
    build_class_to_stack, call_spread_stack, call_stack,
    close_open_upvalues_from as close_open_upvalues_range, closure_from_proto, construct_stack,
    export_all_stack, export_name_stack, get_index_stack, get_property_stack, import_module_stack,
    iter_keys_stack, iter_next_stack, iter_values_stack, len_stack, load_global, load_local,
    load_name, load_upvalue, new_array_from_stack, new_object_to_stack, push_packed_arg,
    read_byte_operand_with_pos, read_const_operand, read_function_proto_operand, read_name_operand,
    read_string_operand, read_type_operand, read_u16_operand_with_pos, read_u32_operand,
    read_u32_operand_with_pos, read_usize_operand_with_pos, set_index_stack, set_property_stack,
    spread_stack, stack_underflow, store_global, store_local, store_name, store_typed_name,
    store_upvalue, throw_match_error_from_stack, throw_value, to_string_stack, type_of_stack,
    unwind_to_handler as unwind_stack_to_handler, wrap_resolved_promise_stack,
};
use super::opcode::Opcode;
use super::upvalue::Upvalue;
use crate::evaluator::builtins::register_globals;

pub(crate) use super::interp_helpers::value_matches_type_annotation;

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
            if let Some(err) = self.check_execution_budget() {
                return err;
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

    /// Sample timeout and instruction-limit guards without paying position
    /// lookup cost on every instruction.
    fn check_execution_budget(&mut self) -> Option<Object> {
        self.instruction_count = self.instruction_count.wrapping_add(1);
        if !self
            .instruction_count
            .is_multiple_of(TIMEOUT_CHECK_INTERVAL)
        {
            return None;
        }

        // D4.1: check instruction limit (resource guard) alongside timeout.
        if self
            .vm
            .is_instruction_limit_exceeded(self.instruction_count)
        {
            let pos = self.chunk.position_at(self.ip);
            if let Some(err) = self.vm.check_instruction_limit(self.instruction_count, pos) {
                return Some(err);
            }
        }

        // Timeout check (existing).
        if self.vm.is_deadline_exceeded() {
            let pos = self.chunk.position_at(self.ip);
            if let Some(timeout) = self.vm.check_timeout(pos) {
                return Some(timeout);
            }
        }

        None
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
                self.stack
                    .push(read_const_operand(self.chunk, &mut self.ip, "CONST")?);
            }
            Opcode::Pop => {
                self.stack.pop();
            }
            Opcode::Dup => {
                let v = self
                    .stack
                    .last()
                    .cloned()
                    .ok_or_else(|| stack_underflow(self.chunk.position_at(self.ip - 1)))?;
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
                let target = read_u32_operand(self.chunk, &mut self.ip, "JUMP")? as usize;
                self.ip = target;
            }
            Opcode::Loop => {
                // Backwards jump (loop back-edge). Same encoding as Jump.
                let target = read_u32_operand(self.chunk, &mut self.ip, "LOOP")? as usize;
                self.ip = target;
            }
            Opcode::JumpIfFalse => {
                let (target, pos) =
                    read_u32_operand_with_pos(self.chunk, &mut self.ip, "JUMP_IF_FALSE")?;
                let cond = self.stack.pop().ok_or_else(|| stack_underflow(pos))?;
                if !cond.is_truthy() {
                    self.ip = target as usize;
                }
            }
            Opcode::JumpIfTrue => {
                let (target, pos) =
                    read_u32_operand_with_pos(self.chunk, &mut self.ip, "JUMP_IF_TRUE")?;
                let cond = self.stack.pop().ok_or_else(|| stack_underflow(pos))?;
                if cond.is_truthy() {
                    self.ip = target as usize;
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
                to_string_stack(&mut self.stack, pos)?;
            }
            Opcode::TypeOf => {
                let pos = self.chunk.position_at(self.ip - 1);
                type_of_stack(&mut self.stack, pos)?;
            }
            Opcode::Await => {
                let pos = self.chunk.position_at(self.ip - 1);
                await_stack(&mut self.stack, &self.env, pos)?;
            }
            Opcode::ImportModule => {
                let (source, pos) = read_string_operand(self.chunk, &mut self.ip, "IMPORT_MODULE")?;
                import_module_stack(&mut self.stack, &self.env, &source, pos)?;
            }
            Opcode::ExportName => {
                let (name, pos) = read_string_operand(self.chunk, &mut self.ip, "EXPORT_NAME")?;
                export_name_stack(&mut self.stack, &self.env, name, pos)?;
            }
            Opcode::ArraySliceFrom => {
                // Stack: [..., array, start]. Pop start, pop array, push tail.
                let pos = self.chunk.position_at(self.ip);
                array_slice_from_stack(&mut self.stack, pos)?;
            }
            Opcode::ExportAll => {
                // Pop the source module's exports object; copy every property
                // into the current module's exports (`export * from "..."`).
                let pos = self.chunk.position_at(self.ip);
                export_all_stack(&mut self.stack, &self.env, pos)?;
            }
            Opcode::WrapResolvedPromise => {
                // Pop a value and push a resolved Promise wrapping it (for
                // dynamic `import()`).
                let pos = self.chunk.position_at(self.ip);
                wrap_resolved_promise_stack(&mut self.stack, pos)?;
            }
            Opcode::Call => {
                let (encoded_arg_count, pos) =
                    read_u16_operand_with_pos(self.chunk, &mut self.ip, "CALL")?;
                let has_this_receiver = encoded_arg_count & 0x8000 != 0;
                let arg_count = (encoded_arg_count & 0x7fff) as usize;
                call_stack(
                    &mut self.stack,
                    &self.env,
                    arg_count,
                    has_this_receiver,
                    pos,
                )?;
            }
            Opcode::PushArg => {
                let pos = self.chunk.position_at(self.ip - 1);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| stack_underflow(pos.clone()))?;
                push_packed_arg(&self.stack, value, false, pos)?;
            }
            Opcode::Spread => {
                let pos = self.chunk.position_at(self.ip - 1);
                spread_stack(&mut self.stack, pos)?;
            }
            Opcode::CallSpread => {
                let pos = self.chunk.position_at(self.ip - 1);
                call_spread_stack(&mut self.stack, &self.env, pos)?;
            }
            Opcode::New => {
                let (arg_count, pos) =
                    read_usize_operand_with_pos(self.chunk, &mut self.ip, "NEW")?;
                construct_stack(&mut self.stack, &self.env, arg_count, pos)?;
            }
            Opcode::NewClass => {
                let (class_idx, pos) =
                    read_usize_operand_with_pos(self.chunk, &mut self.ip, "NEW_CLASS")?;
                build_class_to_stack(&mut self.stack, self.chunk, &self.env, class_idx, pos)?;
            }
            Opcode::Closure => {
                let (proto, _pos) =
                    read_function_proto_operand(self.chunk, &mut self.ip, "CLOSURE")?;
                let upvalues = self.capture_proto_upvalues(&proto)?;
                self.stack
                    .push(closure_from_proto(proto, upvalues, self.env.clone()));
            }
            Opcode::NewArray => {
                let (count, pos) =
                    read_usize_operand_with_pos(self.chunk, &mut self.ip, "NEW_ARRAY")?;
                new_array_from_stack(&mut self.stack, count, pos)?;
            }
            Opcode::NewObject => {
                new_object_to_stack(&mut self.stack);
            }
            Opcode::SetProperty => {
                let (name, pos) = read_string_operand(self.chunk, &mut self.ip, "SET_PROPERTY")?;
                set_property_stack(&mut self.stack, &name, pos)?;
            }
            Opcode::GetProperty => {
                let (name, pos) = read_string_operand(self.chunk, &mut self.ip, "GET_PROPERTY")?;
                get_property_stack(&mut self.stack, &name, pos)?;
            }
            Opcode::GetIndex => {
                let pos = self.chunk.position_at(self.ip - 1);
                get_index_stack(&mut self.stack, pos)?;
            }
            Opcode::SetIndex => {
                let pos = self.chunk.position_at(self.ip - 1);
                set_index_stack(&mut self.stack, pos)?;
            }
            Opcode::IterKeys => {
                let pos = self.chunk.position_at(self.ip - 1);
                iter_keys_stack(&mut self.stack, pos)?;
            }
            Opcode::IterValues => {
                let pos = self.chunk.position_at(self.ip - 1);
                iter_values_stack(&mut self.stack, &self.env, pos)?;
            }
            Opcode::IterNext => {
                let pos = self.chunk.position_at(self.ip - 1);
                iter_next_stack(&mut self.stack, &self.env, pos)?;
            }
            Opcode::Len => {
                let pos = self.chunk.position_at(self.ip - 1);
                len_stack(&mut self.stack, pos)?;
            }

            // —— variables (routed through the environment name table) ——
            Opcode::LoadName => {
                let (name, pos) = read_string_operand(self.chunk, &mut self.ip, "LOAD_NAME")?;
                load_name(&mut self.stack, &self.env, &name, pos)?;
            }
            Opcode::StoreName => {
                let (name, is_const, pos) =
                    read_name_operand(self.chunk, &mut self.ip, "STORE_NAME")?;
                store_name(&mut self.stack, &self.env, name, is_const, pos)?;
            }
            Opcode::StoreTypedName => {
                let (name, is_const, pos) =
                    read_name_operand(self.chunk, &mut self.ip, "STORE_TYPED_NAME")?;
                let type_anno =
                    read_type_operand(self.chunk, &mut self.ip, "STORE_TYPED_NAME", pos.clone())?;
                store_typed_name(&mut self.stack, &self.env, name, is_const, type_anno, pos)?;
            }
            Opcode::AssignName => {
                let (name, pos) = read_string_operand(self.chunk, &mut self.ip, "ASSIGN_NAME")?;
                assign_name(&mut self.stack, &self.env, &name, pos)?;
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
                let (name, pos) = read_string_operand(self.chunk, &mut self.ip, "LOAD_GLOBAL")?;
                load_global(&mut self.stack, &self.env, &name, pos)?;
            }
            Opcode::StoreGlobal => {
                let (name, is_const, pos) =
                    read_name_operand(self.chunk, &mut self.ip, "STORE_GLOBAL")?;
                // The high bit is accepted for parity with `StoreName`'s
                // encoding (the const flag), but the global table has no
                // per-binding const marker. Declarations still go through
                // `StoreName` (which records const-ness in the environment), so
                // a `StoreGlobal` is only ever emitted for assignment to an
                // existing global; `is_const` is therefore informational here.
                let _ = is_const;
                store_global(&mut self.stack, &self.env, name, pos)?;
            }
            Opcode::LoadLocal => {
                let (slot, pos) =
                    read_byte_operand_with_pos(self.chunk, &mut self.ip, "LOAD_LOCAL")?;
                load_local(&mut self.stack, slot, pos)?;
            }
            Opcode::StoreLocal => {
                let (slot, pos) =
                    read_byte_operand_with_pos(self.chunk, &mut self.ip, "STORE_LOCAL")?;
                store_local(&mut self.stack, slot, pos)?;
            }
            Opcode::SuperMethod => {
                let (name, pos) = read_string_operand(self.chunk, &mut self.ip, "SUPER_METHOD")?;
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
                let (index, pos) =
                    read_byte_operand_with_pos(self.chunk, &mut self.ip, "LOAD_UPVALUE")?;
                load_upvalue(&mut self.stack, &self.current_upvalues, index, pos)?;
            }
            Opcode::StoreUpvalue => {
                let (index, pos) =
                    read_byte_operand_with_pos(self.chunk, &mut self.ip, "STORE_UPVALUE")?;
                store_upvalue(&mut self.stack, &self.current_upvalues, index, pos)?;
            }
            Opcode::Throw => {
                let pos = self.chunk.position_at(instruction_ip);
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| stack_underflow(pos.clone()))?;
                return Err(throw_value(value, pos));
            }
            Opcode::ThrowMatchError => {
                let pos = self.chunk.position_at(instruction_ip);
                return Err(throw_match_error_from_stack(&mut self.stack, pos)?);
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
        apply_binary_stack_op(&mut self.stack, op, self.chunk.position_at(self.ip - 1))
    }

    /// Pop one operand, apply a unary op, push the result.
    fn un_op(&mut self, op: &'static str) -> Result<(), Object> {
        apply_unary_stack_op(&mut self.stack, op, self.chunk.position_at(self.ip - 1))
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
        close_open_upvalues_range(&mut self.open_upvalues, &self.stack, first_slot);
    }

    fn unwind_to_handler(&mut self, error: Object) -> bool {
        if let Some(handler_ip) =
            unwind_stack_to_handler(self.chunk, self.last_ip, &mut self.stack, error)
        {
            self.ip = handler_ip;
            true
        } else {
            false
        }
    }
}

/// One-step control-flow outcome.
enum Flow {
    Continue,
    Return(Object),
}

#[cfg(test)]
mod tests;
