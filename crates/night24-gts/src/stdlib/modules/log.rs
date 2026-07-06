use super::super::helpers::*;
use crate::object::{str_obj, CallContext, Object};

pub(crate) fn log_module() -> Object {
    module(vec![
        ("format", native("log.format", log_format)),
        ("debug", native("log.debug", log_debug)),
        ("info", native("log.info", log_info)),
        ("warn", native("log.warn", log_warn)),
        ("error", native("log.error", log_error)),
    ])
}

pub(crate) fn log_format(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "log.format", args);
    let level = match reader.required_string(0, "level") {
        Ok(level) => level,
        Err(err) => return err,
    };
    let message = match reader.required_string(1, "message") {
        Ok(message) => message,
        Err(err) => return err,
    };
    str_obj(format_log_line(&level, &message))
}

pub(crate) fn log_debug(ctx: &mut CallContext, args: &[Object]) -> Object {
    log_named(ctx, args, "log.debug", "debug")
}

pub(crate) fn log_info(ctx: &mut CallContext, args: &[Object]) -> Object {
    log_named(ctx, args, "log.info", "info")
}

pub(crate) fn log_warn(ctx: &mut CallContext, args: &[Object]) -> Object {
    log_named(ctx, args, "log.warn", "warn")
}

pub(crate) fn log_error(ctx: &mut CallContext, args: &[Object]) -> Object {
    log_named(ctx, args, "log.error", "error")
}

pub(crate) fn log_named(ctx: &mut CallContext, args: &[Object], name: &str, level: &str) -> Object {
    let reader = ArgReader::new(ctx, name, args);
    match reader.required_string(0, "message") {
        Ok(message) => str_obj(format_log_line(level, &message)),
        Err(err) => err,
    }
}

pub(crate) fn format_log_line(level: &str, message: &str) -> String {
    format!("[{}] {}", level.to_ascii_uppercase(), message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_log_line_uppercases_level() {
        assert_eq!(
            format_log_line("warn", "disk nearly full"),
            "[WARN] disk nearly full"
        );
    }
}

// ---------------------------------------------------------------------------
// encoding/csv: small RFC4180-ish parser/writer with Go-compatible options.
// ---------------------------------------------------------------------------
