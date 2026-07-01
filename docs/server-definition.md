# Night24 Server 定义

> 日期：2026-07-01  
> 范围：`crates/night24-server`  
> 定位：桌面端 API 网关、本地工作区服务、权限协调器、Agent Core 子进程管理器。

---

## 1. 一句话定义

`night24-server` 是 Tauri 桌面端和 `night24-agent-core` 之间的本地桥梁。

它对桌面端暴露 HTTP/SSE API，对 Agent Core 使用 stdio/JSON-RPC。它不再直接承担 Agent 推理循环本体。

```text
tauri-app
  |
  | HTTP / SSE
  v
night24-server
  |
  | stdio / JSON-RPC
  v
night24-agent-core
```

---

## 2. 核心职责

### 2.1 Desktop API Gateway

Server 是桌面端唯一后端入口。

负责：

- HTTP API 路由。
- SSE 事件流。
- API Key 鉴权。
- OpenAPI 文档。
- 请求参数校验。
- 错误响应归一化。

不负责：

- 桌面 UI。
- 直接渲染文件或页面。
- 直接调用 Tauri API。

---

### 2.2 Agent Core Bridge

Server 管理 `night24-agent-core` 子进程，并桥接协议。

负责：

- 启动 Core 子进程。
- 调用 `core.initialize`。
- 维护 JSON-RPC pending request。
- 读取 Core stdout JSON-RPC 消息。
- 读取 Core stderr 日志并写入 tracing。
- 将 `/reply` 转换为 `agent.reply`。
- 将 Core 的 `agent.event` 转换为 SSE。
- 在 Core 崩溃时标记 active run 为 failed。
- 必要时重启 Core。

不负责：

- Provider 调用。
- Agent loop。
- 工具实际执行。
- 上下文压缩。

---

### 2.3 Workspace Manager

Server 管理当前桌面工作区。

负责：

- 打开本地项目目录。
- 维护当前 workspace。
- 维护最近打开项目。
- 提供文件树。
- 读取文本文件。
- 限制路径不能逃逸 workspace root。
- 忽略大目录和隐藏系统目录。

不负责：

- 完整代码编辑器能力。
- 复杂文件搜索 UI。
- 自动安装依赖。

---

### 2.4 Session Manager

Server 继续管理会话生命周期和持久化。

负责：

- 创建会话。
- 列出会话。
- 获取会话历史。
- 删除会话。
- 重命名会话。
- Fork 会话。
- 将会话绑定到 workspace。
- 接收 Core 事件后更新会话历史。

边界：

- 第一版 conversation 由 server 持久化。
- Core 在 `agent.reply` 时接收 conversation 快照。
- Core 通过事件返回新增消息。

---

### 2.5 Permission Coordinator

Server 是权限确认的协调者。

负责：

- 接收 Core 的 `permission_required` 事件。
- 转发给桌面端。
- 暴露 approve/deny API。
- 将用户决策通过 `permission.resolve` 发回 Core。
- 处理权限请求超时。

不负责：

- 绕过 Core 直接执行工具。
- 替 Core 判定工具是否完成。

---

### 2.6 Event Router

Server 将 Core 的事件转换为桌面端事件。

负责：

- 按 `run_id` 路由事件。
- 按 `seq` 保持事件顺序。
- 将 JSON-RPC notification 转为 SSE payload。
- 对未知事件做兼容转发或 warning。
- 在 `finish/error` 后关闭对应 SSE stream。

---

## 3. 当前不属于 Server 的职责

明确不放在 `night24-server`：

- Agent 推理循环。
- Provider 注册与调用。
- 模型工具调用格式转换。
- 工具执行实现。
- 上下文压缩策略。
- Memory 工具执行。
- MCP tool runtime。
- 复杂 UI 状态。
- 完整终端模拟器。
- 直接预览网页内容。

这些属于：

- `night24-agent-core`
- `night24-core`
- `night24-mcp`
- `tauri-app`

---

## 4. Server 模块建议

建议从当前单文件 `main.rs` 拆分为以下模块：

```text
crates/night24-server/src/
  main.rs
  state.rs
  auth.rs
  error.rs
  routes/
    mod.rs
    health.rs
    sessions.rs
    reply.rs
    workspaces.rs
    permissions.rs
    tools.rs
  core_client/
    mod.rs
    process.rs
    jsonrpc.rs
    router.rs
  workspace/
    mod.rs
    tree.rs
    file.rs
    recent.rs
  sse.rs
```

### 4.1 state.rs

保存全局状态：

```text
AppState
  session_manager
  workspace_manager
  core_client
  permission_registry
  active_runs
  config
```

### 4.2 core_client

负责 stdio/JSON-RPC：

- 子进程启动。
- stdin writer。
- stdout reader。
- stderr reader。
- request/response 匹配。
- notification 分发。
- Core 重启。

### 4.3 workspace

负责 workspace root、文件树、文件读取、最近项目。

### 4.4 routes

HTTP 路由只做参数解析、调用服务、返回响应。

---

## 5. Server 状态模型

### 5.1 AppState

```text
AppState
  session_manager: SessionManager
  workspace_manager: WorkspaceManager
  core_client: AgentCoreClient
  permission_registry: PermissionRegistry
  active_runs: ActiveRunRegistry
  config: ServerConfig
```

### 5.2 Workspace

```json
{
  "id": "workspace-1",
  "name": "night24",
  "root_path": "E:\\code\\issueye\\ai_agent\\night24",
  "created_at": "2026-07-01T10:00:00Z",
  "last_opened_at": "2026-07-01T10:00:00Z"
}
```

### 5.3 ActiveRun

```json
{
  "run_id": "run-1",
  "session_id": "session-1",
  "workspace_id": "workspace-1",
  "status": "running",
  "started_at": "2026-07-01T10:00:00Z"
}
```

### 5.4 PermissionRecord

```json
{
  "permission_id": "perm-1",
  "run_id": "run-1",
  "tool_call_id": "tool-1",
  "tool_name": "developer__shell",
  "status": "pending",
  "created_at": "2026-07-01T10:00:00Z",
  "expires_at": "2026-07-01T10:05:00Z"
}
```

---

## 6. HTTP API

### 6.1 Health

| Method | Path | 说明 |
|---|---|---|
| `GET` | `/healthz` | server 存活 |
| `GET` | `/readyz` | server + core 可用 |

`/healthz` 只表示 server 进程还活着。  
`/readyz` 表示 Core 已初始化，workspace/session store 可用。

---

### 6.2 Sessions

| Method | Path | 说明 |
|---|---|---|
| `GET` | `/sessions` | 会话列表 |
| `POST` | `/sessions` | 创建会话 |
| `DELETE` | `/sessions/{id}` | 删除会话 |
| `GET` | `/sessions/{id}/history` | 会话历史 |
| `PUT` | `/sessions/{id}/name` | 重命名 |
| `POST` | `/sessions/{id}/fork` | Fork |

创建会话应支持 `working_dir` 或默认使用当前 workspace root。

---

### 6.3 Workspaces

| Method | Path | 说明 |
|---|---|---|
| `POST` | `/workspaces/open` | 打开目录 |
| `GET` | `/workspaces/current` | 当前 workspace |
| `GET` | `/workspaces/recent` | 最近 workspace |
| `GET` | `/workspace/tree?path=` | 文件树 |
| `GET` | `/workspace/file?path=` | 读取文件 |

第一版不要求写文件 API。写入由 Agent Core 工具和权限确认控制。

---

### 6.4 Agent

| Method | Path | 说明 |
|---|---|---|
| `POST` | `/reply` | 发起 Agent run，SSE 返回事件 |
| `POST` | `/agent/cancel` | 取消 run |
| `GET` | `/tools` | 返回 Core 工具列表 |

`/reply` 外部保持 SSE，但内部调用 Core 的 `agent.reply`。

---

### 6.5 Permissions

| Method | Path | 说明 |
|---|---|---|
| `POST` | `/permissions/{id}/approve` | 批准权限请求 |
| `POST` | `/permissions/{id}/deny` | 拒绝权限请求 |

Server 收到请求后调用 Core 的 `permission.resolve`。

---

## 7. `/reply` 流程

```text
Tauri POST /reply
  -> Server 验证 session/workspace/provider
  -> Server 创建 run_id
  -> Server 建立 SSE channel
  -> Server 调 Core agent.reply
  -> Core 返回 accepted
  -> Core 持续发送 agent.event
  -> Server 将 event 转 SSE
  -> finish/error 后 Server 保存会话并关闭 SSE
```

要求：

- `/reply` 不再直接构造 `Agent`。
- `/reply` 不再直接调用 Provider。
- `/reply` 不再直接执行工具。
- `/reply` 只桥接请求和事件。

---

## 8. Core 子进程管理

### 8.1 启动

Server 启动时拉起 Core：

```text
night24-agent-core --stdio
```

然后调用：

```text
core.initialize
```

### 8.2 stdout

只允许 JSON-RPC line。

如果收到非 JSON：

- 记录 protocol violation。
- 标记 Core unhealthy。
- 根据策略重启 Core。

### 8.3 stderr

作为日志流处理：

- 读取每行。
- 写入 server tracing。
- 不转发给前端，除非 Core 崩溃时作为诊断摘要。

### 8.4 崩溃

Core 非预期退出时：

- 标记 Core unavailable。
- 所有 active run 发送 `error` SSE。
- 清理 pending request。
- 清理 pending permission。
- 按退避策略重启。

### 8.5 重启策略

建议：

- 首次立即重启。
- 连续失败使用指数退避。
- 短时间失败超过阈值后停止重启，等待用户手动重试。

---

## 9. 权限协调流程

```text
Core agent.event permission_required
  -> Server 记录 PermissionRecord
  -> Server SSE 推给 Tauri
  -> Tauri 用户批准/拒绝
  -> Tauri POST /permissions/{id}/approve|deny
  -> Server 调 Core permission.resolve
  -> Core 继续执行或返回拒绝结果
```

规则：

- `permission_id` 只能处理一次。
- 超时后 server 可以自动 deny。
- 如果 run 已结束，permission 自动失效。
- 所有权限决定应进入时间线事件。

---

## 10. Workspace 文件安全

Server 提供文件树/读取 API 时必须保证：

- 所有路径基于 workspace root 解析。
- 禁止 `..` 逃逸。
- canonical path 必须以 workspace root 开头。
- 默认跳过 `.git`、`target`、`node_modules`、`.venv`、`venv`。
- 文件读取设置大小上限。
- 二进制文件不返回正文。

---

## 11. 认证与本地安全

Server 保留现有 `NIGHT24_API_KEY` 机制：

- 未设置：本地开发开放。
- 已设置：除 `/healthz`、`/readyz`、OpenAPI 外都需要 API key。

桌面端应通过：

- `Authorization: Bearer <key>`
- 或 `X-API-Key: <key>`

---

## 12. 配置项

建议支持：

```text
NIGHT24_BIND_ADDR
NIGHT24_DATABASE_URL
NIGHT24_API_KEY
NIGHT24_AGENT_CORE_BIN
NIGHT24_PERMISSION_MODE
NIGHT24_WORKSPACE_RECENTS_LIMIT
```

`NIGHT24_AGENT_CORE_BIN` 未设置时，server 从同目录或 workspace target 目录查找。

---

## 13. 第一版实现边界

第一版必须实现：

- `/healthz`
- `/readyz`
- sessions CRUD 基础能力，包括 delete
- workspace open/current/tree/file
- `/tools`
- `/reply` 代理 Core
- `/agent/cancel`
- permission approve/deny API
- Core 子进程启动、初始化、崩溃处理

第一版可以暂缓：

- workspace 写文件 API。
- diff API。
- process/dev server API。
- Git commit API。
- 多 Core 实例。
- 远程 Core。

---

## 14. 验收标准

完成 server 定义落地后，应满足：

- Tauri 只调用 server，不直接调用 Core。
- server 启动后能拉起并初始化 Core。
- `/readyz` 能反映 Core 是否可用。
- `/reply` 通过 Core 执行，但对 Tauri 的 SSE 行为稳定。
- Core 崩溃时，前端收到明确错误。
- 权限请求能从 Core 经 server 到桌面端，再回到 Core。
- workspace 文件树和文件读取不会越权。
- `cargo test --workspace` 通过。

