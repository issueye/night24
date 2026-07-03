//! GTP (GoScript Transport Protocol) standard library modules
//!
//! This module provides @std/gtp/* modules for GTP client/server functionality
//! in scripts.

pub mod client;
pub mod server;

use crate::object::Object;

/// Load a GTP stdlib module by name
pub fn load_gtp_module(name: &str) -> Option<Object> {
    match name {
        "@std/gtp/client" => Some(client::gtp_client_module()),
        "@std/gtp/server" => Some(server::gtp_server_module()),
        _ => None,
    }
}
