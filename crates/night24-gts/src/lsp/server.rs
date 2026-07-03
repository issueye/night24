//! GoScript Language Server Protocol (LSP) server.
//!
//! MVP scope (`phase-1-development-plan.md` W2): hand-written JSON-RPC over
//! stdio (no `tower-lsp`), with these capabilities:
//!   - `initialize` / `initialized` / `shutdown` / `exit` lifecycle
//!   - `textDocument/didOpen` / `/didChange` / `/didClose` (full sync)
//!   - `textDocument/diagnostic` (syntax errors from lexer+parser)
//!   - `textDocument/completion` (`@std/*` module names + globals)
//!   - `textDocument/hover` (built-in symbols)
//!   - `textDocument/definition` (best-effort: returns the document's start)
//!
//! The server does NOT use the VM — it parses source directly for diagnostics.
//! Parsing reuses the front-end pipeline (`lexer::Lexer` + `parser::Parser`).
//!
//! Run with: `gs lsp`.

use std::io::{self, BufReader, Write};

use serde_json::{json, Value};

use crate::lexer::Lexer;
use crate::parser::Parser;

use super::document::DocumentStore;
use super::transport;

/// Run the language server on stdio until EOF or `exit` notification.
pub fn run_server() -> io::Result<()> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    let mut state = ServerState::new();

    loop {
        let message = match transport::read_message(&mut reader)? {
            Some(msg) => msg,
            None => return Ok(()), // EOF
        };
        let should_exit = state.handle(message, &mut writer)?;
        if should_exit {
            return Ok(());
        }
    }
}

struct ServerState {
    documents: DocumentStore,
    shutdown: bool,
}

impl ServerState {
    fn new() -> Self {
        Self {
            documents: DocumentStore::new(),
            shutdown: false,
        }
    }

    /// Handle one message. Writes any response to `writer`. Returns `true` if
    /// the server should stop (after `exit`).
    fn handle(&mut self, message: Value, writer: &mut impl Write) -> io::Result<bool> {
        let method = message
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        match method.as_str() {
            "initialize" => {
                let result = self.initialize(&params);
                respond(writer, id, result)?;
            }
            "initialized" => { /* no-op notification */ }
            "shutdown" => {
                self.shutdown = true;
                respond(writer, id, Value::Null)?;
            }
            "exit" => {
                return Ok(true);
            }
            "textDocument/didOpen" => self.handle_did_open(&params),
            "textDocument/didChange" => self.handle_did_change(&params),
            "textDocument/didClose" => self.handle_did_close(&params),
            "textDocument/diagnostic" => {
                let result = self.diagnostic(&params);
                respond(writer, id, result)?;
            }
            "textDocument/completion" => {
                let result = self.completion();
                respond(writer, id, result)?;
            }
            "textDocument/hover" => {
                let result = self.hover(&params);
                respond(writer, id, result)?;
            }
            "textDocument/definition" => {
                let result = self.definition(&params);
                respond(writer, id, result)?;
            }
            _ => {
                // Unknown request → method-not-found error (only for requests).
                if id.is_some() {
                    error(writer, id, -32601, &format!("method not found: {method}"))?;
                }
                // Unknown notifications are silently ignored per LSP spec.
            }
        }
        Ok(false)
    }

    /// `initialize`: advertise capabilities.
    fn initialize(&self, _params: &Value) -> Value {
        json!({
            "capabilities": {
                "textDocumentSync": 1, // 1 = Full document sync
                "diagnosticProvider": { "interFileDependencies": false, "workspaceDiagnostics": false },
                "completionProvider": { "triggerCharacters": [".", "/", "\""] },
                "hoverProvider": true,
                "definitionProvider": true
            },
            "serverInfo": { "name": "goscript-lsp", "version": crate::VERSION }
        })
    }

    fn handle_did_open(&mut self, params: &Value) {
        if let (Some(uri), Some(text)) = extract_text_document_identifier_text(params) {
            self.documents.open(uri, text);
        }
    }

    fn handle_did_change(&mut self, params: &Value) {
        let Some(td) = params.get("textDocument") else {
            return;
        };
        let Some(uri) = td.get("uri").and_then(|v| v.as_str()) else {
            return;
        };
        // Full sync: the last change's full text is authoritative.
        let Some(changes) = params.get("contentChanges").and_then(|v| v.as_array()) else {
            return;
        };
        for change in changes {
            if let Some(text) = change.get("text").and_then(|v| v.as_str()) {
                self.documents.update(uri, text);
            }
        }
    }

    fn handle_did_close(&mut self, params: &Value) {
        if let Some(uri) = extract_text_document_identifier(params) {
            self.documents.close(&uri);
        }
    }

    /// `textDocument/diagnostic`: parse the document, return syntax errors.
    fn diagnostic(&self, params: &Value) -> Value {
        let Some(uri) = extract_text_document_identifier(params) else {
            return json!({ "kind": "full", "items": [] });
        };
        let Some(text) = self.documents.get(&uri) else {
            return json!({ "kind": "full", "items": [] });
        };
        let diags = parse_diagnostics(&uri, text);
        json!({ "kind": "full", "items": diags })
    }

    /// `textDocument/completion`: `@std/*` module names + common globals.
    fn completion(&self) -> Value {
        let items: Vec<Value> = stdlib_module_names()
            .iter()
            .map(|name| {
                json!({
                    "label": name,
                    "kind": 9, // Module
                    "detail": "GoScript standard library"
                })
            })
            .chain(global_completions())
            .collect();
        json!({ "isIncomplete": false, "items": items })
    }

    /// `textDocument/hover`: best-effort documentation for a symbol.
    fn hover(&self, params: &Value) -> Value {
        let Some(td) = params.get("textDocument") else {
            return Value::Null;
        };
        let Some(uri) = td.get("uri").and_then(|v| v.as_str()) else {
            return Value::Null;
        };
        let Some(position) = params.get("position") else {
            return Value::Null;
        };
        let Some(text) = self.documents.get(uri) else {
            return Value::Null;
        };
        let line = position.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let character = position
            .get("character")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        if let Some(word) = word_at(text, line, character) {
            if let Some(doc) = builtin_hover(&word) {
                return json!({
                    "contents": { "kind": "markdown", "value": doc }
                });
            }
        }
        Value::Null
    }

    /// `textDocument/definition`: best-effort — locate a function/variable
    /// declaration in the same document and return its position.
    fn definition(&self, params: &Value) -> Value {
        let Some(td) = params.get("textDocument") else {
            return Value::Null;
        };
        let Some(uri) = td.get("uri").and_then(|v| v.as_str()) else {
            return Value::Null;
        };
        let Some(position) = params.get("position") else {
            return Value::Null;
        };
        let Some(text) = self.documents.get(uri) else {
            return Value::Null;
        };
        let line = position.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let character = position
            .get("character")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        if let Some(word) = word_at(text, line, character) {
            if let Some((def_line, def_char)) = find_declaration(text, &word) {
                return location(uri, def_line, def_char);
            }
        }
        Value::Null
    }
}

// ---------------------------------------------------------------------------
// Diagnostics: parse and map errors to LSP Diagnostic[]
// ---------------------------------------------------------------------------

/// Parse `text` and return LSP diagnostics for syntax/parse errors.
fn parse_diagnostics(uri: &str, text: &str) -> Vec<Value> {
    let lex = Lexer::new(text);
    let mut parser = Parser::new(lex, uri);
    let program = parser.parse_program();
    program
        .errors
        .iter()
        .map(|err| {
            // Parser error messages are "<file>:<line>:<col>: <message>" or
            // plain messages. Try to extract a position; fall back to line 0.
            let (line, col, message) = parse_error_span(err);
            json!({
                "range": range(line, col, line, col),
                "severity": 1, // Error
                "source": "goscript",
                "message": message
            })
        })
        .collect()
}

/// Split a parser error string of the form `file:line:col: message` into
/// `(line0, col0, message)`. LSP lines are 0-based; the parser uses 1-based.
fn parse_error_span(err: &str) -> (usize, usize, String) {
    // Find the last `: ` that follows a `line:col` pattern.
    // Message often starts after "<file>:<line>:<col>: ".
    let bytes = err.as_bytes();
    let mut colons = Vec::new();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b':' {
            colons.push(i);
        }
    }
    // Need at least: file : line : col : message → 3 colons.
    if colons.len() >= 3 {
        let c2 = colons[colons.len() - 3];
        let c1 = colons[colons.len() - 2];
        let c0 = colons[colons.len() - 1];
        let line_str = &err[c2 + 1..c1];
        let col_str = &err[c1 + 1..c0];
        let message = err[c0 + 1..].trim().to_string();
        if let (Ok(line), Ok(col)) = (
            line_str.trim().parse::<usize>(),
            col_str.trim().parse::<usize>(),
        ) {
            // Convert 1-based to 0-based.
            return (line.saturating_sub(1), col.saturating_sub(1), message);
        }
    }
    (0, 0, err.to_string())
}

// ---------------------------------------------------------------------------
// Completion / hover data
// ---------------------------------------------------------------------------

/// The `@std/*` module registry, mirroring `stdlib/mod.rs::load_native_module`.
fn stdlib_module_names() -> Vec<&'static str> {
    vec![
        "@std/fs",
        "@std/path",
        "@std/os",
        "@std/env",
        "@std/json",
        "@std/time",
        "@std/timers",
        "@std/crypto",
        "@std/hash",
        "@std/random",
        "@std/regexp",
        "@std/semver",
        "@std/collections",
        "@std/process",
        "@std/text",
        "@std/url",
        "@std/cache",
        "@std/glob",
        "@std/color",
        "@std/diff",
        "@std/log",
        "@std/table",
        "@std/validation",
        "@std/template",
        "@std/compression",
        "@std/compress/gzip",
        "@std/terminal",
        "@std/cli",
        "@std/tui",
        "@std/toml",
        "@std/yaml",
        "@std/xml",
        "@std/markdown",
        "@std/schema",
        "@std/test",
        "@std/archive/zip",
        "@std/buffer",
        "@std/events",
        "@std/jwt",
        "@std/mime",
        "@std/net/ip",
        "@std/retry",
        "@std/stream",
        "@std/exec",
        "@std/http",
        "@std/rate-limit",
        "@std/prometheus",
        "@std/highlight",
        "@std/sse",
        "@std/db",
        "@std/mail",
        "@std/net/socket/client",
        "@std/net/socket/server",
        "@std/net/ws/client",
        "@std/net/ws/server",
        "@std/net/http/server",
        "@std/web",
        "@std/express",
        "@std/signal",
        "@std/watch",
        "@std/async",
        "@std/pty",
        "@std/runtime",
        "@std/image",
        "@std/pdf",
        "@std/encoding/base64",
        "@std/encoding/hex",
        "@std/encoding/csv",
    ]
}

/// A few always-available global completions.
fn global_completions() -> Vec<Value> {
    [
        "println", "print", "require", "Math", "JSON", "Object", "Array", "String", "Number",
        "Boolean",
    ]
    .iter()
    .map(|name| {
        json!({ "label": name, "kind": 6 }) // Variable
    })
    .collect()
}

/// Hover docs for common built-in symbols (best-effort MVP).
fn builtin_hover(word: &str) -> Option<String> {
    match word {
        "println" => Some("`println(...args)`\n\nPrint arguments to stdout, followed by a newline.".into()),
        "print" => Some("`print(...args)`\n\nPrint arguments to stdout (no trailing newline).".into()),
        "require" => Some("`require(specifier)`\n\nLoad a module (relative path or `@std/*`) and return its exports.".into()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Text helpers (LSP positions are 0-based line/character)
// ---------------------------------------------------------------------------

/// Return the word (identifier run) at a 0-based (line, character).
fn word_at(text: &str, line: usize, character: usize) -> Option<String> {
    let line_str = text.lines().nth(line)?;
    let bytes = line_str.as_bytes();
    if character > bytes.len() {
        return None;
    }
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'@' || b == b'/';
    let mut start = character.min(bytes.len().saturating_sub(1));
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = character;
    while end < bytes.len() && is_ident(bytes[end]) {
        end += 1;
    }
    if end <= start {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes[start..end]).into_owned())
}

/// Find the first declaration of `name` (`let/const/var/function name`) and
/// return its 0-based (line, character).
fn find_declaration(text: &str, name: &str) -> Option<(usize, usize)> {
    // Match `name` appearing after a declarator keyword or as a function name.
    let patterns = [
        format!("function {name}"),
        format!("let {name}"),
        format!("const {name}"),
        format!("var {name}"),
    ];
    for (i, line) in text.lines().enumerate() {
        for pat in &patterns {
            if let Some(idx) = line.find(pat) {
                return Some((i, idx));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// JSON-RPC framing helpers
// ---------------------------------------------------------------------------

fn respond(writer: &mut impl Write, id: Option<Value>, result: Value) -> io::Result<()> {
    let msg = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    transport::write_message(writer, &msg)
}

fn error(writer: &mut impl Write, id: Option<Value>, code: i64, message: &str) -> io::Result<()> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    });
    transport::write_message(writer, &msg)
}

fn range(start_line: usize, start_char: usize, end_line: usize, end_char: usize) -> Value {
    json!({
        "start": { "line": start_line, "character": start_char },
        "end": { "line": end_line, "character": end_char }
    })
}

fn location(uri: &str, line: usize, character: usize) -> Value {
    json!({
        "uri": uri,
        "range": range(line, character, line, character)
    })
}

fn extract_text_document_identifier(params: &Value) -> Option<String> {
    params
        .get("textDocument")
        .and_then(|v| v.get("uri"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn extract_text_document_identifier_text(params: &Value) -> (Option<String>, Option<String>) {
    let uri = extract_text_document_identifier(params);
    let text = params
        .get("textDocument")
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    (uri, text)
}

/// Re-export for tests that want to parse source the same way the server does.
pub fn parse_program(uri: &str, text: &str) -> crate::ast::Program {
    let lex = Lexer::new(text);
    let mut parser = Parser::new(lex, uri);
    parser.parse_program()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_at_extracts_identifier() {
        let text = "let value = 1;";
        assert_eq!(word_at(text, 0, 5), Some("value".to_string()));
        assert_eq!(word_at(text, 0, 4), Some("value".to_string()));
    }

    #[test]
    fn parse_error_span_extracts_line_col_message() {
        let (line, col, msg) = parse_error_span("file.gs:3:7: oops");
        assert_eq!((line, col), (2, 6));
        assert_eq!(msg, "oops");
    }

    #[test]
    fn parse_error_span_falls_back_for_plain_message() {
        let (line, col, msg) = parse_error_span("plain error");
        assert_eq!((line, col), (0, 0));
        assert_eq!(msg, "plain error");
    }

    #[test]
    fn find_declaration_locates_function() {
        let text = "println(\"hi\");\nfunction main() {}\n";
        assert_eq!(find_declaration(text, "main"), Some((1, 0)));
    }

    #[test]
    fn diagnostics_from_syntax_error() {
        // `function {` reliably yields a parser error: "expected Ident".
        let diags = parse_diagnostics("test.gs", "function {");
        assert!(!diags.is_empty(), "expected at least one diagnostic");
        assert_eq!(diags[0]["severity"], 1);
        assert_eq!(diags[0]["source"], "goscript");
    }

    #[test]
    fn diagnostics_empty_for_valid_source() {
        let diags = parse_diagnostics("test.gs", "let x = 1;");
        assert!(diags.is_empty(), "valid source should have no diagnostics");
    }

    #[test]
    fn stdlib_module_names_nonempty() {
        let names = stdlib_module_names();
        assert!(names.contains(&"@std/fs"));
        assert!(names.contains(&"@std/markdown"));
        assert!(names.len() > 50);
    }
}
