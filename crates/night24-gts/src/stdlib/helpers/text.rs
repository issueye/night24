// ---------------------------------------------------------------------------
// text: display-width utilities for terminal-aware string handling.
// ---------------------------------------------------------------------------

/// Remove CSI (`ESC [ ...`) and OSC (`ESC ] ... BEL/ST`) escape sequences.
pub(crate) fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'[' => {
                    // CSI: skip until a 0x40-0x7e final byte.
                    i += 2;
                    while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                }
                b']' => {
                    // OSC: skip until BEL or ST (ESC \).
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    out.push(bytes[i] as char);
                    i += 1;
                }
            }
        } else {
            // Copy this UTF-8 character wholesale.
            let ch_start = i;
            i += 1;
            while i < bytes.len() && (bytes[i] & 0xc0) == 0x80 {
                i += 1;
            }
            out.push_str(std::str::from_utf8(&bytes[ch_start..i]).unwrap_or("\u{fffd}"));
        }
    }
    out
}

/// Display width of a string after stripping ANSI escapes (pure; no CallContext).
pub(crate) fn visible_width(value: &str) -> usize {
    let stripped = strip_ansi(value);
    let mut total = 0usize;
    for r in stripped.chars() {
        if r == '\n' || r == '\r' {
            // width is per-line; callers split on \n first.
            continue;
        }
        total += rune_width(r);
    }
    total
}

/// Wrap a single line (no embedded newlines) to at most `width` display cells.
/// Returns the wrapped lines (pure; no CallContext).
pub(crate) fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let stripped = strip_ansi(line);
    let mut current = String::new();
    let mut used = 0usize;
    let mut out = Vec::new();
    for r in stripped.chars() {
        let w = rune_width(r);
        if used + w > width && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(r);
        used += w;
    }
    out.push(current);
    out
}

/// Display width of a single rune per the Go original's rules.
pub(crate) fn rune_width(r: char) -> usize {
    let code = r as u32;
    if code == 0 || code == '\n' as u32 || code == '\r' as u32 || code == '\t' as u32 {
        return 0;
    }
    if is_combining_rune(r) {
        return 0;
    }
    if is_wide_rune(code) {
        2
    } else {
        1
    }
}

pub(crate) fn is_combining_rune(r: char) -> bool {
    // Combining Diacritical Marks (Mn) and Combining Marks for Symbols (Me).
    matches!(r as u32,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2
        | 0x05C4..=0x05C5 | 0x05C7 | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670
        | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x06E7..=0x06E8 | 0x06EA..=0x06ED
        | 0x0711 | 0x0730..=0x074A | 0x07A6..=0x07B0 | 0x07EB..=0x07F3
        | 0x0816..=0x0819 | 0x081B..=0x0823 | 0x0825..=0x0827 | 0x0829..=0x082D
        | 0x0859..=0x085B | 0x08D4..=0x08E1 | 0x08E3..=0x0902 | 0x093A
        | 0x093C | 0x0941..=0x0948 | 0x094D | 0x0951..=0x0957 | 0x0962..=0x0963
        | 0x0981 | 0x09BC | 0x09C1..=0x09C4 | 0x09CD | 0x09E2..=0x09E3
        | 0x0A01..=0x0A02 | 0x0A3C | 0x0A41..=0x0A42 | 0x0A47..=0x0A48
        | 0x0A4B..=0x0A4D | 0x0A51 | 0x0A70..=0x0A71 | 0x0A75
        | 0x0A81..=0x0A82 | 0x0ABC | 0x0AC1..=0x0AC5 | 0x0AC7..=0x0AC8
        | 0x0ACD | 0x0AE2..=0x0AE3 | 0x0B01 | 0x0B3C | 0x0B3F
        | 0x0B41..=0x0B44 | 0x0B4D | 0x0B56 | 0x0B62..=0x0B63
        | 0x0B82 | 0x0BC0 | 0x0BCD | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F
    )
}

pub(crate) fn is_wide_rune(r: u32) -> bool {
    matches!(r,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF | 0x20000..=0x3FFFD
    )
}
