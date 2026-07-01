# Night24 桌面端当前功能定义

> 日期：2026-07-01  
> 范围：`tauri-app` 当前阶段 MVP  
> 目标：定义桌面端现在需要承载的功能，支撑 server ↔ agent-core 拆分和可视化 vibe coding 的第一版闭环。

---

## 1. 产品定位

当前桌面端不是完整 IDE，而是 Night24 的本地 Agent 工作台。

它负责：

- 连接本地 `night24-server`。
- 打开本地项目。
- 展示会话、文件、Agent 执行过程和结果。
- 承接用户输入、权限确认、取消任务。
- 为后续 diff、测试、预览留出清晰位置。

它暂不负责：

- 完整代码编辑器能力。
- 插件市场。
- 多窗口项目管理。
- 云端账号体系。
- 复杂 Git 工作流。
- 直接调用 `night24-agent-core`。

---

## 2. 当前必须功能

### 2.1 应用启动与连接状态

桌面端启动后应展示 server 连接状态。

必须支持：

- 检测 `night24-server` 是否可访问。
- 展示连接中、已连接、连接失败三种状态。
- 连接失败时提供重试。
- 显示当前 server 地址，例如 `http://localhost:17787`。

后续可选：

- 由 Tauri 自动启动 server。
- 展示 agent-core 是否已由 server 拉起。

验收：

- server 未启动时，用户能明确看到问题。
- server 启动后，用户能一键重试恢复。

---

### 2.2 项目打开与当前工作区

桌面端必须有“打开项目”入口。

必须支持：

- 调用 Tauri 目录选择器选择本地目录。
- 将目录传给 server，创建或切换当前 workspace。
- 在界面顶部或左侧明确显示当前项目名称和路径。
- 记住最近打开的项目列表。

后端依赖：

- `POST /workspaces/open`
- `GET /workspaces/current`
- `GET /workspaces/recent`

验收：

- 用户能选择一个本地项目。
- 新建会话时默认绑定当前项目路径。
- 没有打开项目时，输入框应提示先打开项目。

---

### 2.3 会话管理

桌面端继续保留会话列表，但会话必须绑定项目。

必须支持：

- 新建会话。
- 选择会话。
- 显示会话名称、更新时间。
- 删除会话。
- 加载历史消息。
- 当前会话与当前 workspace 关联。

后端依赖：

- `GET /sessions`
- `POST /sessions`
- `GET /sessions/{id}/history`
- `DELETE /sessions/{id}`
- `PUT /sessions/{id}/name`

验收：

- 删除会话前有确认。
- 会话标题优先使用 `name` 字段。
- 切换会话后消息和当前任务状态正确刷新。

---

### 2.4 聊天与任务输入

聊天仍是当前主要入口，但要从“聊天框”升级为“任务输入”。

必须支持：

- 输入自然语言任务。
- 选择 provider/model。
- 发送后显示用户消息。
- 接收 server SSE 事件并渲染 Agent 消息。
- 任务运行中禁用重复发送，提供取消按钮。
- 支持 `Enter` 发送、`Shift+Enter` 换行。

后端依赖：

- `POST /reply`
- server 内部通过 stdio/JSON-RPC 调 agent-core。

验收：

- 用户能完成一轮请求。
- 运行中可以取消。
- 结束后输入框恢复。

---

### 2.5 Agent 执行时间线

桌面端必须新增执行时间线，用来展示 agent-core 通过 server 转发的事件。

必须支持事件：

- `message`
- `tool_started`
- `tool_finished`
- `tool_failed`
- `permission_required`
- `finish`
- `error`

展示方式：

- `tool_started`：显示工具名和摘要。
- `tool_finished`：显示耗时、结果摘要，可展开。
- `tool_failed`：显示错误信息。
- `finish`：显示任务完成状态。
- `error`：显示错误原因和是否可恢复。

后续可选事件：

- `message_delta`
- `run_output`
- `diff_ready`

验收：

- 用户能看清 Agent 当前在做什么。
- 工具调用不是混在聊天气泡里，而是进入时间线。
- 出错时能定位是 provider、工具、权限还是 server/core 连接问题。

---

### 2.6 权限确认

桌面端必须承接高风险操作确认。

必须支持：

- 收到 `permission_required` 后展示确认卡片。
- 显示工具名、风险级别、摘要、关键参数。
- 用户可以批准或拒绝。
- 批准/拒绝后调用 server API。
- 超时后显示已自动拒绝或已过期。

后端依赖：

- `POST /permissions/{permission_id}/approve`
- `POST /permissions/{permission_id}/deny`
- server 再调用 agent-core 的 `permission.resolve`

验收：

- shell/write file 等高风险操作不会静默执行。
- 用户拒绝后，Agent 能收到拒绝结果并继续或结束。

---

### 2.7 文件树与文件查看

当前阶段只要求“看文件”，不要求完整编辑器。

必须支持：

- 左侧展示当前 workspace 文件树。
- 忽略 `.git`、`target`、`node_modules` 等大目录。
- 点击文件后在右侧查看内容。
- 支持文本文件。
- 二进制文件显示不可预览提示。

后端依赖：

- `GET /workspace/tree?path=`
- `GET /workspace/file?path=`

验收：

- 用户能快速确认 Agent 修改的是哪个项目。
- 用户能打开 Agent 提到的文件。

---

### 2.8 设置面板

当前设置只保留运行必需项。

必须支持：

- server 地址。
- API Key header（如果 server 开启 `NIGHT24_API_KEY`）。
- provider。
- model。
- base URL。
- provider API key 输入。
- permission mode 显示或选择。

安全要求：

- 密钥默认密码框显示。
- 不在普通日志或错误提示里显示明文 key。
- 是否持久化 key 需要单独开关，默认不持久化。

验收：

- 用户能配置模型并发起请求。
- server 开启 API key 时桌面端仍能访问。

---

## 3. 当前可选功能

这些功能可以预留 UI 位置，但不要求第一版完整实现。

### 3.1 Diff 面板

预留右侧 Tab：

- `Files`
- `Diff`
- `Preview`

当前可以先显示空状态：`当前任务尚未产生可审阅变更`。

等后端有：

- `GET /workspace/status`
- `GET /workspace/diff`

再接入真实 diff。

### 3.2 终端/日志面板

当前时间线足够。底部终端可以先不做，等 `run_output` 和 Process API 成熟后再做。

### 3.3 预览面板

当前只预留 Tab，不启动 dev server。

后续依赖：

- `POST /processes/start`
- `GET /processes/{id}/logs`
- preview URL 检测。

---

## 4. 推荐界面结构

```text
┌──────────────────────────────────────────────────────────────┐
│ Top Bar: Project / Server status / Provider / Settings       │
├───────────────┬──────────────────────────┬───────────────────┤
│ Left Sidebar  │ Center                   │ Right Panel        │
│               │                          │                   │
│ Project       │ Chat + Task input        │ Files              │
│ File tree     │ Agent messages           │ Diff (placeholder) │
│ Sessions      │                          │ Preview(empty)     │
├───────────────┴──────────────────────────┴───────────────────┤
│ Bottom / Side Panel: Agent timeline                           │
└──────────────────────────────────────────────────────────────┘
```

### 左侧

- 当前项目。
- 文件树。
- 会话列表。

### 中间

- 当前会话消息。
- 任务输入框。
- 取消按钮。

### 右侧

- 文件查看。
- Diff 占位。
- Preview 占位。

### 底部或右下

- Agent 执行时间线。
- 权限确认卡片。

---

## 5. 关键状态

桌面端至少维护以下状态：

```text
AppState
  serverStatus
  currentWorkspace
  recentWorkspaces
  sessions
  currentSessionId
  messages
  activeRun
  agentEvents
  pendingPermissions
  selectedFile
  selectedRightTab
  settings
```

### activeRun

```json
{
  "run_id": "run-1",
  "session_id": "session-1",
  "status": "running",
  "started_at": "2026-07-01T10:00:00Z"
}
```

### pendingPermission

```json
{
  "permission_id": "perm-1",
  "run_id": "run-1",
  "tool_name": "developer__shell",
  "risk": "high",
  "summary": "运行测试命令",
  "arguments": {
    "command": "cargo test --workspace"
  }
}
```

---

## 6. 桌面端调用的 server API

当前阶段需要 server 提供：

| API | 用途 | 优先级 |
|---|---|---|
| `GET /healthz` | server 连接检查 | P0 |
| `GET /sessions` | 会话列表 | P0 |
| `POST /sessions` | 新建会话 | P0 |
| `GET /sessions/{id}/history` | 会话历史 | P0 |
| `DELETE /sessions/{id}` | 删除会话 | P0 |
| `POST /reply` | 发起 Agent 任务，SSE 返回 | P0 |
| `POST /agent/cancel` | 取消任务 | P0 |
| `GET /tools` | 工具列表/调试 | P1 |
| `POST /workspaces/open` | 打开项目 | P0 |
| `GET /workspaces/current` | 当前项目 | P0 |
| `GET /workspaces/recent` | 最近项目 | P1 |
| `GET /workspace/tree` | 文件树 | P0 |
| `GET /workspace/file` | 文件查看 | P0 |
| `POST /permissions/{id}/approve` | 批准权限 | P0 |
| `POST /permissions/{id}/deny` | 拒绝权限 | P0 |

---

## 7. SSE 事件映射

server 从 agent-core 收到 JSON-RPC `agent.event` 后，桌面端接收 SSE。

建议 SSE payload 保持与 AgentEvent 接近：

```json
{
  "run_id": "run-1",
  "seq": 1,
  "type": "tool_started",
  "created_at": "2026-07-01T10:00:00Z",
  "payload": {
    "tool_name": "developer__read_file",
    "summary": "读取 Cargo.toml"
  }
}
```

桌面端处理规则：

- `message` 进入聊天消息区。
- `tool_*` 进入时间线。
- `permission_required` 进入权限确认区。
- `finish` 更新 activeRun 状态并刷新会话列表。
- `error` 显示错误横幅并写入时间线。

---

## 8. 空状态和错误状态

必须覆盖：

- 未连接 server。
- 未打开项目。
- 当前项目没有文件。
- 没有会话。
- 会话没有消息。
- Agent Core 不可用。
- 当前模型未配置 API Key。
- 权限请求已过期。
- 文件太大无法预览。
- 二进制文件不可预览。

---

## 9. 当前不做

明确不进入当前桌面端范围：

- 内置完整 Monaco 编辑保存流程。
- Git commit UI。
- PR 创建。
- 多 Agent 并行视图。
- 自动安装依赖。
- 自动启动 dev server。
- 视觉拖拽搭建页面。
- 插件市场。
- 云同步和账号登录。

这些能力可以在可视化 coding MVP 稳定后逐步加入。

---

## 10. 第一版验收标准

桌面端第一版完成后，应满足：

- 能连接 server，失败时可见、可重试。
- 能打开本地项目，并显示当前项目。
- 能浏览文件树并查看文本文件。
- 能创建、切换、删除会话。
- 能发送任务并接收 Agent 回复。
- 能展示 Agent 执行时间线。
- 能处理权限确认。
- 能取消运行中的任务。
- 所有高风险操作都不会静默执行。
- 不再依赖独立 `chat-ui`。

