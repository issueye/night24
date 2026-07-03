//! @std/tui — terminal UI: declarative node tree + flexbox layout engine.
//!
//! Redesigned (Ink-inspired) module. UI is expressed as a tree of nodes built
//! by `tui.text` / `tui.box` / `tui.input` / `tui.list` / `tui.table` /
//! `tui.progress` / `tui.checkbox`; `view` returns a node tree which the native
//! flexbox engine (`layout.rs`) measures and lays out, and `render.rs` paints
//! into a string frame. The app runtime (`app.rs`) drives the crossterm event
//! loop and the Elm-architecture `init`/`update`/`view` contract.

pub mod app;
pub mod layout;
pub mod messages;
pub mod node;
pub mod render;

use std::rc::Rc;

use crate::object::{new_error, CallContext, HashData, Object};
use crate::stdlib::helpers::{module, native, required_number, required_string};

use node::{
    node_object, AlignItems, BoxProps, FlexDirection, JustifyContent, NodeKind, Style, TuiNode,
    WrapMode,
};

/// The `@std/tui` module: node constructors + app runtime + low-level helpers.
pub(crate) fn tui_module() -> Object {
    module(vec![
        // App runtime.
        ("createApp", native("tui.createApp", app::tui_create_app)),
        // Node constructors.
        ("text", native("tui.text", tui_text)),
        ("box", native("tui.box", tui_box)),
        ("row", native("tui.row", tui_row)),
        ("column", native("tui.column", tui_column)),
        ("input", native("tui.input", tui_input)),
        ("list", native("tui.list", tui_list)),
        ("table", native("tui.table", tui_table)),
        ("progress", native("tui.progress", tui_progress)),
        ("checkbox", native("tui.checkbox", tui_checkbox)),
        // Message constructors (for testing dispatch without events).
        ("key", native("tui.key", tui_key_msg)),
        ("resize", native("tui.resize", tui_resize_msg)),
        ("tick", native("tui.tick", tui_tick_msg)),
        // Low-level helpers (forwarded).
        ("style", native("tui.style", tui_style)),
        ("width", native("tui.width", tui_width)),
        ("truncate", native("tui.truncate", tui_truncate)),
        ("stripAnsi", native("tui.stripAnsi", tui_strip_ansi)),
    ])
}

// ---------------------------------------------------------------------------
// Node constructors
// ---------------------------------------------------------------------------

/// `tui.text(value, opts?)` → text node.
pub(crate) fn tui_text(ctx: &mut CallContext, args: &[Object]) -> Object {
    let text = match required_string(ctx, "tui.text", args, 0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (style, props, title) = match parse_common_opts(args.get(1), "tui.text") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let wrap = args
        .get(1)
        .and_then(|o| match o {
            Object::Hash(h) => h.borrow().get("wrap").cloned(),
            _ => None,
        })
        .and_then(parse_wrap)
        .unwrap_or_default();
    node_object(TuiNode {
        kind: NodeKind::Text { text, wrap },
        style,
        props,
        title,
    })
}

/// `tui.box(opts?)` → box node. `opts.children` is an array of nodes.
pub(crate) fn tui_box(_ctx: &mut CallContext, args: &[Object]) -> Object {
    let (style, mut props, title) = match parse_common_opts(args.first(), "tui.box") {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Some(direction) = parse_direction_opt(args.first()) {
        props.direction = direction;
    }
    let children = children_of(args.first());
    node_object(TuiNode {
        kind: NodeKind::Box { children },
        style,
        props,
        title,
    })
}

/// `tui.row(children, opts?)` → box node with flexDirection: row.
pub(crate) fn tui_row(ctx: &mut CallContext, args: &[Object]) -> Object {
    box_with_direction(ctx, args, FlexDirection::Row, "tui.row")
}

/// `tui.column(children, opts?)` → box node with flexDirection: column.
pub(crate) fn tui_column(ctx: &mut CallContext, args: &[Object]) -> Object {
    box_with_direction(ctx, args, FlexDirection::Column, "tui.column")
}

fn box_with_direction(
    _ctx: &mut CallContext,
    args: &[Object],
    dir: FlexDirection,
    name: &str,
) -> Object {
    // First arg may be the children array, or an opts object with `children`.
    let (style, mut props, title) = match parse_common_opts(args.first(), name) {
        Ok(v) => v,
        Err(e) => return e,
    };
    props.direction = dir;
    let children = children_of(args.first());
    node_object(TuiNode {
        kind: NodeKind::Box { children },
        style,
        props,
        title,
    })
}

/// `tui.input(opts)` → input node.
pub(crate) fn tui_input(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(Object::Hash(hash)) = args.first() else {
        return new_error(ctx.pos.clone(), "tui.input: options must be an object");
    };
    let h = hash.borrow();
    let (style, props, title) = match parse_common_opts(args.first(), "tui.input") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let value = h.get_string("value").unwrap_or_default();
    let placeholder = h.get_string("placeholder").unwrap_or_default();
    let prompt = h.get_string("prompt").unwrap_or_else(|| "> ".into());
    let focused = h.get_bool("focused").unwrap_or(true);
    let cursor = h
        .get_number("cursor")
        .map(|n| n as i32)
        .unwrap_or_else(|| value.chars().count() as i32);
    node_object(TuiNode {
        kind: NodeKind::Input {
            value,
            cursor,
            placeholder,
            prompt,
            focused,
        },
        style,
        props,
        title,
    })
}

/// `tui.list(opts)` → list node.
pub(crate) fn tui_list(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(Object::Hash(hash)) = args.first() else {
        return new_error(ctx.pos.clone(), "tui.list: options must be an object");
    };
    let h = hash.borrow();
    let (style, props, title) = match parse_common_opts(args.first(), "tui.list") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let items = match h.get("items") {
        Some(Object::Array(arr)) => arr
            .borrow()
            .elements
            .iter()
            .map(crate::stdlib::helpers::object_to_text)
            .collect(),
        _ => Vec::new(),
    };
    let selected = h.get_number("selected").map(|n| n as i32).unwrap_or(0);
    let focused = h.get_bool("focused").unwrap_or(true);
    node_object(TuiNode {
        kind: NodeKind::List {
            items,
            selected,
            focused,
        },
        style,
        props,
        title,
    })
}

/// `tui.table(opts)` → table node.
pub(crate) fn tui_table(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(Object::Hash(hash)) = args.first() else {
        return new_error(ctx.pos.clone(), "tui.table: options must be an object");
    };
    let h = hash.borrow();
    let (style, props, title) = match parse_common_opts(args.first(), "tui.table") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let headers = string_array(&h, "headers");
    let column_widths = number_array(&h, "columnWidths");
    let rows: Vec<Vec<String>> = match h.get("rows") {
        Some(Object::Array(arr)) => arr
            .borrow()
            .elements
            .iter()
            .map(|row| match row {
                Object::Array(cells) => cells
                    .borrow()
                    .elements
                    .iter()
                    .map(crate::stdlib::helpers::object_to_text)
                    .collect(),
                other => vec![crate::stdlib::helpers::object_to_text(other)],
            })
            .collect(),
        _ => Vec::new(),
    };
    node_object(TuiNode {
        kind: NodeKind::Table {
            headers,
            rows,
            column_widths,
        },
        style,
        props,
        title,
    })
}

/// `tui.progress(opts)` → progress node.
pub(crate) fn tui_progress(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(Object::Hash(hash)) = args.first() else {
        return new_error(ctx.pos.clone(), "tui.progress: options must be an object");
    };
    let h = hash.borrow();
    let (style, props, title) = match parse_common_opts(args.first(), "tui.progress") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let value = h.get_number("value").unwrap_or(0.0);
    let total = h.get_number("total").unwrap_or(100.0);
    let label = h.get_string("label").unwrap_or_default();
    node_object(TuiNode {
        kind: NodeKind::Progress {
            value,
            total,
            label,
        },
        style,
        props,
        title,
    })
}

/// `tui.checkbox(opts)` → checkbox node.
pub(crate) fn tui_checkbox(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(Object::Hash(hash)) = args.first() else {
        return new_error(ctx.pos.clone(), "tui.checkbox: options must be an object");
    };
    let h = hash.borrow();
    let (style, props, title) = match parse_common_opts(args.first(), "tui.checkbox") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let checked = h.get_bool("checked").unwrap_or(false);
    let label = h.get_string("label").unwrap_or_default();
    node_object(TuiNode {
        kind: NodeKind::Checkbox { checked, label },
        style,
        props,
        title,
    })
}

// ---------------------------------------------------------------------------
// Message constructors (for manual dispatch/testing)
// ---------------------------------------------------------------------------

pub(crate) fn tui_key_msg(ctx: &mut CallContext, args: &[Object]) -> Object {
    let name = match required_string(ctx, "tui.key", args, 0, "name") {
        Ok(n) => n,
        Err(e) => return e,
    };
    messages::key_message(&name, "")
}

pub(crate) fn tui_resize_msg(ctx: &mut CallContext, args: &[Object]) -> Object {
    let cols = match required_number(ctx, "tui.resize", args, 0, "cols") {
        Ok(n) => n as i32,
        Err(e) => return e,
    };
    let rows = match required_number(ctx, "tui.resize", args, 1, "rows") {
        Ok(n) => n as i32,
        Err(e) => return e,
    };
    messages::resize_message(cols, rows, true)
}

pub(crate) fn tui_tick_msg(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    messages::tick_message()
}

// ---------------------------------------------------------------------------
// Low-level forwarded helpers
// ---------------------------------------------------------------------------

pub(crate) fn tui_style(ctx: &mut CallContext, args: &[Object]) -> Object {
    super::terminal::terminal_style(ctx, args)
}

pub(crate) fn tui_width(ctx: &mut CallContext, args: &[Object]) -> Object {
    super::text::text_width(ctx, args)
}

pub(crate) fn tui_truncate(ctx: &mut CallContext, args: &[Object]) -> Object {
    super::text::text_truncate_width(ctx, args)
}

pub(crate) fn tui_strip_ansi(ctx: &mut CallContext, args: &[Object]) -> Object {
    super::text::text_strip_ansi(ctx, args)
}

// ---------------------------------------------------------------------------
// Option parsing helpers
// ---------------------------------------------------------------------------

/// Parse the common style + flexbox + title options from an opts hash.
/// Returns `(style, props, title)` or an error.
fn parse_common_opts(
    opts: Option<&Object>,
    _name: &str,
) -> Result<(Style, BoxProps, String), Object> {
    let Some(Object::Hash(hash)) = opts else {
        return Ok((Style::default(), BoxProps::default(), String::new()));
    };
    let h = hash.borrow();

    let style = Style {
        fg: h.get_string("color").or_else(|| h.get_string("fg")),
        bg: h.get_string("bg"),
        bold: h.get_bool("bold").unwrap_or(false),
        dim: h.get_bool("dim").unwrap_or(false),
        underline: h.get_bool("underline").unwrap_or(false),
        inverse: h.get_bool("inverse").unwrap_or(false),
    };

    let props = BoxProps {
        direction: parse_direction(h.get_string("flexDirection").as_deref()).unwrap_or_default(),
        width: h.get_number("width").map(|n| n as i32),
        height: h.get_number("height").map(|n| n as i32),
        grow: h.get_number("grow").unwrap_or(0.0),
        padding: h.get_number("padding").map(|n| n as i32).unwrap_or(0),
        margin: h.get_number("margin").map(|n| n as i32).unwrap_or(0),
        border: h.get_bool("border").unwrap_or(false),
        align: parse_align(h.get_string("alignItems").as_deref()).unwrap_or_default(),
        justify: parse_justify(h.get_string("justifyContent").as_deref()).unwrap_or_default(),
    };

    let title = h.get_string("title").unwrap_or_default();
    Ok((style, props, title))
}

/// Extract the `children` array from an opts object (or treat the opts itself
/// as a children array). Each child is resolved to a registered node, falling
/// back to a text node of its string form.
fn children_of(opts: Option<&Object>) -> Vec<TuiNode> {
    let Some(Object::Hash(hash)) = opts else {
        return Vec::new();
    };
    let arr = match hash.borrow().get("children") {
        Some(Object::Array(arr)) => arr.clone(),
        _ => return Vec::new(),
    };
    let arr_ref = arr.borrow();
    arr_ref.elements.iter().filter_map(child_to_node).collect()
}

fn child_to_node(value: &Object) -> Option<TuiNode> {
    match value {
        Object::Hash(hash) => {
            let h = hash.borrow();
            if matches!(h.get("__kind"), Some(Object::String(s)) if &**s == "tuiNode") {
                if let Some(Object::Number(id)) = h.get("__id") {
                    return node::lookup_node(*id as usize);
                }
            }
            None
        }
        Object::String(s) => Some(TuiNode {
            kind: NodeKind::Text {
                text: s.to_string(),
                wrap: WrapMode::Wrap,
            },
            style: Style::default(),
            props: BoxProps::default(),
            title: String::new(),
        }),
        _ => None,
    }
}

fn parse_direction(s: Option<&str>) -> Option<FlexDirection> {
    match s? {
        "row" => Some(FlexDirection::Row),
        "column" => Some(FlexDirection::Column),
        _ => None,
    }
}

fn parse_direction_opt(opts: Option<&Object>) -> Option<FlexDirection> {
    let Some(Object::Hash(hash)) = opts else {
        return None;
    };
    parse_direction(hash.borrow().get_string("flexDirection").as_deref())
}

fn parse_align(s: Option<&str>) -> Option<AlignItems> {
    match s? {
        "flex-start" | "start" => Some(AlignItems::FlexStart),
        "center" => Some(AlignItems::Center),
        "flex-end" | "end" => Some(AlignItems::FlexEnd),
        "stretch" => Some(AlignItems::Stretch),
        _ => None,
    }
}

fn parse_justify(s: Option<&str>) -> Option<JustifyContent> {
    match s? {
        "flex-start" | "start" => Some(JustifyContent::FlexStart),
        "center" => Some(JustifyContent::Center),
        "flex-end" | "end" => Some(JustifyContent::FlexEnd),
        "space-between" => Some(JustifyContent::SpaceBetween),
        _ => None,
    }
}

fn parse_wrap(obj: Object) -> Option<WrapMode> {
    if let Object::String(s) = &obj {
        match s.as_str() {
            "wrap" => Some(WrapMode::Wrap),
            "truncate" => Some(WrapMode::Truncate),
            "end" => Some(WrapMode::End),
            _ => None,
        }
    } else {
        None
    }
}

fn string_array(h: &HashData, key: &str) -> Vec<String> {
    match h.get(key) {
        Some(Object::Array(arr)) => arr
            .borrow()
            .elements
            .iter()
            .map(crate::stdlib::helpers::object_to_text)
            .collect(),
        _ => Vec::new(),
    }
}

fn number_array(h: &HashData, key: &str) -> Vec<i32> {
    match h.get(key) {
        Some(Object::Array(arr)) => arr
            .borrow()
            .elements
            .iter()
            .filter_map(|v| match v {
                Object::Number(n) => Some(*n as i32),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

// Convenience extension until HashData grows native getters used above.
trait HashGet {
    fn get_string(&self, key: &str) -> Option<String>;
    fn get_bool(&self, key: &str) -> Option<bool>;
    fn get_number(&self, key: &str) -> Option<f64>;
}
impl HashGet for HashData {
    fn get_string(&self, key: &str) -> Option<String> {
        match self.get(key) {
            Some(Object::String(s)) => Some(s.to_string()),
            Some(Object::Null | Object::Undefined) | None => None,
            Some(v) => Some(crate::stdlib::helpers::value_to_string(v)),
        }
    }
    fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key) {
            Some(Object::Boolean(b)) => Some(*b),
            Some(Object::Null | Object::Undefined) | None => None,
            Some(v) => Some(v.is_truthy()),
        }
    }
    fn get_number(&self, key: &str) -> Option<f64> {
        match self.get(key) {
            Some(Object::Number(n)) => Some(*n),
            _ => None,
        }
    }
}

// Keep a reference to suppress unused warnings for re-exports used elsewhere.
#[allow(dead_code)]
fn _keep(_rc: Rc<()>) {}
