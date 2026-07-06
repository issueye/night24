# Night24 项目冗余与复杂度优化计划

日期：2026-07-05

## 目标

降低当前项目的代码冗余和功能实现复杂度，优先处理高收益、低风险的结构性问题；对高风险核心链路先补测试和隔离边界，再做拆分。

## 当前复杂度热点

### 桌面端

- `tauri-app/src/App.jsx`：已通过 hook / utility 拆分降至约 419 行，当前主要保留发送任务、顶层页面布局和 hook 接线。
- `tauri-app/src/styles/workspace.css`：仅保留工作区基础 grid；左侧栏、时间线、header、message、panel、banner 和空态样式已拆至专用 CSS 文件。
- `tauri-app/src/styles/desktop-shell.css`：已清空并移除；桌面壳变量、chrome、workspace、sidebar、status、conversation、overlay、event 和 responsive 规则已迁移到对应专用 CSS 文件。
- `tauri-app/src/components/settings/SkillSettings.jsx` 与 `HookSettings.jsx`：已抽共享列表详情壳；样式已按 provider / hook-skill 分离，单组件仍保留各自请求、状态和表单细节。

### Server 与 Agent Core

- `crates/night24-agent-core/src/lib.rs`：约 1369 行。`run_agent_with_events` 混合 provider 流式调用、delta、工具分派、hook、取消、超时和消息回填。
- `crates/night24-server/src/reply.rs`：`reply_core` 同时负责 session、上下文压缩、ReplyParams 构造、core 调用、SSE 转发和持久化。
- `crates/night24-server/src/core_client.rs`：进程生命周期、JSON-RPC 请求池、stdout 解析、事件路由、重启状态集中在一个客户端类型。
- 工具生命周期事件包装重复：普通工具、子代理工具、skill 工具分别维护 BeforeTool/AfterTool/ToolStarted/ToolFinished/ToolFailed。

### GTS 脚本引擎

- `crates/night24-gts/src/evaluator/builtins.rs`：约 2673 行，内置对象、JSON、集合、日期、Promise 等全部集中。
- `crates/night24-gts/src/bytecode/interp.rs`：约 2546 行，主循环、opcode handler、调用、异常、await 和测试混合。
- `crates/night24-gts/src/bytecode/compiler.rs`：约 2428 行，语句、表达式、模块、控制流、函数闭包都在一个 emitter。
- `crates/night24-gts/src/stdlib/modules/web.rs`：约 1027 行，HTTP routing、middleware、response、WebSocket、worker、静态 helper 混合。
- hook 接入目前使用单 worker 执行 GTS，符合 VM 单线程约束，但慢 hook 会阻塞后续 hook；timeout 和 instruction limit 不是对阻塞 I/O 的强制抢占。

## 发现的重复与冗余

- 桌面端会话创建逻辑在 `createSession` 和 `ensureSession` 重复。
- `diff_ready` 事件触发 `openContextTab('diff')` 后又手动 `loadWorkspaceDiff()`，存在重复请求风险。
- 任务结束后 `finish` 分支和 `sendTask finally` 都会刷新 sessions。
- `useApiClient` 暴露 memoized `headers`，`apiJson` 内部又重新拼接鉴权 header，调用方经常再传入 `headers`。
- server 端权限模式在 `reply.rs`、`main.rs`、agent-core `tools.rs` 分散维护。
- server 端 JSON-RPC typed call 模式重复。
- GTS stdlib 中参数读取、callable 判断、对象构造、serde/value 转换存在跨模块重复。

## 优化原则

1. 先拆外围状态和纯函数，不先重写 SSE、provider streaming、tool loop。
2. 核心行为保持不变，优先移动代码和抽公共 helper。
3. 对高风险链路先补测试，再做结构拆分。
4. CSS 不先大规模删除，先按功能迁移并保留导入顺序，避免视觉回归。
5. GTS 先拆边界清楚的内置模块和 helper，再动 interpreter/compiler 主循环。

## 分阶段计划

### Phase 1：低风险前端瘦身

状态：当前批次已完成。

- 已完成：抽出 `useSubAgents`，从 `App.jsx` 移除子代理池加载和轮询状态。
- 已完成：抽出 `estimateContextUsage` 纯函数，降低 `App.jsx` 计算逻辑密度。
- 已完成：移除 `diff_ready` 分支的重复 diff 加载。
- 已完成：移除 finish 分支的重复 session 刷新，保留 `sendTask finally` 统一刷新。
- 已完成：抽 `useTimeline`，承接 `addTimeline` 和 timeline 截断策略。
- 已完成：抽 `useSessions`，统一 session 列表、创建、选择、删除、ensure session。
- 验证：每步运行 `npm run build`，涉及交互行为时补桌面端手工检查。

### Phase 2：设置与右侧面板组件拆分

状态：当前批次已完成。组件拆分、CSS 功能迁移、视觉检查清单和 desktop shell 收尾审计均已完成。

- 已完成：将 `SubAgentPanel.jsx` 拆为 `SubAgentStats`、`SubAgentList`、`SubAgentDetail`。
- 已完成：从 `HookSettings.jsx` 和 `SkillSettings.jsx` 抽通用列表详情壳组件 `SettingsListDetail`。
- 已完成：新增 `docs/desktop-css-visual-checklist.md`，建立 CSS 功能迁移前的全局布局、聊天流、设置弹窗、右侧面板和子代理面板视觉检查基线。
- 已完成：抽出 `tauri-app/src/styles/base.css`，承接 `:root` 设计变量、全局 box sizing、页面根节点尺寸和基础表单字体 reset。
- 已完成：抽出 `tauri-app/src/styles/layout.css`，承接应用外框、顶部栏、品牌区、状态 pill 和共享按钮基础样式。
- 已完成：抽出 `tauri-app/src/styles/settings.css`，承接设置条、设置弹窗、Provider/Hook/Skill 设置管理界面样式。
- 已完成：抽出 `tauri-app/src/styles/chat.css`，承接欢迎态、消息气泡、权限确认卡、工具调用块、输入区和上下文阈值提示样式。
- 已完成：抽出 `tauri-app/src/styles/context.css`，承接右侧浮窗外框、文件树、文件预览和 Diff 面板样式。
- 已完成：抽出 `tauri-app/src/styles/subagents.css`，承接子代理统计、列表、详情、通讯记录、结果和调试区样式。
- 已完成：抽出 `tauri-app/src/styles/permissions.css`，承接权限确认区与权限卡片样式。
- 已完成：抽出 `tauri-app/src/styles/statusbar.css`，承接底部状态栏、运行状态和旋转动画样式。
- 已完成：抽出 `tauri-app/src/styles/markdown.css`，承接消息 Markdown、inline code、代码块和表格渲染样式。
- 已完成：抽出 `tauri-app/src/styles/diff.css`，承接 Diff 面板、变更文件列表和 diff 行渲染样式。
- 已完成：抽出 `tauri-app/src/styles/theme.css`，承接字体密度和 warm/dark 主题变量覆盖。
- 已完成：将剩余编号 CSS 重命名/迁移为 `workspace.css` 与 `desktop-shell.css`，当前 `styles.css` 不再导入编号 CSS。
- 已完成：抽出 `tauri-app/src/styles/sidebar.css`，承接左侧栏、项目树、最近项目、会话列表和菜单底部区域样式。
- 已完成：抽出 `tauri-app/src/styles/timeline.css`，承接 conversation timeline / timeline rail / timeline point 样式。
- 已完成：抽出 `tauri-app/src/styles/desktop-overlays.css`，承接设置条、右侧浮窗、事件浮窗、tab button 和文件预览等桌面覆盖样式。
- 已完成：抽出 `tauri-app/src/styles/desktop-conversation.css`，承接桌面会话区、消息流、composer 和相关响应式覆盖样式。
- 已完成：抽出 `tauri-app/src/styles/desktop-sidebar.css`，承接桌面壳侧栏覆盖、导航行、项目树和会话列表覆盖样式。
- 已完成：抽出 `tauri-app/src/styles/desktop-status.css`，承接桌面状态栏和事件按钮覆盖样式。
- 已完成：抽出 `tauri-app/src/styles/desktop-responsive.css`，承接桌面壳末尾独立响应式布局覆盖。
- 已完成：抽出 `tauri-app/src/styles/settings-providers.css`，承接 Provider 配置、profile 列表和 provider form 样式。
- 已完成：抽出 `tauri-app/src/styles/settings-hooks-skills.css`，承接 Hook/Skill 管理、状态提示、详情、危险操作按钮和对应移动端样式。
- 已完成：继续拆分 `settings-hooks-skills.css`，新增 `settings-hooks.css` 与 `settings-skills.css`，分别承接 Hook 管理专属样式和 Skill 管理专属样式，共享状态/空态/危险按钮/响应式规则保留在原文件末尾。
- 已完成：继续拆分 `chat.css`，新增 `chat-permissions.css`、`chat-tools.css`、`chat-composer.css`，分别承接聊天权限卡、工具调用块和输入区/阈值/滚动按钮样式。
- 已完成：继续拆分 `subagents.css`，新增 `subagents-list.css` 与 `subagents-detail.css`，分别承接子代理列表/状态行和详情/通讯/结果/调试区样式。
- 已完成：抽出 Hook 设置纯配置 helper 到 `tauri-app/src/utils/hooks.js`，组件只保留加载、保存和表单渲染。
- 已完成：移除 `apiJson` 调用方重复传入的 `headers`，保留 `/reply` SSE 原始 `fetch` 显式 header。
- 已完成：抽出桌面端 `apiAuthHeaders` helper，统一 API key / Bearer header 构造，`apiJson` 与 `/reply` SSE 原始 `fetch` 复用同一鉴权 header 入口。
- 已完成：抽出 `useServerStatus`，承接 server 健康检查、readyz 状态和 core 重启流程。
- 已完成：抽出 `useRunControls`，承接取消运行、主动上下文压缩和权限决策 API 流程。
- 已完成：抽出 `useMessageActions`，承接消息可见性过滤、同 id 替换和 assistant 打字机消息展示逻辑。
- 已完成：抽出 `tauri-app/src/utils/reply.js`，集中构造 `/reply` 请求体，收敛 trim、可选字段和 context threshold 解析逻辑。
- 已完成：抽出 Provider profile 查找与表单默认值 helper，收敛初始化、切换、删除 profile 时重复的字段回填逻辑。
- 已完成：从 `workspace.css` 抽出 `empty-states.css`，承接欢迎面板、placeholder 和 empty block 空态样式，导入位置保持在 workspace 后以稳定级联。
- 已完成：从 `workspace.css` 抽出 `workspace-banner.css`，承接顶部 banner 与 banner button 样式，保持原属性与导入级联位置。
- 已完成：从 `workspace.css` 抽出 `workspace-messages.css`，承接工作区消息流基础布局，导入位置保持在 workspace 后、chat/desktop 覆盖前。
- 已完成：从 `workspace.css` 抽出 `workspace-header.css`，承接会话头部、core note、context actions 和 permission trigger 样式，保持原级联顺序。
- 已完成：从 `workspace.css` 抽出 `workspace-panels.css`，承接 `.left-panel` / `.center-panel` 面板容器样式，导入位置保持在 workspace 后、workspace header 前。
- 已完成：从 `desktop-shell.css` 抽出 `desktop-workspace.css`，承接 `.workspace-grid` 桌面壳布局覆盖，导入位置紧跟 desktop shell 以稳定后续覆盖。
- 已完成：从 `desktop-shell.css` 抽出 `desktop-chrome.css`，承接桌面壳 app frame、顶部栏、品牌区、状态 pill 和共享按钮覆盖，导入位置保持在 desktop shell 后。
- 已完成：从 `desktop-shell.css` 抽出 `desktop-theme.css`，承接桌面壳 `:root` 主题变量，导入位置保持在 desktop shell 后、desktop chrome 前。
- 已完成：从 `desktop-overlays.css` 抽出 `desktop-events.css`，承接 TimelinePanel 事件浮窗、事件列表和事件行样式，桌面覆盖层文件保留设置条、上下文浮窗和共享浮窗头部。
- 已完成：完成 `workspace.css` 与 `desktop-shell.css` 收尾审计；`workspace.css` 仅保留基础 grid，`desktop-shell.css` 剩余规则已被后续专用文件覆盖并移除导入。

### Phase 3：Server/Core 结构隔离

- 状态：当前批次已完成。
- 已完成：为 `AgentCoreClient` 增加 typed RPC helper，减少 `call + serde_json::from_value` 重复。
- 已完成：抽出 agent-core stdout 中 `agent.event` 参数识别和事件路由 helper，并增加终止事件清理测试。
- 已完成：从 `reply_core` 抽出 `ReplyParams` 构造、权限模式选择、网络代理清洗和用户消息构造。
- 已完成：抽出 session 准备逻辑，降低 `reply_core` 前半段分支复杂度。
- 已完成：抽 SSE event pump，集中处理 finish/error/diff_ready/session persist。
- 已完成：抽出 `sse_stream_response`，统一 reply SSE 响应状态码和标准 headers。
- 已完成：抽出 `json_error_response`，统一 reply session 准备阶段 JSON 错误响应构造并补充响应体测试。
- 已完成：对 `AgentCoreClient::start_stdout_reader` 增加事件分类 helper，隔离响应分发和 agent event 分发。
- 已完成：抽出 `is_terminal_agent_event` 并补充 finish/error 终止事件识别测试。
- 已完成：抽出 `agent_event_run_id`，统一 agent event 分类和路由中的 run_id 读取逻辑并补充测试。
- 已完成：抽出 `non_empty_path`，集中处理 `NIGHT24_AGENT_CORE_BIN` 空白值忽略逻辑并补充测试。
- 已完成：抽出 API key header 解析 helper，隔离认证 header 读取与常量时间校验并补充测试。
- 已完成：补充 API key 解析边界测试，覆盖 Authorization 非 UTF-8 时回退到 `X-API-Key`。
- 已完成：继续收敛 API key header 解析，将 header 字符串读取与 Bearer token 裁剪拆为纯 helper，并补充非 Bearer Authorization 回退 `X-API-Key` 测试。
- 已完成：统一权限模式解析，避免 server 与 agent-core 行为漂移。
- 已完成：抽出 `handle_permission_decision`，统一 `approve_permission` / `deny_permission` 的 core 调用和 `AcceptedResponse` 构造。
- 已完成：将 Rust 侧 Hook 事件名下沉到 `night24-protocol::HookEvent`，server 校验和 agent-core 执行侧复用同一事件定义。
- 已完成：抽出 Hook 配置入口判断 helper，统一 `script` / `inline_script` 非空白校验，并补充边界测试。
- 已完成：补强 workspace 路径解析 helper 测试，覆盖 workspace root 别名、子路径、绝对路径拒绝和 `..` 逃逸拒绝。
- 已完成：抽出 session mutation 错误映射 helper，统一 rename/fork 的 not found 与内部错误响应，并补充边界测试。
- 已完成：抽出 `json_rpc_method` helper，统一 core stdout JSON-RPC `method` 字段读取，并补充非字符串 method 不误判为通知事件的边界测试。
- 已完成：`reply.rs` 中 SSE 格式化与事件持久化复用 `core_event_type`，收敛重复事件类型读取，并补充非字符串 `type` 默认按 `message` 事件名格式化的边界测试。
- 已完成：`core_client.rs` 抽出 `route_json_rpc_response`，统一 stdout reader 对 pending JSON-RPC response 的投递和移除逻辑，并补充命中/未知 id 的请求池边界测试。
- 已完成：`reply.rs` 抽出 `text_message` helper，收敛 user message 与 fallback assistant message 的文本消息构造，并补充当前用户无回复时才追加 placeholder 的边界测试。
- 已完成：`core_client.rs` 增加 `PendingResponses` / `EventSenders` 类型别名，收敛请求池和事件 sender map 类型签名，并补充接收端关闭时仍清理 pending/sender 的边界测试。
- 已完成：`workspace.rs` 抽出 `classify_diff_stat_line`，集中 diff 文件头、插入行、删除行分类，并补充 `+++` / `---` 文件头不计入增删行的边界测试。
- 已完成：`sessions.rs` 抽出 `forced_compaction_preserve_recent`，集中 forced compact 时保留最近消息数量的边界计算，并补充 0/1/2/7/20 会话长度测试。

### Phase 4：Agent Core 工具生命周期统一

- 状态：当前批次已完成。
- 已完成：从 `run_agent_with_events` 抽出 system prompt / skill 注入构造函数，降低核心循环职责。
- 已完成：新增 `ToolLifecycle` helper，统一普通工具、子代理工具、skill 工具的 BeforeTool / ToolStarted / AfterTool / ToolFinished / ToolFailed 包装路径。
- 已完成：新增 `ensure_tool_permission` helper，统一普通工具、子代理工具、skill 工具的权限确认入口。
- 已完成：抽出 `run_provider_turn` / `emit_provider_message`，隔离 provider streaming、message_delta 和本轮消息收集逻辑。
- 已完成：拆出 `execute_turn_tools` / `execute_tool_request_response`，让 `run_agent_with_events` 保留 provider turn 和消息编排职责。
- 已完成：拆出 `run_started_hook` / `finish_reply_events` / `run_end_hook`，集中处理成功、失败、取消的 finish/error 转换。
- 已完成：补充 `developer__skill_load` 失败路径回归测试，覆盖 lifecycle failure、outer failure、assistant error message、finish tool_response 的顺序和内容。
- 已完成：补充普通内置工具失败路径回归测试，覆盖 lifecycle failure、outer failure 和 finish messages 中 tool_request/tool_response error 的顺序与内容。
- 已完成：补充普通工具权限拒绝回归测试，覆盖拒绝后不触发 `tool_started`、发出 `tool_execution_failed`、finish tool_response 记录拒绝原因。
- 已完成：补充运行中取消普通工具回归测试，覆盖 `tool_started` 后取消、不发 `tool_finished`、发出 cancelled `tool_failed`、finish 状态为 cancelled。
- 已完成：修复子代理/skill 特殊工具 `Confirm` 权限被直接放行的问题，统一发出 `permission_required` 并等待用户决策。
- 已完成：补充子代理工具权限拒绝回归测试，覆盖拒绝后不触发 `tool_started`、不创建子代理记录、finish tool_response 记录拒绝原因。
- 已完成：补充 skill 工具严格权限拒绝回归测试，覆盖拒绝后不触发 `tool_started`、不读取技能正文、finish tool_response 记录拒绝原因。
- 已完成：补充子代理取消工具回归测试，覆盖 `developer__subagent_cancel` 事件顺序、池状态变为 cancelled 和 finish payload。
- 已完成：`subagents.rs` 抽出 `count_status`，收敛子代理池 snapshot 状态统计逻辑，并补充 sync alias 解析与 queued 不计入终态统计的边界测试。
- 下一步：继续围绕子代理/Skill/Hook 可观测性和服务端稳定性做小步补强；避免重新扩大 GTS 深拆范围。

### Phase 5：GTS 引擎模块化

- 状态：本轮已收尾。后续只处理明确缺陷、验证失败或阻塞 Hook/Skill/子代理链路的问题。
- 已完成：从 `evaluator/builtins.rs` 拆出 `builtins/json.rs`，承接 `JSON.stringify` / `JSON.parse` 与 JSON helper。
- 已完成：从 `evaluator/builtins.rs` 拆出 `builtins/date.rs`，承接 Date 实例方法实现。
- 已完成：从 `evaluator/builtins.rs` 拆出 `builtins/primitive.rs`，承接 Number / Boolean 全局对象与 Number 方法实现。
- 已完成：从 `evaluator/builtins.rs` 拆出 `builtins/string.rs`，承接 String 全局对象与 String 方法实现。
- 已完成：从 `evaluator/builtins.rs` 拆出 `builtins/array.rs`，承接 Array 全局对象与 Array 方法实现。
- 已完成：从 `evaluator/builtins.rs` 拆出 `builtins/promise.rs`，承接 Promise 构造器、静态方法与实例方法实现。
- 已完成：从 `evaluator/builtins.rs` 拆出 `builtins/collections.rs`，承接 Map / Set 全局对象与方法实现。
- 已完成：从 `evaluator/builtins.rs` 拆出 `builtins/math.rs`、`builtins/object.rs`、`builtins/globals.rs`，承接 Math、Object、parseInt、parseFloat、isNaN、isFinite。
- 已完成：将 `bytecode/interp.rs` 和 `stdlib/modules/tui/render.rs` 中测试移动到子测试模块，降低运行时代码文件噪声。
- 已完成：从 `stdlib/modules/web.rs` 拆出 `web/helpers.rs` 与 `web/response.rs`，承接 header/query/params helper、web.json/text/static helper 和 buffered response 构造发送。
- 已完成：从 `stdlib/modules/web.rs` 拆出 `web/routing.rs`、`web/request.rs`、`web/ws.rs`，承接 route 注册、普通 HTTP 请求分发、promise/streaming completion 和 WebSocket upgrade 处理。
- 已完成：从 `stdlib/modules/web.rs` 拆出 `web/app.rs` 与 `web/workers.rs`，承接 createApp/app.close 状态构建、listen 入口、serial loop、prefork worker loop 和 worker shutdown 通知。
- 已完成：抽 stdlib helper，新增 `ArgReader` / `ObjectView` 并迁移 web.listen、markdown.renderTerminal、tui opts 标量读取。
- 已完成：新增 `required_callable` / `is_callable` helper，并迁移 web routing 与 events listener 校验。
- 已完成：新增 `ObjectBuilder` helper，并迁移 markdown 返回对象、web.listen 返回对象和 TUI message 对象构建。
- 已完成：为 serde/value 转换 helper 补充单元测试，覆盖嵌套 JSON、runtime data 和非数据对象 fallback。
- 已完成：扩大 callable helper 覆盖面，迁移 async、timers、retry、watch 和 test 模块中的同构函数校验。
- 已完成：继续扩大 callable helper 覆盖面，迁移 cli、net_http_server、net_socket_server、net_ws_server、sse 和 TUI app 中的手写函数校验。
- 已完成：扩大 `ObjectBuilder` 覆盖面，迁移 highlight、schema、semver、process、terminal 中的小型返回对象构建。
- 已完成：为 web routing 补充无网络单元测试，覆盖 route path 拆分和 handler 过滤。
- 已完成：继续扩大 `ArgReader/ObjectView` 覆盖面，迁移 rate_limit、signal、watch 中的 opts 数字读取。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 cache、prometheus、net_ip、xml、archive_zip 中的一次性小对象构建。
- 已完成：为 web helpers 补充无网络单元测试，覆盖 query/header 构建、websocket upgrade 识别和 route params 注入。
- 已完成：抽出 GTS web HTTP handler chain 匹配 helper，并补充无网络单元测试覆盖 middleware prefix params、method filtering、ALL exact matching、websocket skip 和多 handler 顺序。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 url.parse 返回对象与 mime.parseMediaType 返回对象构建。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 db.exec、db.stmt.exec 和 SQL row mapping 的小型返回对象构建。
- 已完成：继续扩大 `ArgReader/ObjectView` 覆盖面，迁移 retry opts 解析并补充默认值与非数字字段测试。
- 已完成：继续扩大 `ObjectView/ObjectBuilder` 覆盖面，抽出 net_http_client options 解析并复用到 request / stream 路径。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，抽出 net_http_server headers/query/request 对象构建并补充纯函数测试。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 GTS web app 实例对象初始化。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 exec command builder 初始化和 mail address / parseMessage 小型返回对象构建。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 env.parse / env.toObject 和 SSE parse event 小型返回对象构建。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 TOML table 转换和 CSV header row 小型返回对象构建。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 MIME media type params 和 process env 快照返回对象构建。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 HTTP client response 对象和 mail RFC5322 headers 返回对象构建。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 GTS web helper 中 headers/query/route params 对象构建，以及 HTTP/WS request 对象初始化，同时保留 route params 后续共享注入语义。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 `test.run()` 统计返回对象和 `pty.tryWait()` 状态快照返回对象；保留 `expect()`/`.not` 链和 PTY 实例等共享状态对象。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 socket / WebSocket / SSE / signal watcher / TUI app/node marker 与鼠标事件对象初始化，同时保留连接、监听器、reader、app 和 node registry 等共享运行状态语义。
- 已完成：继续扩大 `ObjectBuilder` 覆盖面，迁移 cache、rate_limit、prometheus、events、pty、test 和 db 中剩余共享状态对象初始化；`stdlib/modules` 中手写 `HashData::default()` 初始化样板已清零。
- 已完成：继续扩大 `ArgReader/ObjectView` 覆盖面，迁移 buffer、diff、fs、highlight、markdown、net_ws_client、runtime、signal、table、terminal、time 中的低风险必填参数和 options 读取。
- 已完成：继续扩大 `ArgReader/ObjectView` 覆盖面，迁移 crypto、json、mime 中的低风险必填参数和 options 读取，并收敛 JSON stringify options helper。
- 已完成：继续扩大 `ArgReader/ObjectBuilder` 覆盖面，迁移 `path.parse()` 入参读取和返回对象构建，并补充跨平台纯函数测试。
- 已完成：继续扩大 `ArgReader/ObjectBuilder` 覆盖面，迁移 `path.relative`、`path.normalize`、`path.dirname`、`path.basename`、`path.extname`、`path.isAbs`、`path.toSlash`、`path.fromSlash`、`path.format`、`path.splitList` 的低风险参数读取，并为 `path.format` 严格字符串字段语义补充测试。
- 已完成：继续扩大 `ArgReader/ObjectBuilder` 覆盖面，迁移 `url.parse`、`url.resolve`、`url.pathToFileURL`、`url.fileURLToPath` 的低风险字符串参数读取，抽出 `url.format` 对象格式化 helper 并补充严格字符串字段测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 `color.ansi`、`color.strip` 和命名颜色 helper 的低风险字符串/数字参数读取，并补充 ANSI 包装/strip 纯函数测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 `glob.glob`、`glob.match`、`glob.hasMagic` 的低风险字符串参数读取，并补充通配符匹配纯函数测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 `log.format` 和命名日志 helper 的低风险字符串参数读取，并补充日志格式化纯函数测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 `stream.fromString` 的低风险字符串参数读取，并补充 text-backed stream 对象字段测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 `hex.decode` 的低风险字符串参数读取，并补充 HEX 解码大小写和非法输入测试；保留 `hex.encode` 的任意值转 bytes 语义。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 `base64.decode` / `base64.decodeURL` 的低风险字符串参数读取，并补充标准与 URL-safe Base64 解码测试；保留 encode 的任意值转 bytes 语义。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 `compression.gzipCompress` / `compression.gzipDecompress` 的低风险字符串参数读取，并补充 gzip latin1 字符串 round-trip 测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 `gzip.compressFileSync` / `gzip.decompressFileSync` 的低风险路径参数读取，并补充 gzip bytes round-trip 测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 `template.render` / `template.renderHTML` / `template.renderFileSync` / `template.escapeHTML` 的低风险字符串参数读取，并补充 HTML escape 与模板 lookup 测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 text 模块字符串/数字参数读取，抽出宽度截断与换行纯函数，并补充 ANSI strip 后宽度、截断、换行测试。
- 已完成：继续收敛 text 模块宽度参数处理，抽出 `width_limit` 统一负数归零和小数截断语义，并补充边界测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 `sse.parse` 的低风险字符串参数读取，并补充 SSE event/data/id/retry 解析测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 net_ip 模块低风险字符串参数读取，抽出 `join_host_port` 纯函数，并补充 CIDR contains 与 IPv6 host:port 测试。
- 已完成：继续收敛 path 模块 helper，抽出 `path_format_file_name` 统一 `base` 优先和 `name + ext` fallback 逻辑，并补充纯函数边界测试。
- 已完成：继续收敛 path 模块 helper，抽出 `split_path_list` 统一 `path.splitList` 的平台路径列表拆分，并补充平台分隔符测试。
- 已完成：继续收敛 path 模块 helper，抽出 `path_name_parts` 统一 `path.parse` 的 base/name/ext 字段提取，并补充多扩展名边界测试。
- 已完成：继续收敛 path 模块 helper，抽出 `path_format_join` 统一 `path.format` 的 dir/root 拼接选择，并补充 dir 优先于 root 的边界测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 terminal 模块 moveTo/setTitle/style/hyperlink 参数读取，抽出 terminal style/hyperlink 纯字符串 helper，并补充 ANSI/OSC 输出测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 cache 实例方法 key 参数读取，抽出 cache entry/expiration helper，并补充 TTL 边界测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 prometheus 实例方法 name/value 参数读取，并补充 set/inc/get 与 snapshot entry 测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 events 实例方法 event 参数读取，并补充 listener count/list、remove 和 removeAllListeners 语义测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 XML/TOML/YAML parse/read/write 低风险字符串参数读取，并补充 XML round-trip、TOML table/stringify、YAML parse/stringify 测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 validation、jwt.decode 和 semver 低风险字符串/数字参数读取，并补充校验、JWT decode、semver compare/inc/satisfies 测试。
- 已完成：继续扩大 `ArgReader` 覆盖面，迁移 CSV parse/read/write、Mail parse/getHeader 和 Zip list/extract/create 的必填参数读取，并补充 quoted CSV、RFC5322 header unfold、Zip 路径清理/安全目标测试。
- 已完成：多 worker 并行扩大 `ArgReader/ObjectView` 覆盖面，迁移 random、test/TUI、exec、image/pdf、watch、web.static 等低风险参数读取；保留随机数范围转换、TUI 消息和文件监听行为不变。
- 已完成：多 worker 并行清理 `stdlib/modules` 中剩余 direct `required_string(ctx, ...)` / `required_number(ctx, ...)` 调用，覆盖 db/env、cli、process/pty、socket/ws server/client；当前该类旧调用点已清零。
- 已完成：继续扩大 `ArgReader/ObjectBuilder` 覆盖面，迁移 stdlib helper 层 codec/http 参数读取，以及 module/make_buffer/http response/json/serde_value 的纯对象构建；除 `ObjectBuilder` 自身外，stdlib helper/module 手写 `HashData::default()` 样板已清零。
- 已完成：补强 JSON pointer/diff 纯函数回归测试，覆盖 escaped path、嵌套对象自动创建、数组 append/remove、deep clone 独立性和 diff value clone，并修复 `pointer_set` 自动创建路径时的嵌套借用问题。
- 已完成：补强 serde/value 转换边界测试，覆盖 JSON object key 确定排序、整数/浮点形状、非有限数转 null，并修复 integer-valued number 序列化为 JSON float 的问题。
- 已完成：补强 bytecode `try/finally` 控制流回归测试，覆盖 `return`、`break`、`continue`、嵌套 finally 和 finally return/throw 覆盖原始 completion。
- 已完成：在 bytecode compiler 层新增 active finally 跟踪和 pending exit 跳板，让 `return` / `break` / `continue` 先执行必须经过的 finally，再恢复原始控制流。
- 已完成：统一 tree-walker 与 bytecode 的 finally completion 语义，finally 中的 return/throw/break/continue 会覆盖原始 completion，普通 finalizer 表达式仍丢弃。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/emit.rs`，承接常量发射、名称加载、jump placeholder/patch、末尾 opcode 扫描和 operand metadata helper，为后续语句/表达式 emitter 拆分降低耦合。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_helpers.rs`，承接类声明注册和对象/成员属性 key 解析 helper，为后续 class/object literal 编译拆分准备边界。
- 当前保护线：bytecode 与 tree-walker 的 `try/finally` return/break/continue/throw 覆盖语义已有回归测试；后续拆 compiler/interpreter 时必须先跑这组语义测试，避免控制流回退。
- 下一批建议：优先拆 `compiler.rs` 中边界清晰的 expression literal / pattern / function proto helper，再拆 statement/control-flow emitter；每步只移动代码或补测试，不混入语义改动。
- 下一批建议：`interp.rs` 优先拆 opcode 解码/栈操作/异常展开 helper，并继续把测试留在 `bytecode/interp/tests.rs`，避免主循环和测试重新耦合。
- 已完成：从 `bytecode/interp.rs` 继续抽出栈/常量/packed args helper 到 `bytecode/interp_helpers.rs`，承接 stack underflow、字符串常量读取和 packed call args 追加，保持 VM 主循环语义不变。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_functions.rs`，承接 function proto 构造/注册和 lexical-this proto 重建，并保留 bytecode 内部 `compile_method_proto` 转发边界。
- 已完成：继续收敛 `bytecode/compiler_functions.rs` 边界，将函数声明 lowering 下沉到 `compile_func_decl`，主编译器 `Stmt::FuncDecl` 仅保留语句分发。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_match.rs`，承接 match arm、match body、pattern test 和 pattern position 编译 helper，match block 内语句仍通过原 `compile_stmt` 适配回调保持语义。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_assign.rs`，承接 assignment、compound name assignment 和 prefix/postfix update operator lowering，主编译器仅保留表达式调度和共享 opcode/error helper。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_calls.rs`，承接普通 call、super call、this receiver 计算、spread args 和 optional call 共用参数发射 helper。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_literals.rs`，承接 number/bool/null/undefined/string/regexp/template literal 发射，主表达式分发先走 literal fast path。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_control.rs`，先承接 `if` 与 `while` 语句编译，循环 frame 字段维持 bytecode 内部可见，后续继续迁移 for/labeled/break/continue 时避免再扩大主编译器职责。
- 已完成：继续扩大 `bytecode/compiler_control.rs` 覆盖面，迁移 `for` / `for-in` / `for-of` 语句编译，主编译器进一步保留 statement dispatch、try/finally 跳板和 labeled/break/continue 边界。
- 已完成：继续扩大 `bytecode/compiler_control.rs` 覆盖面，迁移 labeled statement 分发；主编译器保留 break/continue finally 跳板作为当前控制流保护边界。
- 已完成：继续收敛 `bytecode/compiler_match.rs` 边界，让 match block 直接接收原始 `compile_stmt` 回调并在模块内创建临时 loop/finalizer 栈，移除主编译器中的 match body 中转函数。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_expr.rs`，承接非字面量表达式分发和递归表达式编译入口；主编译器通过 bytecode 内部 re-export 保持现有模块调用路径。
- 已完成：从 `bytecode/interp.rs` 继续下沉调用/构造语义到 `bytecode/interp_helpers.rs`，承接 `call_value` / `construct_value`，主循环只保留 `Call` / `CallSpread` / `New` 的栈布局处理。
- 已完成：从 `bytecode/interp.rs` 继续抽出 opcode operand 读取 helper 到 `bytecode/interp_helpers.rs`，统一 byte/u16/u32 操作数解码与截断字节码错误处理，主循环只保留 opcode 语义分发。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_collections.rs`，承接 array/object literal 编译、spread 处理和 computed key 发射，复合表达式拆分继续保持行为不变。
- 已完成：从 `bytecode/interp.rs` 继续抽出栈运算和 open upvalue 关闭 helper 到 `bytecode/interp_helpers.rs`，承接一元/二元运算栈弹出、运行时错误传播和闭包捕获值关闭逻辑。
- 已完成：从 `bytecode/interp.rs` 继续抽出异常展开 helper 到 `bytecode/interp_helpers.rs`，集中匹配 protected region、恢复 catch value 和 handler ip。
- 已完成：从 `bytecode/interp.rs` 继续抽出局部变量/upvalue 操作 helper 到 `bytecode/interp_helpers.rs`，承接 local slot 读写、upvalue 读写和对应 VMError 边界。
- 已完成：从 `bytecode/interp.rs` 继续抽出变量名/global 操作 helper 到 `bytecode/interp_helpers.rs`，承接 name/global load/store、typed declaration、assignment 保留值语义和类型检查边界。
- 已完成：从 `bytecode/interp.rs` 继续抽出数组/对象/迭代操作 helper 到 `bytecode/interp_helpers.rs`，承接 array slice、new array/object、property/index get/set、iterator next 和 len 语义。
- 已完成：从 `bytecode/interp.rs` 继续抽出模块 import/export 与 resolved promise 包装 helper 到 `bytecode/interp_helpers.rs`，承接 importer 调用、exports 写入、export * 过滤 default 和动态 import promise 包装。
- 已完成：从 `bytecode/interp.rs` 继续抽出 class/closure 构造 helper 到 `bytecode/interp_helpers.rs`，承接 class declaration 构建、closure object 组装和 upvalue name 映射。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `ToString` / `TypeOf` / `Await` / `ThrowMatchError` 单栈操作 helper，主循环进一步保留 opcode 分发职责。
- 已完成：补强 String `search` 多字节前缀语义，regex byte offset 转换为字符 index，并增加 tree-walker 回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `Call` / `CallSpread` / `New` 栈布局 helper 到 `bytecode/interp_helpers.rs`，主循环只保留 operand 解码、位置计算和 helper 分派。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_classes.rs`，承接 class value 发码、class 声明存名和 class expression 编译入口。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `Spread` 栈操作 helper 到 `bytecode/interp_helpers.rs`，承接数组 packed args 展开和对象 spread 合并。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `read_string_operand` 到 `bytecode/interp_helpers.rs`，统一纯字符串 operand 的读取、位置定位和常量类型校验。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `read_const_operand` 到 `bytecode/interp_helpers.rs`，统一 `Const` 操作数读取和常量池越界 VMError，并补充畸形 bytecode 回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `read_name_operand` 到 `bytecode/interp_helpers.rs`，统一 `StoreName` / `StoreGlobal` / `StoreTypedName` 的名称 operand 与 const 标记解析，并补充非字符串名称 operand 回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `read_type_operand` 到 `bytecode/interp_helpers.rs`，统一 typed declaration 类型表 operand 读取和缺失类型注解 VMError，并补充畸形 bytecode 回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `read_byte_operand_with_pos` 到 `bytecode/interp_helpers.rs`，统一 local/upvalue 单字节 operand 与位置计算，并补充截断 upvalue operand 回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `read_u16_operand_with_pos` / `read_u32_operand_with_pos` 到 `bytecode/interp_helpers.rs`，统一 Call/New/NewArray/NewClass/JumpIf* 操作数位置计算，并补充截断 jump operand 与缺失 closure proto VMError 回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `read_usize_operand_with_pos` 与 `read_function_proto_operand` 到 `bytecode/interp_helpers.rs`，统一 16 位索引转 usize 和 closure prototype 缺失错误处理，并补充截断 closure operand 回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 closure upvalue 捕获 helper 到 `bytecode/interp_helpers.rs`，集中 open upvalue 复用、env closed capture 和 parent upvalue 缺失错误处理，保留现有闭包/upvalue 回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 jump/loop/conditional jump helper 到 `bytecode/interp_helpers.rs`，统一控制流目标读取、条件出栈和栈空错误处理，并补充 `JumpIfTrue` 条件缺失回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `LoadThis` / `SuperMethod` / `Throw` helper 到 `bytecode/interp_helpers.rs`，集中 this 读取、super 方法解析和 throw 出栈封装，并补充 `Throw` 缺失值回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `Call` operand 解码 helper 到 `bytecode/interp_helpers.rs`，统一参数数量与 this receiver 标记解析，并补充 `Call` 缺失 operand 回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `PushArg` 栈操作 helper 到 `bytecode/interp_helpers.rs`，统一参数值出栈与 packed args 追加路径，并补充 `PushArg` 空栈回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `New` / `NewArray` operand helper 到 `bytecode/interp_helpers.rs`，统一构造调用和数组字面量计数读取路径，并补充 `New` 缺失 operand 回归测试。
- 已完成：从 `bytecode/interp.rs` 继续抽出 `NewClass` operand helper 到 `bytecode/interp_helpers.rs`，统一 class declaration 索引读取和 class 构建分派，并补充 `NewClass` 缺失 operand 回归测试。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_access.rs`，承接 dynamic import、await、member/index read、this/super 等访问类表达式发码。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_conditionals.rs`，承接 ternary、optional chain、logical operator 和 nullish coalescing 等条件/短路表达式发码。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_modules.rs`，承接 import、re-export、export declaration 和 export default 发码。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_declarations.rs`，承接 let/var/const 声明、typed declaration 和数组/对象解构声明发码。
- 已完成：继续收敛 `bytecode/compiler_declarations.rs` 边界，将 `Stmt::Let` / `Stmt::Var` / `Stmt::Const` 的声明分发下沉到专用入口，主编译器仅保留语句分发。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_operators.rs`，承接 prefix/infix/update operator 发码和二元运算 opcode 映射。
- 已完成：扩展 `bytecode/compiler_functions.rs`，承接 function expression 与 arrow function 的 Closure 发码入口。
- 已完成：扩展 `bytecode/compiler_calls.rs`，承接 `new` expression 的 callee/args 构造发码入口。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_abrupt.rs`，承接 loop/finally frame、return/break/continue abrupt action 和 pending finally exit 跳板；主编译器保留 `try` 区域编排边界，try/finally 语义测试继续通过。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_try.rs`，承接 try/catch/finally protected region 发码、catch 绑定和 exceptional finally 路径；主编译器进一步收敛为顶层编译与语句分发。
- 已完成：从 `bytecode/compiler.rs` 抽出 `bytecode/compiler_stmt.rs`，承接语句级分发、表达式语句保留值处理和 block 递归编译；主编译器进一步只保留程序级入口、共享 re-export 与测试。
- 已完成：将 `bytecode/compiler.rs` 中测试移动到 `bytecode/compiler/tests.rs`，主 compiler 文件进一步只保留编译入口、共享 re-export 和测试模块声明。
- 已完成：继续收敛 `bytecode/compiler_templates.rs`，抽出模板片段收尾 helper，统一 `${...}` 表达式片段和文本片段的 `Concat` 发射路径，并补充模板插值 opcode 编译测试。
- 已完成：继续收敛 `bytecode/compiler_calls.rs`，抽出 `super()` 构造调用发码 helper，集中 `LoadThis` / `SuperMethod("constructor")` / 带 this receiver 的 `Call` 发射路径，并补充 super constructor opcode 编译测试。
- 已完成：继续收敛 `bytecode/compiler_assign.rs`，抽出名称 operand 发射和复合赋值 opcode 解析 helper，统一简单/复合名称赋值的 `LoadName` / `AssignName` 写入路径，并补充复合赋值 opcode 编译测试。
- 已完成：继续收敛 compiler 字符串 operand 发码，将 `Opcode + string constant + u16 operand` 写入抽到 `compiler_helpers::emit_string_operand`，并接入 assign/access/calls/conditionals/collections 中的 name/property/module/super operand 路径。
- 已完成：继续收敛 `bytecode/compiler_modules.rs`，复用 `emit_string_operand` 统一 import/export/re-export 中 `ImportModule` / `GetProperty` / `StoreName` / `ExportName` 的字符串 operand 发码。
- 已完成：继续收敛声明与临时绑定发码，`compiler_expr` / `compiler_classes` / `compiler_functions` / `compiler_match` / `compiler_try` / `compiler_abrupt` 复用 `emit_string_operand` 统一 identifier load、声明存名、match/try/finally 临时绑定的字符串 operand 写入。
- 已完成：继续收敛 `bytecode/compiler_control.rs`，for-in/for-of 的迭代临时变量、循环变量存名、迭代结果 `done` / `value` 属性读取复用 `emit_string_operand`，保留循环跳转与 break/continue patch 语义不变。
- 已完成：继续收敛 `bytecode/compiler_declarations.rs`，抽出 const-aware 声明名 operand helper，保留 const 高位标记编码，并让对象解构属性读取复用 `emit_string_operand`。
- 已完成：继续收敛 `bytecode/compiler_templates.rs`，抽出 template literal 文本片段字符串常量发码 helper，统一普通文本片段和空模板 fallback 的 `Const` 写入路径。
- 已完成：将字符串 operand 发码 helper 从 `compiler_helpers` 迁移到 `bytecode/emit.rs`，让 `emit_load_name` 与各 compiler 模块复用同一个底层 `emit_string_operand`。
- 已完成：继续收敛 `bytecode/compiler_declarations.rs` 的常量发码，默认 `undefined`、数组解构索引和默认值比较统一复用 `emit_value_constant`，移除局部手写 `Const + operand` 路径。
- 已完成：继续收敛 `bytecode/compiler_assign.rs` / `bytecode/compiler_operators.rs` 的常量发码，`++/--` 的 `1`、`delete` 的 `true` 和 `void` 的 `undefined` 统一复用 `emit_value_constant`，compiler 模块内不再散落手写 `Opcode::Const`。
- 已完成：从 `bytecode/interp.rs` 主循环抽出 `check_execution_budget`，集中 timeout / instruction limit 采样检查，并补充采样边界回归测试。
- 已完成：继续收敛 `bytecode/interp.rs` 栈操作分支，抽出 `dup_stack` helper，统一 `Dup` 的栈顶复制与 underflow 错误处理，并补充空栈 `Dup` 回归测试。
- 已完成：继续收敛 `bytecode/interp.rs` 返回分支，抽出 `return_from_stack` / `return_value` 内部 helper，统一 `Return` / `ReturnNull` 的 upvalue 关闭路径，并补充空栈 `Return` 默认 `undefined` 回归测试。
- 已完成：继续收敛 `bytecode/interp.rs` 闭包分支，抽出 `push_closure_from_operand` 内部 helper，集中函数原型 operand 读取、upvalue 捕获和闭包压栈路径。
- 已完成：继续收敛 `bytecode/interp.rs` 指令位置计算，抽出 `current_instruction_pos` 内部 helper，统一 operandless opcode、二元/一元 op 和 throw 分支的当前位置读取。
- 已完成：继续收敛 `bytecode/interp.rs` 字符串 operand 分支，抽出 `set_property_from_operand` / `get_property_from_operand` / `super_method_from_operand`，集中 property 与 super method 的 operand 读取和 stack helper 调用。
- 已完成：继续收敛 `bytecode/interp.rs` 模块字符串 operand 分支，抽出 `import_module_from_operand` / `export_name_from_operand`，集中 import/export name operand 读取和 stack helper 调用。
- 已完成：继续收敛 `bytecode/interp.rs` 变量字符串 operand 分支，抽出 `load_name_from_operand` / `assign_name_from_operand`，集中动态名称读取和 stack/env helper 调用。
- 已完成：继续收敛 `bytecode/interp.rs` 全局变量 operand 分支，抽出 `load_global_from_operand` / `store_global_from_operand`，集中 global name operand 读取并保留 const 标记兼容语义。
- 已完成：继续收敛 `bytecode/interp.rs` byte operand 分支，抽出 `load_local_from_operand` / `store_local_from_operand` / `load_upvalue_from_operand` / `store_upvalue_from_operand`，集中 local/upvalue operand 读取和 stack helper 调用。
- 已完成：继续收敛 `bytecode/interp.rs` store name operand 分支，抽出 `store_name_from_operand` / `store_typed_name_from_operand`，集中普通/类型声明存名的 name/type operand 读取和 stack/env helper 调用。
- 已完成：继续收敛 `bytecode/interp.rs` 常量 operand 分支，抽出 `push_const_from_operand`，让主 opcode arm 不再直接读取 operand，只保留分发和 helper 调用。
- 已完成：继续收敛 `bytecode/interp.rs` 闭包 operand 分支，将 `push_closure_from_operand` 下沉到 `interp_helpers`，集中函数原型读取、upvalue 捕获和闭包压栈路径。
- 已完成：从 `bytecode/interp_helpers.rs` 拆出 `interp_helpers/operands.rs`，承接 byte/u16/u32/string/name/type/const operand 解码和截断 bytecode VMError 构造，避免 helper 文件继续膨胀。
- 已完成：从 `bytecode/interp_helpers.rs` 拆出 `interp_helpers/modules.rs`，承接 import/export、export * 和动态 import resolved promise 包装 helper，保持 VM 主循环调用入口不变。
- 已完成：从 `bytecode/interp_helpers.rs` 拆出 `interp_helpers/control.rs`，承接 jump/conditional jump、throw 封装、catch unwind 和 match error helper，保持 try/catch/finally 保护线语义不变。
- 已完成：从 `bytecode/interp_helpers.rs` 拆出 `interp_helpers/bindings.rs`，承接 local/upvalue/name/global 访问、typed binding 校验和类型匹配 helper，保留 `interp::value_matches_type_annotation` 转发边界。
- 已完成：从 `bytecode/interp_helpers.rs` 拆出 `interp_helpers/collections.rs`，承接 array/object 创建、property/index get/set、iterator 和 len helper，保持主循环 opcode 调用入口不变。
- 已完成：从 `bytecode/interp_helpers.rs` 拆出 `interp_helpers/calls.rs`，承接 packed args、spread、call/call spread、construct 和 callable/constructor 分派语义，保持 `Call` / `New` opcode 行为不变。
- 已完成：从 `bytecode/interp_helpers.rs` 拆出 `interp_helpers/closures.rs`，承接 open upvalue 关闭/捕获、class 构建、function proto operand 和 closure object 构造 helper。
- 已完成：从 `bytecode/interp_helpers.rs` 拆出 `interp_helpers/stack.rs`、`async_ops.rs` 与 `access.rs`，分别承接基础栈操作、await/promise 处理和 super method operand 读取，`interp_helpers.rs` 收敛为 helper 门面文件。
- 已完成：从 `bytecode/compiler_control.rs` 拆出 `bytecode/compiler_iterators.rs`，承接 `for-in` / `for-of` 迭代语句发码、临时迭代变量和 break/continue patch 逻辑，普通控制流文件只保留 if/while/for/labeled 分发。
- 已完成：从 `bytecode/compiler_declarations.rs` 拆出 `bytecode/compiler_decl_store.rs`，承接声明存储 operand 选择和类型/普通声明写入发码，统一 `var` / `let` / `const` 的 store 边界。
- 已完成：从 `bytecode/compiler_declarations.rs` 拆出 `bytecode/compiler_destructuring.rs`，承接数组/对象解构声明、默认值替换和 rest 绑定发码，声明主文件只保留普通声明分发。
- 已完成：从 `bytecode/compiler_match.rs` 拆出 `bytecode/compiler_match_patterns.rs`，承接 literal/ident/wildcard/or/range pattern test 发码，match 主文件只保留 subject、arm、guard 与 body 跳转编排。
- 已完成：从 `bytecode/compiler_functions.rs` 拆出 `bytecode/compiler_function_proto.rs`，承接 method/function/lexical-this proto 构造和子 chunk 编译，函数模块只保留 function declaration/expression/arrow 的 Closure 发码。
- 收尾策略：GTS 脚本语言引擎模块化本轮到此收口；后续只处理明确缺陷、验证失败或阻塞 Hook/Skill/子代理链路的问题，不再继续投入 compiler/interpreter 深拆。
- 下一步：把优化重心转回桌面端体验、Hook/Skill/子代理可观测性和服务端稳定性；涉及 GTS 时只做最小必要改动，并继续核对 `try/finally` 保护线没有语义回退。
- 中期建议：将 `evaluator::expressions` 中被 bytecode 复用的语义迁移到 `semantics` 或 `runtime_ops`，再处理更大的 interpreter/compiler 主循环拆分。
- Phase 5 固定验证：每个 compiler/interpreter 拆分批次必须运行 `cargo fmt --check`、`cargo test -p night24-gts`、`npm run build`（`tauri-app` 目录）、`git diff --check`；文档-only 批次至少运行 `git diff --check`。

### Phase 6：Hook/Script Engine 稳定化

- 状态：当前批次已完成。后续保留阻塞 I/O 取消、worker pool 等较大设计项，单独规划后再实施。
- 已完成：结构化 hook 输出异常改为显式 stderr warning，避免未知 stream、非字符串 text 等情况静默转 stdout。
- 已完成：补充结构化输出异常语义测试。
- 已完成：补充 top-level 先执行、再调用 `execute(args)` 的生命周期测试。
- 已完成：定义 `ScriptEngine` / `HookEngine` trait 边界，将 GTS worker 封装到 `GtsHookEngine` 后面。
- 已完成：为 hook 配置增加 `allowed_modules`，默认 trusted allow-all，显式配置时接入 GTS VM module allowlist。
- 已完成：server hooks API 和桌面端 Hook 设置面板保留并编辑 `allowed_modules`。
- 已完成：server hooks API 增加事件、engine、执行入口、timeout、instruction limit 基础校验。
- 已完成：server hooks API 抽出 `SUPPORTED_HOOK_ENGINES` / `is_supported_hook_engine`，集中维护 hook engine 支持列表，并补充省略、空白、裁剪后支持值和不支持 engine 的校验测试。
- 已完成：GTS hook worker 改为 source/file 与 `execute(args)` 共用同一个 hook deadline，避免分阶段重复获得完整 timeout 预算。
- 已完成：补充 GTS hook deadline 回归测试，覆盖前一个 hook 超时后单 worker 能继续执行后续 hook。
- 已完成：明确 hook top-level 执行与 `execute(args)` 的生命周期语义。
- 后续建议：为阻塞 stdlib 增加取消 token；评估 per-run worker 或 worker pool，作为单独设计项处理。

## 高风险区域

- 桌面端 `sendTask` 与 `handleAgentEvent`：涉及 SSE、权限、取消、finish/error、消息合并。
- agent-core `run_agent_with_events`：涉及 provider streaming、工具请求、delta、timeout 和取消。
- server `reply_core`：涉及会话持久化、SSE、diff_ready 和 core 事件透传。
- GTS interpreter/compiler 主循环：需要测试保护后再拆。

## 当前已执行优化

- `tauri-app/src/hooks/useSubAgents.js`：新增子代理池 hook。
- `tauri-app/src/hooks/useTimeline.js`：新增时间线状态、追加和清空 hook。
- `tauri-app/src/hooks/useSessions.js`：新增会话列表、创建、选择、删除、ensure session hook。
- `tauri-app/src/hooks/useAgentEvents.js`：承接 agent 事件分支处理，`App.jsx` 仅保留事件接线。
- `tauri-app/src/utils/context.js`：新增上下文 token 估算纯函数。
- `tauri-app/src/utils/sse.js`：新增 SSE block 解析与流读取 helper。
- `tauri-app/src/utils/agentEvents.js`：新增 agent 事件 payload 归一化 helper。
- `tauri-app/src/App.jsx`：移除子代理池本地轮询逻辑和内联上下文估算逻辑。
- `tauri-app/src/hooks/useSubAgents.js`：为子代理池加载增加请求引用与请求 id 保护，静默轮询复用 in-flight 请求并避免旧响应回写。
- `tauri-app/src/App.jsx`：减少 `diff_ready` 和 finish 事件触发的重复请求。
- `tauri-app/src/App.jsx`、`useWorkspaceState.js`、`useSessions.js`：移除 `apiJson` 调用方冗余 headers，仅保留 `/reply` SSE 原始 fetch headers。
- `tauri-app/src/hooks/useApiClient.js`：抽出 `apiAuthHeaders` helper，统一 `apiJson` 与 `/reply` SSE fetch 的鉴权 header 构造入口。
- `tauri-app/src/hooks/useApiClient.js`：抽出 `apiRequestHeaders`，统一 JSON 请求与流式请求的基础 header 拼装，并复用 `apiAuthHeaders` 避免鉴权 header 重复手写。
- `tauri-app/src/hooks/useApiClient.js`：新增 request header 归一化，支持普通对象、`Headers` 实例和 tuple 数组输入，避免调用方传入标准 Headers 时被错误展开。
- `tauri-app/src/utils/settings.js`：新增 `normalizeApiBase`，`apiUrl` 构造前裁剪 server base 首尾空白，并对空白 base 回退默认服务地址。
- `tauri-app/src/utils/settings.js`：`normalizeLocalPath` 增加首尾空白裁剪，`compactWorkspaces` 复用路径归一化结果做最近工作区去重 key，减少同一路径不同写法重复展示。
- `tauri-app/src/hooks/useWorkspaceState.js`：为 workspace diff/status 增加工作区 key 与代际保护，防止切换工作区后旧请求回写新状态。
- `tauri-app/src/hooks/useWorkspaceState.js`：为文件预览请求增加请求编号和工作区 key 校验，防止旧 `/workspace/file` 响应覆盖当前预览。
- `tauri-app/src/hooks/useWorkspaceState.js`：为 workspace 加载增加请求编号保护，打开新工作区时使旧加载请求失效，防止旧 `/workspace/tree` 响应覆盖新状态。
- `tauri-app/src/hooks/useServerStatus.js`：新增 server/core 状态 hook，承接健康检查、ready 状态和 core 重启流程。
- `tauri-app/src/hooks/useRunControls.js`：新增运行控制 hook，承接取消、上下文压缩和权限决策流程。
- `tauri-app/src/hooks/useMessageActions.js`：新增消息操作 hook，承接 add/replace 与打字机消息展示，进一步降低 `App.jsx` 事件接线之外的职责。
- `tauri-app/src/components/SubAgentPanel.jsx`：拆分为统计、列表、详情子组件。
- `tauri-app/src/components/settings/SettingsListDetail.jsx`：新增 settings 列表详情共享壳。
- `tauri-app/src/utils/hooks.js`：抽出 Hook 事件列表、Hook 归一化和保存配置转换 helper。
- `crates/night24-server/src/core_client.rs`：新增 typed RPC helper 和 agent event 路由 helper。
- `crates/night24-server/src/reply.rs`：拆出 ReplyParams 构造相关 helper，并补充默认值/代理清洗测试。
- `crates/night24-server/src/main.rs`：抽出权限 approve/deny 共用处理 helper，减少重复响应构造。
- `crates/night24-server/src/main.rs`：抽出 `accepted_response` helper，复用 `/agent/cancel` 与权限 approve/deny 的 accepted 响应对象构造，并补充可选 id 字段保持测试。
- `crates/night24-server/src/main.rs`：抽出 `NIGHT24_DATABASE_URL` / `NIGHT24_DATA_DIR` 非空环境变量读取 helper，并补充纯函数测试。
- `crates/night24-server/src/main.rs`：复用非空环境变量 helper 读取 API key 与 provider key，统一 trim 并忽略空白值。
- `crates/night24-server/src/main.rs`：抽出 provider base URL / model 环境变量默认值 helper，统一 trim、空白值回退和缺省值回退，并补充纯函数测试。
- `crates/night24-server/src/core_client.rs`：`NIGHT24_AGENT_CORE_BIN` 路径 helper 对首尾空白做 trim 后再构造 `PathBuf`，并补充空白值过滤与路径裁剪测试。
- `crates/night24-server/src/workspace.rs`：抽出 workspace recents limit 解析 helper，统一处理空白、非法值、零值和 trim 后数字，并补充边界测试。
- `crates/night24-server/src/sessions.rs`：抽出 session type 解析 helper，统一 create session 入参 trim、已知类型映射和未知值回退，并补充边界测试。
- `crates/night24-server/src/sessions.rs`：抽出 session mutation 错误映射 helper，统一 rename/fork 的 404/500 响应分支。
- `crates/night24-protocol/src/hooks.rs`：新增 `HookEvent` 协议类型、合法事件列表和 snake_case 序列化/解析测试。
- `crates/night24-agent-core/src/hooks.rs`、`crates/night24-server/src/hooks.rs`：复用协议层 `HookEvent`，消除 Rust 侧 Hook 事件名重复维护。
- `crates/night24-agent-core/src/lib.rs`：抽出 system prompt / skill 注入构造函数。
- `crates/night24-agent-core/src/lib.rs`：抽出 provider turn、工具执行循环、finish/error 转换 helper。
- `crates/night24-agent-core/src/hooks.rs`：结构化 hook 输出异常转为显式 stderr warning。
- `crates/night24-agent-core/src/hooks.rs`：新增 `ScriptEngine` / `HookEngine` trait 边界和 `GtsHookEngine` 封装。
- `crates/night24-agent-core/src/hooks.rs`：新增 hook `allowed_modules` 配置并接入 GTS VM 模块 allowlist。
- `crates/night24-agent-core/src/tests.rs`：新增 GTS hook malformed outputs、top-level/execute 生命周期、allowed_modules 拒绝危险模块测试。
- `crates/night24-server/src/hooks.rs`：hooks API 保留 `allowed_modules` 配置字段。
- `crates/night24-server/src/hooks.rs`：新增 hook 配置基础校验与测试。
- `tauri-app/src/components/settings/HookSettings.jsx`：Hook 设置面板支持编辑模块白名单。
- `crates/night24-gts/src/evaluator/builtins/json.rs`：拆出 JSON 内置实现。
- `crates/night24-gts/src/evaluator/builtins/date.rs`：拆出 Date 方法实现。
- `crates/night24-gts/src/evaluator/builtins/primitive.rs`：拆出 Number / Boolean 相关内置实现。
- `crates/night24-gts/src/evaluator/builtins/string.rs`：拆出 String 相关内置实现。
- `crates/night24-gts/src/evaluator/builtins/array.rs`：拆出 Array 相关内置实现。
- `crates/night24-gts/src/evaluator/builtins/promise.rs`：拆出 Promise 相关内置实现。
- `crates/night24-gts/src/evaluator/builtins/collections.rs`：拆出 Map / Set 相关内置实现。
- `crates/night24-gts/src/evaluator/builtins/math.rs`：拆出 Math 相关内置实现。
- `crates/night24-gts/src/evaluator/builtins/object.rs`：拆出 Object 相关内置实现。
- `crates/night24-gts/src/evaluator/builtins/globals.rs`：拆出 parseInt / parseFloat / isNaN / isFinite 全局函数。
- `crates/night24-gts/src/bytecode/interp/tests.rs`：从 `interp.rs` 外移 bytecode interpreter 测试。
- `crates/night24-gts/src/bytecode/compiler_conditionals.rs`：从 `compiler.rs` 拆出 ternary、optional chain、logical operator 和 nullish coalescing 条件/短路表达式发码。
- `crates/night24-gts/src/bytecode/compiler_modules.rs`：从 `compiler.rs` 拆出 import/export/re-export 模块语句发码。
- `crates/night24-gts/src/bytecode/compiler_declarations.rs`：从 `compiler.rs` 拆出声明与解构声明发码。
- `crates/night24-gts/src/bytecode/compiler_operators.rs`：从 `compiler.rs` 拆出 prefix/infix/update operator 发码和二元 opcode 映射。
- `crates/night24-gts/src/bytecode/compiler_functions.rs`：承接 function expression 与 arrow function Closure 发码入口。
- `crates/night24-gts/src/bytecode/compiler_calls.rs`：承接 `new` expression 构造发码入口。
- `crates/night24-gts/src/stdlib/modules/tui/render/tests.rs`：从 `render.rs` 外移 TUI render 测试。
- `crates/night24-gts/src/stdlib/modules/web/helpers.rs`：拆出 web header/query/params helper 和 web.json/text/static helper。
- `crates/night24-gts/src/stdlib/modules/web/response.rs`：拆出 buffered HTTP response 构造与发送逻辑。
- `crates/night24-gts/src/stdlib/modules/web/routing.rs`：拆出 route/use/ws 注册与 handler 校验逻辑。
- `crates/night24-gts/src/stdlib/modules/web/request.rs`：拆出普通 HTTP 请求分发、handler chain、promise completion 和 active stream polling。
- `crates/night24-gts/src/stdlib/modules/web/ws.rs`：拆出 WebSocket upgrade 和连接对象构造逻辑。
- `crates/night24-gts/src/stdlib/modules/web/app.rs`：拆出 createApp/app.close 和 WebApp 状态构建逻辑。
- `crates/night24-gts/src/stdlib/modules/web/workers.rs`：拆出 listen、serial loop、prefork worker loop 和 worker shutdown 通知逻辑。
- `crates/night24-gts/src/stdlib/helpers/args.rs`：新增 `ArgReader` / `ObjectView`，统一 required arg 和 opts hash 标量读取。
- `crates/night24-gts/src/stdlib/modules/markdown.rs`、`tui/mod.rs`、`web/workers.rs`：迁移到 `ArgReader` / `ObjectView`。
- `crates/night24-gts/src/stdlib/helpers/args.rs`：新增 `required_callable` / `is_callable`，统一 GTS callable 参数校验。
- `crates/night24-gts/src/stdlib/modules/events.rs`、`web/routing.rs`：迁移 listener / route handler 校验到 callable helper。
- `crates/night24-gts/src/stdlib/helpers/object_builder.rs`：新增 `ObjectBuilder`，统一小型 Hash 对象构建。
- `crates/night24-gts/src/stdlib/modules/markdown.rs`、`web/workers.rs`、`tui/messages.rs`：迁移小型返回对象构建到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/helpers/serde_value.rs`：补充 serde/value 转换单元测试。
- `crates/night24-gts/src/stdlib/modules/async_.rs`、`timers.rs`、`retry.rs`、`watch.rs`、`test.rs`：扩大 callable helper 覆盖面。
- `crates/night24-gts/src/stdlib/modules/cli.rs`、`net_http_server.rs`、`net_socket_server.rs`、`net_ws_server.rs`、`sse.rs`、`tui/app.rs`：继续扩大 callable helper 覆盖面。
- `crates/night24-gts/src/stdlib/modules/highlight.rs`、`schema.rs`、`semver.rs`、`process.rs`、`terminal.rs`：扩大 `ObjectBuilder` 覆盖面。
- `crates/night24-gts/src/stdlib/modules/web/routing.rs`：补充 route path 拆分和 handler 过滤单元测试。
- `crates/night24-gts/src/stdlib/modules/rate_limit.rs`、`signal.rs`、`watch.rs`：扩大 `ArgReader/ObjectView` opts 读取覆盖面。
- `crates/night24-gts/src/stdlib/modules/cache.rs`、`prometheus.rs`、`net_ip.rs`、`xml.rs`、`archive_zip.rs`：扩大 `ObjectBuilder` 覆盖面。
- `crates/night24-gts/src/stdlib/modules/web/helpers.rs`：补充 query/header/websocket/params 纯函数测试。
- `crates/night24-agent-core/src/tests.rs`：新增 skill load 工具失败路径事件顺序回归测试。
- `crates/night24-agent-core/src/tests.rs`：新增普通内置工具失败路径事件顺序和 finish tool_response 回归测试。
- `crates/night24-agent-core/src/tests.rs`：新增普通工具权限拒绝和运行中取消回归测试。
- `crates/night24-agent-core/src/lib.rs`：子代理/skill 特殊工具接入统一权限确认，`Confirm` 会发出 `permission_required` 并等待决策。
- `crates/night24-agent-core/src/tools.rs`：新增 `ensure_tool_permission`，统一普通工具、子代理工具、skill 工具的权限确认入口。
- `crates/night24-agent-core/src/hooks.rs`：GTS hook source/file 与 `execute(args)` 共用同一个 deadline。
- `crates/night24-agent-core/src/tests.rs`：补充 GTS hook deadline 回归测试，覆盖超时后 worker 继续处理后续 hook。
- `crates/night24-core/src/provider/tool_router.rs`：echo provider 增加 `tool:subagent_cancel:<id>` 测试/调试路由。
- `crates/night24-agent-core/src/tests.rs`：新增子代理工具权限拒绝和取消工具回归测试。
- `crates/night24-gts/src/stdlib/modules/retry.rs`：迁移 retry options 解析到 `ObjectView`，补充默认值和类型回退测试。
- `crates/night24-gts/src/stdlib/modules/net_http_client.rs`：抽出 HTTP options 解析，复用到 request / stream 路径并补充解析测试。
- `crates/night24-gts/src/stdlib/modules/net_http_server.rs`：抽出 HTTP server headers/query/request 对象构建并补充测试。
- `crates/night24-gts/src/stdlib/modules/web/app.rs`：迁移 web app 对象初始化到 `ObjectBuilder`。
- `tauri-app/src/styles/markdown.css`：抽出 Markdown 渲染、inline code、代码块和表格样式。
- `tauri-app/src/styles/permissions.css`、`statusbar.css`：抽出权限确认区和底部状态栏样式。
- `tauri-app/src/styles/diff.css`、`theme.css`：抽出 Diff 面板样式与字体/主题覆盖样式。
- `tauri-app/src/styles/workspace.css`、`desktop-shell.css`：消除剩余编号 CSS 导入，保留原级联顺序。
- `tauri-app/src/styles/sidebar.css`、`timeline.css`：继续从 `workspace.css` 抽出左侧栏/项目树/会话列表和时间线样式。
- `tauri-app/src/styles/desktop-overlays.css`：从 `desktop-shell.css` 抽出设置条、右侧浮窗、事件浮窗和文件预览覆盖样式。
- `tauri-app/src/styles/desktop-conversation.css`：从 `desktop-shell.css` 抽出桌面会话区、消息流和 composer 覆盖样式。
- `tauri-app/src/styles/desktop-sidebar.css`：从 `desktop-shell.css` 抽出桌面侧栏、导航、项目树和会话列表覆盖样式。
- `tauri-app/src/styles/desktop-status.css`、`desktop-responsive.css`：从 `desktop-shell.css` 抽出状态栏和独立响应式覆盖样式。
- `tauri-app/src/styles/desktop-chrome.css`：从 `desktop-shell.css` 抽出桌面壳 app frame、顶部栏、品牌区、状态 pill 和共享按钮覆盖样式。
- `tauri-app/src/styles/desktop-events.css`：从 `desktop-overlays.css` 抽出 TimelinePanel 事件浮窗、事件列表和事件行样式。
- `tauri-app/src/styles/settings-providers.css`、`settings-hooks-skills.css`：从 `settings.css` 抽出 Provider 和 Hook/Skill 设置管理样式。
- `tauri-app/src/styles/settings-hooks.css`、`settings-skills.css`：从 `settings-hooks-skills.css` 继续抽出 Hook 和 Skill 管理专属样式。
- `tauri-app/src/styles/chat-permissions.css`、`chat-tools.css`、`chat-composer.css`：从 `chat.css` 抽出聊天权限卡、工具调用块和输入区相关样式。
- `tauri-app/src/styles/subagents-list.css`、`subagents-detail.css`：从 `subagents.css` 抽出子代理列表和详情区域样式。
- `tauri-app/src/components/subagents/status.js`：抽出子代理状态展示元数据和摘要文本 helper，列表/详情复用同一状态映射；统计栏补充 cancelled 计数展示。
- `crates/night24-agent-core/src/subagents.rs`、`tauri-app/src/components/subagents/SubAgentStats.jsx`：子代理池 snapshot 增加 queued 计数，桌面端统计栏补充排队中状态展示。
- `tauri-app/src/components/subagents/status.js`：新增子代理统计兜底派生 helper；当后端聚合字段缺失时从 `subagents` 列表计算各状态数量。
- `crates/night24-server/src/reply.rs`：抽出 `sse_stream_response`，统一 reply SSE 标准响应 headers。
- `crates/night24-server/src/reply.rs`：抽出 `json_error_response`，统一 reply session 准备阶段 JSON 错误响应。
- `crates/night24-server/src/core_client.rs`：抽出终止事件识别 helper，并补充 finish/error 分类测试。
- `crates/night24-server/src/core_client.rs`：抽出 `agent_event_run_id`，统一 agent event run_id 读取和测试。
- `crates/night24-server/src/core_client.rs`：抽出 `non_empty_path`，集中处理 agent-core 二进制路径环境变量空白值。
- `crates/night24-server/src/auth.rs`：抽出 `provided_api_key`，统一 Bearer / X-API-Key header 解析并补充纯函数测试。
- `crates/night24-server/src/auth.rs`：补充 Authorization 非 UTF-8 时回退 `X-API-Key` 的边界测试。
- `crates/night24-gts/src/stdlib/modules/exec.rs`：迁移 command builder 初始化到 `ObjectBuilder::new().into_shared()`。
- `crates/night24-gts/src/stdlib/modules/mail.rs`：迁移 address 和 parseMessage 返回对象到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/env.rs`：迁移 env.parse 和 env.toObject 返回对象到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/sse.rs`：迁移 SSE parse event 返回对象到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/toml.rs`：迁移 TOML table 返回对象到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/encoding_csv.rs`：迁移 CSV header row 返回对象到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/mime.rs`：迁移 media type params 返回对象到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/process.rs`：迁移 process env 快照返回对象到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/net_http_client.rs`：迁移 HTTP response / stream response 返回对象到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/mail.rs`：迁移 RFC5322 headers 返回对象到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/web/helpers.rs`、`request.rs`、`ws.rs`：迁移 web headers/query/params 和 HTTP/WS request 对象构建到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/test.rs`、`pty.rs`：迁移测试统计结果和 PTY 非阻塞状态快照对象到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/net_socket_client.rs`、`net_socket_server.rs`、`net_ws_client.rs`、`net_ws_server.rs`、`sse.rs`、`signal.rs`、`tui/app.rs`、`tui/node.rs`：迁移 socket/WS/SSE/signal/TUI 对象初始化到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/cache.rs`、`rate_limit.rs`、`prometheus.rs`、`events.rs`、`pty.rs`、`test.rs`、`db.rs`：迁移剩余共享状态对象初始化到 `ObjectBuilder::into_shared()`。
- `crates/night24-gts/src/stdlib/modules/buffer.rs`、`diff.rs`、`fs.rs`、`highlight.rs`、`markdown.rs`、`net_ws_client.rs`、`runtime.rs`、`signal.rs`、`table.rs`、`terminal.rs`、`time.rs`：继续迁移低风险参数/options 读取到 `ArgReader/ObjectView`。
- `crates/night24-gts/src/stdlib/modules/crypto.rs`、`json.rs`、`mime.rs`、`stdlib/helpers/json.rs`：继续迁移低风险参数/options 读取到 `ArgReader/ObjectView`。
- `crates/night24-gts/src/stdlib/modules/path.rs`：迁移 `path.parse()` 到 `ArgReader` 与 `ObjectBuilder`，并补充路径解析返回对象单元测试。
- `crates/night24-gts/src/stdlib/modules/path.rs`：继续迁移 path 模块剩余低风险字符串参数读取到 `ArgReader`，抽出 `path_format_object` 并补充 `path.format` 字段优先级和严格字符串字段测试。
- `crates/night24-gts/src/stdlib/modules/url.rs`：迁移 URL 模块低风险字符串参数读取到 `ArgReader`，抽出 `url_format_object` / `url_parts_from_object` 并补充 `url.format` 对象格式化测试。
- `crates/night24-gts/src/stdlib/modules/color.rs`：迁移 color 模块字符串/数字参数读取到 `ArgReader`，并补充 ANSI 包装和 escape stripping 单元测试。
- `crates/night24-gts/src/stdlib/modules/glob.rs`：迁移 glob 模块字符串参数读取到 `ArgReader`，并补充 separator normalization、`*` / `?` 匹配测试。
- `crates/night24-gts/src/stdlib/modules/log.rs`：迁移 log 模块字符串参数读取到 `ArgReader`，并补充日志 level 大写格式化测试。
- `crates/night24-gts/src/stdlib/modules/stream.rs`：迁移 `stream.fromString` 字符串参数读取到 `ArgReader`，并补充 text-backed stream 原始文本字段测试。
- `crates/night24-gts/src/stdlib/modules/encoding_hex.rs`：迁移 `hex.decode` 字符串参数读取到 `ArgReader`，并补充 HEX 解码大小写和非法输入测试。
- `crates/night24-gts/src/stdlib/modules/encoding_base64.rs`：迁移 `base64.decode` / `base64.decodeURL` 字符串参数读取到 `ArgReader`，并补充标准与 URL-safe Base64 解码测试。
- `crates/night24-gts/src/stdlib/modules/compression.rs`：迁移 compression gzip 字符串参数读取到 `ArgReader`，并补充 gzip latin1 字符串 round-trip 测试。
- `crates/night24-gts/src/stdlib/modules/compress_gzip.rs`：迁移 gzip 文件同步接口路径参数读取到 `ArgReader`，并补充 gzip bytes round-trip 测试。
- `crates/night24-gts/src/stdlib/modules/template.rs`：迁移 template 模块字符串参数读取到 `ArgReader`，并补充 HTML escape 和基础模板 lookup 测试。
- `crates/night24-gts/src/stdlib/modules/text.rs`：迁移 text 模块字符串/数字参数读取到 `ArgReader`，抽出宽度截断与换行纯函数，并补充 ANSI strip 后显示宽度相关测试。
- `crates/night24-gts/src/stdlib/modules/sse.rs`：迁移 `sse.parse` 字符串参数读取到 `ArgReader`，并补充 SSE event/data/id/retry 解析测试。
- `crates/night24-gts/src/stdlib/modules/net_ip.rs`：迁移 net_ip 模块字符串参数读取到 `ArgReader`，抽出 `join_host_port` 纯函数，并补充 CIDR contains 与 IPv6 host:port 测试。
- `crates/night24-gts/src/stdlib/modules/terminal.rs`：迁移 terminal moveTo/setTitle/style/hyperlink 参数读取到 `ArgReader`，抽出 terminal style/hyperlink 纯字符串 helper，并补充 ANSI/OSC 输出测试。
- `crates/night24-gts/src/stdlib/modules/cache.rs`：迁移 cache 实例方法 key 参数读取到 `ArgReader`，抽出 cache entry/expiration helper，并补充 TTL 边界测试。
- `crates/night24-gts/src/stdlib/modules/prometheus.rs`：迁移 prometheus 实例方法 name/value 参数读取到 `ArgReader`，并补充 set/inc/get 与 snapshot entry 测试。
- `crates/night24-gts/src/stdlib/modules/events.rs`：迁移 events 实例方法 event 参数读取到 `ArgReader`，补充 listener count/list、remove 和 removeAllListeners 语义测试，并修复删除最后一个 listener 时的嵌套可变借用问题。
- `crates/night24-gts/src/stdlib/modules/xml.rs`：迁移 XML parse/read/write 字符串参数读取到 `ArgReader`，并补充 XML node object round-trip 与转义测试。
- `crates/night24-gts/src/stdlib/modules/toml.rs`：迁移 TOML parse/read 字符串参数读取到 `ArgReader`，并补充 TOML table 转对象和基础 stringify 测试。
- `crates/night24-gts/src/stdlib/modules/yaml.rs`：迁移 YAML parse/read 字符串参数读取到 `ArgReader`，并补充 YAML mapping parse 和基础 stringify 测试。
- `crates/night24-gts/src/stdlib/modules/validation.rs`：迁移 validation type/email/min/max 参数读取到 `ArgReader`，并补充 email、type、min/max 行为测试。
- `crates/night24-gts/src/stdlib/modules/jwt.rs`：迁移 `jwt.decode` token 参数读取到 `ArgReader`，并补充 payload decode 与 invalid token format 测试。
- `crates/night24-gts/src/stdlib/modules/semver.rs`：迁移 semver parse/compare/inc/satisfies 字符串参数读取到 `ArgReader`，抽出 `inc_semver`，并补充 prerelease compare、inc 和 range satisfies 测试。
- `crates/night24-gts/src/stdlib/modules/encoding_csv.rs`：迁移 CSV parse/read/write 必填参数读取到 `ArgReader`，并补充 quoted field parse/stringify 测试。
- `crates/night24-gts/src/stdlib/modules/mail.rs`：迁移 mail parseAddress/parseAddressList/parseMessage/parseDate/getHeader 参数读取到 `ArgReader`，并补充 quoted comma address 与 folded header 测试。
- `crates/night24-gts/src/stdlib/modules/archive_zip.rs`：迁移 zip list/extract/create 必填参数读取到 `ArgReader`，并补充 Zip entry 路径清理和安全目标测试。
- `crates/night24-gts/src/stdlib/modules/random.rs`：迁移 random int/float/sample/length/bytes 数字参数读取到 `ArgReader`，保持范围检查和整数转换语义。
- `crates/night24-gts/src/stdlib/modules/test.rs`、`tui/mod.rs`：迁移 test.test/test.describe 与 tui.text/key/resize 参数读取到 `ArgReader`，继续复用 callable/ObjectView helper。
- `crates/night24-gts/src/stdlib/modules/exec.rs`、`image.rs`、`pdf.rs`、`watch.rs`、`web/helpers.rs`：迁移 command.setDir、image.info、pdf.info、watch.file、web.static 参数读取到 `ArgReader/ObjectView`。
- `crates/night24-gts/src/stdlib/modules/db.rs`、`env.rs`：迁移数据库 driver/dsn/query 和环境变量 key/content 参数读取到 `ArgReader`，保留 SQL 参数与环境变量副作用语义。
- `crates/night24-gts/src/stdlib/modules/cli.rs`：迁移 CLI flag、FlagSet get/changed 和 args validator 数字参数读取到 `ArgReader`。
- `crates/night24-gts/src/stdlib/modules/process.rs`、`pty.rs`：迁移 process chdir/getenv/setenv/unsetenv 与 PTY spawn/write/resize 参数读取到 `ArgReader`。
- `crates/night24-gts/src/stdlib/modules/net_socket_client.rs`、`net_socket_server.rs`、`net_ws_server.rs`：迁移 socket/ws host/port/deadline 参数读取到 `ArgReader`，并继续复用 callable/ObjectBuilder helper。
- `crates/night24-gts/src/stdlib/helpers/codec.rs`、`http.rs`：迁移 helper 层剩余直接参数读取到 `ArgReader`。
- `crates/night24-gts/src/stdlib/helpers/core.rs`、`encoding.rs`、`http.rs`、`json.rs`、`serde_value.rs`：迁移 module、buffer、HTTP response、JSON pointer/deep clone、serde value 转对象等纯对象构建到 `ObjectBuilder`。
- `crates/night24-gts/src/stdlib/modules/terminal.rs`：终端样式/超链接 helper 抽出纯函数测试，并清理测试中的手写 Hash 初始化。
- `crates/night24-gts/src/stdlib/helpers/json.rs`：补充 JSON pointer/diff 行为测试，并修复 `pointer_set` 缺失中间 Hash 时读写同一 `RefCell` 的嵌套借用问题。
- `crates/night24-gts/src/stdlib/helpers/serde_value.rs`：补充 serde/value 数值和排序边界测试，并让有限整数值序列化为 JSON integer，保留非有限数转 null 语义。

## 验证策略

- 全仓格式：`cargo fmt --check`。
- 前端结构优化：`npm run build`。
- Tauri 桌面壳：`cargo build --manifest-path tauri-app/src-tauri/Cargo.toml`。
- Protocol/Server/Core：`cargo test -p night24-protocol`、`cargo test -p night24-agent-core`、`cargo test -p night24-server`。
- GTS 拆分：`cargo test -p night24-gts`，拆 parser/interpreter/compiler 前先增加针对性测试；bytecode 批次同步运行 `cargo fmt --check`、`npm run build`（`tauri-app` 目录）和 `git diff --check`。

## 下一阶段执行计划

### 桌面端代码与方法实现优化

目标：继续降低 `App.jsx` 的职责密度，把稳定状态流和纯业务方法迁移到 hook / utility 中，避免后续添加子代理控制台、预览、Git 工作流时继续堆叠在根组件。

#### D1：抽 `useTimeline`

范围：

- 新增 `tauri-app/src/hooks/useTimeline.js`。
- 接管 `timeline` 状态、`addTimeline`、截断策略、清空策略。
- 保持 `TimelinePanel` 和 `ChatPanel` 入参不变，降低 UI 回归风险。

迁移后 `App.jsx` 保留：

- `const { timeline, addTimeline, clearTimeline } = useTimeline()`。
- 各事件分支继续调用 `addTimeline`。

验收：

- 发送任务、工具事件、权限事件、取消事件仍能进入事件面板。
- `npm run build` 通过。

#### D2：抽 `useSessions`

状态：已完成。

范围：

- 新增 `tauri-app/src/hooks/useSessions.js`。
- 接管 `sessions`、`currentSessionId`、`messages`、`loadSessions`、`createSession`、`selectSession`、`deleteSession`、`ensureSession`。
- 合并 `createSession` 与 `ensureSession` 中重复的 `POST /sessions` 逻辑。

边界：

- 不在本阶段重写 SSE。
- `sendTask` 仍在 `App.jsx` 内，但通过 `ensureSession` 和 `appendUserMessage` 操作会话状态。

验收：

- 新建、切换、删除会话正常。
- 当前 workspace 切换后 session 列表正确刷新。
- `npm run build` 通过。

#### D3：抽 SSE 与 Agent 事件解析纯函数

状态：已完成。

追加完成：`utils/sse.js` 已补强 SSE block 分隔和流结束 decoder flush，覆盖 CRLF/LF 分隔与尾部字节边界场景，保持 `parseSseBlock` / `readSseStream` 外部接口不变。

追加完成：`useMessageActions.js` 已抽出 `findMessageIndex`，统一按 message id 查找逻辑；无 id 消息改为直接追加/替换，避免 typewriter 更新时误替换其它无 id 消息。

追加完成：`useRunControls.js` 已抽出 pending permission 过滤 helper，取消任务时固定 active run id，并在取消接口不可用时清理当前 run 的 pending permissions；`compactContext` 兼容 `contextUsage` 缺失。

范围：

- 新增 `tauri-app/src/utils/sse.js`：`parseSseBlock`、payload unwrap。
- 新增 `tauri-app/src/utils/agentEvents.js`：事件类型归一化、权限 payload 归一化、finish/error payload 归一化。

边界：

- 暂不把 `handleAgentEvent` 整体搬出 `App.jsx`，先抽无状态解析函数。
- 保留现有 UI 写入顺序，避免 message delta、finish messages 合并行为变化。

验收：

- 补充最小单元测试或在后续引入测试框架前保留纯函数手工验证样例。
- `message`、`message_delta`、`permission_required`、`tool_*`、`finish`、`error` 均保持现有表现。

#### D4：拆设置与子代理面板

状态：已完成：`SubAgentPanel.jsx` 已拆为统计、列表、详情子组件；`HookSettings.jsx` / `SkillSettings.jsx` 已抽共享列表详情壳组件。

范围：

- `SubAgentPanel.jsx` 拆为 `SubAgentStats`、`SubAgentList`、`SubAgentDetail`。
- `HookSettings.jsx` 和 `SkillSettings.jsx` 抽共享的列表/详情壳组件，但不改变接口。

验收：

- 子代理列表、详情、消息、结果预览正常。
- hooks/skills 设置面板可加载、保存、复制调用名。
- `npm run build` 通过。

#### D5：Workspace 状态请求去重

状态：已完成：`useWorkspaceState.js` 已为 workspace diff/status 加载增加 in-flight guard，重复触发时复用同一个请求；切换工作区时通过工作区 key 与代际保护阻止旧请求回写新状态，并同步清理旧 diff 加载态。

追加完成：`useWorkspaceState.js` 已为连续打开工作区增加请求序号保护，避免旧的 open/tree/loadWorkspace 结果覆盖新工作区状态；`utils/messages.js` 已抽出 `updateFirstTextBlock`，收敛 message delta 与文本覆盖的重复 text block 更新逻辑。

边界：

- 不改变 `loadWorkspaceDiff` 的调用接口。
- 不改变 `/workspace/status` 与 `/workspace/diff` API。

验收：

- `diff_ready`、手动打开 diff tab、finish 刷新等连续触发不会并发重复拉取 diff/status。
- `npm run build` 通过。

### 服务端继续优化

目标：继续收敛 `reply.rs` 和 `core_client.rs` 的职责，把会话准备、SSE 转发、事件持久化拆成可测试单元，同时不改变 HTTP API。

追加完成：`workspace.rs` 已将 `parse_git_status` 的单行解析抽成 `parse_git_status_line`，集中 porcelain v1 状态字符校验，并补充普通修改、rename、Unicode 路径和畸形行测试。

追加完成：`hooks.rs` 已抽出 hook 配置校验错误构造 helper，收敛 `hooks[index]` 错误前缀拼接，并补充多 hook 配置中后续 hook 失败时保留准确 index 的边界测试。

追加完成：`workspace.rs` 已抽出 `git_status_path`，集中 porcelain 路径与 rename 目标路径提取逻辑，并补充普通路径、rename 目标和空路径边界测试。

追加完成：`core_client.rs` 的 agent event 路由在消费端关闭时会移除对应 run sender，避免非终止事件继续保留无效 sender，并补充断开连接后的清理测试。

#### S1：抽 session run 准备逻辑

范围：

- 在 `reply.rs` 中抽出 `prepare_reply_session` 或等价 helper。
- 统一处理：
  - 请求 session id 解析。
  - session 创建/读取。
  - workspace working_dir 决策。
  - user message 构造。
  - 上下文压缩前置处理。

验收：

- 现有 `reply_params_use_request_provider_defaults`、`persists_streamed_message_delta_for_history_reload` 等测试继续通过。
- 增加测试覆盖“无 session id 自动创建”和“workspace working_dir 优先级”。
- 审计补强：已覆盖“无 session id 自动创建”和“已有 session working_dir 优先于当前 workspace”。

#### S2：抽 SSE event pump

状态：已完成。

范围：

- 从 `reply_core` 中抽出 `pump_core_events`。
- 集中处理：
  - Core event stream 读取。
  - SSE encode。
  - `finish` / `error` terminal 判断。
  - `diff_ready` 触发。
  - 会话最终持久化输入数据收集。

边界：

- 不改变 SSE payload 格式。
- 不改变桌面端事件名。

验收：

- message delta 持久化测试继续通过。
- finish messages 替换 streamed partial message 的测试继续通过。

#### S3：抽 agent-core stdout 分类与路由

状态：已完成。

范围：

- `core_client.rs` 已抽出 stdout line/value 分类、agent event run_id/terminal 判断和事件 sender 路由 helper。
- 已补充边界测试，覆盖数字 JSON-RPC id、非法 id、空白 stdout 行、非终止 agent event 保留 sender、未知 sender/缺 run_id 事件忽略。

验收：

- `cargo test -p night24-server` 通过。
- 仅拆分和补测，不改变 agent-core stdout 协议行为。
- 新增测试覆盖 terminal event 后不继续写入 sender。

#### S4：统一权限模式解析

状态：已完成。

追加完成：`auth.rs` 的 `provided_api_key` 改为从 header 借用 `&str`，减少鉴权路径字符串分配，同时保留 Bearer 优先、X-API-Key fallback、trim 和非 UTF-8 fallback 语义。

范围：

- 将 server `main.rs`、server `reply.rs`、agent-core `tools.rs` 中的权限模式字符串解析收敛到一个共享枚举或协议类型。
- 第一阶段可以先在 `night24-protocol` 定义规范化函数，避免跨 crate 迁移过大。

验收：

- `strict`、`permissive`、`allow_all`、`allow-all`、`deny_all`、`deny-all` 行为一致。
- server 与 agent-core 相关测试通过。

### 推荐执行顺序

1. D1 `useTimeline`：已完成。
2. S1 session run 准备逻辑：已完成。
3. D2 `useSessions`：已完成。
4. S2 SSE event pump：已完成。
5. D3/S3：已完成。
6. D4/S4：已完成。

### 当前批次完成标准

- `App.jsx` 减少到约 550 行以内：已完成，当前约 419 行。
- `reply_core` 主函数只保留 orchestration：准备参数、调用 core、转发事件、返回 SSE。
- `core_client.rs` 中 stdout reader 的分支由可测试分类函数驱动。
- 通过：
  - `npm run build`
  - `cargo test -p night24-server`
  - `cargo test -p night24-agent-core`

### CSS 优化批次：workspace 面板样式抽取

状态：已完成。

范围：

- 从 `workspace.css` 抽出 `.left-panel` / `.center-panel` 面板容器规则到 `workspace-panels.css`。
- 在 `styles.css` 中将 `workspace-panels.css` 放在 `workspace.css` 之后、`workspace-header.css` 之前，保持原级联顺序。

验收：

- `npm run build` 通过。
- `git diff --check` 通过。

### CSS 优化批次：desktop 主题变量抽取

状态：已完成。

范围：

- 从 `desktop-shell.css` 抽出桌面端 `:root` 主题变量到 `desktop-theme.css`。
- 在 `styles.css` 中将 `desktop-theme.css` 放在 `desktop-shell.css` 之后、`desktop-chrome.css` 之前，保持原级联顺序。

验收：

- `npm run build` 通过。
- `git diff --check` 通过。

### CSS 优化批次：desktop 事件浮窗抽取

状态：已完成。

范围：

- 从 `desktop-overlays.css` 抽出 TimelinePanel 事件浮窗、事件列表和事件行规则到 `desktop-events.css`。
- 在 `styles.css` 中将 `desktop-events.css` 放在 `desktop-overlays.css` 之后，共享 `.float-head` 等覆盖层样式继续留在 `desktop-overlays.css`。

验收：

- `npm run build` 通过。
- `git diff --check` 通过。
