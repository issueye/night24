//! The `console` global object and its methods.

use std::rc::Rc;

use crate::object::*;

/// Build the `console` global object.
pub fn console_object() -> Object {
    let hash = Rc::new(std::cell::RefCell::new(HashData::default()));
    let make = |name: &str, to_stderr: bool, prefix: &str| -> (String, Object) {
        let prefix = prefix.to_string();
        let func: BuiltinFn = Rc::new(move |ctx, args| {
            let parts: Vec<String> = args.iter().map(|a| a.inspect()).collect();
            let line = if prefix.is_empty() {
                parts.join(" ")
            } else {
                format!("{}{}", prefix, parts.join(" "))
            };
            if to_stderr {
                ctx.vm().push_stderr(line);
            } else {
                ctx.vm().push_stdout(line);
            }
            Object::Undefined
        });
        (
            name.into(),
            Object::Builtin(Rc::new(Builtin {
                name: format!("console.{}", name),
                func,
                extra: None,
            })),
        )
    };
    {
        let mut h = hash.borrow_mut();
        for (name, val) in [
            make("log", false, ""),
            make("info", false, "[INFO] "),
            make("warn", true, "[WARN] "),
            make("error", true, "[ERROR] "),
            make("debug", false, "[DEBUG] "),
            make("trace", true, ""),
        ] {
            h.set(name, val);
        }
    }
    Object::Hash(hash)
}
