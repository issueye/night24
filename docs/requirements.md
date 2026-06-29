# Night24 AI Agent 后端 — 需求分析文档

> 基于 `E:\code\github\goose` 项目分析与 `night24` 项目现状（空仓库）的仿写方案。

---

## 1. 项目概述与目标

### 1.1 项目定位
Night24 是一个**开源自托管 AI Agent 后端服务**，为用户提供可通过 API / WebSocket / ACP 协议接入的智能代理执行引擎。它接收自然语言请求，调度大语言模型（LLM）完成推理，并通过可扩展的**工具/扩展系统**执行实际任务（文件操作、命令执行、外部服务调用等）。

### 1.2 核心目标
| 目标 | 说明 |
|------|------|
| **可复用的 Agent 执行引擎** | 提供稳定、可观测的“请求 → 推理 → 工具调用 → 结果回传”闭环。 |
| **多模型适配** | 支持 OpenAI、Anthropic、Ollama 等主流模型，屏蔽 provider 差异。 |
| **扩展生态** | 基于 MCP（Model Context Protocol）或内置扩展提供工具能力。 |
| **会话持久化** | 支持多会话生命周期管理、历史回溯、断点续聊。 |
| **可观测与可运维** | 提供结构化日志、指标、追踪、健康检查接口。 |

### 1.3 仿写参考基准
参考 `goose` 项目的核心设计：
- **三层架构**：`goose-core`（领域逻辑）→ `goose-server`（HTTP/gRPC 服务层）→ `goose-ui`（前端/客户端）。
- **交互循环**：Human Request → Provider Chat → Tool Execution → Response to Model → Context Revision → Model Response。
- **Provider 抽象**：统一的 `Provider` trait，不同模型通过实现该 trait 接入。

---

## 2. 项目范围

### 2.1 包含范围（In-Scope）
- 后端服务框架（Rust + Axum / Python FastAPI 二选一，建议 Rust 以贴近 goose）。
- Agent 核心执行引擎（Interactive Loop）。
- 多 LLM Provider 适配层（至少覆盖 OpenAI-Compatible + Ollama）。
- 会话管理（创建、查询、列表、删除、Fork）。
- SSE（Server-Sent Events）流式响应接口。
- 内置扩展系统（Shell、文件读写、记忆/向量检索占位）。
- 配置管理、权限控制、基础安全扫描。
- OpenAPI / OpenTelemetry 可观测支持。

### 2.2 暂不包含范围（Out-of-Scope，后续迭代）
- 桌面 GUI / 移动端客户端。
- 本地大模型推理 runtime（可后续通过 Ollama 集成）。
- 完整的 ACP（Agent Client Protocol）服务端（可作为 Phase 2 接入）。
- 复杂的 Recipe 模板市场与可视化编辑器。

---

## 3. 功能模块

### 3.1 模块总览
```
night24/
├── crates/                    # Rust workspace（推荐）
│   ├── night24-core/          # 核心领域层
│   │   ├── agent/             # Agent 执行引擎、工具调用、重试
│   │   ├── session/           # 会话管理、持久化、命名
│   │   ├── providers/         # Provider 抽象与具体实现
│   │   ├── extensions/        # 内置扩展与 MCP 客户端
│   │   ├── context_mgmt/      # 上下文压缩、摘要、Token 管理
│   │   ├── permission/        # 权限模型与确认流程
│   │   ├── security/          # 提示词注入、越权检测
│   │   └── recipe/            # 可复用任务模板
│   ├── night24-server/        # HTTP 服务层
│   │   ├── routes/            # REST API 路由
│   │   ├── state/             # 应用状态管理
│   │   └── openapi/           # OpenAPI 文档生成
│   └── night24-mcp/           # MCP 扩展服务器
│       ├── memory/            # 长期记忆
│       └── computer_control/  # 电脑控制（占位）
├── ui/
│   └── text/                  # TUI 客户端（可选，仿 goose text UI）
└── docs/
    ├── requirements.md        # 本文档
    └── plan.md                # 开发计划
```

### 3.2 核心模块详述

#### 3.2.1 Agent 执行引擎 (`night24-core::agent`)
- **Interactive Loop**：管理从用户输入到模型响应的完整循环。
- **Tool Execution**：解析模型返回的 tool call，路由到对应扩展执行，并处理结果。
- **Tool Confirmation**：敏感操作（Shell、文件写入）需权限确认。
- **Retry & Error Handling**：对 Provider 错误、工具执行错误进行重试或回传给模型自愈。
- **Sub-agent**：支持将子任务委托给独立 Agent 实例（可选 Phase 2）。

#### 3.2.2 Provider 适配层 (`night24-core::providers`)
- **Provider Trait**：定义 `stream()`、`complete()` 等统一接口。
- **消息格式转换**：将内部 `Message` 模型转换为各 Provider 要求的格式（OpenAI / Anthropic / Ollama）。
- **流式响应**：统一 `MessageStream`，支持 partial text 增量回传。
- **Token 统计**：统一采集输入/输出 Token、缓存读写 Token，用于成本估算。

#### 3.2.3 会话管理 (`night24-core::session`)
- **Session 生命周期**：创建、运行、暂停、归档、删除。
- **持久化**：SQLite 存储会话元数据、对话历史、配置快照。
- **会话 Fork / Truncate**：支持基于历史时间点创建分支会话。
- **会话命名**：自动根据对话内容生成会话标题。

#### 3.2.4 扩展系统 (`night24-core::extensions`)
- **内置扩展**：
  - `developer`：Shell 命令执行、文件系统读写。
  - `memory`：长期记忆存储与检索。
- **MCP 客户端**：实现 MCP 协议客户端，连接外部 MCP 服务器。
- **动态加载**：支持运行时启用/禁用扩展。

#### 3.2.5 上下文管理 (`night24-core::context_mgmt`)
- **Compaction**：当对话超过 Token 阈值时，使用轻量模型生成摘要，压缩历史。
- **Revision**：删除冗余、过时的工具输出，保留关键上下文。

#### 3.2.6 权限与安全 (`night24-core::permission` / `security`)
- **权限级别**：默认允许 / 默认拒绝 / 每次确认。
- **安全扫描**：
  - Adversary Inspector：检测 prompt injection。
  - Egress Inspector：检测敏感数据外泄。
- **工具分类**：Shell、Read、Write 等类别对应不同审批策略。

#### 3.2.7 HTTP 服务层 (`night24-server`)
- **REST API**：
  - `POST /reply` — 发送消息并流式返回 SSE。
  - `POST /agent/start` — 创建新会话并启动 Agent。
  - `POST /agent/stop` — 停止 Agent。
  - `GET /sessions` — 列出会话。
  - `GET /sessions/{id}` — 获取会话详情。
  - `PUT /sessions/{id}/name` — 重命名会话。
  - `POST /sessions/{id}/fork` — Fork 会话。
  - `GET /tools` — 获取可用工具列表。
  - `POST /extensions` — 动态加载扩展。
- **WebSocket / SSE**：支持实时推送模型增量输出。
- **认证**：API Key / Bearer Token 中间件。
- **健康检查**：`GET /healthz`。

---

## 4. 非功能需求

### 4.1 性能
| 指标 | 目标 |
|------|------|
| 首 Token 延迟（TTFT） | < 2s（外部 API 场景） |
| 并发会话数 | 单实例 ≥ 50（LRU 缓存策略，闲置会话可驱逐） |
| 工具调用 P99 延迟 | < 5s（本地 Shell / 文件 I/O） |

### 4.2 可靠性
- 进程崩溃重启后，可恢复正在运行的会话状态。
- 会话数据持久化到 SQLite，支持备份与迁移。
- 关键操作（会话创建、消息追加）具备幂等性。

### 4.3 可观测性
- 结构化日志（tracing / logfmt）。
- OpenTelemetry 指标与链路追踪（可选 feature flag）。
- SSE 事件中包含 `token_state`，前端可实时展示 Token 消耗。

### 4.4 安全性
- API 接口需认证。
- 敏感配置（API Key）通过环境变量或密钥链注入，不落盘明文。
- 工具执行沙箱化（Phase 1 仅做基础隔离，Phase 2 引入容器沙箱）。

### 4.5 可扩展性
- Provider 和扩展通过 trait / 插件化设计，新增模型或工具无需改动核心循环。
- 支持通过配置文件热加载扩展列表。

---

## 5. 核心接口设计（草案）

### 5.1 Chat / Reply（SSE 流式）
```
POST /reply
Content-Type: application/json

{
  "session_id": "uuid",
  "user_message": {
    "role": "user",
    "content": [{"type": "text", "text": "请帮我分析当前目录"}]
  }
}
```

**SSE 事件类型**：
- `Message`：助手文本增量或工具请求。
- `Error`：执行错误。
- `Finish`：本轮结束。
- `Notification`：系统通知（如权限请求）。
- `Ping`：保活。

### 5.2 会话管理
```
POST /agent/start
{
  "working_dir": "/home/user/project",
  "model": "gpt-4o",
  "provider": "openai",
  "extensions": ["developer", "memory"]
}

GET /sessions
GET /sessions/{id}
PUT /sessions/{id}/name
POST /sessions/{id}/fork
```

---

## 6. 数据模型

### 6.1 Session（会话）
```yaml
id: string (uuid)
name: string
session_type: enum [User, Scheduled, SubAgent, Hidden, Terminal, Gateway, Acp]
working_dir: path
provider_name: string
model_config: object
conversation: array<Message>
usage: { input_tokens, output_tokens, total_tokens }
accumulated_usage: ...
created_at: datetime
updated_at: datetime
archived_at: datetime?
```

### 6.2 Message（消息）
```yaml
role: enum [user, assistant, system, tool]
content: array<ContentBlock>
  - type: text | tool_request | tool_response | thinking
  - text?: string
  - tool_call?: { id, name, arguments }
  - tool_result?: { id, content, is_error }
metadata: { provider_metadata?, inference_metadata? }
```

### 6.3 Provider（模型提供商）
```yaml
name: string
display_name: string
default_model: string
known_models: array<ModelInfo>
config_keys: array<ConfigKey>
```

---

## 7. 与 Goose 的差异与简化策略

| 特性 | Goose | Night24（Phase 1） | 原因 |
|------|-------|--------------------|------|
| 语言 | Rust | Rust | 保持技术栈一致 |
| 前端 | Desktop + Text UI | 无（纯后端） | 聚焦后端，前端后续独立 |
| ACP | 完整支持 | 占位 / 后期接入 | 降低初期复杂度 |
| Local Inference | 完整 runtime | 通过 Ollama provider 接入 | 避免引入 heavy runtime |
| Recipe 市场 | 完整 YAML 模板 | 基础模板支持 | 先跑通核心循环 |
| Sub-agent | 完整实现 | 基础实现 | Phase 2 再扩展 |
| OAuth 登录 | 多提供商 OAuth | API Key 优先 | 简化认证，快速落地 |

---

## 8. 验收标准
1. 可通过 API 创建会话并流式获取模型回复。
2. 至少支持 2 个 Provider（OpenAI-Compatible、Ollama）。
3. 支持内置 Shell 和文件读写工具。
4. 会话数据可持久化，重启不丢失。
5. 提供 OpenAPI 文档与基础健康检查接口。
6. 具备基础权限确认与安全扫描能力。
