use std::env;
use std::io::{IsTerminal, Write};

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, HashData, Object};

pub(crate) fn terminal_module() -> Object {
    module(vec![
        ("isTTY", native("terminal.isTTY", terminal_is_tty)),
        ("size", native("terminal.size", terminal_size)),
        (
            "capabilities",
            native("terminal.capabilities", terminal_capabilities),
        ),
        ("read", native("terminal.read", terminal_read)),
        ("write", native("terminal.write", terminal_write)),
        ("writeln", native("terminal.writeln", terminal_writeln)),
        (
            "renderFrame",
            native("terminal.renderFrame", terminal_render_frame),
        ),
        (
            "setRawMode",
            native("terminal.setRawMode", terminal_set_raw_mode),
        ),
        ("start", native("terminal.start", terminal_start)),
        ("clear", native("terminal.clear", terminal_clear_screen)),
        (
            "clearScreen",
            native("terminal.clearScreen", terminal_clear_screen),
        ),
        (
            "clearLine",
            native("terminal.clearLine", terminal_clear_line),
        ),
        ("moveTo", native("terminal.moveTo", terminal_move_to)),
        ("setTitle", native("terminal.setTitle", terminal_set_title)),
        ("style", native("terminal.style", terminal_style)),
        (
            "hyperlink",
            native("terminal.hyperlink", terminal_hyperlink),
        ),
        // New: real crossterm-backed screen/cursor control.
        (
            "enterAlternateScreen",
            native(
                "terminal.enterAlternateScreen",
                terminal_enter_alternate_screen,
            ),
        ),
        (
            "leaveAlternateScreen",
            native(
                "terminal.leaveAlternateScreen",
                terminal_leave_alternate_screen,
            ),
        ),
        (
            "hideCursor",
            native("terminal.hideCursor", terminal_hide_cursor),
        ),
        (
            "showCursor",
            native("terminal.showCursor", terminal_show_cursor),
        ),
    ])
}

pub(crate) fn terminal_is_tty(_ctx: &mut CallContext, args: &[Object]) -> Object {
    let stream = match args.first() {
        Some(Object::String(value)) => value.to_ascii_lowercase(),
        _ => "stdout".to_string(),
    };
    let interactive = match stream.as_str() {
        "stdin" | "in" => std::io::stdin().is_terminal(),
        "stderr" | "err" => std::io::stderr().is_terminal(),
        _ => std::io::stdout().is_terminal(),
    };
    bool_obj(interactive)
}

pub(crate) fn terminal_size(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let (cols, rows) = terminal_dimensions();
    module(vec![
        ("cols", num_obj(cols as f64)),
        ("rows", num_obj(rows as f64)),
    ])
}

pub(crate) fn terminal_capabilities(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    // Raw mode and a virtual terminal only apply when stdout is a real TTY.
    // Under a pipe/CI (non-TTY), entering raw mode has no effect and would
    // mislead callers; report false so they can fall back to plain I/O.
    let is_tty = std::io::stdout().is_terminal();
    module(vec![
        ("clearScrollback", bool_obj(is_tty)),
        ("alternateScreen", bool_obj(is_tty)),
        (
            "resizeEvents",
            bool_obj(is_tty && crossterm::terminal::supports_keyboard_enhancement().is_ok()),
        ),
        ("virtualTerminal", bool_obj(is_tty)),
        ("rawMode", bool_obj(is_tty)),
    ])
}

/// Read one line from stdin (blocking, line-buffered). Returns the line
/// without the trailing newline, or "" at EOF.
pub(crate) fn terminal_read(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    use std::io::BufRead;
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) => str_obj(""),
        Ok(_) => str_obj(line.trim_end_matches(['\n', '\r'])),
        Err(_) => str_obj(""),
    }
}

pub(crate) fn terminal_write(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(value) = args.first() else {
        return new_error(ctx.pos.clone(), "terminal.write requires text");
    };
    let text = object_to_text(value);
    match std::io::stdout().write_all(text.as_bytes()) {
        Ok(_) => num_obj(text.len() as f64),
        Err(e) => new_error(ctx.pos.clone(), format!("terminal.write: {}", e)),
    }
}

pub(crate) fn terminal_writeln(ctx: &mut CallContext, args: &[Object]) -> Object {
    let text = args.first().map(object_to_text).unwrap_or_default() + "\n";
    match std::io::stdout().write_all(text.as_bytes()) {
        Ok(_) => num_obj(text.len() as f64),
        Err(e) => new_error(ctx.pos.clone(), format!("terminal.write: {}", e)),
    }
}

pub(crate) fn terminal_render_frame(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(frame) = args.first() else {
        return new_error(ctx.pos.clone(), "terminal.renderFrame requires frame");
    };
    let mut text = String::new();
    let full = ArgReader::new(ctx, "terminal.renderFrame", args)
        .object_view(1)
        .and_then(|opts| ObjectView::new(&opts).bool("full"))
        .unwrap_or(false);
    if full {
        text.push_str("\x1b[2J");
    }
    text.push_str("\x1b[H");
    text.push_str(&object_to_text(frame));
    match std::io::stdout().write_all(text.as_bytes()) {
        Ok(_) => {
            let _ = std::io::stdout().flush();
            num_obj(text.len() as f64)
        }
        Err(e) => new_error(ctx.pos.clone(), format!("terminal.renderFrame: {}", e)),
    }
}

// Raw-mode refcount so nested enable/disable calls stay balanced.
// (A plain comment, not a doc-comment: doc-comments on a macro are reported as
// "unused" by rustc since they can't attach to macro-expanded items.)
thread_local! {
    static RAW_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Enable raw mode (crossterm). Returns `{raw: bool, restore: fn}` where
/// `restore` disables raw mode. Idempotent / refcounted.
pub(crate) fn terminal_set_raw_mode(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let enabled = enable_raw_mode_refcount();
    module(vec![
        ("raw", bool_obj(enabled)),
        (
            "restore",
            native("terminal.restoreRawMode", |_ctx, _args| {
                disable_raw_mode_refcount();
                Object::Undefined
            }),
        ),
    ])
}

/// Decrement the raw-mode refcount; disable when it reaches zero.
/// Enable raw mode with reference counting, gated on stdout being a real TTY.
/// Returns `true` if raw mode is now active (or was already active).
/// In a non-TTY (pipe/CI), raw mode has no effect and we report `false` so
/// callers can fall back to plain line-buffered I/O.
fn enable_raw_mode_refcount() -> bool {
    if !std::io::stdout().is_terminal() {
        return false;
    }
    RAW_DEPTH.with(|d| {
        let depth = d.get();
        if depth == 0 {
            if crossterm::terminal::enable_raw_mode().is_ok() {
                d.set(1);
                true
            } else {
                false
            }
        } else {
            d.set(depth + 1);
            true
        }
    })
}

fn disable_raw_mode_refcount() {
    RAW_DEPTH.with(|d| {
        let depth = d.get();
        if depth <= 1 {
            let _ = crossterm::terminal::disable_raw_mode();
            d.set(0);
        } else {
            d.set(depth - 1);
        }
    });
}

/// Start an interactive terminal session: enables raw mode and returns a
/// session object with write/writeln/size/restore/stop methods. `stop` and
/// `restore` both drop out of raw mode.
pub(crate) fn terminal_start(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let active = enable_raw_mode_refcount();
    module(vec![
        ("active", bool_obj(active)),
        ("write", native("terminal.session.write", terminal_write)),
        (
            "writeln",
            native("terminal.session.writeln", terminal_writeln),
        ),
        ("size", native("terminal.session.size", terminal_size)),
        (
            "restore",
            native("terminal.session.restore", |_ctx, _args| {
                disable_raw_mode_refcount();
                Object::Undefined
            }),
        ),
        (
            "stop",
            native("terminal.session.stop", |_ctx, _args| {
                disable_raw_mode_refcount();
                Object::Undefined
            }),
        ),
    ])
}

/// Switch to the alternate screen buffer.
pub(crate) fn terminal_enter_alternate_screen(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen);
    Object::Undefined
}

/// Leave the alternate screen buffer, returning to the main buffer.
pub(crate) fn terminal_leave_alternate_screen(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
    Object::Undefined
}

/// Hide the cursor.
pub(crate) fn terminal_hide_cursor(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Hide);
    Object::Undefined
}

/// Show the cursor.
pub(crate) fn terminal_show_cursor(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Show);
    Object::Undefined
}

pub(crate) fn terminal_clear_screen(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    str_obj("\x1b[2J\x1b[H")
}

pub(crate) fn terminal_clear_line(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    str_obj("\x1b[2K\r")
}

pub(crate) fn terminal_move_to(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "terminal.moveTo", args);
    let row = match reader.required_number(0, "row") {
        Ok(row) => row.max(1.0) as i64,
        Err(err) => return err,
    };
    let col = match reader.required_number(1, "col") {
        Ok(col) => col.max(1.0) as i64,
        Err(err) => return err,
    };
    str_obj(format!("\x1b[{};{}H", row, col))
}

pub(crate) fn terminal_set_title(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "terminal.setTitle", args);
    let title = match reader.required_string(0, "title") {
        Ok(title) => title,
        Err(err) => return err,
    };
    if !std::io::stdout().is_terminal() {
        return Object::Undefined;
    }
    let text = format!("\x1b]0;{}\x07", title);
    match std::io::stdout().write_all(text.as_bytes()) {
        Ok(_) => num_obj(text.len() as f64),
        Err(e) => new_error(ctx.pos.clone(), format!("terminal.setTitle: {}", e)),
    }
}

pub(crate) fn terminal_style(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "terminal.style", args);
    let text = match reader.required_string(0, "text") {
        Ok(text) => text,
        Err(err) => return err,
    };
    let hash = reader.object_view(1);
    str_obj(terminal_style_text(&text, hash.as_deref()))
}

fn terminal_style_text(text: &str, hash: Option<&HashData>) -> String {
    let Some(hash) = hash else {
        return text.to_string();
    };
    let opts = ObjectView::new(hash);
    let mut codes = Vec::<String>::new();
    for (key, code) in [
        ("bold", "1"),
        ("dim", "2"),
        ("underline", "4"),
        ("inverse", "7"),
    ] {
        if matches!(opts.object(key), Some(Object::Boolean(true))) {
            codes.push(code.to_string());
        }
    }
    if let Some(fg) = strict_string_opt(&opts, "fg").or_else(|| strict_string_opt(&opts, "color")) {
        if let Some(code) = terminal_color_code(&fg, false) {
            codes.push(code.to_string());
        }
    }
    if let Some(bg) = strict_string_opt(&opts, "bg") {
        if let Some(code) = terminal_color_code(&bg, true) {
            codes.push(code.to_string());
        }
    }
    if codes.is_empty() {
        text.to_string()
    } else {
        format!("\x1b[{}m{}\x1b[0m", codes.join(";"), text)
    }
}

pub(crate) fn terminal_hyperlink(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "terminal.hyperlink", args);
    let text = match reader.required_string(0, "text") {
        Ok(text) => text,
        Err(err) => return err,
    };
    let url = match reader.required_string(1, "url") {
        Ok(url) => url,
        Err(err) => return err,
    };
    str_obj(terminal_hyperlink_text(&text, &url))
}

fn terminal_hyperlink_text(text: &str, url: &str) -> String {
    format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, text)
}

fn strict_string_opt(opts: &ObjectView<'_>, key: &str) -> Option<String> {
    match opts.object(key) {
        Some(Object::String(value)) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyperlink_wraps_text_with_osc8_sequence() {
        assert_eq!(
            terminal_hyperlink_text("Night24", "https://example.com/night24"),
            "\x1b]8;;https://example.com/night24\x1b\\Night24\x1b]8;;\x1b\\"
        );
    }

    #[test]
    fn style_wraps_text_with_basic_ansi_codes() {
        let style = ObjectBuilder::new()
            .set("bold", Object::Boolean(true))
            .set("fg", str_obj("green"))
            .into_shared();
        let style = style.borrow();

        assert_eq!(
            terminal_style_text("ready", Some(&style)),
            "\x1b[1;32mready\x1b[0m"
        );
    }
}

// ---------------------------------------------------------------------------
// tui: lightweight script-driven terminal UI helpers.
// ---------------------------------------------------------------------------

pub(crate) fn terminal_size_object() -> Object {
    let (cols, rows) = terminal_dimensions();
    ObjectBuilder::new()
        .set("cols", num_obj(cols as f64))
        .set("rows", num_obj(rows as f64))
        .build()
}

pub(crate) fn terminal_cols() -> i32 {
    terminal_dimensions().0
}

pub(crate) fn terminal_rows() -> i32 {
    terminal_dimensions().1
}

fn terminal_dimensions() -> (i32, i32) {
    let cols = env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|v| *v > 0);
    let rows = env::var("LINES")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|v| *v > 0);

    if let (Some(cols), Some(rows)) = (cols, rows) {
        return (cols, rows);
    }

    if let Ok((actual_cols, actual_rows)) = crossterm::terminal::size() {
        if actual_cols > 0 && actual_rows > 0 {
            return (
                cols.unwrap_or(actual_cols as i32),
                rows.unwrap_or(actual_rows as i32),
            );
        }
    }

    (cols.unwrap_or(80), rows.unwrap_or(24))
}
