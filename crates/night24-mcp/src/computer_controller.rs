//! Computer control extension (placeholder).
//!
//! Reserved for a future extension that lets the agent drive the desktop
//! (mouse, keyboard, screenshots) via the MCP protocol. Not implemented yet;
//! deliberately left as an empty type so it can be wired up without touching
//! the module graph when work begins.

#[derive(Debug, Default)]
pub struct ComputerController;

impl ComputerController {
    pub fn new() -> Self {
        Self
    }
}
