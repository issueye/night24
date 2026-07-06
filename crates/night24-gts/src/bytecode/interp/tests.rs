use super::*;
use crate::bytecode::compile;
use crate::lexer::Lexer;
use crate::object::str_obj;
use crate::object::Environment;
use crate::object::HashData;
use crate::object::Promise;
use crate::object::PromiseState;
use crate::object::VirtualMachine;
use crate::parser::Parser;
use std::cell::RefCell;
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

fn state_for_budget_tests() -> VmState<'static> {
    let chunk = Box::leak(Box::new(Chunk::new()));
    let vm = VirtualMachine::new();
    vm.set_instruction_limit(TIMEOUT_CHECK_INTERVAL - 1);
    let env = Environment::new_root(vm.clone());
    VmState::new(chunk, env, Vec::new(), vm)
}

#[test]
fn execution_budget_is_sampled_at_interval_boundary() {
    let mut state = state_for_budget_tests();
    for _ in 0..(TIMEOUT_CHECK_INTERVAL - 1) {
        assert!(state.check_execution_budget().is_none());
    }

    let err = state
        .check_execution_budget()
        .expect("instruction limit should trip at sample boundary");
    let Object::Error(data) = err else {
        panic!("expected instruction limit error");
    };
    let data = data.borrow();
    assert_eq!(data.name, "Error");
    assert!(data.message.starts_with("MemoryLimitError:"));
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
fn return_runs_finally_before_leaving_function() {
    let result = run_src(
        r#"
            let log = "";
            function run() {
                try {
                    log = log + "try:";
                    return "value";
                } finally {
                    log = log + "finally:";
                }
            }
            let value = run();
            log + value;
            "#,
    );
    assert!(matches!(result, Object::String(s) if s.as_ref() == "try:finally:value"));
}

#[test]
fn finally_return_overrides_try_return() {
    let result = run_src(
        r#"
            function run() {
                try {
                    return "try";
                } finally {
                    return "finally";
                }
            }
            run();
            "#,
    );
    assert!(matches!(result, Object::String(s) if s.as_ref() == "finally"));
}

#[test]
fn break_runs_finally_before_exiting_loop() {
    let result = run_src(
        r#"
            let log = "";
            while (true) {
                try {
                    log = log + "try:";
                    break;
                } finally {
                    log = log + "finally:";
                }
            }
            log + "done";
            "#,
    );
    assert!(matches!(result, Object::String(s) if s.as_ref() == "try:finally:done"));
}

#[test]
fn continue_runs_finally_before_next_iteration() {
    let result = run_src(
        r#"
            let log = "";
            let i = 0;
            while (i < 2) {
                i = i + 1;
                try {
                    log = log + "try:";
                    continue;
                } finally {
                    log = log + "finally:";
                }
            }
            log + "done";
            "#,
    );
    assert!(matches!(
        result,
        Object::String(s) if s.as_ref() == "try:finally:try:finally:done"
    ));
}

#[test]
fn nested_finally_runs_inner_then_outer_on_return() {
    let result = run_src(
        r#"
            let log = "";
            function run() {
                try {
                    try {
                        log = log + "try:";
                        return "value";
                    } finally {
                        log = log + "inner:";
                    }
                } finally {
                    log = log + "outer:";
                }
            }
            let value = run();
            log + value;
            "#,
    );
    assert!(matches!(result, Object::String(s) if s.as_ref() == "try:inner:outer:value"));
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
fn finally_return_override_matches_treewalker() {
    let (tree, bytecode) = run_src_tree_and_bytecode(
        r#"
            function run() {
                try {
                    return "try";
                } finally {
                    return "finally";
                }
            }
            run();
            "#,
    );
    assert!(matches!(tree, Object::String(s) if s.as_ref() == "finally"));
    assert!(matches!(bytecode, Object::String(s) if s.as_ref() == "finally"));
}

#[test]
fn finally_throw_override_matches_treewalker() {
    let (tree, bytecode) = run_src_tree_and_bytecode(
        r#"
            try {
                throw "first";
            } finally {
                throw "second";
            }
            "#,
    );
    assert_error_same(tree, bytecode);
}

#[test]
fn finally_loop_control_matches_treewalker() {
    let (tree, bytecode) = run_src_tree_and_bytecode(
        r#"
            let log = "";
            let i = 0;
            while (i < 2) {
                i = i + 1;
                try {
                    log = log + "try:";
                    continue;
                } finally {
                    log = log + "finally:";
                }
            }
            log + "done";
            "#,
    );
    assert!(matches!(
        tree,
        Object::String(s) if s.as_ref() == "try:finally:try:finally:done"
    ));
    assert!(matches!(
        bytecode,
        Object::String(s) if s.as_ref() == "try:finally:try:finally:done"
    ));
}

#[test]
fn finally_return_overrides_break_matches_treewalker() {
    let (tree, bytecode) = run_src_tree_and_bytecode(
        r#"
            function run() {
                while (true) {
                    try {
                        break;
                    } finally {
                        return "finally";
                    }
                }
                return "after";
            }
            run();
            "#,
    );
    assert!(matches!(tree, Object::String(s) if s.as_ref() == "finally"));
    assert!(matches!(bytecode, Object::String(s) if s.as_ref() == "finally"));
}

#[test]
fn finally_return_overrides_continue_matches_treewalker() {
    let (tree, bytecode) = run_src_tree_and_bytecode(
        r#"
            function run() {
                let i = 0;
                while (i < 2) {
                    i = i + 1;
                    try {
                        continue;
                    } finally {
                        return "finally";
                    }
                }
                return "after";
            }
            run();
            "#,
    );
    assert!(matches!(tree, Object::String(s) if s.as_ref() == "finally"));
    assert!(matches!(bytecode, Object::String(s) if s.as_ref() == "finally"));
}

#[test]
fn catch_binding_with_finally_matches_treewalker() {
    let (tree, bytecode) = run_src_tree_and_bytecode(
        r#"
            let label = "";
            try {
                throw "boom";
            } catch (err) {
                label = err.name + ":" + err.message;
            } finally {
                label = label + ":finally";
            }
            label;
            "#,
    );
    assert!(matches!(tree, Object::String(s) if s.as_ref() == "Error:boom:finally"));
    assert!(matches!(bytecode, Object::String(s) if s.as_ref() == "Error:boom:finally"));
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
fn const_opcode_missing_constant_is_vmerror() {
    let mut chunk = Chunk::new();
    chunk.write_op(Opcode::Const, Position::default());
    chunk.write_u16(0, Position::default());
    chunk.write_op(Opcode::Return, Position::default());

    let result = run_chunk(chunk);
    let Object::Error(data) = result else {
        panic!("expected VMError, got {result:?}");
    };
    let data = data.borrow();
    assert_eq!(data.name, "Error");
    assert!(data.message.contains("CONST constant index 0 out of range"));
}

#[test]
fn store_name_non_string_operand_is_vmerror() {
    let mut chunk = Chunk::new();
    let value = chunk.add_constant(Object::Number(1.0));
    let bad_name = chunk.add_constant(Object::Number(2.0));
    chunk.write_op(Opcode::Const, Position::default());
    chunk.write_u16(value, Position::default());
    chunk.write_op(Opcode::StoreName, Position::default());
    chunk.write_u16(bad_name, Position::default());
    chunk.write_op(Opcode::Return, Position::default());

    let result = run_chunk(chunk);
    let Object::Error(data) = result else {
        panic!("expected VMError, got {result:?}");
    };
    let data = data.borrow();
    assert_eq!(data.name, "Error");
    assert!(data.message.contains("STORE_NAME operand is not a string"));
}

#[test]
fn store_typed_name_missing_type_annotation_is_vmerror() {
    let mut chunk = Chunk::new();
    let value = chunk.add_constant(Object::Number(1.0));
    let name = chunk.add_constant(str_obj("x".to_string()));
    chunk.write_op(Opcode::Const, Position::default());
    chunk.write_u16(value, Position::default());
    chunk.write_op(Opcode::StoreTypedName, Position::default());
    chunk.write_u16(name, Position::default());
    chunk.write_u16(0, Position::default());
    chunk.write_op(Opcode::Return, Position::default());

    let result = run_chunk(chunk);
    let Object::Error(data) = result else {
        panic!("expected VMError, got {result:?}");
    };
    let data = data.borrow();
    assert_eq!(data.name, "Error");
    assert!(data.message.contains("missing type annotation 0"));
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
fn load_upvalue_missing_operand_is_vmerror() {
    let mut chunk = Chunk::new();
    chunk.write_op(Opcode::LoadUpvalue, Position::default());

    let result = run_chunk_with_upvalues(chunk, Vec::new());

    let Object::Error(data) = result else {
        panic!("expected VMError, got {result:?}");
    };
    let data = data.borrow();
    assert_eq!(data.name, "Error");
    assert!(data.message.contains("LOAD_UPVALUE missing operand"));
}

#[test]
fn jump_if_false_missing_operand_is_vmerror() {
    let mut chunk = Chunk::new();
    chunk.write_op(Opcode::JumpIfFalse, Position::default());

    let result = run_chunk(chunk);

    let Object::Error(data) = result else {
        panic!("expected VMError, got {result:?}");
    };
    let data = data.borrow();
    assert_eq!(data.name, "Error");
    assert!(data.message.contains("JUMP_IF_FALSE missing operand"));
}

#[test]
fn closure_missing_proto_is_vmerror() {
    let mut chunk = Chunk::new();
    chunk.write_op(Opcode::Closure, Position::default());
    chunk.write_u16(0, Position::default());

    let result = run_chunk(chunk);

    let Object::Error(data) = result else {
        panic!("expected VMError, got {result:?}");
    };
    let data = data.borrow();
    assert_eq!(data.name, "Error");
    assert!(data.message.contains("missing function prototype 0"));
}

#[test]
fn closure_missing_operand_is_vmerror() {
    let mut chunk = Chunk::new();
    chunk.write_op(Opcode::Closure, Position::default());

    let result = run_chunk(chunk);

    let Object::Error(data) = result else {
        panic!("expected VMError, got {result:?}");
    };
    let data = data.borrow();
    assert_eq!(data.name, "Error");
    assert!(data.message.contains("CLOSURE missing operand"));
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
    let src = "let s = 0; let a = void (s = s + 5); let o = {x:1}; let b = delete o.x; [a, b, s]";
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

    let updated = run_src("let values = [1, 2, 3]\nvalues[1] = values[0] + values[2]\nvalues[1]");
    assert!(matches!(updated, Object::Number(n) if n == 4.0));
}

#[test]
fn object_property_and_index_assignment_update_hash() {
    let result = run_src("let key = \"score\"\nlet doc = {}\ndoc[key] = 14\ndoc.score + doc[key]");
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
    let src =
        "let s = 0\nfor (let i = 1; i <= 10; i = i + 1) { if (i === 4) { break }\ns = s + i }\ns";
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
