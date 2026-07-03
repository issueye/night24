// ---------------------------------------------------------------------------
// rate-limit: token-bucket rate limiter (@std/rate-limit)
// ---------------------------------------------------------------------------

pub(crate) fn terminal_color_code(name: &str, bg: bool) -> Option<i32> {
    let base = match name.to_ascii_lowercase().as_str() {
        "black" => 30,
        "red" | "error" => 31,
        "green" | "success" => 32,
        "yellow" | "warning" => 33,
        "blue" => 34,
        "magenta" => 35,
        "cyan" | "accent" => 36,
        "white" => 37,
        "gray" | "grey" | "muted" => 90,
        _ => return None,
    };
    Some(if bg { base + 10 } else { base })
}

pub(crate) fn terminal_style_string(text: &str, fg: &str, bold: bool) -> String {
    let mut codes: Vec<i32> = Vec::new();
    if bold {
        codes.push(1);
    }
    if let Some(code) = terminal_color_code(fg, false) {
        codes.push(code);
    }
    if codes.is_empty() {
        text.to_string()
    } else {
        let joined: Vec<String> = codes.iter().map(|c| c.to_string()).collect();
        format!("\x1b[{}m{}\x1b[0m", joined.join(";"), text)
    }
}

// ---------------------------------------------------------------------------
// sse: Server-Sent Events parser + stream reader (@std/sse)
// ---------------------------------------------------------------------------
