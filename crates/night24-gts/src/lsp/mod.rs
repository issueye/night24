//! GoScript Language Server Protocol (LSP) implementation.
//!
//! Dependency-free MVP language server (`phase-1-development-plan.md` W2):
//! hand-written JSON-RPC over stdio. It does NOT use the VM — it parses source
//! directly for diagnostics and serves static completion/hover/definition data.
//!
//! Run with `gs lsp`.
//!
//! Modules:
//! - [`transport`]: Content-Length framed JSON-RPC read/write over stdio.
//! - [`document`]: in-memory document store (full text sync).
//! - [`server`]: request/notification handling + diagnostics.

pub mod document;
pub mod server;
pub mod transport;

pub use server::run_server;
