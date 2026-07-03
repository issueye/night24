//! Individual native `@std/*` modules, one file per module.
//!
//! File naming mirrors the Go originals under `gts/internal/stdlib/*.go`
//! (e.g. `net_http_client.rs` <-> `net_http_client.go`). Each file exposes a
//! `<name>_module()` constructor returning the module's [`Object`] and pulls in
//! shared primitives via `use super::super::helpers::*;`.

pub mod archive_zip;
pub mod async_;
pub mod buffer;
pub mod cache;
pub mod cli;
pub mod collections;
pub mod color;
pub mod compress_gzip;
pub mod compression;
pub mod crypto;
#[cfg(feature = "db")]
pub mod db;
pub mod diff;
pub mod encoding_base64;
pub mod encoding_csv;
pub mod encoding_hex;
pub mod env;
pub mod events;
pub mod exec;
pub mod fs;
pub mod glob;
pub mod hash;
pub mod highlight;
pub mod image;
pub mod json;
pub mod jwt;
pub mod log;
pub mod mail;
pub mod markdown;
pub mod mime;
pub mod net_http_client;
pub mod net_http_server;
pub mod net_ip;
pub mod net_socket_client;
pub mod net_socket_server;
pub mod net_ws_client;
pub mod net_ws_server;
pub mod os;
pub mod path;
pub mod pdf;
pub mod process;
pub mod prometheus;
pub mod pty;
pub mod random;
pub mod rate_limit;
pub mod regexp;
pub mod retry;
pub mod runtime;
pub mod schema;
pub mod semver;
pub mod signal;
pub mod sse;
pub mod stream;
pub mod table;
pub mod template;
pub mod terminal;
pub mod test;
pub mod text;
pub mod time;
pub mod timers;
pub mod toml;
pub mod tui;
pub mod url;
pub mod validation;
pub mod watch;
pub mod web;
pub mod xml;
pub mod yaml;
