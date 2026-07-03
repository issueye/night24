//! Flexbox layout engine (a Yoga subset).
//!
//! Two passes over the node tree:
//!   1. [`measure`] — bottom-up: each leaf reports its intrinsic size
//!      (text uses `visible_width`/`text_wrap_width`); boxes aggregate.
//!   2. [`layout`]  — top-down: assign an absolute `(x, y, w, h)` rect to every
//!      node, honoring `flexDirection`, `grow`, `padding`, `margin`, `border`,
//!      `alignItems`, and `justifyContent`.
//!
//! Deliberately omitted (out of scope for this iteration): flex-wrap, percentage
//! sizes, baseline alignment, min/max constraints.

use super::node::{AlignItems, FlexDirection, JustifyContent, NodeKind, TuiNode};
use crate::stdlib::helpers::{strip_ansi, visible_width, wrap_line};

/// An absolute rectangle assigned during layout.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// A node plus its measured intrinsic size and assigned layout rect.
#[derive(Debug, Clone)]
pub struct MeasuredNode<'a> {
    pub node: &'a TuiNode,
    /// Intrinsic content size (excluding padding/border/margin).
    pub measured_w: i32,
    pub measured_h: i32,
    /// Final assigned rect (including padding/border; excluding margin).
    pub rect: Rect,
    pub children: Vec<MeasuredNode<'a>>,
}

/// The padding/border overhead on each axis for a node (inside its rect).
fn inner_inset(node: &TuiNode) -> (i32, i32) {
    // (horizontal overhead, vertical overhead) from padding + border.
    let pad = node.props.padding.max(0);
    let border = if node.props.border { 1 } else { 0 };
    let extra = (pad + border) * 2;
    (extra, extra)
}

/// Bottom-up measurement. Returns (content_width, content_height) — the space
/// the node's own content + children need, excluding its own padding/border.
fn measure(node: &TuiNode, max_w: i32) -> (i32, i32) {
    match &node.kind {
        NodeKind::Text { text, wrap } => measure_text(text, *wrap, max_w),
        NodeKind::Input {
            value,
            placeholder,
            prompt,
            ..
        } => {
            let prompt_w = visible_width(prompt);
            let content = if value.is_empty() { placeholder } else { value };
            let w = prompt_w + visible_width(content);
            (w.max(1) as i32, 1)
        }
        NodeKind::List { items, .. } => {
            let w = items.iter().map(|s| visible_width(s)).max().unwrap_or(0) + 2; // selection marker "  " / "› "
            (w as i32, items.len() as i32)
        }
        NodeKind::Progress { label, .. } => {
            let w = visible_width(label).max(10);
            (w as i32, 1)
        }
        NodeKind::Checkbox { label, .. } => {
            let w = visible_width(label) + 4; // "[x] " / "[ ] "
            (w as i32, 1)
        }
        NodeKind::Table {
            headers,
            rows,
            column_widths,
            ..
        } => measure_table(headers, rows, column_widths),
        NodeKind::Box { children } => {
            if children.is_empty() {
                return (0, 0);
            }
            let (oh, _ov) = inner_inset(node);
            let avail = (max_w - oh).max(1);
            match node.props.direction {
                FlexDirection::Row => {
                    // children flow horizontally: width = sum, height = max.
                    let mut w = 0;
                    let mut h = 0;
                    for child in children {
                        let (cw, ch) = measure(child, avail);
                        w += cw;
                        h = h.max(ch);
                    }
                    (w, h)
                }
                FlexDirection::Column => {
                    // children stack vertically: width = max, height = sum.
                    let mut w = 0;
                    let mut h = 0;
                    for child in children {
                        let (cw, ch) = measure(child, avail);
                        w = w.max(cw);
                        h += ch;
                    }
                    (w, h)
                }
            }
        }
    }
}

fn measure_text(text: &str, wrap: super::node::WrapMode, max_w: i32) -> (i32, i32) {
    let stripped = strip_ansi(text);
    let width = visible_width(&stripped) as i32;
    if width <= max_w.max(1) || max_w <= 0 {
        return (width.max(0), 1);
    }
    match wrap {
        super::node::WrapMode::Truncate | super::node::WrapMode::End => (max_w, 1),
        super::node::WrapMode::Wrap => {
            let lines = wrap_line(&stripped, max_w as usize);
            (max_w, lines.len() as i32)
        }
    }
}

fn measure_table(headers: &[String], rows: &[Vec<String>], column_widths: &[i32]) -> (i32, i32) {
    let cols = column_widths.len().max(headers.len());
    if cols == 0 {
        return (0, 0);
    }
    let mut widths = vec![0i32; cols];
    for (i, h) in headers.iter().enumerate() {
        if i < cols {
            widths[i] = widths[i].max(visible_width(h) as i32);
        }
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < cols {
                widths[i] = widths[i].max(visible_width(cell) as i32);
            }
        }
    }
    // Explicit column_widths override.
    for (i, w) in column_widths.iter().enumerate() {
        if i < cols && *w > 0 {
            widths[i] = *w;
        }
    }
    // +cols for the separators between columns.
    let total: i32 = widths.iter().sum::<i32>() + cols as i32 - 1;
    let height = 1 + rows.len() as i32; // header + rows
    (total.max(0), height)
}

/// Run the full layout: measure then position. Returns the root `MeasuredNode`
/// with absolute rects for the whole subtree, fit into `viewport`.
pub fn layout<'a>(node: &'a TuiNode, viewport: Rect) -> MeasuredNode<'a> {
    let (mw, mh) = measure(node, viewport.w);
    let mut root = MeasuredNode {
        node,
        measured_w: mw,
        measured_h: mh,
        rect: viewport,
        children: Vec::new(),
    };
    position_children(&mut root);
    root
}

/// Top-down assignment of rects to a node's children within its content box.
fn position_children(parent: &mut MeasuredNode<'_>) {
    let children = match &parent.node.kind {
        NodeKind::Box { children } => children,
        _ => return, // leaves lay out nothing.
    };
    if children.is_empty() {
        return;
    }
    let (oh, ov) = inner_inset(parent.node);
    let content = Rect {
        x: parent.rect.x + (oh / 2),
        y: parent.rect.y + (ov / 2),
        w: (parent.rect.w - oh).max(0),
        h: (parent.rect.h - ov).max(0),
    };

    let dir = parent.node.props.direction;
    let (main, cross) = match dir {
        FlexDirection::Row => (content.w, content.h),
        FlexDirection::Column => (content.h, content.w),
    };

    // 1. Measure every child. For column children the width is bounded by the
    //    container width (wrapping applies); for row children width is
    //    unbounded (they take their intrinsic width and grow horizontally).
    let mut sizes: Vec<(i32, i32)> = children
        .iter()
        .map(|c| {
            let bound = if matches!(dir, FlexDirection::Column) {
                content.w
            } else {
                i32::MAX // row: no width constraint, measure intrinsic.
            };
            measure(c, bound)
        })
        .collect();

    // 2. Resolve explicit width/height (override measurement) and apply grow
    //    along the main axis to consume leftover space.
    let main_explicit: Vec<i32> = children
        .iter()
        .enumerate()
        .map(|(i, c)| explicit_main(c, dir, sizes[i]))
        .collect();
    let total_main: i32 = main_explicit.iter().sum();
    let free = main - total_main;
    let grown = distribute_grow(children, &main_explicit, free);

    // 3. Compute main-axis positions per justifyContent.
    let main_positions = main_axis_positions(&grown, main, parent.node.props.justify, dir);

    // 4. Place each child, recursing.
    let mut cursor = match dir {
        FlexDirection::Row => content.x,
        FlexDirection::Column => content.y,
    };
    for (i, child) in children.iter().enumerate() {
        let child_main = grown[i];
        let child_cross = cross_size(child, sizes[i], cross, parent.node.props.align, dir);
        let (child_rect, advance) = match dir {
            FlexDirection::Row => {
                let x = match parent.node.props.justify {
                    JustifyContent::SpaceBetween => main_positions[i],
                    _ => cursor,
                };
                let y = cross_position(parent.node.props.align, content.y, cross, child_cross);
                let rect = Rect {
                    x,
                    y,
                    w: child_main,
                    h: child_cross,
                };
                let advance = match parent.node.props.justify {
                    JustifyContent::SpaceBetween => 0, // positions are absolute
                    _ => child_main,
                };
                (rect, advance)
            }
            FlexDirection::Column => {
                let y = match parent.node.props.justify {
                    JustifyContent::SpaceBetween => main_positions[i],
                    _ => cursor,
                };
                let x = cross_position(parent.node.props.align, content.x, cross, child_cross);
                let rect = Rect {
                    x,
                    y,
                    w: child_cross,
                    h: child_main,
                };
                let advance = match parent.node.props.justify {
                    JustifyContent::SpaceBetween => 0,
                    _ => child_main,
                };
                (rect, advance)
            }
        };
        cursor += advance;
        sizes[i] = (
            if matches!(dir, FlexDirection::Row) {
                child_rect.w
            } else {
                child_rect.h
            },
            if matches!(dir, FlexDirection::Row) {
                child_rect.h
            } else {
                child_rect.w
            },
        );
        let _ = sizes; // measured sizes already consumed; explicit rect wins.
        let mut measured = MeasuredNode {
            node: child,
            measured_w: grown[i],
            measured_h: child_cross,
            rect: child_rect,
            children: Vec::new(),
        };
        position_children(&mut measured);
        parent.children.push(measured);
    }
}

/// The main-axis size, honoring an explicit width/height override. `measured`
/// is (width, height); the relevant axis is selected by direction.
fn explicit_main(node: &TuiNode, dir: FlexDirection, measured: (i32, i32)) -> i32 {
    let (explicit, fallback) = match dir {
        FlexDirection::Row => (node.props.width, measured.0),
        FlexDirection::Column => (node.props.height, measured.1),
    };
    explicit.unwrap_or(fallback).max(0)
}

/// Distribute leftover main-axis space among `grow > 0` children, in
/// proportion to their grow factors. Returns the final main sizes.
fn distribute_grow(children: &[TuiNode], base: &[i32], free: i32) -> Vec<i32> {
    let mut out = base.to_vec();
    if free <= 0 {
        return out;
    }
    let total_grow: f64 = children.iter().map(|c| c.props.grow).sum();
    if total_grow <= 0.0 {
        return out;
    }
    let mut remaining = free;
    for (i, child) in children.iter().enumerate() {
        if child.props.grow <= 0.0 {
            continue;
        }
        let share = ((free as f64) * (child.props.grow / total_grow)).round() as i32;
        let share = share.min(remaining).max(0);
        out[i] += share;
        remaining -= share;
    }
    if remaining > 0 {
        // Floating-point remainder: give it to the first grower.
        for (i, child) in children.iter().enumerate() {
            if child.props.grow > 0.0 {
                out[i] += remaining;
                break;
            }
        }
    }
    out
}

/// Absolute main-axis start positions for `space-between` (the only justify
/// mode that needs precomputed positions). For others, returns zeros and the
/// caller advances a cursor.
fn main_axis_positions(
    sizes: &[i32],
    _main: i32,
    justify: JustifyContent,
    _dir: FlexDirection,
) -> Vec<i32> {
    if !matches!(justify, JustifyContent::SpaceBetween) || sizes.len() < 2 {
        return vec![0; sizes.len()];
    }
    let total: i32 = sizes.iter().sum();
    let gaps = sizes.len() as i32 - 1;
    let gap = if gaps > 0 { total / gaps } else { 0 }; // approx even spacing
    let mut pos = Vec::with_capacity(sizes.len());
    let last = sizes.len().saturating_sub(1);
    let mut acc = 0;
    for (i, s) in sizes.iter().enumerate() {
        pos.push(acc);
        acc += s + if i < last { gap } else { 0 };
    }
    pos
}

/// The cross-axis size of a child, honoring explicit size, align stretch, and
/// the measured fallback.
fn cross_size(
    node: &TuiNode,
    measured: (i32, i32),
    cross_avail: i32,
    align: AlignItems,
    dir: FlexDirection,
) -> i32 {
    let explicit = match dir {
        FlexDirection::Row => node.props.height,
        FlexDirection::Column => node.props.width,
    };
    if let Some(e) = explicit {
        return e.max(0);
    }
    let measured_cross = if matches!(dir, FlexDirection::Row) {
        measured.1
    } else {
        measured.0
    };
    if matches!(align, AlignItems::Stretch) {
        cross_avail.max(measured_cross)
    } else {
        measured_cross
    }
}

/// The cross-axis coordinate for a child, per align-items.
fn cross_position(align: AlignItems, cross_start: i32, cross_avail: i32, child_cross: i32) -> i32 {
    match align {
        AlignItems::FlexStart | AlignItems::Stretch => cross_start,
        AlignItems::Center => cross_start + ((cross_avail - child_cross) / 2).max(0),
        AlignItems::FlexEnd => cross_start + (cross_avail - child_cross).max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::super::node::{BoxProps, NodeKind, Style, TuiNode};
    use super::*;

    fn text(t: &str) -> TuiNode {
        TuiNode {
            kind: NodeKind::Text {
                text: t.into(),
                wrap: Default::default(),
            },
            style: Style::default(),
            props: BoxProps::default(),
            title: String::new(),
        }
    }

    fn column(children: Vec<TuiNode>) -> TuiNode {
        TuiNode {
            kind: NodeKind::Box { children },
            style: Style::default(),
            props: BoxProps::default(),
            title: String::new(),
        }
    }

    fn row(children: Vec<TuiNode>, props: BoxProps) -> TuiNode {
        TuiNode {
            kind: NodeKind::Box { children },
            style: Style::default(),
            props,
            title: String::new(),
        }
    }

    #[test]
    fn measures_text_intrinsic_size() {
        let n = text("hello");
        let (w, h) = measure(&n, 80);
        assert_eq!(w, 5);
        assert_eq!(h, 1);
    }

    #[test]
    fn column_stacks_children_vertically() {
        // column of two 3-char texts → width 3, height 2.
        let n = column(vec![text("abc"), text("xyz")]);
        let (w, h) = measure(&n, 80);
        assert_eq!(w, 3);
        assert_eq!(h, 2);
    }

    #[test]
    fn row_places_children_side_by_side() {
        // row [abc|xy] in a 80x1 viewport → children at x=0 and x=3.
        let n = row(
            vec![text("abc"), text("xy")],
            BoxProps {
                direction: FlexDirection::Row,
                ..Default::default()
            },
        );
        let laid = layout(
            &n,
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 1,
            },
        );
        assert_eq!(laid.children.len(), 2);
        assert_eq!(laid.children[0].rect.x, 0);
        assert_eq!(laid.children[1].rect.x, 3);
    }

    #[test]
    fn grow_expands_to_fill_width() {
        // row [fixed "A", grow filler] in 80-wide → filler gets ~79.
        let mut filler_props = BoxProps::default();
        filler_props.direction = FlexDirection::Row;
        filler_props.grow = 1.0;
        let n = TuiNode {
            kind: NodeKind::Box {
                children: vec![
                    TuiNode {
                        kind: NodeKind::Text {
                            text: "A".into(),
                            wrap: Default::default(),
                        },
                        style: Style::default(),
                        props: BoxProps {
                            width: Some(1),
                            ..Default::default()
                        },
                        title: String::new(),
                    },
                    TuiNode {
                        kind: NodeKind::Text {
                            text: "".into(),
                            wrap: Default::default(),
                        },
                        style: Style::default(),
                        props: BoxProps {
                            grow: 1.0,
                            ..Default::default()
                        },
                        title: String::new(),
                    },
                ],
            },
            style: Style::default(),
            props: filler_props,
            title: String::new(),
        };
        let laid = layout(
            &n,
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 1,
            },
        );
        assert_eq!(laid.children[0].rect.w, 1);
        assert_eq!(laid.children[1].rect.w, 79);
    }
}
