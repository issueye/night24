# Night24 Server ↔ Agent Core JSON-RPC 协议

> 版本：`2026-07-01`  
> 传输：newline-delimited JSON-RPC 2.0 over stdio  
> 适用进程：`night24-server` ↔ `night24-agent-core`

---

## 1. 设计目标

该协议用于把 Agent 执行引擎从 `night24-server` 中拆出，让 server 作为桌面端与 Agent Core 的桥梁。

目标：

- 保持 Tauri 到 server 的 HTTP/SSE API 稳定。
- Core 进程只通过 stdin/stdout 与 server 通信。
- 支持流式 Agent 事件、工具调用状态、权限确认、取消、错误上报。
- stdout 只承载协议消息，stderr 只承载日志。
- 协议类型可直接落到 `night24-protocol` crate。

非目标：

- 不让 Core 直接暴露 HTTP 服务。
- 不把 workspace 文件树、预览进程、Git diff 全部塞进 Core。
- 不在第一版实现多 Agent Core 负载均衡，但字段需支持 `run_id` 路由。

---

## 2. 传输规则

### 2.1 消息边界

每条 JSON-RPC 消息占一行：

```text
{"jsonrpc":"2.0","id":"1","method":"core.initialize","params":{...}}\n
```

要求：

- 编码：UTF-8。
- 换行：`\n`。接收方应兼容 `\r\n`。
- 单行必须是完整 JSON。
- stdout 不允许输出非 JSON 内容。
- Core 日志、panic hook、tracing 输出必须写 stderr。

### 2.2 并发

- server 可以并发发送多个 request。
- Core 必须按 `id` 返回 response，不要求按请求顺序返回。
- Notification 无 `id`，不需要 response。
- 长任务使用 `accepted + event stream` 模式：`agent.reply` 快速返回 `accepted`，后续通过 `agent.event` notification 推送。

### 2.3 ID

`id` 可以是 string 或 number。Night24 统一使用 string：

```text
rpc-{uuid}
```

业务运行 ID 使用：

```text
run-{uuid}
```

---

## 3. JSON-RPC 基础模型

### 3.1 Request

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-1",
  "method": "agent.reply",
  "params": {}
}
```

### 3.2 Success Response

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-1",
  "result": {}
}
```

### 3.3 Error Response

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-1",
  "error": {
    "code": -32602,
    "message": "invalid params",
    "data": {
      "field": "working_dir"
    }
  }
}
```

### 3.4 Notification

```json
{
  "jsonrpc": "2.0",
  "method": "agent.event",
  "params": {}
}
```

---

## 4. 错误码

标准 JSON-RPC 错误码：

| Code | 名称 | 使用场景 |
|---:|---|---|
| `-32700` | Parse error | 单行不是合法 JSON |
| `-32600` | Invalid Request | 缺少 `jsonrpc/method/id` 等 |
| `-32601` | Method not found | 未知 method |
| `-32602` | Invalid params | 参数结构或字段非法 |
| `-32603` | Internal error | 未分类内部错误 |

Night24 自定义错误码：

| Code | 名称 | 使用场景 |
|---:|---|---|
| `-32001` | CoreNotInitialized | 未 initialize 就调用业务方法 |
| `-32002` | RunNotFound | `run_id` 不存在 |
| `-32003` | RunAlreadyFinished | run 已结束，不能继续操作 |
| `-32004` | PermissionRequestNotFound | 权限请求不存在或已处理 |
| `-32005` | ProviderUnavailable | provider 未配置或缺少 key |
| `-32006` | ToolExecutionFailed | 工具执行失败 |
| `-32007` | Cancelled | run 被取消 |
| `-32008` | Timeout | run 或工具调用超时 |
| `-32009` | ProtocolViolation | 收到违反协议状态机的消息 |

---

## 5. 通用类型

### 5.1 Capability

```json
{
  "name": "agent.reply",
  "version": 1
}
```

### 5.2 ToolDefinition

复用现有 `night24_core::model::Tool` 结构：

```json
{
  "name": "developer__read_file",
  "description": "Read the content of a file within the working directory.",
  "input_schema": {
    "type": "object",
    "properties": {
      "path": { "type": "string" }
    },
    "required": ["path"]
  }
}
```

### 5.3 Message

复用现有 `night24_core::model::Message` JSON 结构：

```json
{
  "id": "msg-1",
  "role": "assistant",
  "content": [
    { "type": "text", "text": "完成了。" }
  ],
  "created_at": "2026-07-01T10:00:00Z"
}
```

### 5.4 ProviderConfig

```json
{
  "provider": "openai",
  "model": "gpt-4o-mini",
  "base_url": "https://api.openai.com/v1",
  "api_key_ref": "server:openai"
}
```

说明：

- `api_key_ref` 是引用名，不直接传明文 key。
- 第一版如必须传 key，可临时用 `api_key`，但不写入日志，不持久化。
- 长期应由 server 注入环境或通过安全通道传递。

---

## 6. 方法总览

| Method | 方向 | 类型 | 说明 |
|---|---|---|---|
| `core.initialize` | server → core | request | 初始化协议、能力和运行配置 |
| `core.shutdown` | server → core | request | 优雅关闭 Core |
| `core.ping` | 双向 | request | 健康检查 |
| `agent.tools` | server → core | request | 获取 Core 可用工具 |
| `agent.reply` | server → core | request | 启动一轮 Agent 执行 |
| `agent.cancel` | server → core | request | 取消 run |
| `agent.event` | core → server | notification | 推送 Agent 事件 |
| `permission.resolve` | server → core | request | 回复权限确认结果 |

第一版不要求 Core 主动 request server。权限请求也通过 `agent.event: permission_required` 推送，server 再调用 `permission.resolve`。

---

## 7. core.initialize

### 7.1 Request

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-1",
  "method": "core.initialize",
  "params": {
    "protocol_version": "2026-07-01",
    "client": {
      "name": "night24-server",
      "version": "0.1.0"
    },
    "workspace_root": "E:\\code\\issueye\\ai_agent\\night24",
    "environment": {
      "permission_mode": "strict",
      "default_provider": "echo"
    },
    "capabilities": [
      { "name": "permission.resolve", "version": 1 },
      { "name": "agent.cancel", "version": 1 }
    ]
  }
}
```

### 7.2 Response

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-1",
  "result": {
    "protocol_version": "2026-07-01",
    "server": {
      "name": "night24-agent-core",
      "version": "0.1.0"
    },
    "capabilities": [
      { "name": "agent.reply", "version": 1 },
      { "name": "agent.tools", "version": 1 },
      { "name": "agent.cancel", "version": 1 },
      { "name": "agent.event", "version": 1 }
    ]
  }
}
```

规则：

- 除 `core.ping` 外，业务方法必须在 initialize 成功后调用。
- 重复 initialize 返回 `ProtocolViolation`，除非未来显式支持 reinitialize。

---

## 8. core.shutdown

### Request

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-2",
  "method": "core.shutdown",
  "params": {
    "reason": "server_exit",
    "grace_ms": 3000
  }
}
```

### Response

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-2",
  "result": {
    "accepted": true
  }
}
```

规则：

- Core 应停止接收新 run。
- 未完成 run 应发 `error` 或 `finish` 事件后退出。
- 超过 `grace_ms` 后 server 可以强杀进程。

---

## 9. core.ping

### Request

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-3",
  "method": "core.ping",
  "params": {
    "nonce": "abc"
  }
}
```

### Response

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-3",
  "result": {
    "nonce": "abc",
    "status": "ok"
  }
}
```

---

## 10. agent.tools

### Request

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-4",
  "method": "agent.tools",
  "params": {
    "include_disabled": false
  }
}
```

### Response

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-4",
  "result": {
    "tools": [
      {
        "name": "developer__read_file",
        "description": "Read the content of a file within the working directory.",
        "input_schema": {
          "type": "object",
          "properties": {
            "path": { "type": "string" }
          },
          "required": ["path"]
        }
      }
    ]
  }
}
```

---

## 11. agent.reply

`agent.reply` 启动一轮 Agent 执行。它不直接返回完整回答，而是快速返回 `run_id`，后续通过 `agent.event` notification 推送。

### 11.1 Request

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-5",
  "method": "agent.reply",
  "params": {
    "run_id": "run-1",
    "session": {
      "id": "session-1",
      "name": "修复登录 bug",
      "working_dir": "E:\\code\\project",
      "conversation": []
    },
    "input": {
      "text": "修复登录失败的问题"
    },
    "provider": {
      "provider": "echo",
      "model": "echo-v1"
    },
    "limits": {
      "max_turns": 10,
      "turn_timeout_ms": 60000,
      "tool_timeout_ms": 30000,
      "total_timeout_ms": 180000
    },
    "options": {
      "stream_message_delta": false,
      "emit_tool_events": true,
      "permission_mode": "strict"
    }
  }
}
```

### 11.2 Response

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-5",
  "result": {
    "accepted": true,
    "run_id": "run-1"
  }
}
```

### 11.3 字段说明

`session.conversation` 使用现有 `Message[]`。第一版由 server 传入历史，Core 返回事件，server 负责最终持久化。

`working_dir` 必须是 server 已验证过的 workspace 内路径。Core 仍需二次校验路径不越界。

`provider.api_key_ref` 可选。若 Core 进程环境中已有对应 key，可不传。

---

## 12. agent.event

Core 使用 notification 推送事件：

```json
{
  "jsonrpc": "2.0",
  "method": "agent.event",
  "params": {
    "run_id": "run-1",
    "seq": 1,
    "type": "tool_started",
    "created_at": "2026-07-01T10:00:00Z",
    "payload": {}
  }
}
```

规则：

- `seq` 在同一个 `run_id` 内从 1 递增。
- server 应按 `seq` 转发给前端；如乱序，可缓冲或记录协议错误。
- 每个 run 必须以 `finish` 或 `error` 结束。

### 12.1 message

```json
{
  "run_id": "run-1",
  "seq": 2,
  "type": "message",
  "created_at": "2026-07-01T10:00:01Z",
  "payload": {
    "message": {
      "id": "msg-1",
      "role": "assistant",
      "content": [
        { "type": "text", "text": "我先检查项目结构。" }
      ],
      "created_at": "2026-07-01T10:00:01Z"
    }
  }
}
```

### 12.2 message_delta

可选。第一版可以不启用。

```json
{
  "run_id": "run-1",
  "seq": 3,
  "type": "message_delta",
  "created_at": "2026-07-01T10:00:01Z",
  "payload": {
    "message_id": "msg-1",
    "delta": "我先检查"
  }
}
```

### 12.3 tool_started

```json
{
  "run_id": "run-1",
  "seq": 4,
  "type": "tool_started",
  "created_at": "2026-07-01T10:00:02Z",
  "payload": {
    "tool_call_id": "tool-1",
    "tool_name": "developer__read_file",
    "summary": "读取 Cargo.toml",
    "arguments": {
      "path": "Cargo.toml"
    }
  }
}
```

### 12.4 tool_finished

```json
{
  "run_id": "run-1",
  "seq": 5,
  "type": "tool_finished",
  "created_at": "2026-07-01T10:00:03Z",
  "payload": {
    "tool_call_id": "tool-1",
    "tool_name": "developer__read_file",
    "duration_ms": 35,
    "summary": "读取完成，2300 字符",
    "result_preview": "[workspace]\\nresolver = \"2\"",
    "is_error": false
  }
}
```

### 12.5 tool_failed

```json
{
  "run_id": "run-1",
  "seq": 6,
  "type": "tool_failed",
  "created_at": "2026-07-01T10:00:03Z",
  "payload": {
    "tool_call_id": "tool-2",
    "tool_name": "developer__shell",
    "duration_ms": 1000,
    "error": {
      "code": "permission_denied",
      "message": "user denied developer__shell"
    }
  }
}
```

### 12.6 permission_required

```json
{
  "run_id": "run-1",
  "seq": 7,
  "type": "permission_required",
  "created_at": "2026-07-01T10:00:04Z",
  "payload": {
    "permission_id": "perm-1",
    "tool_call_id": "tool-3",
    "tool_name": "developer__shell",
    "risk": "high",
    "summary": "运行测试命令",
    "arguments": {
      "command": "cargo test --workspace"
    },
    "timeout_ms": 300000
  }
}
```

### 12.7 run_output

用于命令、测试、构建等输出。第一版如工具仍返回整段字符串，可先不发该事件。

```json
{
  "run_id": "run-1",
  "seq": 8,
  "type": "run_output",
  "created_at": "2026-07-01T10:00:05Z",
  "payload": {
    "source": "developer__shell",
    "stream": "stdout",
    "text": "running 41 tests\n"
  }
}
```

### 12.8 diff_ready

用于后续 diff-first 修改闭环。

```json
{
  "run_id": "run-1",
  "seq": 9,
  "type": "diff_ready",
  "created_at": "2026-07-01T10:00:06Z",
  "payload": {
    "files_changed": 2,
    "insertions": 20,
    "deletions": 5,
    "summary": "修改登录表单校验和测试"
  }
}
```

### 12.9 finish

```json
{
  "run_id": "run-1",
  "seq": 10,
  "type": "finish",
  "created_at": "2026-07-01T10:00:10Z",
  "payload": {
    "status": "completed",
    "messages": [],
    "usage": {
      "input_tokens": 1000,
      "output_tokens": 300,
      "total_tokens": 1300
    }
  }
}
```

`messages` 可选。第一版可以用前面的 `message` 事件持久化，也可以在 `finish` 中返回最终新增消息列表。

### 12.10 error

```json
{
  "run_id": "run-1",
  "seq": 11,
  "type": "error",
  "created_at": "2026-07-01T10:00:10Z",
  "payload": {
    "code": "provider_unavailable",
    "message": "OPENAI_API_KEY is missing",
    "recoverable": true
  }
}
```

---

## 13. permission.resolve

Server 在用户确认后调用 Core。

### 13.1 Approve

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-6",
  "method": "permission.resolve",
  "params": {
    "run_id": "run-1",
    "permission_id": "perm-1",
    "decision": "approve"
  }
}
```

### 13.2 Deny

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-7",
  "method": "permission.resolve",
  "params": {
    "run_id": "run-1",
    "permission_id": "perm-1",
    "decision": "deny",
    "reason": "用户拒绝运行 shell 命令"
  }
}
```

### 13.3 Response

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-6",
  "result": {
    "accepted": true
  }
}
```

规则：

- `permission_id` 只能 resolve 一次。
- 超时未处理时 Core 应按 deny 处理，并发 `tool_failed` 或对应 tool response。
- 若 run 已结束，返回 `RunAlreadyFinished`。

---

## 14. agent.cancel

### Request

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-8",
  "method": "agent.cancel",
  "params": {
    "run_id": "run-1",
    "reason": "user_cancelled"
  }
}
```

### Response

```json
{
  "jsonrpc": "2.0",
  "id": "rpc-8",
  "result": {
    "accepted": true
  }
}
```

规则：

- Core 收到 cancel 后应尽快停止 provider stream 和工具执行。
- 如果无法立即停止正在运行的系统命令，先发 `error/cancelled`，再清理后台任务。
- cancel 后仍允许 Core 发最终 `error` 或 `finish(cancelled)`。

---

## 15. 状态机

### 15.1 Core 生命周期

```text
spawned
  -> initialized
  -> draining
  -> exited
```

规则：

- `spawned`：只接受 `core.initialize` 和 `core.ping`。
- `initialized`：接受业务方法。
- `draining`：不接受新 run，只处理已有 run 和 shutdown。
- `exited`：进程结束，由 server 重启。

### 15.2 Run 生命周期

```text
accepted
  -> running
  -> waiting_permission
  -> running
  -> completed

accepted/running/waiting_permission
  -> cancelling
  -> cancelled

accepted/running/waiting_permission
  -> failed
```

每个 run 最终必须进入：

- `completed`
- `cancelled`
- `failed`

并发送一个终止事件：

- `finish`
- `error`

---

## 16. 安全规则

- stdout 不得输出日志、调试文本、panic backtrace。
- `api_key` 字段不得写入日志。
- server 负责 workspace 根目录选择与权限确认。
- Core 对路径仍需二次校验，不信任 server 传入路径。
- Core 不主动访问桌面 UI、不主动打开浏览器、不直接操作 Tauri。
- 高风险工具必须通过 `permission_required` 等待 server resolve。

---

## 17. 版本兼容

`protocol_version` 使用日期字符串：

```text
2026-07-01
```

兼容策略：

- 同主版本日期内允许新增可选字段。
- 删除字段、重命名字段、改变字段语义必须升级协议版本。
- 未知字段必须忽略。
- 未知 event type，server 应转发为 generic event 或记录 warning，不应崩溃。
- 未知 method 必须返回 `Method not found`。

---

## 18. 第一版最小实现范围

第一版必须实现：

- `core.initialize`
- `core.ping`
- `core.shutdown`
- `agent.tools`
- `agent.reply`
- `agent.cancel`
- `agent.event`:
  - `message`
  - `tool_started`
  - `tool_finished`
  - `tool_failed`
  - `finish`
  - `error`

第一版预留但可以后实现：

- `permission_required`
- `permission.resolve`
- `message_delta`
- `run_output`
- `diff_ready`

---

## 19. 示例完整流程

### 19.1 初始化

Server -> Core:

```json
{"jsonrpc":"2.0","id":"rpc-1","method":"core.initialize","params":{"protocol_version":"2026-07-01","client":{"name":"night24-server","version":"0.1.0"},"capabilities":[{"name":"agent.cancel","version":1}]}}
```

Core -> Server:

```json
{"jsonrpc":"2.0","id":"rpc-1","result":{"protocol_version":"2026-07-01","server":{"name":"night24-agent-core","version":"0.1.0"},"capabilities":[{"name":"agent.reply","version":1},{"name":"agent.tools","version":1},{"name":"agent.event","version":1}]}}
```

### 19.2 发起回复

Server -> Core:

```json
{"jsonrpc":"2.0","id":"rpc-2","method":"agent.reply","params":{"run_id":"run-1","session":{"id":"session-1","name":"session","working_dir":"E:\\code\\project","conversation":[]},"input":{"text":"hello"},"provider":{"provider":"echo","model":"echo-v1"},"limits":{"max_turns":1,"turn_timeout_ms":60000,"tool_timeout_ms":30000,"total_timeout_ms":180000},"options":{"stream_message_delta":false,"emit_tool_events":true,"permission_mode":"permissive"}}}
```

Core -> Server:

```json
{"jsonrpc":"2.0","id":"rpc-2","result":{"accepted":true,"run_id":"run-1"}}
```

Core -> Server:

```json
{"jsonrpc":"2.0","method":"agent.event","params":{"run_id":"run-1","seq":1,"type":"message","created_at":"2026-07-01T10:00:00Z","payload":{"message":{"id":"msg-1","role":"assistant","content":[{"type":"text","text":"hello"}],"created_at":"2026-07-01T10:00:00Z"}}}}
```

Core -> Server:

```json
{"jsonrpc":"2.0","method":"agent.event","params":{"run_id":"run-1","seq":2,"type":"finish","created_at":"2026-07-01T10:00:01Z","payload":{"status":"completed"}}}
```

---

## 20. Rust 类型落地建议

建议 `night24-protocol` 中按以下结构组织：

```text
src/
  lib.rs
  jsonrpc.rs
  methods.rs
  events.rs
  error.rs
```

关键枚举建议使用 serde internally tagged：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AgentEventPayload {
    Message { message: Message },
    MessageDelta { message_id: String, delta: String },
    ToolStarted { tool_call_id: String, tool_name: String, summary: String, arguments: serde_json::Value },
    ToolFinished { tool_call_id: String, tool_name: String, duration_ms: u64, summary: String, result_preview: String, is_error: bool },
    ToolFailed { tool_call_id: String, tool_name: String, duration_ms: u64, error: EventError },
    PermissionRequired { permission_id: String, tool_call_id: String, tool_name: String, risk: RiskLevel, summary: String, arguments: serde_json::Value, timeout_ms: u64 },
    RunOutput { source: String, stream: OutputStream, text: String },
    DiffReady { files_changed: u32, insertions: u32, deletions: u32, summary: String },
    Finish { status: FinishStatus, messages: Vec<Message>, usage: Option<Usage> },
    Error { code: String, message: String, recoverable: bool },
}
```

实现时可以先用 `serde_json::Value` 承载 `payload`，等第一版跑通后再收紧类型。

