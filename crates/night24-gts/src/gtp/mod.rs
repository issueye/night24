//! GTP (GoScript Transport Protocol) implementation
//!
//! This module provides the Rust implementation of the GTP protocol,
//! which enables communication between the GTS runtime and plugins.
//!
//! # Protocol Overview
//!
//! GTP uses JSON Lines (one JSON object per line) for message framing.
//! The protocol supports:
//! - Handshake (hello/ready frames)
//! - Remote procedure calls (call/result frames)
//! - Event notifications (event frames)
//!
//! # Transport Abstraction
//!
//! The protocol is transport-agnostic. Implementations include:
//! - stdio (stdin/stdout pipes)
//! - TCP sockets
//! - WebSockets
//!
//! # Example
//!
//! ```no_run
//! use gts::gtp::{Frame, Value};
//! use gts::gtp::codec::{JsonlEncoder, JsonlDecoder};
//! use std::io::Cursor;
//!
//! // Create a hello frame
//! let frame = Frame::hello("h1".to_string(), Some("gts_r".to_string()));
//!
//! // Encode to JSON Lines
//! let mut buf = Vec::new();
//! {
//!     let mut encoder = JsonlEncoder::new(&mut buf);
//!     encoder.encode(&frame).unwrap();
//! }
//!
//! // Decode from JSON Lines
//! let mut decoder = JsonlDecoder::new(Cursor::new(buf));
//! let decoded = decoder.decode().unwrap();
//! assert_eq!(decoded.frame_type, "hello");
//! ```

pub mod codec;
pub mod frame;
pub mod plugin;
pub mod transport;
pub mod transports;

// Re-export commonly used types
pub use codec::{JsonlDecoder, JsonlEncoder};
pub use frame::{Frame, GtpError, Value, VERSION};
pub use transport::Transport;
