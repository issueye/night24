//! App runtime: createApp / run / dispatch / render.
//!
//! Preserves the Elm-architecture contract (init / update / view) from the
//! legacy module, but `view` now returns a node tree (built by the `tui.*`
//! constructors) which the flexbox engine lays out and renders into a frame.
//! The crossterm event loop is migrated from `tui_legacy`.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::object::{new_error, num_obj, str_obj, CallContext, HashData, Object};
use crate::stdlib::helpers::{call_script_function, value_to_string};

use super::super::terminal::{terminal_cols, terminal_rows, terminal_size_object};
use super::layout::{layout, Rect};
use super::node::{lookup_node, NodeKind, TuiNode};
use super::render::render_frame;

/// One running app: its spec (init/update/view), current state, and run flags.
#[derive(Clone)]
pub(crate) struct TuiApp {
    spec: Rc<RefCell<HashData>>,
    state: RefCell<Object>,
    running: Cell<bool>,
    stopped: Cell<bool>,
    /// Last rendered frame (for diff-based incremental drawing to avoid flicker).
    last_frame: RefCell<Option<Vec<String>>>,
    last_size: Cell<Option<(i32, i32)>>,
}

thread_local! {
    static TUI_APPS: RefCell<Vec<(usize, Rc<TuiApp>)>> = const { RefCell::new(Vec::new()) };
    static NEXT_APP_ID: Cell<usize> = const { Cell::new(0) };
}

fn next_app_id() -> usize {
    NEXT_APP_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    })
}

/// Register an app and return a marker object (`{__kind:"tuiApp", __id:N}`).
fn app_marker(app: Rc<TuiApp>) -> Object {
    let id = next_app_id();
    let marker = Rc::new(RefCell::new(HashData::default()));
    marker.borrow_mut().set("__kind", str_obj("tuiApp"));
    marker.borrow_mut().set("__id", num_obj(id as f64));
    TUI_APPS.with(|apps| apps.borrow_mut().push((id, app)));
    Object::Hash(marker)
}

/// Recover the app bound to the current call's receiver marker.
fn bound_app(ctx: &CallContext, name: &str) -> Result<Rc<TuiApp>, Object> {
    let Some(Object::Hash(marker)) = ctx.receiver.clone() else {
        return Err(new_error(
            ctx.pos.clone(),
            format!("{name}: missing app receiver"),
        ));
    };
    let id = match marker.borrow().get("__id") {
        Some(Object::Number(n)) => *n as usize,
        _ => {
            return Err(new_error(
                ctx.pos.clone(),
                format!("{name}: invalid app receiver"),
            ))
        }
    };
    TUI_APPS.with(|apps| {
        apps.borrow()
            .iter()
            .find(|(aid, _)| *aid == id)
            .map(|(_, a)| a.clone())
            .ok_or_else(|| new_error(ctx.pos.clone(), format!("{name}: invalid app receiver")))
    })
}

/// Build a bound native function carrying the app marker as `extra`.
fn native_bound(
    name: &str,
    func: impl Fn(&mut CallContext<'_>, &[Object]) -> Object + 'static,
    extra: Object,
) -> Object {
    Object::Builtin(Rc::new(crate::object::Builtin {
        name: name.into(),
        func: Rc::new(func),
        extra: Some(extra),
    }))
}

// ---------------------------------------------------------------------------
// createApp
// ---------------------------------------------------------------------------

pub(crate) fn tui_create_app(ctx: &mut CallContext, args: &[Object]) -> Object {
    let spec = match args.first() {
        Some(Object::Hash(hash)) => hash.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "tui.createApp: spec must be an object"),
        None => return new_error(ctx.pos.clone(), "tui.createApp requires spec"),
    };
    let app = Rc::new(TuiApp {
        spec: spec.clone(),
        state: RefCell::new(Object::Undefined),
        running: Cell::new(false),
        stopped: Cell::new(false),
        last_frame: RefCell::new(None),
        last_size: Cell::new(None),
    });
    if let Some(init_fn) = hash_function(&spec.borrow(), "init") {
        let size = terminal_size_object();
        let result = call_script_function(&init_fn, ctx.env, &[size]);
        if result.is_runtime_error() {
            return result;
        }
        *app.state.borrow_mut() = result;
    } else if let Some(value) = spec.borrow().get("state").cloned() {
        *app.state.borrow_mut() = value;
    }
    app_object(app)
}

fn app_object(app: Rc<TuiApp>) -> Object {
    let obj = Rc::new(RefCell::new(HashData::default()));
    obj.borrow_mut().set("__tuiApp", app_marker(app.clone()));
    obj.borrow_mut().set(
        "dispatch",
        native_bound("tui.app.dispatch", app_dispatch, app_marker(app.clone())),
    );
    obj.borrow_mut().set(
        "render",
        native_bound("tui.app.render", app_render, app_marker(app.clone())),
    );
    obj.borrow_mut().set(
        "run",
        native_bound("tui.app.run", app_run, app_marker(app.clone())),
    );
    obj.borrow_mut().set(
        "stop",
        native_bound("tui.app.stop", app_stop, app_marker(app.clone())),
    );
    obj.borrow_mut().set(
        "state",
        native_bound("tui.app.state", app_state, app_marker(app)),
    );
    Object::Hash(obj)
}

// ---------------------------------------------------------------------------
// App methods
// ---------------------------------------------------------------------------

pub(crate) fn app_dispatch(ctx: &mut CallContext, args: &[Object]) -> Object {
    let app = match bound_app(ctx, "tui.app.dispatch") {
        Ok(app) => app,
        Err(err) => return err,
    };
    let msg = args.first().cloned().unwrap_or(Object::Undefined);
    match do_dispatch(ctx, &app, msg) {
        Ok(()) => app.state.borrow().clone(),
        Err(err) => err,
    }
}

pub(crate) fn app_render(ctx: &mut CallContext, args: &[Object]) -> Object {
    let app = match bound_app(ctx, "tui.app.render") {
        Ok(app) => app,
        Err(err) => return err,
    };
    let size = match args.first() {
        Some(Object::Hash(hash)) => Object::Hash(hash.clone()),
        Some(Object::Null | Object::Undefined) | None => terminal_size_object(),
        Some(_) => return new_error(ctx.pos.clone(), "tui.app.render: size must be an object"),
    };
    match do_render(ctx, &app, size) {
        Ok(frame) => str_obj(frame),
        Err(err) => err,
    }
}

pub(crate) fn app_run(ctx: &mut CallContext, args: &[Object]) -> Object {
    let app = match bound_app(ctx, "tui.app.run") {
        Ok(app) => app,
        Err(err) => return err,
    };
    if app.running.get() {
        return new_error(ctx.pos.clone(), "tui.app.run: app is already running");
    }
    if let Some(arg) = args.first() {
        if !matches!(arg, Object::Hash(_) | Object::Null | Object::Undefined) {
            return new_error(ctx.pos.clone(), "tui.app.run: options must be an object");
        }
    }
    let opts = args.first().and_then(|arg| match arg {
        Object::Hash(hash) => Some(hash.clone()),
        _ => None,
    });
    let tick_ms = opts
        .as_ref()
        .and_then(|h| hash_number(&h.borrow(), "tickMs"))
        .filter(|v| *v > 0.0)
        .unwrap_or(120.0) as u64;
    let alternate_screen = opts
        .as_ref()
        .and_then(|h| hash_bool(&h.borrow(), "alternateScreen"))
        .unwrap_or(false);
    let hide_cursor = opts
        .as_ref()
        .and_then(|h| hash_bool(&h.borrow(), "hideCursor"))
        .unwrap_or(false);
    app.running.set(true);
    app.stopped.set(false);
    let result = run_loop(ctx, &app, tick_ms, alternate_screen, hide_cursor);
    app.running.set(false);
    match result {
        Ok(()) => app.state.borrow().clone(),
        Err(err) => err,
    }
}

pub(crate) fn app_stop(ctx: &mut CallContext, _args: &[Object]) -> Object {
    match bound_app(ctx, "tui.app.stop") {
        Ok(app) => {
            app.stopped.set(true);
            Object::Undefined
        }
        Err(err) => err,
    }
}

pub(crate) fn app_state(ctx: &mut CallContext, _args: &[Object]) -> Object {
    match bound_app(ctx, "tui.app.state") {
        Ok(app) => app.state.borrow().clone(),
        Err(err) => err,
    }
}

// ---------------------------------------------------------------------------
// Core: dispatch / render (Elm contract)
// ---------------------------------------------------------------------------

fn do_dispatch(ctx: &mut CallContext, app: &Rc<TuiApp>, msg: Object) -> Result<(), Object> {
    if let Some(update_fn) = hash_function(&app.spec.borrow(), "update") {
        let state = app.state.borrow().clone();
        let result = call_script_function(&update_fn, ctx.env, &[state, msg]);
        if result.is_runtime_error() {
            return Err(result);
        }
        if let Object::Hash(hash) = &result {
            if let Some(next) = hash.borrow().get("state").cloned() {
                *app.state.borrow_mut() = next;
            } else {
                *app.state.borrow_mut() = result.clone();
            }
            if hash_bool(&hash.borrow(), "quit").unwrap_or(false) {
                app.stopped.set(true);
            }
        } else {
            *app.state.borrow_mut() = result;
        }
    } else if let Object::Hash(hash) = msg {
        if hash_string(&hash.borrow(), "type").as_deref() == Some("quit") {
            app.stopped.set(true);
        }
    }
    Ok(())
}

/// Run `view(state, size)`, interpret the result as a node tree, lay it out,
/// and render it to a frame string.
fn do_render(ctx: &mut CallContext, app: &Rc<TuiApp>, size: Object) -> Result<String, Object> {
    let (cols, rows) = size_dims(&size);
    if let Some(view_fn) = hash_function(&app.spec.borrow(), "view") {
        let state = app.state.borrow().clone();
        let result = call_script_function(&view_fn, ctx.env, &[state, size]);
        if result.is_runtime_error() {
            return Err(result);
        }
        let root = object_to_node(&result).unwrap_or_else(|| fallback_node(&result));
        let laid = layout(
            &root,
            Rect {
                x: 0,
                y: 0,
                w: cols,
                h: rows,
            },
        );
        Ok(render_frame(
            &laid,
            Rect {
                x: 0,
                y: 0,
                w: cols,
                h: rows,
            },
        ))
    } else {
        // No view: dump state as text.
        Ok(value_to_string(&app.state.borrow()))
    }
}

fn fallback_node(value: &Object) -> TuiNode {
    TuiNode {
        kind: NodeKind::Text {
            text: value_to_string(value),
            wrap: super::node::WrapMode::Wrap,
        },
        style: super::node::Style::default(),
        props: super::node::BoxProps::default(),
        title: String::new(),
    }
}

// ---------------------------------------------------------------------------
// Run loop (crossterm). Migrated from tui_legacy::tui_app_run_loop.
// ---------------------------------------------------------------------------

fn run_loop(
    ctx: &mut CallContext,
    app: &Rc<TuiApp>,
    tick_ms: u64,
    alternate_screen: bool,
    hide_cursor: bool,
) -> Result<(), Object> {
    use std::io::IsTerminal;
    let mut stdout = std::io::stdout();
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !interactive {
        // CI / non-TTY: single dispatch + render to stdout, then stop.
        do_dispatch(
            ctx,
            app,
            super::messages::resize_message(terminal_cols(), terminal_rows(), true),
        )?;
        render_to_stdout(ctx, app)?;
        return Ok(());
    }

    use crossterm::{
        cursor::{Hide, Show},
        event::{self, Event, KeyEventKind},
        execute,
        terminal::{
            self, disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
            LeaveAlternateScreen,
        },
    };

    if enable_raw_mode().is_err() {
        return Err(new_error(
            ctx.pos.clone(),
            "tui.app.run: failed to enable raw mode",
        ));
    }
    if alternate_screen {
        let _ = execute!(stdout, EnterAlternateScreen);
    }
    let _ = execute!(
        stdout,
        Clear(ClearType::All),
        crossterm::cursor::MoveTo(0, 0)
    );
    if hide_cursor {
        let _ = execute!(stdout, Hide);
    }

    let mut last_size =
        terminal::size().unwrap_or((terminal_cols() as u16, terminal_rows() as u16));
    let _ = do_dispatch(
        ctx,
        app,
        super::messages::resize_message(last_size.0 as i32, last_size.1 as i32, true),
    );
    let mut result = render_to_stdout(ctx, app);
    let mut next_tick = Instant::now() + Duration::from_millis(tick_ms.max(1));

    while result.is_ok() && !app.stopped.get() {
        let now = Instant::now();
        let timeout = next_tick.saturating_duration_since(now);
        match event::poll(timeout) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) => {
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                        result = do_dispatch(ctx, app, key_event_message(key))
                            .and_then(|_| render_to_stdout(ctx, app));
                    }
                }
                Ok(Event::Paste(text)) => {
                    result = do_dispatch(ctx, app, super::messages::raw_message(text))
                        .and_then(|_| render_to_stdout(ctx, app));
                }
                Ok(Event::Resize(cols, rows)) => {
                    last_size = (cols, rows);
                    result = do_dispatch(
                        ctx,
                        app,
                        super::messages::resize_message(cols as i32, rows as i32, true),
                    )
                    .and_then(|_| render_to_stdout(ctx, app));
                }
                Ok(Event::Mouse(mouse)) => {
                    result = do_dispatch(ctx, app, mouse_event_message(mouse))
                        .and_then(|_| render_to_stdout(ctx, app));
                }
                Ok(_) => {}
                Err(err) => {
                    result = Err(new_error(ctx.pos.clone(), format!("tui.app.run: {}", err)));
                }
            },
            Ok(false) => {
                next_tick = Instant::now() + Duration::from_millis(tick_ms.max(1));
                if let Ok(size) = terminal::size() {
                    if size != last_size {
                        last_size = size;
                        result = do_dispatch(
                            ctx,
                            app,
                            super::messages::resize_message(size.0 as i32, size.1 as i32, true),
                        );
                    }
                }
                if result.is_ok() {
                    result = do_dispatch(ctx, app, super::messages::tick_message())
                        .and_then(|_| render_to_stdout(ctx, app));
                }
            }
            Err(err) => result = Err(new_error(ctx.pos.clone(), format!("tui.app.run: {}", err))),
        }
    }

    if hide_cursor {
        let _ = execute!(stdout, Show);
    }
    if alternate_screen {
        let _ = execute!(stdout, LeaveAlternateScreen);
    }
    let _ = disable_raw_mode();
    result
}

fn render_to_stdout(ctx: &mut CallContext, app: &Rc<TuiApp>) -> Result<(), Object> {
    use std::io::Write;
    let size = terminal_size_object();
    let dims = size_dims(&size);
    let frame = do_render(ctx, app, size)?;

    // Split the frame into lines for diff-based incremental drawing.
    // This avoids flicker by only rewriting rows that actually changed,
    // and erasing trailing residue with \x1b[K (clear-to-end-of-line).
    let new_lines: Vec<String> = frame.split('\n').map(|s| s.to_string()).collect();

    let mut out = String::with_capacity(frame.len() + new_lines.len() * 8);
    let prev = app.last_frame.borrow();

    match prev.as_ref() {
        Some(prev_lines)
            if prev_lines.len() == new_lines.len() && app.last_size.get() == Some(dims) =>
        {
            // Diff mode: only write rows that differ. Position cursor with
            // \x1b[<row>;<col>H (1-based) for each changed row.
            let mut any_changed = false;
            for (i, new_line) in new_lines.iter().enumerate() {
                let old_line = &prev_lines[i];
                if new_line != old_line {
                    any_changed = true;
                    // \x1b[H moves to (1,1); add row offset for line i (0-based → 1-based).
                    out.push_str(&format!("\x1b[{};1H{}\x1b[K", i + 1, new_line));
                }
            }
            if !any_changed {
                // Frame identical — nothing to write, skip entirely.
                return Ok(());
            }
        }
        _ => {
            // First render or size changed: full repaint.
            // Move cursor home, write each line with \x1b[K to erase residue.
            out.push_str("\x1b[2J\x1b[H");
            for (i, line) in new_lines.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                out.push_str(line);
                out.push_str("\x1b[K"); // clear to end of line
            }
        }
    }

    drop(prev);

    std::io::stdout()
        .write_all(out.as_bytes())
        .map_err(|err| new_error(ctx.pos.clone(), format!("tui.app.render: {}", err)))?;
    let _ = std::io::stdout().flush();

    *app.last_frame.borrow_mut() = Some(new_lines);
    app.last_size.set(Some(dims));
    Ok(())
}

// ---------------------------------------------------------------------------
// Object → node tree conversion
// ---------------------------------------------------------------------------

/// Convert a script-side node marker (or a raw string) into a `TuiNode`.
/// Strings are wrapped in a text node; markers are looked up by id.
fn object_to_node(value: &Object) -> Option<TuiNode> {
    match value {
        Object::Hash(hash) => {
            let h = hash.borrow();
            if matches!(h.get("__kind"), Some(Object::String(s)) if &**s == "tuiNode") {
                if let Some(Object::Number(id)) = h.get("__id") {
                    return lookup_node(*id as usize);
                }
            }
            None
        }
        Object::String(s) => Some(TuiNode {
            kind: NodeKind::Text {
                text: s.to_string(),
                wrap: super::node::WrapMode::Wrap,
            },
            style: super::node::Style::default(),
            props: super::node::BoxProps::default(),
            title: String::new(),
        }),
        Object::Array(arr) => {
            // An array of nodes → an anonymous column box.
            let children: Vec<TuiNode> = arr
                .borrow()
                .elements
                .iter()
                .filter_map(object_to_node)
                .collect();
            Some(TuiNode {
                kind: NodeKind::Box { children },
                style: super::node::Style::default(),
                props: super::node::BoxProps::default(),
                title: String::new(),
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Event-message helpers (mirror legacy key/mouse translation)
// ---------------------------------------------------------------------------

fn key_event_message(event: crossterm::event::KeyEvent) -> Object {
    use crossterm::event::{KeyCode, KeyModifiers};
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        let name = match event.code {
            KeyCode::Char('c') | KeyCode::Char('C') => "ctrl+c",
            KeyCode::Char('o') | KeyCode::Char('O') => "ctrl+o",
            KeyCode::Char('q') | KeyCode::Char('Q') => "ctrl+q",
            KeyCode::Char('r') | KeyCode::Char('R') => "ctrl+r",
            KeyCode::Char('s') | KeyCode::Char('S') => "ctrl+s",
            _ => "",
        };
        if !name.is_empty() {
            return super::messages::key_message(name, "");
        }
    }
    match event.code {
        KeyCode::Backspace => super::messages::key_message("backspace", ""),
        KeyCode::Enter => super::messages::key_message("enter", ""),
        KeyCode::Left => super::messages::key_message("left", ""),
        KeyCode::Right => super::messages::key_message("right", ""),
        KeyCode::Up => super::messages::key_message("up", ""),
        KeyCode::Down => super::messages::key_message("down", ""),
        KeyCode::Home => super::messages::key_message("home", ""),
        KeyCode::End => super::messages::key_message("end", ""),
        KeyCode::PageUp => super::messages::key_message("pageUp", ""),
        KeyCode::PageDown => super::messages::key_message("pageDown", ""),
        KeyCode::Tab => super::messages::key_message("tab", ""),
        KeyCode::BackTab => super::messages::key_message("shift+tab", ""),
        KeyCode::Esc => super::messages::key_message("escape", ""),
        KeyCode::Char(ch) => super::messages::text_message(&ch.to_string(), &ch.to_string()),
        _ => super::messages::key_message("unknown", ""),
    }
}

fn mouse_event_message(event: crossterm::event::MouseEvent) -> Object {
    use crossterm::event::MouseEventKind;
    let (action, button) = match event.kind {
        MouseEventKind::Down(b) => ("down", mouse_button_number(b)),
        MouseEventKind::Up(b) => ("release", mouse_button_number(b)),
        MouseEventKind::Drag(b) => ("drag", mouse_button_number(b)),
        MouseEventKind::Moved => ("move", 0),
        MouseEventKind::ScrollUp => ("wheel", 64),
        MouseEventKind::ScrollDown => ("wheel", 65),
        MouseEventKind::ScrollLeft => ("wheelLeft", 66),
        MouseEventKind::ScrollRight => ("wheelRight", 67),
    };
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set("type", str_obj("mouse"));
    hash.borrow_mut().set("action", str_obj(action));
    hash.borrow_mut()
        .set("button", crate::object::num_obj(button as f64));
    hash.borrow_mut()
        .set("x", crate::object::num_obj(event.column as f64));
    hash.borrow_mut()
        .set("y", crate::object::num_obj(event.row as f64));
    Object::Hash(hash)
}

fn mouse_button_number(button: crossterm::event::MouseButton) -> i32 {
    use crossterm::event::MouseButton;
    match button {
        MouseButton::Left => 0,
        MouseButton::Right => 2,
        MouseButton::Middle => 1,
    }
}

// ---------------------------------------------------------------------------
// Hash accessors (local copies to avoid cross-module visibility churn)
// ---------------------------------------------------------------------------

fn hash_function(hash: &HashData, key: &str) -> Option<Object> {
    match hash.get(key) {
        Some(Object::Function(_) | Object::Builtin(_) | Object::Closure(_)) => {
            hash.get(key).cloned()
        }
        _ => None,
    }
}

fn hash_string(hash: &HashData, key: &str) -> Option<String> {
    match hash.get(key) {
        Some(Object::String(v)) => Some(v.to_string()),
        Some(Object::Null | Object::Undefined) | None => None,
        Some(v) => Some(value_to_string(v)),
    }
}

fn hash_bool(hash: &HashData, key: &str) -> Option<bool> {
    match hash.get(key) {
        Some(Object::Boolean(v)) => Some(*v),
        Some(Object::Null | Object::Undefined) | None => None,
        Some(v) => Some(v.is_truthy()),
    }
}

fn hash_number(hash: &HashData, key: &str) -> Option<f64> {
    match hash.get(key) {
        Some(Object::Number(v)) => Some(*v),
        _ => None,
    }
}

fn size_dims(size: &Object) -> (i32, i32) {
    if let Object::Hash(h) = size {
        let b = h.borrow();
        let cols = match b.get("cols") {
            Some(Object::Number(n)) => *n as i32,
            _ => terminal_cols(),
        };
        let rows = match b.get("rows") {
            Some(Object::Number(n)) => *n as i32,
            _ => terminal_rows(),
        };
        (cols, rows)
    } else {
        (terminal_cols(), terminal_rows())
    }
}
