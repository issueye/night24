# Night24 阶段开发计划（基于现状分析）

> 本文档基于 2026-06-29 对代码库的实际审计结果制定，优先级按"风险/价值"排序。
> 详见 `docs/plan.md`（原始规划）与本文件的差异说明。

---

## 现状基线

| 检查项 | 结果 |
|--------|------|
| `cargo build --workspace` | ✅ 通过（rustc 1.95.0） |
| `cargo test --workspace` | ⚠️ 22 passed / 1 failed（test_web_scraper_tool 依赖外网） |
| 核心闭环（/reply SSE） | ✅ 可用 |
| 内置工具 | ✅ 15 个 |
| Provider | ✅ 6 个（echo/openai/openai_responses/anthropic/ollama/stepfun） |

### 已识别的阻断性问题
1. 🔴 **硬编码密钥泄露**：`night24-server/src/main.rs:122-129` 写死 StepFun API Key
2. 🟠 **孤儿文件**：`night24-core/src/provider.mod.rs`（646 行）无任何 `mod` 引用
3. 🟡 **测试不稳定**：`test_web_scraper_tool` 直接请求 `example.com`

---

## Phase 1 — 安全与卫生（立即执行，高优先级）

**目标**：消除密钥泄露、清理死代码、让测试套件在任何环境稳定通过。

### 1.1 移除硬编码 API Key
- [ ] `main.rs` 中 `with_anthropic` / `with_stepfun` 的 key 改为从环境变量读取（`ANTHROPIC_API_KEY` / `STEPFUN_API_KEY`），缺失时跳过注册该 provider。
- [ ] 在 README 的 "Environment Variables" 章节补充这些变量。
- [ ] （文档提示）提醒用户该 key 已进入 git 历史，需在服务方后台作废并轮换。

### 1.2 删除孤儿文件
- [ ] 删除 `crates/night24-core/src/provider.mod.rs`（已确认不被 `lib.rs` / `provider/mod.rs` 引用）。

### 1.3 修复外网依赖测试
- [ ] `test_web_scraper_tool` 改为针对本地 HTML 片段验证解析逻辑（抽取 HTML→text 的纯函数），消除对 `example.com` 的依赖。
- [ ] 必要时把网络抓取部分提取为可注入的 client，便于测试。

### 1.4 验证
- [ ] `cargo build --workspace` 通过。
- [ ] `cargo test --workspace` **全部通过（0 failed）**。

**里程碑**：测试套件全绿，无密钥泄露，无死代码。

---

## Phase 2 — 计划项补齐（中优先级）

**目标**：兑现 `plan.md` Phase 2/3/5 中已宣称但未实现的能力。

### 2.1 权限确认流程
- [ ] `PermissionManager` 支持按工具名配置 `Allow/Deny/Confirm` 策略表。
- [ ] `Confirm` 级别在 `/reply` 流中通过 SSE 事件 `Notification` 上报，等待前端确认（先实现"默认放行 / 默认拒绝"两档，确认交互留接口）。
- [ ] 敏感工具（`developer__shell` / `developer__write_file`）默认 `Confirm`。

### 2.2 Session Fork 与自动命名
- [ ] `SessionStore::fork_session(source_id, at_index?)`：复制历史到新 session，返回新 id。
- [ ] `POST /sessions/{id}/fork` 路由。
- [ ] 会话自动命名：首轮对话后，用当前 provider 生成简短标题；失败时回退为 `session-{短id}`。

### 2.3 API Key 认证中间件
- [ ] 当设置了 `NIGHT24_API_KEY` 环境变量时，除 `/healthz` 与 `/swagger-ui` 外所有路由要求 `Authorization: Bearer <key>`。
- [ ] 未设置变量时 = 开放模式（向后兼容本地开发）。

**里程碑**：API 具备基础鉴权、会话可 Fork、敏感操作可配置权限。

---

## Phase 3 — 扩展能力（低优先级）

**目标**：让 MCP 不再是空壳。

### 3.1 MCP memory 扩展
- [ ] `night24-mcp/src/memory.rs`：基于 SQLite 的长期记忆 store（`store`/`recall`/`list`）。
- [ ] 通过 `rmcp` 暴露为 MCP tool，供 agent 调用。

### 3.2 （可选）computer_controller 占位清理或实现
- [ ] 当前仅 1 行 `pub struct`；要么实现基础能力，要么删除并从 `lib.rs` 移除导出。

---

## Phase 4 — 质量增强（低优先级）

- [ ] 安全检查升级：`SecurityInspector` 增加轻量 prompt-injection 启发式（指令覆盖、角色劫持关键词）。
- [ ] 上下文压缩升级：超长工具输出先截断/摘要，再整体压缩。
- [ ] Provider 错误指数退避重试。

---

## 执行顺序

```
Phase 1.1 → 1.2 → 1.3 → 1.4（验证）→ Phase 2.* → Phase 3 → Phase 4
```

Phase 1 必须全部完成且测试全绿后，才进入 Phase 2。
