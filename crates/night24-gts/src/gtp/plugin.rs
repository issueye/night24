//! Plugin manager for GTP plugins (F2.2, Phase 3).
//!
//! Manages plugin subprocess lifecycle: spawn a plugin process, perform the
//! GTP handshake (hello → ready), invoke methods (call → result), and detect
//! crashes for error isolation (one dead plugin doesn't take down the host).
//!
//! Communication is over the plugin's stdio (JSON Lines), using
//! `StreamTransport<ChildStdout, ChildStdin>`.
//!
//! This is the Rust-side manager. A script-facing `@std/gtp/host` module wraps
//! it (registered separately); here we provide the reusable core.

use std::collections::HashMap;
use std::io;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::gtp::frame::{Frame, Value};
use crate::gtp::transport::{StreamTransport, Transport};

/// Plugin manager: owns multiple plugin subprocesses keyed by name.
pub struct PluginManager {
    plugins: HashMap<String, Plugin>,
}

/// A single plugin instance: its child process and stdio transport.
pub struct Plugin {
    pub name: String,
    pub capabilities: Vec<String>,
    pub modules: HashMap<String, Vec<String>>, // module → methods
    child: Option<Child>,
    transport: Option<StreamTransport<ChildStdout, ChildStdin>>,
    /// Set when the child exited or a transport error occurred. A dead plugin
    /// reports errors on call but does NOT panic the host (error isolation).
    dead: bool,
    dead_reason: String,
}

impl PluginManager {
    /// Create a new plugin manager.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    /// Spawn a plugin subprocess and perform the GTP handshake.
    ///
    /// `command` is the executable (e.g. the `gs` binary or any GTP-speaking
    /// program); `args` are its arguments (e.g. `["plugin.gs"]`). The plugin
    /// must speak GTP over stdio: read `call`/`hello` frames from stdin, write
    /// `result`/`ready` frames to stdout.
    pub fn spawn_plugin(&mut self, name: &str, command: &str, args: &[String]) -> io::Result<()> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other(format!("plugin '{}': no stdin pipe", name)))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other(format!("plugin '{}': no stdout pipe", name)))?;
        let mut transport = StreamTransport::new(stdout, stdin);

        // Handshake: send hello, expect ready (with capabilities/modules).
        let hello = Frame {
            frame_type: "hello".to_string(),
            ..Default::default()
        };
        transport.send_frame(&hello).map_err(|e| {
            io::Error::other(format!("plugin '{}': hello send failed: {}", name, e))
        })?;
        let ready = transport.recv_frame().map_err(|e| {
            io::Error::other(format!("plugin '{}': ready recv failed: {}", name, e))
        })?;
        if ready.frame_type != "ready" {
            return Err(io::Error::other(format!(
                "plugin '{}': expected ready frame, got '{}'",
                name, ready.frame_type
            )));
        }
        let modules = parse_modules(&ready);
        let capabilities = ready.capabilities.unwrap_or_default();

        self.plugins.insert(
            name.to_string(),
            Plugin {
                name: name.to_string(),
                capabilities,
                modules,
                child: Some(child),
                transport: Some(transport),
                dead: false,
                dead_reason: String::new(),
            },
        );
        Ok(())
    }

    /// Call a method on a plugin. `module`/`method` select the target; if the
    /// plugin has exited, returns an error (error isolation — never panics).
    pub fn call(
        &mut self,
        name: &str,
        module: &str,
        method: &str,
        args: Vec<Value>,
    ) -> io::Result<Value> {
        let plugin = self
            .plugins
            .get_mut(name)
            .ok_or_else(|| io::Error::other(format!("plugin '{}' is not loaded", name)))?;
        if plugin.dead {
            return Err(io::Error::other(format!(
                "plugin '{}' is dead: {}",
                name, plugin.dead_reason
            )));
        }
        let id = format!("host-call-{}", next_id());
        let frame = Frame::call(id.clone(), module.to_string(), method.to_string(), args);
        let transport = plugin
            .transport
            .as_mut()
            .ok_or_else(|| io::Error::other(format!("plugin '{}': no transport", name)))?;
        if let Err(e) = transport.send_frame(&frame) {
            plugin.mark_dead(format!("send failed: {}", e));
            return Err(io::Error::other(format!(
                "plugin '{}' send failed: {}",
                name, e
            )));
        }
        let result = match transport.recv_frame() {
            Ok(f) => f,
            Err(e) => {
                plugin.mark_dead(format!("recv failed: {}", e));
                return Err(io::Error::other(format!(
                    "plugin '{}' recv failed: {}",
                    name, e
                )));
            }
        };
        if result.ok == Some(true) {
            Ok(result.result.unwrap_or_else(Value::undefined))
        } else if let Some(err) = result.error {
            Err(io::Error::other(format!("{}: {}", err.name, err.message)))
        } else {
            Err(io::Error::other(format!(
                "plugin '{}' returned an invalid result frame",
                name
            )))
        }
    }

    /// Check whether a plugin is alive (child running, not marked dead).
    pub fn is_alive(&mut self, name: &str) -> bool {
        let Some(plugin) = self.plugins.get_mut(name) else {
            return false;
        };
        if plugin.dead {
            return false;
        }
        // Probe the child: try_wait returns Ok(Some(_)) if exited.
        if let Some(child) = plugin.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    plugin.mark_dead("child process exited".to_string());
                    return false;
                }
                Ok(None) => return true,
                Err(_) => return false,
            }
        }
        false
    }

    /// Reap and drop a plugin (kills the child if still running).
    pub fn unload(&mut self, name: &str) {
        if let Some(mut plugin) = self.plugins.remove(name) {
            if let Some(child) = plugin.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// Names of all loaded plugins (alive or dead).
    pub fn plugin_names(&self) -> Vec<&str> {
        self.plugins.keys().map(String::as_str).collect()
    }
}

impl Plugin {
    /// Mark this plugin dead (crashed or errored). Subsequent calls fail fast.
    fn mark_dead(&mut self, reason: String) {
        self.dead = true;
        self.dead_reason = reason;
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Parse the `modules` field of a `ready` frame into a module→methods map.
/// The wire format is a JSON object: `{ "module": ["method", ...], ... }`.
fn parse_modules(ready: &Frame) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    if let Some(serde_json::Value::Object(obj)) = &ready.modules {
        for (k, v) in obj {
            if let serde_json::Value::Array(arr) = v {
                let methods: Vec<String> = arr
                    .iter()
                    .filter_map(|m| m.as_str().map(str::to_string))
                    .collect();
                map.insert(k.clone(), methods);
            }
        }
    }
    map
}

fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_starts_empty() {
        let m = PluginManager::new();
        assert!(m.plugin_names().is_empty());
    }

    #[test]
    fn is_alive_for_unknown_plugin_is_false() {
        let mut m = PluginManager::new();
        assert!(!m.is_alive("nope"));
    }

    #[test]
    fn call_unknown_plugin_errors() {
        let mut m = PluginManager::new();
        let r = m.call("nope", "mod", "method", vec![]);
        assert!(r.is_err());
    }

    #[test]
    fn parse_modules_extracts_methods() {
        let ready = Frame {
            modules: Some(serde_json::json!({
                "math": ["add", "sub"],
                "io": ["read"],
            })),
            ..Default::default()
        };
        let map = parse_modules(&ready);
        assert_eq!(map.get("math").map(|v| v.len()), Some(2));
        assert_eq!(
            map.get("io").map(|v| v.as_slice()),
            Some(&["read".to_string()][..])
        );
    }
}
