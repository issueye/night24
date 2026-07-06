use super::super::layout::layout;
use super::super::node::{BoxProps, NodeKind, Style, TuiNode, WrapMode};
use super::*;

fn text_node(t: &str) -> TuiNode {
    TuiNode {
        kind: NodeKind::Text {
            text: t.into(),
            wrap: WrapMode::Truncate,
        },
        style: Style::default(),
        props: BoxProps::default(),
        title: String::new(),
    }
}

#[test]
fn renders_single_text() {
    let node = text_node("Hi");
    let laid = layout(
        &node,
        Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 1,
        },
    );
    let frame = render_frame(
        &laid,
        Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 1,
        },
    );
    // "Hi" left-aligned, padded to width 10.
    assert_eq!(frame, "Hi        ");
}

#[test]
fn renders_wide_text_without_overflowing_display_width() {
    let node = text_node("你好");
    let laid = layout(
        &node,
        Rect {
            x: 0,
            y: 0,
            w: 6,
            h: 1,
        },
    );
    let frame = render_frame(
        &laid,
        Rect {
            x: 0,
            y: 0,
            w: 6,
            h: 1,
        },
    );
    assert_eq!(visible_width(&frame), 6);
    assert_eq!(frame, "你好  ");
}

#[test]
fn input_preserves_prompt_and_wide_value_width() {
    let node = TuiNode {
        kind: NodeKind::Input {
            value: "你".into(),
            cursor: 1,
            placeholder: String::new(),
            prompt: "> ".into(),
            focused: false,
        },
        style: Style::default(),
        props: BoxProps {
            width: Some(6),
            ..Default::default()
        },
        title: String::new(),
    };
    let laid = layout(
        &node,
        Rect {
            x: 0,
            y: 0,
            w: 6,
            h: 1,
        },
    );
    let frame = render_frame(
        &laid,
        Rect {
            x: 0,
            y: 0,
            w: 6,
            h: 1,
        },
    );
    assert_eq!(visible_width(&frame), 6);
    assert_eq!(frame, "> 你  ");
}

#[test]
fn renders_box_with_border() {
    let node = TuiNode {
        kind: NodeKind::Box {
            children: vec![text_node("X")],
        },
        style: Style::default(),
        props: BoxProps {
            border: true,
            width: Some(5),
            height: Some(3),
            ..Default::default()
        },
        title: String::new(),
    };
    let laid = layout(
        &node,
        Rect {
            x: 0,
            y: 0,
            w: 5,
            h: 3,
        },
    );
    let frame = render_frame(
        &laid,
        Rect {
            x: 0,
            y: 0,
            w: 5,
            h: 3,
        },
    );
    let lines: Vec<&str> = frame.lines().collect();
    assert_eq!(lines[0], "┌───┐");
    assert_eq!(lines[1], "│X  │");
    assert_eq!(lines[2], "└───┘");
}

#[test]
fn renders_progress_bar() {
    let node = TuiNode {
        kind: NodeKind::Progress {
            value: 5.0,
            total: 10.0,
            label: String::new(),
        },
        style: Style::default(),
        props: BoxProps {
            width: Some(8),
            ..Default::default()
        },
        title: String::new(),
    };
    let laid = layout(
        &node,
        Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 1,
        },
    );
    let frame = render_frame(
        &laid,
        Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 1,
        },
    );
    // [███░░░] = 8 chars total (6 inner, 3 filled).
    assert_eq!(frame, "[███░░░]");
}
