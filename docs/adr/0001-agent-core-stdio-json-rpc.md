# ADR 0001: Agent Core 使用 stdio/JSON-RPC 与 Server 通信

日期：2026-07-01  
状态：Accepted

## 背景

Night24 下一阶段要从聊天式 Agent 后端演进为可视化 vibe coding 桌面工作台。现有 `night24-server` 同时承担 HTTP API、会话管理、Provider 注册、Agent loop、工具执行和 SSE 输出，边界过宽。

为了让桌面端、桥接服务和 Agent 执行引擎解耦，需要把 Agent Core 拆为独立进程。用户明确选择使用 `stdio/JSON-RPC`，而不是本机 HTTP/SSE。

## 决策

新增独立 Agent Core 进程，通过 `stdin/stdout` 与 `night24-server` 进行 JSON-RPC 通信。

目标进程关系：

```text
tauri-app
  |
  | HTTP/SSE
  v
night24-server
  |
  | stdio / JSON-RPC
  v
night24-agent-core
```

`night24-core` 保持为共享领域库，不直接等同于独立进程。新增 `night24-agent-core` 作为二进制 crate，依赖 `night24-core`。

## 职责边界

### tauri-app

- 桌面 UI。
- 文件选择、窗口管理、预览面板。
- 只调用 `night24-server`，不直接调用 Agent Core。

### night24-server

- 桌面端 API 网关。
- 管理 workspace、可视化状态、权限确认、进程生命周期。
- 启动、监控、重启 `night24-agent-core` 子进程。
- 把 HTTP/SSE 请求转换为 JSON-RPC 请求。
- 把 Core 事件转换为前端事件。

### night24-agent-core

- 执行 Agent loop。
- 管理 Provider、工具调用、上下文压缩、安全检查。
- 输出结构化 Agent 事件。
- 不依赖 Axum，不暴露 HTTP 服务，不关心 UI。

## 通信协议

传输：newline-delimited JSON-RPC 2.0 over stdio。

每条消息占一行，`stdout` 只输出 JSON-RPC 消息，日志必须写入 `stderr`。

字段级协议见：`docs/protocol-server-agent-core-json-rpc.md`。

### Request 示例

```json
{"jsonrpc":"2.0","id":"req-1","method":"agent.reply","params":{"session_id":"s1","text":"修复这个 bug","provider":"openai","model":"gpt-4o-mini","working_dir":"E:\\code\\project"}}
```

### Response 示例

```json
{"jsonrpc":"2.0","id":"req-1","result":{"accepted":true,"run_id":"run-1"}}
```

### Notification 示例

```json
{"jsonrpc":"2.0","method":"agent.event","params":{"run_id":"run-1","type":"tool_started","tool_name":"developer__read_file","summary":"读取 Cargo.toml"}}
```

## 首批方法

| Method | 方向 | 说明 |
|---|---|---|
| `core.initialize` | server -> core | 初始化环境、版本、能力 |
| `core.shutdown` | server -> core | 优雅退出 |
| `agent.reply` | server -> core | 发起一轮 Agent 执行 |
| `agent.cancel` | server -> core | 取消指定 run |
| `agent.tools` | server -> core | 返回可用工具 |
| `agent.event` | core -> server | Agent 结构化事件通知 |
| `permission.request` | core -> server | 请求用户确认敏感操作 |
| `permission.resolve` | server -> core | 返回批准/拒绝结果 |

## Agent 事件

首版事件类型：

- `message`
- `message_delta`
- `tool_started`
- `tool_finished`
- `tool_failed`
- `permission_required`
- `diff_ready`
- `run_output`
- `finish`
- `error`

事件必须携带 `run_id`，便于 server 复用单个 Core 进程处理多个会话或后续扩展多 Core 进程。

## 错误处理

- Core 启动失败：server 返回明确错误给 Tauri，并允许重试。
- Core 非预期退出：server 标记所有运行中的 run 为 failed，并推送 `error` 事件。
- JSON 解析失败：接收方返回 JSON-RPC `Parse error`，并记录 stderr。
- 请求超时：server 可取消 run，必要时重启 Core。
- stdout 非 JSON 内容视为协议错误；Core 日志只能写 stderr。

## 影响

收益：

- Agent Core 崩溃不会直接拖垮 server 和桌面 UI。
- server 可以专注桥接、权限、workspace、diff、process、可视化事件。
- Core 可以独立测试、独立运行，未来可替换为远程/多实例。
- 不占用额外本地端口，适合桌面应用打包。

代价：

- 需要维护稳定 JSON-RPC 协议。
- 流式事件要通过 notification 表达，server 需要做事件路由。
- 进程生命周期、超时和日志隔离要严格实现。

## 迁移路径

1. 新增 `night24-protocol` crate，定义 JSON-RPC payload、Agent 请求、Agent 事件。
2. 新增 `night24-agent-core` binary，先实现 `core.initialize`、`agent.tools`、`agent.reply`。
3. `night24-server` 增加 Core 子进程管理器。
4. `/reply` 保持 HTTP/SSE 外部接口不变，内部改为 JSON-RPC 调 Core。
5. 从 server 中逐步移出 Agent 执行逻辑，仅保留桥接和 UI-facing API。
6. 再继续 workspace、diff、permission、process 等可视化能力。
