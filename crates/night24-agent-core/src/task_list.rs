use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl TaskStatus {
    fn from_value(value: Option<&str>) -> Self {
        match value
            .unwrap_or("pending")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "done" | "complete" | "completed" | "finished" | "x" | "完成" => {
                TaskStatus::Completed
            }
            "active" | "current" | "in_progress" | "in-progress" | "doing" | "进行中" => {
                TaskStatus::InProgress
            }
            _ => TaskStatus::Pending,
        }
    }

    fn markdown_title(&self, title: &str) -> String {
        match self {
            TaskStatus::Pending => title.to_string(),
            TaskStatus::InProgress => format!("{title}（进行中）"),
            TaskStatus::Completed => title.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct TaskItem {
    id: String,
    title: String,
    status: TaskStatus,
}

#[derive(Debug, Default)]
pub(super) struct TaskListState {
    items: Vec<TaskItem>,
    report: Option<String>,
}

impl TaskListState {
    pub(super) fn execute(&mut self, tool_name: &str, arguments: &Value) -> anyhow::Result<String> {
        match tool_name {
            "developer__task_list_create" => self.create(arguments),
            "developer__task_list_update" => self.update(arguments),
            "developer__task_list_status" => Ok(self.markdown()),
            "developer__task_list_finish" => self.finish(arguments),
            _ => anyhow::bail!("unknown task list tool: {tool_name}"),
        }
    }

    fn create(&mut self, arguments: &Value) -> anyhow::Result<String> {
        let tasks = arguments
            .get("tasks")
            .and_then(|value| value.as_array())
            .ok_or_else(|| anyhow::anyhow!("missing `tasks` for developer__task_list_create"))?;

        let mut items = Vec::new();
        for (index, task) in tasks.iter().enumerate() {
            let (title, status) = task_item_from_value(index, task)?;
            items.push(TaskItem {
                id: format!("task-{}", index + 1),
                title,
                status,
            });
        }

        if items.is_empty() {
            anyhow::bail!("task list requires at least one task");
        }

        self.items = items;
        self.report = None;
        Ok(self.markdown())
    }

    fn update(&mut self, arguments: &Value) -> anyhow::Result<String> {
        if self.items.is_empty() {
            anyhow::bail!("task list has not been created");
        }

        let index = self.resolve_index(arguments)?;
        if let Some(title) = arguments
            .get("title")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.items[index].title = title.to_string();
        }

        if let Some(status) = arguments.get("status").and_then(|value| value.as_str()) {
            self.items[index].status = TaskStatus::from_value(Some(status));
        }

        Ok(self.markdown())
    }

    fn finish(&mut self, arguments: &Value) -> anyhow::Result<String> {
        if self.items.is_empty() {
            anyhow::bail!("task list has not been created");
        }

        for item in &mut self.items {
            item.status = TaskStatus::Completed;
        }
        self.report = arguments
            .get("report")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        Ok(self.markdown())
    }

    fn resolve_index(&self, arguments: &Value) -> anyhow::Result<usize> {
        if let Some(index) = arguments.get("index").and_then(|value| value.as_u64()) {
            let index = usize::try_from(index).unwrap_or(usize::MAX);
            if index == 0 || index > self.items.len() {
                anyhow::bail!("task index out of range");
            }
            return Ok(index - 1);
        }

        if let Some(id) = arguments.get("id").and_then(|value| value.as_str()) {
            if let Some(index) = self.items.iter().position(|item| item.id == id) {
                return Ok(index);
            }
            anyhow::bail!("task id not found: {id}");
        }

        anyhow::bail!("missing `index` or `id` for developer__task_list_update")
    }

    pub(super) fn markdown(&self) -> String {
        let mut lines = vec!["## 任务列表".to_string()];
        if self.items.is_empty() {
            lines.push("- [ ] 尚未创建任务".to_string());
        } else {
            for item in &self.items {
                let check = if item.status == TaskStatus::Completed {
                    "x"
                } else {
                    " "
                };
                lines.push(format!(
                    "- [{check}] {}",
                    item.status.markdown_title(&item.title)
                ));
            }
        }

        if let Some(report) = &self.report {
            lines.push(String::new());
            lines.push("## 完成报告".to_string());
            lines.push(report.clone());
        }

        lines.join("\n")
    }

    pub(super) fn has_open_tasks(&self) -> bool {
        self.report.is_none()
            && self
                .items
                .iter()
                .any(|item| item.status != TaskStatus::Completed)
    }
}

fn task_item_from_value(index: usize, value: &Value) -> anyhow::Result<(String, TaskStatus)> {
    if let Some(title) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok((title.to_string(), TaskStatus::Pending));
    }

    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("task {} must be a string or object", index + 1))?;
    let title = object
        .get("title")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("task {} is missing `title`", index + 1))?;
    let status = TaskStatus::from_value(object.get("status").and_then(|value| value.as_str()));
    Ok((title.to_string(), status))
}

pub(super) fn is_task_list_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "developer__task_list_create"
            | "developer__task_list_update"
            | "developer__task_list_status"
            | "developer__task_list_finish"
    )
}

pub(super) fn summarize_task_list_tool(tool_name: &str, arguments: &Value) -> String {
    match tool_name {
        "developer__task_list_create" => arguments
            .get("tasks")
            .and_then(|value| value.as_array())
            .map(|tasks| format!("Create task list with {} tasks", tasks.len()))
            .unwrap_or_else(|| "Create task list".to_string()),
        "developer__task_list_update" => "Update task list".to_string(),
        "developer__task_list_status" => "Show task list".to_string(),
        "developer__task_list_finish" => "Finish task list".to_string(),
        _ => format!("Call {tool_name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_update_task_list() {
        let mut state = TaskListState::default();

        let created = state
            .execute(
                "developer__task_list_create",
                &serde_json::json!({"tasks": ["Inspect project", "Write report"]}),
            )
            .unwrap();
        assert!(created.contains("- [ ] Inspect project"));

        let updated = state
            .execute(
                "developer__task_list_update",
                &serde_json::json!({"index": 1, "status": "completed"}),
            )
            .unwrap();
        assert!(updated.contains("- [x] Inspect project"));
        assert!(updated.contains("- [ ] Write report"));
    }

    #[test]
    fn finish_task_list_adds_completion_report() {
        let mut state = TaskListState::default();
        state
            .execute(
                "developer__task_list_create",
                &serde_json::json!({"tasks": ["A", "B"]}),
            )
            .unwrap();

        let finished = state
            .execute(
                "developer__task_list_finish",
                &serde_json::json!({"report": "Done"}),
            )
            .unwrap();

        assert!(finished.contains("- [x] A"));
        assert!(finished.contains("- [x] B"));
        assert!(finished.contains("## 完成报告\nDone"));
    }
}
