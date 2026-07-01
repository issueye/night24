# 2026-07-01 并行开发派发记录

目标：根据当前文档和代码，把 Night24 推进到“开始基本使用”的状态。

基本可用定义：

- Tauri 只保留 `tauri-app` 前端入口。
- 桌面 UI 以 Vite + React 为当前入口。
- Server 能启动并提供桌面端需要的基础 API。
- Server 最终必须桥接 `night24-agent-core`；桥接完成前允许返回稳定的 Core 未就绪状态或占位错误。
- 能打开本地项目，浏览文件树和文本文件。
- 能创建/切换/删除会话。
- 能发起一轮 Agent 请求并看到消息或事件返回。
- 能看到基础 Agent 时间线事件。
- 权限/取消 API 即使未完全接入 Core，也有稳定占位响应，不让桌面端崩溃。
- `cargo test --workspace` 通过。

---

## Worker 分工

### Worker A: protocol / agent-core

负责人：worker `Schrodinger`

写入范围：

- `crates/night24-protocol/**`
- `crates/night24-agent-core/**`
- 必要时根 `Cargo.toml`

交付物：

- `night24-protocol` crate。
- `night24-agent-core` binary crate。
- stdio newline-delimited JSON-RPC 骨架。
- `core.initialize`、`core.ping`、`core.shutdown`、`agent.tools`。
- `agent.reply` 最小 accepted + message + finish 事件。

不得修改：

- `crates/night24-server/**`
- `tauri-app/**`

---

### Worker B: server API / bridge shell

负责人：worker `Lovelace`

写入范围：

- `crates/night24-server/**`

交付物：

- `DELETE /sessions/{id}`
- `GET /tools`
- `GET /readyz`
- workspace API:
  - `POST /workspaces/open`
  - `GET /workspaces/current`
  - `GET /workspaces/recent`
  - `GET /workspace/tree`
  - `GET /workspace/file`
- `POST /agent/cancel`
- `POST /permissions/{id}/approve`
- `POST /permissions/{id}/deny`
- `/reply` SSE 尽量兼容旧消息和新 finish 事件。
- Core bridge：
  - 启动或定位 `night24-agent-core`
  - 调用 `core.initialize`
  - `/readyz` 反映 Core ready/unready
  - `/tools` 通过 `agent.tools` 获取工具，或在 Core 不可用时返回稳定占位/错误
  - `/reply` 调用 `agent.reply`，并把 `agent.event` 转为 SSE

不得修改：

- `crates/night24-protocol/**`
- `crates/night24-agent-core/**`
- `tauri-app/**`

当前状态：

- server 路由面已在推进，包含 readiness、workspace、tools、cancel 和 permission 占位能力。
- 已补齐最小 Core bridge：server 启动时定位/拉起 `night24-agent-core`，调用 `core.initialize`，`/readyz` 可报告 Core ready。
- `/tools` 已优先走 `agent.tools`，烟测显示 source 为 `night24-agent-core`。
- `/reply` 已走 `agent.reply` 并把 `agent.event` 转为 SSE；烟测确认返回 `message` 与 `finish`，会话历史能保存 user/assistant 两条消息。
- `agent.cancel` 与 `permission.resolve` 已通过 JSON-RPC 走 Core 的最小 accepted 实现。

---

### Worker C: Tauri desktop MVP

负责人：worker `Tesla` / 主线程接续

写入范围：

- `tauri-app/index.html`
- `tauri-app/src/**`
- `tauri-app/package.json`
- `tauri-app/vite.config.js`
- 必要时 `tauri-app/src-tauri/src/main.rs`
- 必要时 `tauri-app/src-tauri/Cargo.toml`

交付物：

- server 连接状态。
- 当前项目和打开项目入口。
- 文件树和文件查看。
- 会话列表和消息区。
- 任务输入。
- 右侧 Files/Diff/Preview tab。
- Agent 时间线。
- SSE 兼容旧 Message 和新 AgentEvent。
- API 缺失时优雅降级。

不得修改：

- `crates/**`
- `docs/**`

当前状态：

- 已切换为 Vite + React 前端，当前入口是 `tauri-app/index.html` 和 `tauri-app/src/**`。
- 已实现 server 状态、打开项目、最近项目、文件树、文件预览、会话列表、任务输入、SSE 事件兼容、取消入口、权限确认入口、Files/Diff/Preview tab 和 Agent 时间线。
- Tauri 已指向 React dev server / build output。
- `select_directory` 依赖的 Tauri `dialog` feature 已补齐。

---

### Worker D: integration / verification plan

负责人：worker `Boyle`

写入范围：

- `docs/plans/2026-07-01-worker-integration-plan.md`
- `docs/plans/2026-07-01-parallel-dispatch.md`

交付物：

- 合并顺序。
- 冲突风险。
- 最小验收步骤。
- 最终验证命令。

不得修改：

- `crates/**`
- `tauri-app/**`

---

## 集成顺序

1. 合入 Worker A：协议和 agent-core 必须先能编译。
2. 合入 Worker B：server API 和基础桥接。
3. 合入 Worker C：桌面端接 API。
4. 合入 Worker D：对照验证清单补缺。
5. 主线程运行：
   - `cargo test --workspace`
   - `cargo build --workspace`
   - `Set-Location tauri-app; npm install; npm run build`
   - 手动启动 server。
   - 打开 Tauri React 页面做基本流程验证。

如果 A 未完成，B 仍可先用占位 `/tools` 和 `/reply` 兼容现有 Agent 路径。  
如果 B 未完成，C 必须显示空状态/错误状态，不阻塞 UI 渲染。

---

## 主线程后续验收

必须检查：

- `rg -n "chat-ui|Chat UI"` 无独立前端残留。
- `rg -n "old static HTML|retired static HTML|legacy web entry" docs tauri-app` 不再把旧静态 HTML 当作验收入口。
- `cargo test --workspace` 通过。
- 新增 crates 被 workspace 编译。
- Server API 路由存在。
- `/readyz` 返回 Core ready。
- `/tools` 和 `/reply` 走 `night24-agent-core`，并在 Core 不可用时返回稳定错误且不挂起。
- Tauri 页面不会因为 API 404 或连接失败崩溃。
- `/reply` 至少能返回一个可渲染消息或错误事件。
- 文件树 API 不越过 workspace root。

当前主线程已验证：

- `cargo test --workspace`
- `cargo build --workspace`
- `tauri-app: npm run build`
- `cargo build --manifest-path tauri-app/src-tauri/Cargo.toml`
- server API 烟测：`/healthz`、`/readyz`、`/tools`、`/workspaces/open`、`/workspace/tree`、`/workspace/file`、越界路径拒绝、`/reply`、`/agent/cancel`、`/permissions/{id}/approve`
