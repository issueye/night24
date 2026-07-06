//! TuiNode: the in-memory representation of a UI element, and its conversion
//! to/from language-level `Object`s.
//!
//! A node is one of:
//!   - text:   a styled string leaf
//!   - box:    a flexbox container (the layout root / generic element)
//!   - input / list / table / progress / checkbox: specialized components
//!
//! Every node carries a [`Style`] and a [`BoxProps`] (flexbox sizing). Boxes
//! additionally own a list of child nodes.

use std::cell::Cell;
use std::cell::RefCell;

use crate::object::{str_obj, Object};
use crate::stdlib::helpers::ObjectBuilder;

/// Flexbox main-axis direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexDirection {
    Row,
    #[default]
    Column,
}

/// Cross-axis alignment for children (CSS `align-items`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignItems {
    #[default]
    FlexStart,
    Center,
    FlexEnd,
    Stretch,
}

/// Main-axis distribution of children (CSS `justify-content`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JustifyContent {
    #[default]
    FlexStart,
    Center,
    FlexEnd,
    SpaceBetween,
}

/// Text overflow policy for a text node wider than its allotted width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrapMode {
    /// Wrap onto multiple lines.
    #[default]
    Wrap,
    /// Truncate to fit (no ellipsis).
    Truncate,
    /// Truncate with a trailing `…`.
    End,
}

/// Fore/background + emphasis styling for a node's content.
#[derive(Debug, Clone, Default)]
pub struct Style {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
    pub dim: bool,
    pub underline: bool,
    pub inverse: bool,
}

/// Flexbox sizing/spacing properties (all optional → auto/zero defaults).
#[derive(Debug, Clone, Default)]
pub struct BoxProps {
    pub direction: FlexDirection,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub grow: f64,
    pub padding: i32,
    pub margin: i32,
    pub border: bool,
    pub align: AlignItems,
    pub justify: JustifyContent,
}

/// The kind of a node. `Box` carries children; the rest are leaves or render
/// their own sub-tree (see `render.rs`).
#[derive(Debug, Clone)]
pub enum NodeKind {
    Text {
        text: String,
        wrap: WrapMode,
    },
    Box {
        children: Vec<TuiNode>,
    },
    Input {
        value: String,
        cursor: i32,
        placeholder: String,
        prompt: String,
        focused: bool,
    },
    List {
        items: Vec<String>,
        selected: i32,
        focused: bool,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        column_widths: Vec<i32>,
    },
    Progress {
        value: f64,
        total: f64,
        label: String,
    },
    Checkbox {
        checked: bool,
        label: String,
    },
}

/// A UI node: kind + shared style + flexbox props + optional title (for boxes).
#[derive(Debug, Clone)]
pub struct TuiNode {
    pub kind: NodeKind,
    pub style: Style,
    pub props: BoxProps,
    pub title: String,
}

impl TuiNode {
    /// True for box-like nodes that lay out children along an axis.
    pub fn is_box(&self) -> bool {
        matches!(self.kind, NodeKind::Box { .. })
    }

    /// Borrow this node's children if it is a box (empty slice otherwise).
    pub fn children(&self) -> &[TuiNode] {
        match &self.kind {
            NodeKind::Box { children } => children,
            _ => &[],
        }
    }
}

/// Build a node marker `Object` (a Hash with `__kind: "tuiNode"`) wrapping a
/// node, registering it under a fresh integer id so callbacks can recover it.
/// Mirrors the legacy app-marker pattern but uses a monotonic id (not pointer
/// identity, which is unstable for Vec entries).
pub fn node_object(node: TuiNode) -> Object {
    let id = next_node_id();
    let marker = ObjectBuilder::new()
        .set("__kind", str_obj("tuiNode"))
        .set("__id", crate::object::num_obj(id as f64))
        .into_shared();
    TUI_NODES.with(|nodes| nodes.borrow_mut().push((id, node)));
    Object::Hash(marker)
}

thread_local! {
    pub(crate) static TUI_NODES: RefCell<Vec<(usize, TuiNode)>> = const { RefCell::new(Vec::new()) };
    static NEXT_NODE_ID: Cell<usize> = const { Cell::new(0) };
}

fn next_node_id() -> usize {
    NEXT_NODE_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    })
}

/// Recover a registered node by the id stored in its marker.
pub fn lookup_node(id: usize) -> Option<TuiNode> {
    TUI_NODES.with(|nodes| {
        nodes
            .borrow()
            .iter()
            .find(|(nid, _)| *nid == id)
            .map(|(_, n)| n.clone())
    })
}
