//! Render a laid-out `MeasuredNode` tree into a string frame.
//!
//! A 2-D cell buffer (`Vec<Vec<String>>`) is painted by walking the tree, then
//! flattened to a newline-joined frame. Each leaf writes its glyphs at its
//! assigned rect; boxes optionally draw a border first. Content is clipped to
//! the viewport.

use super::layout::{MeasuredNode, Rect};
use super::node::{NodeKind, Style, WrapMode};
use crate::stdlib::helpers::{rune_width, strip_ansi, visible_width, wrap_line};

/// Paint `root` (and its descendants) into a frame string of `viewport.h`
/// lines, each up to `viewport.w` cells wide.
pub fn render_frame(root: &MeasuredNode<'_>, viewport: Rect) -> String {
    if viewport.w <= 0 || viewport.h <= 0 {
        return String::new();
    }
    let w = viewport.w as usize;
    let h = viewport.h as usize;
    let mut grid: Vec<Vec<String>> = (0..h).map(|_| vec![" ".to_string(); w]).collect();
    paint(root, &mut grid, viewport);
    grid.into_iter()
        .map(|row| row.join(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Recursively paint a node: box border (if any), then content/children.
fn paint(node: &MeasuredNode<'_>, grid: &mut [Vec<String>], viewport: Rect) {
    // Clip: skip nodes fully outside the viewport.
    if node.rect.x >= viewport.w
        || node.rect.y >= viewport.h
        || node.rect.x + node.rect.w <= 0
        || node.rect.y + node.rect.h <= 0
    {
        return;
    }

    // Border first (so content overwrites the inner area).
    if node.node.props.border {
        draw_border(node, grid, viewport);
    }

    match &node.node.kind {
        NodeKind::Box { .. } => {
            for child in &node.children {
                paint(child, grid, viewport);
            }
        }
        NodeKind::Text { text, wrap } => {
            paint_text(text, *wrap, &node.node.style, node.rect, grid, viewport);
        }
        NodeKind::Input {
            value,
            cursor,
            placeholder,
            prompt,
            focused,
        } => {
            paint_input(
                value,
                *cursor,
                placeholder,
                prompt,
                *focused,
                &node.node.style,
                node.rect,
                grid,
                viewport,
            );
        }
        NodeKind::List {
            items,
            selected,
            focused,
        } => {
            paint_list(
                items,
                *selected,
                *focused,
                &node.node.style,
                node.rect,
                grid,
                viewport,
            );
        }
        NodeKind::Table {
            headers,
            rows,
            column_widths,
        } => {
            paint_table(
                headers,
                rows,
                column_widths,
                &node.node.style,
                node.rect,
                grid,
                viewport,
            );
        }
        NodeKind::Progress {
            value,
            total,
            label,
        } => {
            paint_progress(
                *value,
                *total,
                label,
                &node.node.style,
                node.rect,
                grid,
                viewport,
            );
        }
        NodeKind::Checkbox { checked, label } => {
            paint_checkbox(*checked, label, &node.node.style, node.rect, grid, viewport);
        }
    }
}

/// Draw a single-line box border around `node.rect`, with an optional title.
fn draw_border(node: &MeasuredNode<'_>, grid: &mut [Vec<String>], viewport: Rect) {
    let r = clip_rect(node.rect, viewport);
    if r.w < 2 || r.h < 2 {
        return;
    }
    let last_col = r.x + r.w - 1;
    let last_row = r.y + r.h - 1;

    let put = |grid: &mut [Vec<String>], x: i32, y: i32, ch: &str| {
        if let Some(row) = grid.get(y as usize) {
            let _ = row; // borrow check
        }
        if let Some(row) = grid.get_mut(y as usize) {
            if let Some(cell) = row.get_mut(x as usize) {
                *cell = ch.to_string();
            }
        }
    };

    // Corners.
    put(grid, r.x, r.y, "┌");
    put(grid, last_col, r.y, "┐");
    put(grid, r.x, last_row, "└");
    put(grid, last_col, last_row, "┘");
    // Top/bottom edges.
    for x in (r.x + 1)..last_col {
        put(grid, x, r.y, "─");
        put(grid, x, last_row, "─");
    }
    // Left/right edges.
    for y in (r.y + 1)..last_row {
        put(grid, r.x, y, "│");
        put(grid, last_col, y, "│");
    }

    // Optional title on the top edge.
    let title = title_of(node.node);
    if !title.is_empty() {
        let label = format!(" {} ", title);
        let start = r.x + 1;
        for (i, ch) in label.chars().enumerate() {
            let x = start + i as i32;
            if x < last_col {
                put(grid, x, r.y, &ch.to_string());
            }
        }
    }
}

/// Render styled text into `rect`, honoring wrap mode.
fn paint_text(
    text: &str,
    wrap: WrapMode,
    style: &Style,
    rect: Rect,
    grid: &mut [Vec<String>],
    viewport: Rect,
) {
    let r = clip_rect(rect, viewport);
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    let inner = text_content_box(rect, viewport);
    if inner.w <= 0 || inner.h <= 0 {
        return;
    }
    let stripped = strip_ansi(text);
    let lines: Vec<String> = match wrap {
        WrapMode::Wrap => wrap_line(&stripped.replace('\n', " "), inner.w as usize),
        _ => vec![stripped.replace('\n', " ")],
    };
    let styled = |s: &str| style_apply(style, s);
    for (row, line) in lines.iter().take(inner.h as usize).enumerate() {
        let mut displayed = strip_ansi(line);
        if displayed.chars().count() > inner.w as usize {
            displayed = match wrap {
                WrapMode::End => {
                    let kept: String = displayed
                        .chars()
                        .take(inner.w.saturating_sub(1) as usize)
                        .collect();
                    format!("{}…", kept)
                }
                _ => displayed.chars().take(inner.w as usize).collect(),
            };
        }
        write_row(
            grid,
            inner.x,
            inner.y + row as i32,
            &styled(&displayed),
            viewport,
        );
    }
}

/// Write a string into a single grid row, left-aligned. ANSI escape sequences
/// are preserved but do NOT consume grid columns (they're appended to the
/// current cell), so visible layout is unaffected by styling.
fn write_row(grid: &mut [Vec<String>], x: i32, y: i32, text: &str, viewport: Rect) {
    if y < 0 || y >= viewport.h {
        return;
    }
    let Some(row) = grid.get_mut(y as usize) else {
        return;
    };
    let mut col = x;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // CSI escape: append the whole sequence to the current cell.
            let start = i;
            i += 2;
            while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // consume the final byte
            }
            if col >= 0 && col < viewport.w {
                if let Some(cell) = row.get_mut(col as usize) {
                    cell.push_str(std::str::from_utf8(&bytes[start..i]).unwrap_or(""));
                }
            }
            continue;
        }
        if col >= viewport.w {
            break;
        }
        // One UTF-8 char. Advance by terminal display width, not by scalar
        // count, so CJK glyphs do not push the physical terminal into an
        // unintended wrap that leaves stale rows behind.
        let start = i;
        i += 1;
        while i < bytes.len() && (bytes[i] & 0xc0) == 0x80 {
            i += 1;
        }
        if let Ok(ch) = std::str::from_utf8(&bytes[start..i]) {
            let rune = ch.chars().next().unwrap_or('\u{fffd}');
            let width = rune_width(rune).max(1) as i32;
            if col >= 0 && col < viewport.w {
                if let Some(cell) = row.get_mut(col as usize) {
                    *cell = ch.to_string();
                }
                for pad_col in (col + 1)..(col + width).min(viewport.w) {
                    if let Some(cell) = row.get_mut(pad_col as usize) {
                        *cell = String::new();
                    }
                }
            }
            col += width;
        } else {
            col += 1;
        }
    }
}

/// Apply ANSI styling to a string per a Style spec.
fn style_apply(style: &Style, text: &str) -> String {
    let mut codes: Vec<&str> = Vec::new();
    if style.bold {
        codes.push("1");
    }
    if style.dim {
        codes.push("2");
    }
    if style.underline {
        codes.push("4");
    }
    if style.inverse {
        codes.push("7");
    }
    if let Some(fg) = color_code(style.fg.as_deref(), false) {
        codes.push(fg);
    }
    if let Some(bg) = color_code(style.bg.as_deref(), true) {
        codes.push(bg);
    }
    if codes.is_empty() {
        text.to_string()
    } else {
        format!("\x1b[{}m{}\x1b[0m", codes.join(";"), text)
    }
}

/// Map a named color to its ANSI code (subset matching terminal.style).
fn color_code(name: Option<&str>, bg: bool) -> Option<&'static str> {
    let n = name?;
    let code = match n.to_ascii_lowercase().as_str() {
        "black" => "30",
        "red" | "error" => "31",
        "green" | "success" => "32",
        "yellow" | "warn" | "warning" => "33",
        "blue" | "accent" => "34",
        "magenta" => "35",
        "cyan" => "36",
        "white" => "37",
        "gray" | "grey" | "muted" => "90",
        _ => return None,
    };
    if bg {
        // 30→40, 90→100.
        let n: u32 = code.parse().ok()?;
        Some(BG_CODES.get(&(n as usize)).copied().unwrap_or(""))
    } else {
        Some(code)
    }
}

static BG_CODES: std::sync::LazyLock<std::collections::HashMap<usize, &'static str>> =
    std::sync::LazyLock::new(|| {
        let mut m = std::collections::HashMap::new();
        m.insert(30, "40");
        m.insert(31, "41");
        m.insert(32, "42");
        m.insert(33, "43");
        m.insert(34, "44");
        m.insert(35, "45");
        m.insert(36, "46");
        m.insert(37, "47");
        m.insert(90, "100");
        m
    });

/// The content area inside a node's rect (after border + padding).
fn text_content_box(rect: Rect, viewport: Rect) -> Rect {
    let border = 0; // text nodes have no own border; padding handled by parent box.
    let _ = viewport;
    Rect {
        x: rect.x + border,
        y: rect.y + border,
        w: rect.w - border * 2,
        h: rect.h - border * 2,
    }
}

/// Clip a rect to the viewport.
fn clip_rect(rect: Rect, viewport: Rect) -> Rect {
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = (rect.x + rect.w).min(viewport.w);
    let y1 = (rect.y + rect.h).min(viewport.h);
    Rect {
        x: x0,
        y: y0,
        w: (x1 - x0).max(0),
        h: (y1 - y0).max(0),
    }
}

fn title_of(node: &super::node::TuiNode) -> &str {
    node.title.as_str()
}

// ---------------------------------------------------------------------------
// Component painters
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn paint_input(
    value: &str,
    cursor: i32,
    placeholder: &str,
    prompt: &str,
    focused: bool,
    style: &Style,
    rect: Rect,
    grid: &mut [Vec<String>],
    viewport: Rect,
) {
    let inner = text_content_box(rect, viewport);
    if inner.w <= 0 {
        return;
    }
    let prompt_w = visible_width(prompt) as i32;
    let value_w = (inner.w - prompt_w).max(1);
    write_row(
        grid,
        inner.x,
        inner.y,
        &style_apply(style, prompt),
        viewport,
    );

    let display = if value.is_empty() && !placeholder.is_empty() {
        style_apply(
            &Style {
                dim: true,
                ..Style::default()
            },
            placeholder,
        )
    } else {
        let chars: Vec<char> = value.chars().collect();
        let cur = (cursor.max(0) as usize).min(chars.len());
        if focused {
            let before: String = chars[..cur].iter().collect();
            let after: String = chars[cur..].iter().collect();
            format!("{}\x1b[7m \x1b[0m{}", before, after)
        } else {
            value.to_string()
        }
    };
    let cropped = crop_to_width(&display, value_w as usize);
    write_row(grid, inner.x + prompt_w, inner.y, &cropped, viewport);
}

fn paint_list(
    items: &[String],
    selected: i32,
    focused: bool,
    style: &Style,
    rect: Rect,
    grid: &mut [Vec<String>],
    viewport: Rect,
) {
    let inner = text_content_box(rect, viewport);
    if inner.w <= 0 {
        return;
    }
    for (i, item) in items.iter().take(inner.h as usize).enumerate() {
        let is_sel = i as i32 == selected;
        let marker = if is_sel { "› " } else { "  " };
        let line = format!("{}{}", marker, item);
        let rendered = if is_sel && focused {
            style_apply(
                &Style {
                    inverse: true,
                    ..style.clone()
                },
                &line,
            )
        } else if is_sel {
            style_apply(
                &Style {
                    bold: true,
                    ..style.clone()
                },
                &line,
            )
        } else {
            line
        };
        let cropped = crop_to_width(&rendered, inner.w as usize);
        write_row(grid, inner.x, inner.y + i as i32, &cropped, viewport);
    }
}

fn paint_table(
    headers: &[String],
    rows: &[Vec<String>],
    column_widths: &[i32],
    style: &Style,
    rect: Rect,
    grid: &mut [Vec<String>],
    viewport: Rect,
) {
    let inner = text_content_box(rect, viewport);
    if inner.w <= 0 {
        return;
    }
    let cols = column_widths.len().max(headers.len());
    if cols == 0 {
        return;
    }
    let mut widths: Vec<i32> = vec![0; cols];
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
    for (i, w) in column_widths.iter().enumerate() {
        if i < cols && *w > 0 {
            widths[i] = *w;
        }
    }

    let mut render_row = |cells: &[String], y: i32, bold: bool| {
        let mut x = inner.x;
        let row_style = if bold {
            Style {
                bold: true,
                ..style.clone()
            }
        } else {
            style.clone()
        };
        for (i, cell) in cells.iter().enumerate().take(cols) {
            if i >= cols {
                break;
            }
            let w = widths[i] as usize;
            let padded = pad_to_width(cell, w);
            write_row(grid, x, y, &style_apply(&row_style, &padded), viewport);
            x += widths[i] + 1; // +1 column gap
        }
    };

    if !headers.is_empty() {
        render_row(headers, inner.y, true);
    }
    let header_offset = if headers.is_empty() { 0 } else { 1 };
    for (ri, row) in rows.iter().enumerate() {
        let y = inner.y + header_offset + ri as i32;
        if y >= inner.y + inner.h {
            break;
        }
        render_row(row, y, false);
    }
}

fn paint_progress(
    value: f64,
    total: f64,
    label: &str,
    style: &Style,
    rect: Rect,
    grid: &mut [Vec<String>],
    viewport: Rect,
) {
    let inner = text_content_box(rect, viewport);
    if inner.w <= 4 {
        return;
    }
    let ratio = if total > 0.0 {
        (value / total).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let bar_w = inner.w - 2; // [  ]
    let filled = (ratio * bar_w as f64).round() as i32;
    let bar: String =
        "█".repeat(filled.max(0) as usize) + &"░".repeat((bar_w - filled.max(0)).max(0) as usize);
    let line = if label.is_empty() {
        format!("[{}]", bar)
    } else {
        format!("{} [{}]", label, bar)
    };
    let cropped = crop_to_width(&style_apply(style, &line), inner.w as usize);
    write_row(grid, inner.x, inner.y, &cropped, viewport);
}

fn paint_checkbox(
    checked: bool,
    label: &str,
    style: &Style,
    rect: Rect,
    grid: &mut [Vec<String>],
    viewport: Rect,
) {
    let inner = text_content_box(rect, viewport);
    if inner.w <= 0 {
        return;
    }
    let mark = if checked { "x" } else { " " };
    let line = format!("[{}] {}", mark, label);
    let cropped = crop_to_width(&style_apply(style, &line), inner.w as usize);
    write_row(grid, inner.x, inner.y, &cropped, viewport);
}

/// Crop a string (which may contain ANSI codes) to at most `width` visible
/// cells, preserving escape sequences.
fn crop_to_width(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0usize;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            // Copy the whole escape sequence verbatim.
            out.push(bytes[i] as char);
            i += 1;
            if i < bytes.len() && bytes[i] == b'[' {
                out.push('[');
                i += 1;
                while i < bytes.len() {
                    out.push(bytes[i] as char);
                    if (0x40..=0x7e).contains(&bytes[i]) {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            } else {
                // OSC or lone ESC; copy one more.
                if i < bytes.len() {
                    out.push(bytes[i] as char);
                    i += 1;
                }
            }
            continue;
        }
        if used >= width {
            break;
        }
        // Copy one UTF-8 char, accounting for terminal display width.
        let start = i;
        i += 1;
        while i < bytes.len() && (bytes[i] & 0xc0) == 0x80 {
            i += 1;
        }
        if let Ok(ch) = std::str::from_utf8(&bytes[start..i]) {
            let rune = ch.chars().next().unwrap_or('\u{fffd}');
            let w = rune_width(rune).max(1);
            if used + w > width {
                break;
            }
            out.push_str(ch);
            used += w;
        }
    }
    if used >= width {
        out.push_str("\x1b[0m");
    }
    out
}

/// Pad/truncate a plain (no-ANSI) string to exactly `width` cells.
fn pad_to_width(s: &str, width: usize) -> String {
    let w = visible_width(s);
    if w >= width {
        s.chars().take(width).collect()
    } else {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(width - w));
        out
    }
}

#[cfg(test)]
mod tests {
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
}
