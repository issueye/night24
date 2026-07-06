use super::super::helpers::*;
use crate::object::{str_obj, CallContext, Object};

pub(crate) fn color_module() -> Object {
    module(vec![
        ("ansi", native("color.ansi", color_ansi)),
        ("strip", native("color.strip", color_strip)),
        ("stripAnsi", native("color.stripAnsi", color_strip)),
        ("red", native("color.red", color_red)),
        ("green", native("color.green", color_green)),
        ("yellow", native("color.yellow", color_yellow)),
        ("blue", native("color.blue", color_blue)),
        ("magenta", native("color.magenta", color_magenta)),
        ("cyan", native("color.cyan", color_cyan)),
        ("bold", native("color.bold", color_bold)),
        ("dim", native("color.dim", color_dim)),
        ("underline", native("color.underline", color_underline)),
        ("reset", str_obj("\x1b[0m")),
    ])
}

pub(crate) fn color_ansi(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "color.ansi", args);
    let text = match reader.required_string(0, "text") {
        Ok(text) => text,
        Err(err) => return err,
    };
    let code = match reader.required_number(1, "code") {
        Ok(code) => code,
        Err(err) => return err,
    };
    ansi_wrap(&text, code as i64)
}

pub(crate) fn color_strip(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "color.strip", args);
    match reader.required_string(0, "text") {
        Ok(text) => str_obj(strip_ansi(&text)),
        Err(err) => err,
    }
}

pub(crate) fn color_red(ctx: &mut CallContext, args: &[Object]) -> Object {
    color_named(ctx, args, "color.red", 31)
}

pub(crate) fn color_green(ctx: &mut CallContext, args: &[Object]) -> Object {
    color_named(ctx, args, "color.green", 32)
}

pub(crate) fn color_yellow(ctx: &mut CallContext, args: &[Object]) -> Object {
    color_named(ctx, args, "color.yellow", 33)
}

pub(crate) fn color_blue(ctx: &mut CallContext, args: &[Object]) -> Object {
    color_named(ctx, args, "color.blue", 34)
}

pub(crate) fn color_magenta(ctx: &mut CallContext, args: &[Object]) -> Object {
    color_named(ctx, args, "color.magenta", 35)
}

pub(crate) fn color_cyan(ctx: &mut CallContext, args: &[Object]) -> Object {
    color_named(ctx, args, "color.cyan", 36)
}

pub(crate) fn color_bold(ctx: &mut CallContext, args: &[Object]) -> Object {
    color_named(ctx, args, "color.bold", 1)
}

pub(crate) fn color_dim(ctx: &mut CallContext, args: &[Object]) -> Object {
    color_named(ctx, args, "color.dim", 2)
}

pub(crate) fn color_underline(ctx: &mut CallContext, args: &[Object]) -> Object {
    color_named(ctx, args, "color.underline", 4)
}

pub(crate) fn color_named(ctx: &mut CallContext, args: &[Object], name: &str, code: i64) -> Object {
    let reader = ArgReader::new(ctx, name, args);
    match reader.required_string(0, "text") {
        Ok(text) => ansi_wrap(&text, code),
        Err(err) => err,
    }
}

pub(crate) fn ansi_wrap(text: &str, code: i64) -> Object {
    str_obj(format!("\x1b[{}m{}\x1b[0m", code, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_as_string(object: Object) -> String {
        match object {
            Object::String(value) => value.to_string(),
            _ => panic!("expected string"),
        }
    }

    #[test]
    fn ansi_wrap_adds_reset_sequence() {
        assert_eq!(
            object_as_string(ansi_wrap("night24", 31)),
            "\x1b[31mnight24\x1b[0m"
        );
    }

    #[test]
    fn strip_ansi_removes_color_sequences() {
        assert_eq!(strip_ansi("\x1b[31mnight24\x1b[0m"), "night24");
    }
}

// ---------------------------------------------------------------------------
// diff: line-oriented comparison helpers.
// ---------------------------------------------------------------------------
