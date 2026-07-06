pub mod error;
pub mod events;
pub mod hooks;
pub mod jsonrpc;
pub mod methods;
pub mod permission;

pub use error::*;
pub use events::*;
pub use hooks::*;
pub use jsonrpc::*;
pub use methods::*;
pub use permission::*;

pub const PROTOCOL_VERSION: &str = "2026-07-01";
