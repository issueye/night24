# Night24 AI Agent 后端 — 开发计划文档

> 基于需求分析文档，制定可落地的分阶段开发计划。

---

## 1. 技术选型

### 1.1 语言与框架
| 组件 | 选型 | 理由 |
|------|------|------|
| 语言 | **Rust 2021 Edition** | 与 goose 保持一致，性能优异，生态成熟。 |
| 异步运行时 | **Tokio** | Rust 事实标准，Axum 依赖。 |
| Web 框架 | **Axum 0.8** |  goose 已验证，支持 HTTP/1.1、HTTP/2、SSE、中间件。 |
| 序列化 | **serde + serde_json** | Rust 标准。 |
| 数据库 | **SQLite + sqlx** | 轻量、单文件、无需独立服务，适合会话持久化。 |
| MCP 客户端 | **rmcp 1.4** | goose 同款，提供完整的 MCP 协议客户端实现。 |
| 配置管理 | **config / etcetera** | 支持多环境配置、目录规范。 |
| 日志与追踪 | **tracing + tracing-subscriber** | 结构化日志，支持 OpenTelemetry。 |
| OpenAPI | **utoipa 4.2** | 自动生成 API 文档， goose 已使用。 |
| 认证 | 自定义 Bearer Token / API Key | 简单可靠，Phase 1 足够。 |
| CLI 参数解析 | **clap 4** | goose 同款，功能强大。 |

### 1.2 外部依赖
| 依赖 | 用途 | 接入方式 |
|------|------|----------|
| OpenAI API | 文本生成、工具调用 | HTTP + `reqwest` |
| Ollama | 本地/私有模型推理 | HTTP + `reqwest` |
| SQLite | 会话与历史存储 | `sqlx::sqlite` |
| 系统 Shell | 命令执行 | `std::process::Command` / `shell-words` |

### 1.3 Workspace 结构（Rust）
```
night24/
├── Cargo.toml                  # Workspace root
├── crates/
│   ├── night24-core/           # 核心领域层
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── agent.rs
│   │       ├── conversation.rs
│   │       ├── providers.rs
│   │       ├── session.rs
│   │       ├── extensions.rs
│   │       └── ...
│   ├── night24-server/         # HTTP 服务层
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── routes/
│   │       ├── state.rs
│   │       └── ...
│   └── night24-mcp/            # MCP 扩展服务器
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── memory.rs
│           └── computer_controller.rs
├── ui/
│   └── text/                   # 可选 TUI
└── docs/
    ├── requirements.md
    └── plan.md
```

---

## 2. 阶段划分

### Phase 0：基建与脚手架（1 周）
**目标**：建立可编译的 workspace，跑通“Hello World”级 HTTP 服务。

- [x] 初始化 Rust workspace。
- [ ] 实现 `night24-core::conversation`（Message、Role、ContentBlock 基础模型）。
- [ ] 实现 `night24-server::main` + Axum `/healthz`。
- [ ] 配置 `sqlx`  migration 与 `sessions.db` 初始化。
- [ ] 配置 CI（GitHub Actions）：lint、format、test、build。

**里程碑**：`cargo run --bin night24-server` 启动成功，访问 `/healthz` 返回 OK。

---

### Phase 1：Agent 核心循环（2 周）
**目标**：实现“用户输入 → 模型推理 → 工具执行 → 流式回传”的最小闭环。

- [ ] **Provider 抽象**：
  - 定义 `Provider` trait（`stream` / `complete`）。
  - 实现 `OpenAIProvider`（OpenAI-Compatible）。
  - 实现 `OllamaProvider`（本地 Ollama）。
- [ ] **Tool 模型**：定义 `Tool`、`ToolCall`、`ToolResult`。
- [ ] **Agent Loop**：
  - 接收用户消息。
  - 调用 Provider 获取响应（stream）。
  - 解析 tool call。
  - 执行工具（内置 `developer__shell`、`developer__write_file`）。
  - 将工具结果回传给 Provider。
  - 循环直到模型返回最终文本。
- [ ] **SSE 接口**：`POST /reply` 流式返回 `MessageEvent`。
- [ ] **Token 统计**：采集并暴露 usage 信息。

**里程碑**：通过 API 发送消息，Agent 能自主执行 Shell 命令并将结果总结返回。

---

### Phase 2：会话管理（1.5 周）
**目标**：实现多会话生命周期管理。

- [ ] **Session 模型**：SQLite schema（`sessions` 表 + `messages` 表）。
- [ ] **SessionManager**：
  - `create_session` / `get_session` / `list_sessions` / `delete_session`。
  - `update_session`（重命名、归档）。
  - `fork_session`（基于历史快照创建新会话）。
- [ ] **会话路由**：
  - `POST /agent/start` → 创建 Session + 初始化 Agent。
  - `GET /sessions` / `GET /sessions/{id}`。
  - `PUT /sessions/{id}/name`。
  - `POST /sessions/{id}/fork`。
- [ ] **会话自动命名**：使用 Provider 为会话生成标题。

**里程碑**：可创建多个会话，每个会话维护独立上下文，重启后历史不丢失。

---

### Phase 3：扩展与工具系统（2 周）
**目标**：建立可插拔的扩展能力。

- [ ] **内置扩展**：
  - `developer__shell`：执行系统命令（带权限确认）。
  - `developer__write_file` / `developer__read_file`：文件操作。
- [ ] **MCP 客户端**：
  - 基于 `rmcp` 实现 `McpClient`。
  - 支持 `list_tools`、`call_tool`。
  - 动态加载外部 MCP Server。
- [ ] **扩展管理 API**：
  - `GET /tools`：列出所有可用工具。
  - `POST /extensions`：加载扩展。
  - `DELETE /extensions/{name}`：卸载扩展。
- [ ] **权限模型**：
  - 三级权限：允许 / 拒绝 / 确认。
  - 敏感工具（Shell、Write）默认确认。

**里程碑**：可通过 API 动态加载 MCP 扩展，Agent 能调用外部 MCP 工具。

---

### Phase 4：上下文压缩与安全（1.5 周）
**目标**：保证长对话稳定性与基础安全。

- [ ] **Compaction**：
  - 检测 Token 超限。
  - 使用轻量模型（或主模型的 cheap 模式）生成摘要。
  - 替换历史消息为摘要块。
- [ ] **Security Inspectors**：
  - `AdversaryInspector`：检测用户输入中的 prompt injection 模式。
  - `EgressInspector`：检测工具输出中的敏感信息外泄（简化版）。
- [ ] **Error Handling**：
  - 工具执行错误回传给模型，允许模型自愈。
  - Provider 错误自动重试（指数退避）。

**里程碑**：对话超过 32K Token 后自动压缩，不丢失核心上下文；注入攻击被拦截并记录。

---

### Phase 5：可观测与生产就绪（1 周）
**目标**：达到可部署、可运维的状态。

- [ ] **OpenAPI 文档**：通过 `utoipa` 自动生成，挂载到 `/docs`。
- [ ] **健康检查与 readiness**：`/healthz`、`/readyz`。
- [ ] **结构化日志**：`tracing` + `tracing-subscriber`，支持 JSON / pretty 格式。
- [ ] **配置管理**：
  - 支持环境变量 + 配置文件（`night24.toml`）。
  - 配置项：监听地址、数据库路径、默认 Provider、API Key 路径。
- [ ] **Graceful Shutdown**：信号处理，等待 in-flight 请求完成。
- [ ] **Dockerfile**：提供容器化镜像。

**里程碑**：`docker build && docker run` 一键启动，OpenAPI 可浏览，日志可观测。

---

## 3. 里程碑汇总

| 里程碑 | 时间 | 交付物 |
|--------|------|--------|
| **M0：脚手架** | 第 1 周末 | 可编译的 Rust workspace、Axum /healthz、SQLite 初始化。 |
| **M1：最小闭环** | 第 3 周末 | /reply SSE 接口、OpenAI + Ollama Provider、Shell 工具执行。 |
| **M2：会话管理** | 第 4.5 周末 | 多会话 CRUD、Fork、历史持久化。 |
| **M3：扩展系统** | 第 6.5 周末 | 内置开发者工具、MCP 客户端、动态加载。 |
| **M4：上下文与安全** | 第 8 周末 | Token 压缩、注入检测、错误自愈。 |
| **M5：生产就绪** | 第 9 周末 | OpenAPI、Docker、配置管理、可观测性。 |

---

## 4. 团队与排期

### 4.1 角色分工
| 角色 | 职责 | 建议人数 |
|------|------|----------|
| Rust 后端工程师 | 核心引擎、Provider、扩展系统 | 1-2 |
| 全栈/运维工程师 | Docker、CI、配置管理、部署 | 0.5 |
| 测试工程师 | 集成测试、E2E 测试 | 0.5（可兼职） |

### 4.2 排期建议（按单人满负荷估算）
```
第 1 周      Phase 0：基建
第 2-3 周    Phase 1：Agent 核心循环
第 4-4.5 周  Phase 2：会话管理
第 5-6.5 周  Phase 3：扩展系统
第 7-8 周    Phase 4：上下文与安全
第 9 周      Phase 5：生产就绪
```

**并行建议**：Phase 2（会话管理）与 Phase 1 末期可轻度并行，因为会话接口不依赖核心 loop 的完整实现。

---

## 5. 风险与缓解

| 风险 | 影响 | 概率 | 缓解措施 |
|------|------|------|----------|
| MCP 协议（rmcp）版本变动 | 扩展系统需要重写 | 中 | 封装 `McpClient` trait，隔离外部依赖。 |
| 上下文压缩算法效果不佳 | 长对话质量下降 | 中 | Phase 1 先不做压缩，Phase 4 采用保守策略（只摘要超长工具输出）。 |
| Ollama 模型能力差异 | 工具调用解析失败 | 高 | 强制使用支持 function calling 的模型（如 `llama3.1`、`qwen2.5`）。 |
| SQLite 并发瓶颈 | 多会话写入竞争 | 低 | 使用 `sqlx` 连接池 + WAL 模式；若并发极高，后续迁移 PostgreSQL。 |

---

## 6. 后续演进路线（Phase 6+）
- **Sub-agent  orchestration**：支持多 Agent 并行/串行协作。
- **Recipe 系统**：YAML/JSON 模板化任务流。
- **ACP 支持**：作为 ACP Server 接入 JetBrains / Zed 等编辑器。
- **本地推理 Runtime**：集成 `llama.cpp` / `candle`，实现完全离线运行。
- **向量记忆**：集成 embedding 模型 + 向量数据库（如 `sqlite-vec` / `pgvector`）。
- **多模态**：支持图片输入、语音输入（Whisper）。
- **Plugin Market**：社区扩展分享平台。
