use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

use night24_protocol::{AgentEvent, AgentEventKind, PermissionDecision};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

use crate::hooks::{HookContext, HookRunner};
use crate::rpc::agent_event_notification;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CoreState {
    Spawned,
    Initialized,
    Draining,
}

#[derive(Clone)]
pub(super) struct RunHandle {
    pub(super) cancelled: Arc<AtomicBool>,
}

pub(super) struct PermissionHandle {
    pub(super) run_id: String,
    pub(super) sender: oneshot::Sender<PermissionDecision>,
}

#[derive(Clone)]
pub(super) struct RunContext {
    pub(super) run_id: String,
    pub(super) emit_tool_events: bool,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) seq: Arc<AtomicU64>,
    pub(super) output: Option<UnboundedSender<String>>,
    pub(super) collected: Option<Arc<Mutex<Vec<String>>>>,
    pub(super) permissions: Arc<Mutex<HashMap<String, PermissionHandle>>>,
    pub(super) hooks: Arc<HookRunner>,
}

impl RunContext {
    pub(super) fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    pub(super) fn emit(&self, kind: AgentEventKind) -> String {
        agent_event_notification(AgentEvent::new(self.run_id.clone(), self.next_seq(), kind))
    }

    pub(super) fn send(&self, kind: AgentEventKind) {
        let message = self.emit(kind);
        if let Some(output) = &self.output {
            let _ = output.send(message);
        } else if let Some(collected) = &self.collected {
            if let Ok(mut collected) = collected.lock() {
                collected.push(message);
            }
        }
    }

    pub(super) async fn run_hooks(&self, context: HookContext<'_>) {
        for output in self.hooks.run(&context).await {
            self.send(AgentEventKind::RunOutput {
                source: output.source,
                stream: output.stream,
                text: output.text,
            });
        }
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub(super) fn drain_collected(&self) -> Vec<String> {
        self.collected
            .as_ref()
            .and_then(|collected| {
                collected
                    .lock()
                    .ok()
                    .map(|mut values| values.drain(..).collect())
            })
            .unwrap_or_default()
    }
}
