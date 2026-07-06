use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{new_error, num_obj, strict_equal, CallContext, HashData, Object};

pub(crate) fn test_module() -> Object {
    module(vec![
        ("test", native("test.test", test_test)),
        ("it", native("test.it", test_test)),
        ("describe", native("test.describe", test_describe)),
        ("expect", native("test.expect", test_expect)),
        ("run", native("test.run", test_run)),
        // Lifecycle hooks (W4.2): each accepts a function registered into the
        // current hook queues. Hooks run before/after each case (each) or
        // around the whole run (all), invoked by `run`.
        ("beforeEach", native("test.beforeEach", hook_before_each)),
        ("afterEach", native("test.afterEach", hook_after_each)),
        ("beforeAll", native("test.beforeAll", hook_before_all)),
        ("afterAll", native("test.afterAll", hook_after_all)),
    ])
}

#[derive(Clone)]
enum TestNode {
    // Reserved: `describe()` currently only executes the body and registers
    // nested cases flatly into TEST_ROOT; grouping into Suite nodes (and
    // surfacing suite names in `run` output) is planned but not yet wired.
    #[allow(dead_code)]
    Suite {
        name: String,
        children: Vec<TestNode>,
    },
    Case {
        name: String,
        func: Object,
    },
}

thread_local! {
    static TEST_ROOT: std::cell::RefCell<Vec<TestNode>> = const { std::cell::RefCell::new(Vec::new()) };
    static EXPECT_FAILS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    // Lifecycle hooks (W4.2). Each queue holds script functions registered via
    // beforeEach/afterEach/beforeAll/afterAll; `run` invokes them around cases.
    static BEFORE_EACH: std::cell::RefCell<Vec<Object>> = const { std::cell::RefCell::new(Vec::new()) };
    static AFTER_EACH: std::cell::RefCell<Vec<Object>> = const { std::cell::RefCell::new(Vec::new()) };
    static BEFORE_ALL: std::cell::RefCell<Vec<Object>> = const { std::cell::RefCell::new(Vec::new()) };
    static AFTER_ALL: std::cell::RefCell<Vec<Object>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn register_hook(
    queue: &'static std::thread::LocalKey<std::cell::RefCell<Vec<Object>>>,
    func: Object,
) {
    queue.with(|q| q.borrow_mut().push(func));
}

pub(crate) fn hook_before_each(ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(value) if is_callable(value) => {
            register_hook(&BEFORE_EACH, value.clone());
            Object::Undefined
        }
        _ => new_error(ctx.pos.clone(), "beforeEach requires a function"),
    }
}

pub(crate) fn hook_after_each(ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(value) if is_callable(value) => {
            register_hook(&AFTER_EACH, value.clone());
            Object::Undefined
        }
        _ => new_error(ctx.pos.clone(), "afterEach requires a function"),
    }
}

pub(crate) fn hook_before_all(ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(value) if is_callable(value) => {
            register_hook(&BEFORE_ALL, value.clone());
            Object::Undefined
        }
        _ => new_error(ctx.pos.clone(), "beforeAll requires a function"),
    }
}

pub(crate) fn hook_after_all(ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(value) if is_callable(value) => {
            register_hook(&AFTER_ALL, value.clone());
            Object::Undefined
        }
        _ => new_error(ctx.pos.clone(), "afterAll requires a function"),
    }
}

pub(crate) fn test_test(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "test.test", args);
    let name = match reader.required_string(0, "name") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let func = match args.get(1) {
        Some(v) => v.clone(),
        None => return new_error(ctx.pos.clone(), "test requires name and function"),
    };
    TEST_ROOT.with(|r| {
        r.borrow_mut().push(TestNode::Case { name, func });
    });
    Object::Undefined
}

pub(crate) fn test_describe(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "test.describe", args);
    let name = match reader.required_string(0, "name") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let func = match args.get(1) {
        Some(Object::Function(_) | Object::Closure(_)) => args[1].clone(),
        Some(v) => v.clone(),
        None => return new_error(ctx.pos.clone(), "describe requires name and function"),
    };
    // Execute the describe body synchronously; nested test()/it() calls
    // register into the current suite.
    let _ = call_script_function(&func, ctx.env, &[]);
    let _ = name;
    Object::Undefined
}

pub(crate) fn test_expect(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match args.first() {
        Some(v) => v.clone(),
        None => return new_error(ctx.pos.clone(), "expect requires a value"),
    };
    let expectation = ObjectBuilder::new().set("__value__", value).into_shared();

    // Each matcher closure captures its own clone of the Rc so the original
    // can still be returned.
    let e1 = expectation.clone();
    expectation.borrow_mut().set(
        "toBe",
        native("test.expect.toBe", move |ctx, args| {
            expect_matcher(ctx, &e1, args, ExpectOp::Be)
        }),
    );
    let e2 = expectation.clone();
    expectation.borrow_mut().set(
        "toEqual",
        native("test.expect.toEqual", move |ctx, args| {
            expect_matcher(ctx, &e2, args, ExpectOp::Equal)
        }),
    );
    let e3 = expectation.clone();
    expectation.borrow_mut().set(
        "toBeTruthy",
        native("test.expect.toBeTruthy", move |ctx, _args| {
            expect_truthy(ctx, &e3, true)
        }),
    );
    let e4 = expectation.clone();
    expectation.borrow_mut().set(
        "toBeFalsy",
        native("test.expect.toBeFalsy", move |ctx, _args| {
            expect_truthy(ctx, &e4, false)
        }),
    );
    // —— W4.2 additional matchers ——
    let e5 = expectation.clone();
    expectation.borrow_mut().set(
        "toBeNull",
        native("test.expect.toBeNull", move |ctx, _args| {
            expect_kind(ctx, &e5, |v| matches!(v, Object::Null), "null")
        }),
    );
    let e6 = expectation.clone();
    expectation.borrow_mut().set(
        "toBeUndefined",
        native("test.expect.toBeUndefined", move |ctx, _args| {
            expect_kind(ctx, &e6, |v| matches!(v, Object::Undefined), "undefined")
        }),
    );
    let e7 = expectation.clone();
    expectation.borrow_mut().set(
        "toBeDefined",
        native("test.expect.toBeDefined", move |ctx, _args| {
            expect_kind(ctx, &e7, |v| !matches!(v, Object::Undefined), "defined")
        }),
    );
    let e8 = expectation.clone();
    expectation.borrow_mut().set(
        "toHaveLength",
        native("test.expect.toHaveLength", move |ctx, args| {
            expect_length(ctx, &e8, args)
        }),
    );
    let e9 = expectation.clone();
    expectation.borrow_mut().set(
        "toContain",
        native("test.expect.toContain", move |ctx, args| {
            expect_contain(ctx, &e9, args)
        }),
    );
    let e10 = expectation.clone();
    expectation.borrow_mut().set(
        "toBeGreaterThan",
        native("test.expect.toBeGreaterThan", move |ctx, args| {
            expect_compare(ctx, &e10, args, true)
        }),
    );
    let e11 = expectation.clone();
    expectation.borrow_mut().set(
        "toBeLessThan",
        native("test.expect.toBeLessThan", move |ctx, args| {
            expect_compare(ctx, &e11, args, false)
        }),
    );
    let e12 = expectation.clone();
    expectation.borrow_mut().set(
        "toThrow",
        native("test.expect.toThrow", move |ctx, _args| {
            expect_throw(ctx, &e12)
        }),
    );
    // Negation chain: `expect(x).not.toBe(y)`. `not` returns a copy of the
    // expectation with the `__negate__` flag set; matchers consult it.
    // NOTE: read `__value__` BEFORE handing off the Rc, since `expectation` is
    // still being mutated by the surrounding `set` calls (same Rc).
    let not_value = expectation
        .borrow()
        .get("__value__")
        .cloned()
        .unwrap_or(Object::Undefined);
    expectation
        .borrow_mut()
        .set("not", build_negated_chain(not_value));
    Object::Hash(expectation)
}

/// Build a negated view of an expectation: a fresh expectation carrying the
/// original value, whose matchers invert their pass/fail result. Used by
/// `.not`. (Takes the value by clone to avoid borrowing the source Rc.)
fn build_negated_chain(value: Object) -> Object {
    let neg = ObjectBuilder::new()
        .set("__value__", value)
        .set("__negate__", Object::Boolean(true))
        .into_shared();

    let n1 = neg.clone();
    neg.borrow_mut().set(
        "toBe",
        native("test.expect.not.toBe", move |ctx, args| {
            negate_matcher_result(expect_matcher(ctx, &n1, args, ExpectOp::Be))
        }),
    );
    let n2 = neg.clone();
    neg.borrow_mut().set(
        "toEqual",
        native("test.expect.not.toEqual", move |ctx, args| {
            negate_matcher_result(expect_matcher(ctx, &n2, args, ExpectOp::Equal))
        }),
    );
    let n3 = neg.clone();
    neg.borrow_mut().set(
        "toBeTruthy",
        native("test.expect.not.toBeTruthy", move |ctx, _args| {
            negate_matcher_result(expect_truthy(ctx, &n3, true))
        }),
    );
    let n4 = neg.clone();
    neg.borrow_mut().set(
        "toBeFalsy",
        native("test.expect.not.toBeFalsy", move |ctx, _args| {
            negate_matcher_result(expect_truthy(ctx, &n4, false))
        }),
    );
    let n5 = neg.clone();
    neg.borrow_mut().set(
        "toBeNull",
        native("test.expect.not.toBeNull", move |ctx, _args| {
            negate_matcher_result(expect_kind(ctx, &n5, |v| matches!(v, Object::Null), "null"))
        }),
    );
    let n6 = neg.clone();
    neg.borrow_mut().set(
        "toContain",
        native("test.expect.not.toContain", move |ctx, args| {
            negate_matcher_result(expect_contain(ctx, &n6, args))
        }),
    );
    Object::Hash(neg)
}

/// Invert a matcher outcome: Undefined (pass) becomes an Error (fail), and an
/// Error (fail) becomes Undefined (pass). Used by the `.not` chain.
fn negate_matcher_result(result: Object) -> Object {
    match result {
        Object::Error(_) => Object::Undefined,
        Object::Undefined => new_error(
            crate::ast::Position::default(),
            "expect().not: matcher unexpectedly passed",
        ),
        other => other,
    }
}

pub(crate) enum ExpectOp {
    Be,
    Equal,
}

pub(crate) fn expect_matcher(
    ctx: &mut CallContext,
    expectation: &Rc<RefCell<HashData>>,
    args: &[Object],
    op: ExpectOp,
) -> Object {
    let actual = expectation
        .borrow()
        .get("__value__")
        .cloned()
        .unwrap_or(Object::Undefined);
    let expected = match args.first() {
        Some(v) => v.clone(),
        None => return new_error(ctx.pos.clone(), "matcher requires an expected value"),
    };
    let passed = match op {
        ExpectOp::Be => strict_equal(&actual, &expected),
        ExpectOp::Equal => deep_equal(&actual, &expected),
    };
    if passed {
        Object::Undefined
    } else {
        let label = match op {
            ExpectOp::Be => "to be",
            ExpectOp::Equal => "to equal",
        };
        new_error(
            ctx.pos.clone(),
            format!(
                "Expected {} {} {}",
                actual.inspect(),
                label,
                expected.inspect()
            ),
        )
    }
}

pub(crate) fn expect_truthy(
    ctx: &mut CallContext,
    expectation: &Rc<RefCell<HashData>>,
    expect_truthy: bool,
) -> Object {
    let actual = expectation
        .borrow()
        .get("__value__")
        .cloned()
        .unwrap_or(Object::Undefined);
    let truthy = is_truthy(&actual);
    let passed = if expect_truthy { truthy } else { !truthy };
    if passed {
        Object::Undefined
    } else {
        let label = if expect_truthy { "truthy" } else { "falsy" };
        new_error(
            ctx.pos.clone(),
            format!("Expected {} to be {}", actual.inspect(), label),
        )
    }
}

/// Match on a predicate over the value's variant (null/undefined/defined).
pub(crate) fn expect_kind(
    ctx: &mut CallContext,
    expectation: &Rc<RefCell<HashData>>,
    pred: impl Fn(&Object) -> bool,
    label: &str,
) -> Object {
    let actual = expectation
        .borrow()
        .get("__value__")
        .cloned()
        .unwrap_or(Object::Undefined);
    if pred(&actual) {
        Object::Undefined
    } else {
        new_error(
            ctx.pos.clone(),
            format!("Expected {} to be {}", actual.inspect(), label),
        )
    }
}

/// `.toHaveLength(n)`: strings, arrays, and objects with a numeric `length`.
pub(crate) fn expect_length(
    ctx: &mut CallContext,
    expectation: &Rc<RefCell<HashData>>,
    args: &[Object],
) -> Object {
    let actual = expectation
        .borrow()
        .get("__value__")
        .cloned()
        .unwrap_or(Object::Undefined);
    let expected = match args.first() {
        Some(Object::Number(n)) => *n as i64,
        _ => return new_error(ctx.pos.clone(), "toHaveLength requires a number"),
    };
    let len = match &actual {
        Object::String(s) => s.chars().count() as i64,
        Object::Array(a) => a.borrow().elements.len() as i64,
        Object::Hash(h) => match h.borrow().get("length") {
            Some(Object::Number(n)) => *n as i64,
            _ => return new_error(ctx.pos.clone(), "toHaveLength: value has no length"),
        },
        _ => return new_error(ctx.pos.clone(), "toHaveLength: value has no length"),
    };
    if len == expected {
        Object::Undefined
    } else {
        new_error(
            ctx.pos.clone(),
            format!("Expected length {} to be {}", len, expected),
        )
    }
}

/// `.toContain(item)`: arrays contain the item (deep-equal); strings contain
/// the substring.
pub(crate) fn expect_contain(
    ctx: &mut CallContext,
    expectation: &Rc<RefCell<HashData>>,
    args: &[Object],
) -> Object {
    let actual = expectation
        .borrow()
        .get("__value__")
        .cloned()
        .unwrap_or(Object::Undefined);
    let needle = match args.first() {
        Some(v) => v.clone(),
        None => return new_error(ctx.pos.clone(), "toContain requires an item"),
    };
    let contained = match (&actual, &needle) {
        (Object::Array(a), _) => a.borrow().elements.iter().any(|e| deep_equal(e, &needle)),
        (Object::String(s), Object::String(sub)) => s.contains(sub.as_str()),
        (Object::Hash(h), Object::String(key)) => h.borrow().contains(key),
        _ => false,
    };
    if contained {
        Object::Undefined
    } else {
        new_error(
            ctx.pos.clone(),
            format!(
                "Expected {} to contain {}",
                actual.inspect(),
                needle.inspect()
            ),
        )
    }
}

/// `.toBeGreaterThan(n)` / `.toBeLessThan(n)` (greater=true / greater=false).
pub(crate) fn expect_compare(
    ctx: &mut CallContext,
    expectation: &Rc<RefCell<HashData>>,
    args: &[Object],
    greater: bool,
) -> Object {
    let actual = expectation
        .borrow()
        .get("__value__")
        .cloned()
        .unwrap_or(Object::Undefined);
    let rhs = match args.first() {
        Some(Object::Number(n)) => *n,
        _ => return new_error(ctx.pos.clone(), "comparison requires a number"),
    };
    let lhs = match actual {
        Object::Number(n) => n,
        _ => return new_error(ctx.pos.clone(), "comparison requires a numeric value"),
    };
    let passed = if greater { lhs > rhs } else { lhs < rhs };
    if passed {
        Object::Undefined
    } else {
        let label = if greater { "greater than" } else { "less than" };
        new_error(
            ctx.pos.clone(),
            format!("Expected {} to be {} {}", lhs, label, rhs),
        )
    }
}

/// `.toThrow()`: the value must be a function; calling it should throw (return
/// an Error). Pass if it throws, fail otherwise.
pub(crate) fn expect_throw(ctx: &mut CallContext, expectation: &Rc<RefCell<HashData>>) -> Object {
    let func = expectation
        .borrow()
        .get("__value__")
        .cloned()
        .unwrap_or(Object::Undefined);
    let is_fn = is_callable(&func);
    if !is_fn {
        return new_error(ctx.pos.clone(), "toThrow: expect value must be a function");
    }
    let result = call_script_function(&func, ctx.env, &[]);
    if matches!(result, Object::Error(_)) {
        Object::Undefined
    } else {
        new_error(
            ctx.pos.clone(),
            "Expected function to throw, but it did not",
        )
    }
}

pub(crate) fn test_run(ctx: &mut CallContext, _args: &[Object]) -> Object {
    let mut total = 0usize;
    let mut passed = 0usize;
    let mut failed = 0usize;

    // beforeAll hooks run once before the whole suite.
    BEFORE_ALL.with(|q| {
        for func in q.borrow().iter() {
            let _ = call_script_function(func, ctx.env, &[]);
        }
    });

    TEST_ROOT.with(|r| {
        let nodes = r.borrow_mut().clone();
        for node in &nodes {
            if let TestNode::Case { name, func } = node {
                total += 1;
                EXPECT_FAILS.with(|f| f.borrow_mut().clear());
                // beforeEach runs before each case.
                BEFORE_EACH.with(|q| {
                    for hook in q.borrow().iter() {
                        let _ = call_script_function(hook, ctx.env, &[]);
                    }
                });
                let result = call_script_function(func, ctx.env, &[]);
                // afterEach runs after each case (regardless of pass/fail).
                AFTER_EACH.with(|q| {
                    for hook in q.borrow().iter() {
                        let _ = call_script_function(hook, ctx.env, &[]);
                    }
                });
                let failed_here = matches!(result, Object::Error(_))
                    || EXPECT_FAILS.with(|f| !f.borrow().is_empty());
                if failed_here {
                    failed += 1;
                    let _ = name;
                } else {
                    passed += 1;
                }
            }
        }
    });

    // afterAll hooks run once after the whole suite.
    AFTER_ALL.with(|q| {
        for func in q.borrow().iter() {
            let _ = call_script_function(func, ctx.env, &[]);
        }
    });

    ObjectBuilder::new()
        .set("total", num_obj(total as f64))
        .set("passed", num_obj(passed as f64))
        .set("failed", num_obj(failed as f64))
        .build()
}
