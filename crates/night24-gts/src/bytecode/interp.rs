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
#[cfg(test)]
use super::interp_helpers::capture_open_upvalue as capture_open_upvalue_ref;
use super::interp_helpers::{
    apply_binary_stack_op, apply_unary_stack_op, array_slice_from_stack, assign_name_from_operand,
    await_stack, build_class_from_operand, call_from_operand, call_spread_stack,
    close_open_upvalues_from as close_open_upvalues_range, conditional_jump_from_stack,
    construct_from_operand, dup_stack, export_all_stack, export_name_from_operand, get_index_stack,
    get_property_from_operand, import_module_from_operand, iter_keys_stack, iter_next_stack,
    iter_values_stack, jump_to_operand, len_stack, load_global_from_operand,
    load_local_from_operand, load_name_from_operand, load_this, load_upvalue_from_operand,
    new_array_from_operand, new_object_to_stack, push_arg_stack, push_closure_from_operand,
    push_const_from_operand, set_index_stack, set_property_from_operand, spread_stack,
    store_global_from_operand, store_local_from_operand, store_name_from_operand,
    store_typed_name_from_operand, store_upvalue_from_operand, super_method_from_operand,
    throw_from_stack, throw_match_error_from_stack, to_string_stack, type_of_stack,
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
        self.last_ip = self.ip;
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
                push_const_from_operand(self.chunk, &mut self.ip, &mut self.stack)?;
            }
            Opcode::Pop => {
                self.stack.pop();
            }
            Opcode::Dup => {
                let pos = self.current_instruction_pos();
                dup_stack(&mut self.stack, pos)?;
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
                jump_to_operand(self.chunk, &mut self.ip, "JUMP")?;
            }
            Opcode::Loop => {
                jump_to_operand(self.chunk, &mut self.ip, "LOOP")?;
            }
            Opcode::JumpIfFalse => {
                conditional_jump_from_stack(
                    self.chunk,
                    &mut self.ip,
                    &mut self.stack,
                    "JUMP_IF_FALSE",
                    false,
                )?;
            }
            Opcode::JumpIfTrue => {
                conditional_jump_from_stack(
                    self.chunk,
                    &mut self.ip,
                    &mut self.stack,
                    "JUMP_IF_TRUE",
                    true,
                )?;
            }

            Opcode::Return => {
                return Ok(self.return_from_stack());
            }
            Opcode::ReturnNull => {
                return Ok(self.return_value(Object::Null));
            }
            Opcode::ToString => {
                let pos = self.current_instruction_pos();
                to_string_stack(&mut self.stack, pos)?;
            }
            Opcode::TypeOf => {
                let pos = self.current_instruction_pos();
                type_of_stack(&mut self.stack, pos)?;
            }
            Opcode::Await => {
                let pos = self.current_instruction_pos();
                await_stack(&mut self.stack, &self.env, pos)?;
            }
            Opcode::ImportModule => {
                import_module_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
            }
            Opcode::ExportName => {
                export_name_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
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
                call_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
            }
            Opcode::PushArg => {
                let pos = self.current_instruction_pos();
                push_arg_stack(&mut self.stack, pos)?;
            }
            Opcode::Spread => {
                let pos = self.current_instruction_pos();
                spread_stack(&mut self.stack, pos)?;
            }
            Opcode::CallSpread => {
                let pos = self.current_instruction_pos();
                call_spread_stack(&mut self.stack, &self.env, pos)?;
            }
            Opcode::New => {
                construct_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
            }
            Opcode::NewClass => {
                build_class_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
            }
            Opcode::Closure => {
                push_closure_from_operand(
                    self.chunk,
                    &mut self.ip,
                    &mut self.stack,
                    &self.env,
                    &mut self.open_upvalues,
                    &self.current_upvalues,
                )?;
            }
            Opcode::NewArray => {
                new_array_from_operand(self.chunk, &mut self.ip, &mut self.stack)?;
            }
            Opcode::NewObject => {
                new_object_to_stack(&mut self.stack);
            }
            Opcode::SetProperty => {
                set_property_from_operand(self.chunk, &mut self.ip, &mut self.stack)?;
            }
            Opcode::GetProperty => {
                get_property_from_operand(self.chunk, &mut self.ip, &mut self.stack)?;
            }
            Opcode::GetIndex => {
                let pos = self.current_instruction_pos();
                get_index_stack(&mut self.stack, pos)?;
            }
            Opcode::SetIndex => {
                let pos = self.current_instruction_pos();
                set_index_stack(&mut self.stack, pos)?;
            }
            Opcode::IterKeys => {
                let pos = self.current_instruction_pos();
                iter_keys_stack(&mut self.stack, pos)?;
            }
            Opcode::IterValues => {
                let pos = self.current_instruction_pos();
                iter_values_stack(&mut self.stack, &self.env, pos)?;
            }
            Opcode::IterNext => {
                let pos = self.current_instruction_pos();
                iter_next_stack(&mut self.stack, &self.env, pos)?;
            }
            Opcode::Len => {
                let pos = self.current_instruction_pos();
                len_stack(&mut self.stack, pos)?;
            }

            // —— variables (routed through the environment name table) ——
            Opcode::LoadName => {
                load_name_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
            }
            Opcode::StoreName => {
                store_name_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
            }
            Opcode::StoreTypedName => {
                store_typed_name_from_operand(
                    self.chunk,
                    &mut self.ip,
                    &mut self.stack,
                    &self.env,
                )?;
            }
            Opcode::AssignName => {
                assign_name_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
            }
            Opcode::LoadThis => {
                load_this(&mut self.stack, &self.env);
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
                load_global_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
            }
            Opcode::StoreGlobal => {
                store_global_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
            }
            Opcode::LoadLocal => {
                load_local_from_operand(self.chunk, &mut self.ip, &mut self.stack)?;
            }
            Opcode::StoreLocal => {
                store_local_from_operand(self.chunk, &mut self.ip, &mut self.stack)?;
            }
            Opcode::SuperMethod => {
                super_method_from_operand(self.chunk, &mut self.ip, &mut self.stack, &self.env)?;
            }
            Opcode::LoadUpvalue => {
                load_upvalue_from_operand(
                    self.chunk,
                    &mut self.ip,
                    &mut self.stack,
                    &self.current_upvalues,
                )?;
            }
            Opcode::StoreUpvalue => {
                store_upvalue_from_operand(
                    self.chunk,
                    &mut self.ip,
                    &mut self.stack,
                    &self.current_upvalues,
                )?;
            }
            Opcode::Throw => {
                let pos = self.current_instruction_pos();
                return Err(throw_from_stack(&mut self.stack, pos)?);
            }
            Opcode::ThrowMatchError => {
                let pos = self.current_instruction_pos();
                return Err(throw_match_error_from_stack(&mut self.stack, pos)?);
            }

            other => {
                return Err(new_error(
                    self.current_instruction_pos(),
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
        let pos = self.current_instruction_pos();
        apply_binary_stack_op(&mut self.stack, op, pos)
    }

    /// Pop one operand, apply a unary op, push the result.
    fn un_op(&mut self, op: &'static str) -> Result<(), Object> {
        let pos = self.current_instruction_pos();
        apply_unary_stack_op(&mut self.stack, op, pos)
    }

    fn current_instruction_pos(&self) -> Position {
        self.chunk.position_at(self.last_ip)
    }

    #[cfg(test)]
    fn capture_open_upvalue(&mut self, slot: usize) -> Rc<Upvalue> {
        capture_open_upvalue_ref(&mut self.open_upvalues, slot)
    }

    fn close_open_upvalues_from(&mut self, first_slot: usize) {
        close_open_upvalues_range(&mut self.open_upvalues, &self.stack, first_slot);
    }

    fn return_from_stack(&mut self) -> Flow {
        let value = self.stack.pop().unwrap_or(Object::Undefined);
        self.return_value(value)
    }

    fn return_value(&mut self, value: Object) -> Flow {
        self.close_open_upvalues_from(0);
        Flow::Return(value)
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
