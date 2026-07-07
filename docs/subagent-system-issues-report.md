# 子代理系统问题分析报告

生成时间：2026-07-07

## 背景

当前子代理系统已经具备创建、查询、等待、取消和桌面端展示能力，但从近期使用现象看，仍存在主会话被异常结束、子代理面板误弹、渲染顺序混乱、异步结果未被消费等问题。

本报告基于当前代码结构分析，重点覆盖核心运行、服务端接口和桌面端展示链路。

## 总体结论

当前主要问题不是单纯的桌面端样式或刷新问题，而是子代理运行事件和主会话运行事件没有隔离，并且子代理池是全局内存池，没有按会话或 run 过滤。

这两个结构性问题会直接导致：

- 子代理的 `finish` 或 `error` 被桌面端当作主任务结束处理。
- 子代理消息、工具调用和主会话消息混在同一个流里，造成显示顺序混乱。
- 旧会话或其他会话的子代理进入当前会话面板。
- 当前会话运行时，只要全局子代理数量变化，就可能自动打开子代理面板。

## 关键问题

### 1. 子代理事件会污染主会话输出流

位置：

- `crates/night24-agent-core/src/lib.rs`
- `run_subagent_once`

当前子代理执行完成后，会把子代理产生的全部事件转发到父会话输出流：

```rust
if let Some(output) = parent_output {
    for event in &events {
        let _ = output.send(event.clone());
    }
}
```

风险：

- 子代理的 `message` 会进入主聊天消息列表。
- 子代理的 `tool_started`、`tool_finished` 会进入主会话时间线。
- 子代理的 `finish` 可能触发桌面端 `finishRun`。
- 子代理的 `error` 可能触发桌面端错误提示和运行结束。

影响：

这是“出现报错就直接退出对话”“完成之后渲染顺序混乱”“最后结果报告跑到上面”等现象的核心原因之一。

### 2. 子代理池是全局内存池，未按会话或 run 隔离

位置：

- `crates/night24-agent-core/src/subagents.rs`
- `crates/night24-protocol/src/methods.rs`
- `crates/night24-server/src/main.rs`

`SubAgentPool` 目前是全局 `HashMap`：

```rust
records: Arc<Mutex<HashMap<String, SubAgentRecord>>>,
```

虽然每条子代理记录保存了 `parent_run_id`，但查询参数只有：

- `subagent_id`
- `include_messages`
- `include_result`

服务端 `/agent/subagents` 也没有传入 `parent_run_id` 或 `session_id` 过滤条件。

风险：

- 桌面端拿到的是所有子代理，而不是当前会话的子代理。
- 旧会话或其他会话的子代理可能出现在当前面板中。
- 子代理数量变化会影响当前会话的辅助面板状态。

### 3. 桌面端自动打开子代理面板的判断过粗

位置：

- `tauri-app/src/App.jsx`
- `tauri-app/src/hooks/useSubAgents.js`

桌面端当前通过全局子代理数量变化判断是否打开面板：

```js
if (count > previousCount && visibleSessionRunning) {
  openContextTab('agents');
}
```

风险：

- 只要全局池数量增加，并且当前会话正在运行，就会打开子代理面板。
- 该新增子代理不一定属于当前会话。
- 与全局池未隔离问题叠加后，容易出现面板误弹和展示错位。

### 4. 异步子代理没有强制等待或结果消费机制

位置：

- `crates/night24-agent-core/src/lib.rs`
- `spawn_subagent`

异步子代理通过独立线程启动后立即返回：

```rust
std::thread::Builder::new()
    .name(format!("night24-subagent-{subagent_id}"))
    .spawn(move || {
        ...
    })
```

父代理是否等待子代理结果，主要依赖系统提示和模型自觉调用 `developer__subagent_wait`。

风险：

- 父代理可能在子代理未完成时给出最终报告。
- 子代理失败后，父代理不一定知道或恢复。
- 用户看到主任务完成，但子代理仍在后台运行。

### 5. 子代理状态没有持久化

当前子代理状态只保存在内存池中。

风险：

- core 重启后子代理历史丢失。
- 桌面端重连后无法稳定恢复子代理运行情况。
- 已经拆分出的会话表、消息表、任务表无法完整表达子代理生命周期。

### 6. 子代理结果提取方式较脆弱

位置：

- `crates/night24-agent-core/src/lib.rs`
- `subagent_result_from_events`

当前通过倒序扫描事件中的 `finish` 或 `error` 提取结果。

风险：

- 如果 provider 只返回 delta，或者异常路径没有标准 `finish`，会得到 `subagent produced no terminal event`。
- 子代理真实输出可能已经产生，但无法被正确记录为结果。

### 7. 缺少并发和资源限制

当前模型可以连续创建多个异步子代理。

风险：

- 长上下文分析时可能堆积大量后台任务。
- 没有每个父 run 的最大子代理数。
- 没有全局队列上限、超时清理和资源回收策略。

### 8. 权限和错误归属不够清晰

子代理和父代理共享部分运行上下文，例如权限、hooks、subagent pool。

风险：

- 权限请求展示时不容易区分来自主代理还是子代理。
- 子代理工具失败可能被用户理解成主任务工具失败。
- 桌面端错误提示缺少清晰归属。

## 修复优先级

### P0：隔离子代理事件流

目标：

- 子代理事件不再直接进入主会话消息流。
- 子代理的 `finish` 和 `error` 不应触发主 run 结束。
- 桌面端通过专门的子代理事件或池状态更新展示子代理过程。

建议方案：

- 移除 `run_subagent_once` 中对子代理原始事件的父流转发。
- 新增统一事件，例如 `subagent_updated`、`subagent_message`、`subagent_finished`。
- 桌面端只把这些事件投递到子代理面板，不进入主聊天消息合并逻辑。

### P0：按当前 run 或 session 过滤子代理池

目标：

- `/agent/subagents` 支持 `parent_run_id` 或 `session_id`。
- 桌面端只加载当前会话或当前运行对应的子代理。
- 自动打开面板只响应当前 run 下的子代理变化。

建议方案：

- `SubAgentPoolParams` 增加 `parent_run_id`。
- `SubAgentPool::snapshot` 支持过滤。
- `SubAgentPoolQuery` 增加同名查询参数。
- `useSubAgents` 接收当前 `runId/sessionId` 并拼接查询参数。

### P1：持久化子代理生命周期

目标：

- 子代理状态可恢复。
- 子代理与父会话、父 run、子 run 有稳定关联。
- 错误、结果、工具调用摘要可查询。

建议方案：

- 新增 `subagent_runs` 表，或者在现有 task/session 结构上增加子代理运行记录。
- 字段建议包括：`id`、`parent_session_id`、`parent_run_id`、`child_run_id`、`name`、`task`、`mode`、`status`、`result`、`error`、`created_at`、`updated_at`。

### P1：异步子代理结果消费约束

目标：

- 父代理不会在依赖子代理结果时提前结束。
- 用户能清楚看到主任务和子代理任务的关系。

建议方案：

- 父 run 结束前检查是否存在当前 run 下未完成的子代理。
- 如果有未完成子代理，返回明确状态或提示模型等待。
- 在桌面端展示“主任务已完成，子代理仍在运行”或“等待子代理结果”。

### P2：补充并发限制和清理策略

目标：

- 控制后台任务数量。
- 避免子代理池无限增长。

建议方案：

- 每个父 run 限制最大子代理数。
- 全局限制同时运行子代理数。
- 终态子代理按时间归档或只保留最近 N 条。

### P2：增强结果提取和错误归属

目标：

- 子代理输出更稳定。
- 错误提示能明确说明来自哪个子代理。

建议方案：

- 直接从标准化消息聚合结果，而不是只扫描终止事件。
- 子代理错误事件携带 `subagent_id`、`parent_run_id`、`child_run_id`。
- 桌面端错误提示增加“子代理名称/任务”上下文。

## 建议实施顺序

1. 移除子代理原始事件对父流的直接转发，改为专用子代理事件。
2. 给子代理查询接口增加 `parent_run_id` 过滤，并修改桌面端只查询当前 run。
3. 修改桌面端自动打开逻辑，只响应当前 run 的子代理创建事件。
4. 增加子代理持久化表和恢复逻辑。
5. 增加异步子代理等待约束、并发限制和清理策略。
6. 补充回归测试，覆盖子代理 `finish/error` 不会结束主 run、跨会话子代理不会展示到当前会话。

## 需要补充的测试

- 子代理 `finish` 不会触发父 run 的 `finishRun`。
- 子代理 `error` 不会触发父 run 的错误终止。
- `/agent/subagents?parent_run_id=...` 只返回指定父 run 的子代理。
- 当前会话运行时，其他 run 新增子代理不会打开当前子代理面板。
- 异步子代理未完成时，父 run 的最终报告不会误表示全部完成。
- core 重启后，已完成子代理记录可以从持久化数据恢复。

## 总结

当前子代理系统已经具备基础能力，但还缺少运行隔离、数据归属和生命周期管理。最优先要修复的是事件隔离和查询作用域过滤。只有先解决这两点，桌面端展示优化、tabs 面板、自动打开和错误重试才不会继续被底层事件串流问题影响。
