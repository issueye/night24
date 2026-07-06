use crate::lexer::Lexer;
use crate::object::{Environment, Object, VirtualMachine};
use crate::parser::Parser;

use super::eval_program;

fn run_tree_walker(src: &str) -> Object {
    let lexer = Lexer::new(src);
    let mut parser = Parser::new(lexer, "tree-walker-finally.gs");
    let program = parser.parse_program();
    assert!(
        program.errors.is_empty(),
        "parse errors: {:?}",
        program.errors
    );

    let vm = VirtualMachine::new();
    let env = Environment::new_root(vm);
    eval_program(&program, &env)
}

#[test]
fn tree_walker_finally_return_overrides_try_return() {
    let result = run_tree_walker(
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
fn tree_walker_finally_throw_overrides_original_throw() {
    let result = run_tree_walker(
        r#"
            try {
                throw "original";
            } finally {
                throw "finally";
            }
            "#,
    );

    let Object::Error(data) = result else {
        panic!("expected runtime error, got {result:?}");
    };
    assert_eq!(data.borrow().message, "finally");
}

#[test]
fn tree_walker_finally_runs_before_function_return() {
    let result = run_tree_walker(
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
fn tree_walker_finally_runs_before_break() {
    let result = run_tree_walker(
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
fn tree_walker_finally_runs_before_continue() {
    let result = run_tree_walker(
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

    assert!(matches!(result, Object::String(s) if s.as_ref() == "try:finally:try:finally:done"));
}

#[test]
fn tree_walker_string_search_reports_character_index_after_multibyte_prefix() {
    let result = run_tree_walker(
        r#"
            let plain = "\u4f60a".search("a");
            let regex = "\u00e9x".search(/x/);
            plain * 10 + regex;
            "#,
    );

    assert!(matches!(result, Object::Number(n) if (n - 11.0).abs() < f64::EPSILON));
}
