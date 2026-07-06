use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::warn;

type SubscriberMap = Arc<RwLock<HashMap<String, Vec<mpsc::Sender<serde_json::Value>>>>>;

#[derive(Clone)]
pub(crate) struct RunEventStore {
    dir: PathBuf,
    subscribers: SubscriberMap,
}

impl RunEventStore {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) async fn append_and_publish(
        &self,
        run_id: &str,
        event: &serde_json::Value,
    ) -> anyhow::Result<()> {
        self.append(run_id, event)?;
        self.publish(run_id, event.clone()).await;
        Ok(())
    }

    pub(crate) fn load_after(
        &self,
        run_id: &str,
        after_seq: Option<u64>,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let path = self.event_path(run_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(path)?;
        let mut events = Vec::new();
        for line in content.lines().filter(|line| !line.trim().is_empty()) {
            let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
                warn!(run_id, "skipping malformed persisted run event");
                continue;
            };
            if event_seq(&event).is_some_and(|seq| after_seq.is_none_or(|after| seq > after)) {
                events.push(event);
            }
        }
        Ok(events)
    }

    pub(crate) fn has_terminal(&self, run_id: &str) -> anyhow::Result<bool> {
        let path = self.event_path(run_id);
        if !path.exists() {
            return Ok(false);
        }

        let content = std::fs::read_to_string(path)?;
        for line in content.lines().filter(|line| !line.trim().is_empty()) {
            let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if is_terminal_event(&event) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) async fn subscribe(&self, run_id: &str) -> mpsc::Receiver<serde_json::Value> {
        let (tx, rx) = mpsc::channel(64);
        let mut subscribers = self.subscribers.write().await;
        subscribers.entry(run_id.to_string()).or_default().push(tx);
        rx
    }

    fn append(&self, run_id: &str, event: &serde_json::Value) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.event_path(run_id))?;
        serde_json::to_writer(&mut file, event)?;
        file.write_all(b"\n")?;
        Ok(())
    }

    async fn publish(&self, run_id: &str, event: serde_json::Value) {
        let terminal = is_terminal_event(&event);
        let mut subscribers = self.subscribers.write().await;
        let Some(senders) = subscribers.get_mut(run_id) else {
            return;
        };

        senders.retain(|sender| match sender.try_send(event.clone()) {
            Ok(()) => true,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => false,
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => false,
        });

        if terminal {
            subscribers.remove(run_id);
        }
    }

    fn event_path(&self, run_id: &str) -> PathBuf {
        self.dir.join(format!("{}.jsonl", sanitize_run_id(run_id)))
    }
}

pub(crate) fn event_seq(event: &serde_json::Value) -> Option<u64> {
    event.get("seq").and_then(|seq| seq.as_u64())
}

pub(crate) fn is_terminal_event(event: &serde_json::Value) -> bool {
    matches!(
        event.get("type").and_then(|kind| kind.as_str()),
        Some("finish" | "error")
    )
}

fn sanitize_run_id(run_id: &str) -> String {
    run_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn persisted_events_can_be_loaded_after_seq() {
        let dir =
            std::env::temp_dir().join(format!("night24-run-events-test-{}", uuid::Uuid::new_v4()));
        let store = RunEventStore::new(dir.clone());

        store
            .append_and_publish(
                "run-1",
                &serde_json::json!({ "type": "message_delta", "run_id": "run-1", "seq": 1 }),
            )
            .await
            .unwrap();
        store
            .append_and_publish(
                "run-1",
                &serde_json::json!({ "type": "finish", "run_id": "run-1", "seq": 2 }),
            )
            .await
            .unwrap();

        let events = store.load_after("run-1", Some(1)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["seq"], 2);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn subscribers_receive_live_events_without_persisted_backlog_in_memory() {
        let dir =
            std::env::temp_dir().join(format!("night24-run-events-test-{}", uuid::Uuid::new_v4()));
        let store = RunEventStore::new(dir.clone());
        let mut rx = store.subscribe("run-1").await;

        store
            .append_and_publish(
                "run-1",
                &serde_json::json!({ "type": "finish", "run_id": "run-1", "seq": 1 }),
            )
            .await
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event["type"], "finish");
        assert!(rx.recv().await.is_none());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn persisted_events_are_isolated_by_run_id() {
        let dir =
            std::env::temp_dir().join(format!("night24-run-events-test-{}", uuid::Uuid::new_v4()));
        let store = RunEventStore::new(dir.clone());

        store
            .append_and_publish(
                "run-1",
                &serde_json::json!({ "type": "message_delta", "run_id": "run-1", "seq": 1 }),
            )
            .await
            .unwrap();
        store
            .append_and_publish(
                "run-2",
                &serde_json::json!({ "type": "message_delta", "run_id": "run-2", "seq": 1 }),
            )
            .await
            .unwrap();
        store
            .append_and_publish(
                "run-1",
                &serde_json::json!({ "type": "finish", "run_id": "run-1", "seq": 2 }),
            )
            .await
            .unwrap();

        let run_1_events = store.load_after("run-1", None).unwrap();
        let run_2_events = store.load_after("run-2", None).unwrap();

        assert_eq!(run_1_events.len(), 2);
        assert!(run_1_events
            .iter()
            .all(|event| event["run_id"].as_str() == Some("run-1")));
        assert_eq!(run_2_events.len(), 1);
        assert_eq!(run_2_events[0]["run_id"], "run-2");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn subscribers_only_receive_events_for_matching_run_id() {
        let dir =
            std::env::temp_dir().join(format!("night24-run-events-test-{}", uuid::Uuid::new_v4()));
        let store = RunEventStore::new(dir.clone());
        let mut run_1_rx = store.subscribe("run-1").await;
        let mut run_2_rx = store.subscribe("run-2").await;

        store
            .append_and_publish(
                "run-2",
                &serde_json::json!({ "type": "message_delta", "run_id": "run-2", "seq": 1 }),
            )
            .await
            .unwrap();

        assert!(run_1_rx.try_recv().is_err());
        let run_2_event = run_2_rx.try_recv().unwrap();
        assert_eq!(run_2_event["run_id"], "run-2");

        let _ = std::fs::remove_dir_all(dir);
    }
}
