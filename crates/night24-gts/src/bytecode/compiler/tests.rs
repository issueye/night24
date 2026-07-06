use super::super::emit::operand_width;
use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn compile_src(src: &str) -> Chunk {
    let lexer = Lexer::new(src);
    let mut parser = Parser::new(lexer, "t.gs");
    let program = parser.parse_program();
    assert!(
        program.errors.is_empty(),
        "parse errors: {:?}",
        program.errors
    );
    compile(&program).expect("compile should succeed for stage-0 inputs")
}

#[test]
fn compiles_literal_number() {
    let chunk = compile_src("42");
    assert_eq!(chunk.code[0], Opcode::Const as u8);
    assert!(matches!(chunk.constants[0], Object::Number(n) if n == 42.0));
    assert_eq!(*chunk.code.last().unwrap(), Opcode::Return as u8);
}

#[test]
fn compiles_add_post_order() {
    // 1 + 2 + 3  ⇒  CONST 1, CONST 2, ADD, CONST 3, ADD, RETURN
    let chunk = compile_src("1 + 2 + 3");
    // Walk the instruction stream properly (don't flat-filter bytes: a
    // CONST operand byte could collide with an opcode value).
    let spine = decode_opcode_spine(&chunk);
    let expected = vec![
        Opcode::Const,
        Opcode::Const,
        Opcode::Add,
        Opcode::Const,
        Opcode::Add,
        Opcode::Return,
    ];
    assert_eq!(spine, expected);
}

/// Decode just the opcode bytes, skipping each instruction's operands.
fn decode_opcode_spine(chunk: &Chunk) -> Vec<Opcode> {
    let mut out = Vec::new();
    let mut ip = 0;
    while ip < chunk.code.len() {
        let op = Opcode::from_byte(chunk.code[ip]).expect("valid opcode");
        out.push(op);
        ip += 1;
        ip += operand_width(op) as usize;
    }
    out
}

#[test]
fn rejects_unsupported_node() {
    // Unsupported nodes must be refused rather than silently miscompiled.
    // Computed-member postfix update is not yet supported.
    let lexer = Lexer::new("let a = 1; a.b++");
    let mut parser = Parser::new(lexer, "t.gs");
    let program = parser.parse_program();
    let result = compile(&program);
    assert!(
        result.is_err(),
        "unsupported postfix update on member should not compile"
    );
}

#[test]
fn compiles_throw_opcode() {
    let chunk = compile_src("throw \"boom\";");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::Throw));
}

#[test]
fn compiles_await_opcode() {
    let chunk = compile_src("await value;");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::Await));
}

#[test]
fn compiles_prefix_identity_opcode() {
    let chunk = compile_src("+42");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::Identity));
}

#[test]
fn compiles_ternary_branch_opcodes() {
    let chunk = compile_src("true ? 1 : 2");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::JumpIfFalse));
    assert!(spine.contains(&Opcode::Jump));
}

#[test]
fn compiles_optional_chain_nullish_checks() {
    let chunk = compile_src("let obj = null; obj?.name;");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::Dup));
    assert!(spine.contains(&Opcode::JumpIfTrue));
    assert!(spine.contains(&Opcode::GetProperty));
}

#[test]
fn compiles_nullish_coalescing_short_circuit() {
    let chunk = compile_src("null ?? 42");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::Dup));
    assert!(spine.contains(&Opcode::JumpIfTrue));
    assert!(spine.contains(&Opcode::Jump));
}

#[test]
fn compiles_prefix_increment_uses_load_assign() {
    // ++x → LOAD_NAME x ; CONST 1 ; ADD ; ASSIGN_NAME x
    let chunk = compile_src("let x = 1; ++x");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::LoadName));
    assert!(spine.contains(&Opcode::Add));
    assert!(spine.contains(&Opcode::AssignName));
    // Prefix must NOT emit a Pop (the new value is the result as-is).
    let last_meaningful = spine
        .iter()
        .copied()
        .filter(|op| !matches!(op, Opcode::Const | Opcode::Return | Opcode::ReturnNull))
        .last();
    assert!(matches!(last_meaningful, Some(Opcode::AssignName)));
}

#[test]
fn compiles_postfix_increment_preserves_old_value() {
    // x++ → LOAD_NAME x ; DUP ; CONST 1 ; ADD ; ASSIGN_NAME x ; POP
    // The trailing Pop drops the new value, leaving the old as the result.
    let chunk = compile_src("let x = 1; x++");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::Dup));
    assert!(spine.contains(&Opcode::Add));
    assert!(spine.contains(&Opcode::Pop));
}

#[test]
fn compiles_export_star_uses_import_export_all() {
    // `export * from "m"` → IMPORT_MODULE ; EXPORT_ALL
    let chunk = compile_src("export * from \"./m\"");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::ImportModule));
    assert!(spine.contains(&Opcode::ExportAll));
}

#[test]
fn compiles_dynamic_import_as_promise() {
    // `import("./m")` → IMPORT_MODULE ; WRAP_RESOLVED_PROMISE
    let chunk = compile_src("import(\"./m\")");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::ImportModule));
    assert!(spine.contains(&Opcode::WrapResolvedPromise));
}

#[test]
fn compiles_template_interpolation_to_string_concat() {
    let chunk = compile_src("`hi ${name}`");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::ToString));
    assert!(spine.contains(&Opcode::Concat));
}

#[test]
fn compiles_super_constructor_call_with_this_receiver() {
    let chunk = compile_src("function Child() { super(); }");
    let child = chunk
        .protos
        .iter()
        .find(|proto| proto.name == "Child")
        .expect("Child proto");
    let child_chunk = child.chunk.borrow().clone().expect("Child chunk");
    let spine = decode_opcode_spine(&child_chunk);
    assert!(spine.contains(&Opcode::LoadThis));
    assert!(spine.contains(&Opcode::SuperMethod));
    assert!(spine.contains(&Opcode::Call));
}

#[test]
fn records_async_function_proto() {
    let chunk = compile_src("async function answer() { return 42; }");
    assert_eq!(chunk.protos.len(), 1);
    assert!(chunk.protos[0].is_async);
}

#[test]
fn records_async_arrow_proto() {
    let chunk = compile_src("let answer = async (value) => value;");
    assert_eq!(chunk.protos.len(), 1);
    assert!(chunk.protos[0].is_async);
    assert!(chunk.protos[0].lexical_this);
}

#[test]
fn compiles_typed_declaration_metadata() {
    let chunk = compile_src("let value: number = 1;");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::StoreTypedName));
    assert_eq!(chunk.types.len(), 1);
    assert_eq!(chunk.types[0].to_string(), "number");
}

#[test]
fn compiles_import_bindings_from_module_object() {
    let chunk = compile_src(r#"import def, { named, other as alias } from "mod";"#);
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::ImportModule));
    assert!(spine.contains(&Opcode::GetProperty));
    assert!(spine.contains(&Opcode::StoreName));
    assert!(chunk
        .constants
        .iter()
        .any(|value| matches!(value, Object::String(s) if s.as_ref() == "mod")));
}

#[test]
fn compiles_export_declarations_to_export_name() {
    let chunk = compile_src("export const value = 42; export { value as answer };");
    let spine = decode_opcode_spine(&chunk);
    assert!(spine.contains(&Opcode::ExportName));
    assert!(chunk
        .constants
        .iter()
        .any(|value| matches!(value, Object::String(s) if s.as_ref() == "answer")));
}

#[test]
fn records_try_protected_region() {
    let chunk = compile_src("try { 1; } catch (err) { 2; } finally { 3; }");
    assert_eq!(chunk.protected_regions.len(), 2);
    let region = &chunk.protected_regions[0];
    assert!(region.try_start < region.try_end);
    assert!(region.try_end < region.handler_ip);
    assert!(region.finally_ip.is_some());
    assert!(region.finally_ip.unwrap() > region.handler_ip);
    assert_eq!(region.catch_binding_slot, None);
    assert_eq!(
        Opcode::from_byte(chunk.code[region.handler_ip as usize]),
        Some(Opcode::StoreName)
    );
    let catch_region = &chunk.protected_regions[1];
    assert_eq!(catch_region.handler_ip, region.finally_ip.unwrap());
}

#[test]
fn function_proto_records_resolved_upvalues() {
    let chunk =
        compile_src("function outer() { let x = 1; function inner() { return x; } return inner; }");
    let outer = chunk
        .protos
        .iter()
        .find(|proto| proto.name == "outer")
        .expect("outer proto");
    let outer_chunk = outer.chunk.borrow().clone().expect("outer chunk");
    let inner = outer_chunk
        .protos
        .iter()
        .find(|proto| proto.name == "inner")
        .expect("inner proto");

    assert_eq!(inner.upvalue_desc.len(), 1);
    assert_eq!(inner.upvalue_desc[0].name, "x");
    assert_eq!(
        inner.upvalue_desc[0].source,
        crate::bytecode::closure::UpvalueSource::LocalSlot(1)
    );
}
