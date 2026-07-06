use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, timeout};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SubAgentMode {
    Sync,
    Async,
}

impl SubAgentMode {
    pub(super) fn from_value(value: Option<&str>) -> Self {
        match value
            .unwrap_or("async")
            .trim()
            .to_ascii_lowercase()
            .replace('-', "_")
            .as_str()
        {
            "sync" | "synchronous" | "blocking" => Self::Sync,
            _ => Self::Async,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Async => "async",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SubAgentStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl SubAgentStatus {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SubAgentMessageDirection {
    ParentToChild,
    ChildToParent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SubAgentMessage {
    pub(super) direction: SubAgentMessageDirection,
    pub(super) text: String,
    pub(super) created_at: DateTime<Utc>,
}

#[derive(Debug)]
struct SubAgentRecord {
    id: String,
    name: String,
    task: String,
    mode: SubAgentMode,
    status: SubAgentStatus,
    parent_run_id: String,
    child_run_id: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    messages: Vec<SubAgentMessage>,
    result: Option<String>,
    error: Option<String>,
    raw_events: Vec<String>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SubAgentSnapshot {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) task: String,
    pub(super) mode: String,
    pub(super) status: String,
    pub(super) parent_run_id: String,
    pub(super) child_run_id: String,
    pub(super) created_at: DateTime<Utc>,
    pub(super) updated_at: DateTime<Utc>,
    pub(super) message_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) result_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) messages: Vec<SubAgentMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SubAgentPoolSnapshot {
    pub(super) total: usize,
    pub(super) queued: usize,
    pub(super) running: usize,
    pub(super) completed: usize,
    pub(super) failed: usize,
    pub(super) cancelled: usize,
    pub(super) subagents: Vec<SubAgentSnapshot>,
}

#[derive(Debug, Clone)]
pub(super) struct SubAgentHandle {
    pub(super) id: String,
    pub(super) cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Default, Clone)]
pub(super) struct SubAgentPool {
    records: Arc<Mutex<HashMap<String, SubAgentRecord>>>,
}

impl SubAgentPool {
    pub(super) fn create(
        &self,
        parent_run_id: &str,
        child_run_id: &str,
        name: Option<&str>,
        task: &str,
        mode: SubAgentMode,
    ) -> anyhow::Result<SubAgentHandle> {
        let id = format!("subagent-{}", uuid::Uuid::new_v4());
        let now = Utc::now();
        let cancelled = Arc::new(AtomicBool::new(false));
        let record = SubAgentRecord {
            id: id.clone(),
            name: name
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("subagent")
                .to_string(),
            task: task.to_string(),
            mode,
            status: SubAgentStatus::Queued,
            parent_run_id: parent_run_id.to_string(),
            child_run_id: child_run_id.to_string(),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
            result: None,
            error: None,
            raw_events: Vec::new(),
            cancelled: cancelled.clone(),
        };

        self.records
            .lock()
            .map_err(|_| anyhow::anyhow!("subagent pool lock poisoned"))?
            .insert(id.clone(), record);

        Ok(SubAgentHandle { id, cancelled })
    }

    pub(super) fn mark_running(&self, id: &str) {
        self.update(id, |record| record.status = SubAgentStatus::Running);
    }

    pub(super) fn mark_completed(&self, id: &str, result: String, raw_events: Vec<String>) {
        self.update(id, |record| {
            if record.cancelled.load(Ordering::SeqCst) {
                record.status = SubAgentStatus::Cancelled;
            } else {
                record.status = SubAgentStatus::Completed;
            }
            record.result = Some(result);
            record.raw_events = raw_events;
        });
    }

    pub(super) fn mark_failed(&self, id: &str, error: String, raw_events: Vec<String>) {
        self.update(id, |record| {
            if record.cancelled.load(Ordering::SeqCst) {
                record.status = SubAgentStatus::Cancelled;
            } else {
                record.status = SubAgentStatus::Failed;
            }
            record.error = Some(error);
            record.raw_events = raw_events;
        });
    }

    pub(super) fn add_message(
        &self,
        id: &str,
        direction: SubAgentMessageDirection,
        text: String,
    ) -> anyhow::Result<SubAgentSnapshot> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| anyhow::anyhow!("subagent pool lock poisoned"))?;
        let record = records
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("subagent not found: {id}"))?;
        record.messages.push(SubAgentMessage {
            direction,
            text,
            created_at: Utc::now(),
        });
        record.updated_at = Utc::now();
        Ok(snapshot_record(record, true, true))
    }

    pub(super) fn cancel(&self, id: Option<&str>) -> anyhow::Result<SubAgentPoolSnapshot> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| anyhow::anyhow!("subagent pool lock poisoned"))?;

        match id {
            Some(id) => {
                let record = records
                    .get_mut(id)
                    .ok_or_else(|| anyhow::anyhow!("subagent not found: {id}"))?;
                record.cancelled.store(true, Ordering::SeqCst);
                if !record.status.is_terminal() {
                    record.status = SubAgentStatus::Cancelled;
                    record.updated_at = Utc::now();
                }
            }
            None => {
                for record in records.values_mut() {
                    record.cancelled.store(true, Ordering::SeqCst);
                    if !record.status.is_terminal() {
                        record.status = SubAgentStatus::Cancelled;
                        record.updated_at = Utc::now();
                    }
                }
            }
        }

        Ok(snapshot_records(&records, true, true))
    }

    pub(super) fn snapshot(
        &self,
        id: Option<&str>,
        include_messages: bool,
        include_result: bool,
    ) -> anyhow::Result<serde_json::Value> {
        let records = self
            .records
            .lock()
            .map_err(|_| anyhow::anyhow!("subagent pool lock poisoned"))?;

        if let Some(id) = id {
            let record = records
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("subagent not found: {id}"))?;
            return Ok(serde_json::to_value(snapshot_record(
                record,
                include_messages,
                include_result,
            ))?);
        }

        Ok(serde_json::to_value(snapshot_records(
            &records,
            include_messages,
            include_result,
        ))?)
    }

    pub(super) async fn wait_for_terminal(
        &self,
        id: &str,
        wait_ms: u64,
        include_messages: bool,
        include_result: bool,
    ) -> anyhow::Result<SubAgentSnapshot> {
        let wait = Duration::from_millis(wait_ms.max(1));
        timeout(wait, async {
            loop {
                if let Some(snapshot) =
                    self.terminal_snapshot(id, include_messages, include_result)?
                {
                    return Ok(snapshot);
                }
                sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("timed out waiting for subagent: {id}"))?
    }

    fn terminal_snapshot(
        &self,
        id: &str,
        include_messages: bool,
        include_result: bool,
    ) -> anyhow::Result<Option<SubAgentSnapshot>> {
        let records = self
            .records
            .lock()
            .map_err(|_| anyhow::anyhow!("subagent pool lock poisoned"))?;
        let record = records
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("subagent not found: {id}"))?;
        if record.status.is_terminal() {
            Ok(Some(snapshot_record(
                record,
                include_messages,
                include_result,
            )))
        } else {
            Ok(None)
        }
    }

    fn update(&self, id: &str, update: impl FnOnce(&mut SubAgentRecord)) {
        if let Ok(mut records) = self.records.lock() {
            if let Some(record) = records.get_mut(id) {
                update(record);
                record.updated_at = Utc::now();
            }
        }
    }
}

fn snapshot_records(
    records: &HashMap<String, SubAgentRecord>,
    include_messages: bool,
    include_result: bool,
) -> SubAgentPoolSnapshot {
    let mut subagents = records
        .values()
        .map(|record| snapshot_record(record, include_messages, include_result))
        .collect::<Vec<_>>();
    subagents.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));

    SubAgentPoolSnapshot {
        total: subagents.len(),
        queued: count_status(&subagents, SubAgentStatus::Queued),
        running: count_status(&subagents, SubAgentStatus::Running),
        completed: count_status(&subagents, SubAgentStatus::Completed),
        failed: count_status(&subagents, SubAgentStatus::Failed),
        cancelled: count_status(&subagents, SubAgentStatus::Cancelled),
        subagents,
    }
}

fn count_status(subagents: &[SubAgentSnapshot], status: SubAgentStatus) -> usize {
    let status = status.as_str();
    subagents
        .iter()
        .filter(|item| item.status == status)
        .count()
}

fn snapshot_record(
    record: &SubAgentRecord,
    include_messages: bool,
    include_result: bool,
) -> SubAgentSnapshot {
    SubAgentSnapshot {
        id: record.id.clone(),
        name: record.name.clone(),
        task: record.task.clone(),
        mode: record.mode.as_str().to_string(),
        status: record.status.as_str().to_string(),
        parent_run_id: record.parent_run_id.clone(),
        child_run_id: record.child_run_id.clone(),
        created_at: record.created_at,
        updated_at: record.updated_at,
        message_count: record.messages.len(),
        result_preview: record.result.as_deref().map(preview),
        result: if include_result {
            record.result.clone()
        } else {
            None
        },
        error: record.error.clone(),
        messages: if include_messages {
            record.messages.clone()
        } else {
            Vec::new()
        },
    }
}

fn preview(text: &str) -> String {
    const MAX_PREVIEW: usize = 500;
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_PREVIEW {
        compact
    } else {
        compact.chars().take(MAX_PREVIEW).collect::<String>() + "..."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_mode_accepts_sync_aliases_and_defaults_to_async() {
        assert_eq!(SubAgentMode::from_value(Some("sync")), SubAgentMode::Sync);
        assert_eq!(
            SubAgentMode::from_value(Some(" synchronous ")),
            SubAgentMode::Sync
        );
        assert_eq!(
            SubAgentMode::from_value(Some("blocking")),
            SubAgentMode::Sync
        );
        assert_eq!(SubAgentMode::from_value(Some("async")), SubAgentMode::Async);
        assert_eq!(
            SubAgentMode::from_value(Some("unknown")),
            SubAgentMode::Async
        );
        assert_eq!(SubAgentMode::from_value(None), SubAgentMode::Async);
    }

    #[test]
    fn pool_snapshot_counts_all_statuses() {
        let pool = SubAgentPool::default();

        let running = pool
            .create(
                "parent",
                "child-running",
                Some("running"),
                "task",
                SubAgentMode::Async,
            )
            .unwrap();
        pool.mark_running(&running.id);

        let completed = pool
            .create(
                "parent",
                "child-completed",
                Some("completed"),
                "task",
                SubAgentMode::Async,
            )
            .unwrap();
        pool.mark_completed(&completed.id, "done".to_string(), Vec::new());

        let failed = pool
            .create(
                "parent",
                "child-failed",
                Some("failed"),
                "task",
                SubAgentMode::Async,
            )
            .unwrap();
        pool.mark_failed(&failed.id, "boom".to_string(), Vec::new());

        let cancelled = pool
            .create(
                "parent",
                "child-cancelled",
                Some("cancelled"),
                "task",
                SubAgentMode::Async,
            )
            .unwrap();
        pool.cancel(Some(&cancelled.id)).unwrap();

        pool.create(
            "parent",
            "child-queued",
            Some("queued"),
            "task",
            SubAgentMode::Async,
        )
        .unwrap();

        let snapshot = pool.snapshot(None, false, false).unwrap();
        let snapshot: SubAgentPoolSnapshot = serde_json::from_value(snapshot).unwrap();

        assert_eq!(snapshot.total, 5);
        assert_eq!(snapshot.queued, 1);
        assert_eq!(snapshot.running, 1);
        assert_eq!(snapshot.completed, 1);
        assert_eq!(snapshot.failed, 1);
        assert_eq!(snapshot.cancelled, 1);
    }
}
