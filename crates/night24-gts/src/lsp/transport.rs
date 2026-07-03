//! LSP JSON-RPC transport over stdio.
//!
//! The Language Server Protocol frames messages with a `Content-Length` header
//! followed by a blank line and a UTF-8 JSON body:
//!
//! ```text
//! Content-Length: 123\r\n
//! \r\n
//! { ...json... }
//! ```
//!
//! This module reads framed messages from stdin and writes framed responses to
//! stdout. It uses only `serde_json` (already a dependency) and std — no
//! `tower-lsp`, keeping the language server dependency-free (decision per
//! `phase-1-development-plan.md` W2).

use std::io::{self, BufRead, Write};

use serde_json::Value;

/// Read one framed JSON-RPC message from `reader`. Returns `Ok(None)` on EOF.
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    let mut line = String::new();

    // Parse headers until a blank line.
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            // EOF before any header → end of stream.
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            // Blank line separates headers from the body.
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            if let Ok(len) = rest.trim().parse::<usize>() {
                content_length = Some(len);
            }
        }
        // Other headers (Content-Type, …) are ignored.
    }

    let len = content_length.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "LSP message missing Content-Length header",
        )
    })?;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    let value = serde_json::from_slice(&buf).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("LSP message body is not valid JSON: {e}"),
        )
    })?;
    Ok(Some(value))
}

/// Write one framed JSON-RPC message to `writer` (flushes after writing).
pub fn write_message(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}
