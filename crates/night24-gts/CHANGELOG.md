# Changelog

本文件记录 GoScript (Rust) 各版本的变更。格式参考 [Keep a Changelog](https://keepachangelog.com/)。

## [0.2.0] — 2026-06-28

工程化与体验基线:CI 回归门、LSP 编辑器集成、文档、标准库补完与多处 bug 修复。

### Added

- **CI 流水线** (`.github/workflows/ci.yml`):Linux + Windows 矩阵,lint(fmt + clippy 0-warning 门)/build/test/parity/perf 分组;主 test 门串行执行消除网络套件端口竞争。
- **LSP 语言服务器** (`gs lsp`,`src/lsp/`):手写 JSON-RPC over stdio(零依赖),支持 `initialize`/`diagnostic`/`completion`/`hover`/`definition`;VS Code 扩展骨架(`editors/vscode/`,含语法高亮 grammar)。
- **`-e` / `--eval` CLI flag**:内联求值(对标 `node -e`),如 `gs -e "println(1+2)"`。
- **`@std/markdown.createStream`**:流式 markdown token 解析(`.next()`/`.tokens()`/`.headings()`)。
- **`@std/test` 增强**:链式断言(`toHaveLength`/`toContain`/`toBeNull`/`toBeUndefined`/`toBeDefined`/`toBeGreaterThan`/`toBeLessThan`/`toThrow`)+ `not` 否定链;生命周期钩子(`beforeEach`/`afterEach`/`beforeAll`/`afterAll`)。
- **标准库参考自动生成**:`gs --api_doc all` 输出聚合 Markdown(`src/apidoc.rs::format_all_modules_markdown`),产物 `docs/book/stdlib-reference.md`。
- **bench-compare 性能回归门**:`bench/bench_compare.sh`(record/compare + JSON baseline + 劣化百分比);CI `perf` job 与 `bench/baselines/baseline.json` 对比,劣化 >25% 失败。
- **GitHub Pages 文档部署**:CI `docs`/`deploy-pages` job 用 mdBook 构建 `docs/book/` 并部署(push 到 main 触发)。
- **Parity fixture 扩容**:75 → 99(错误路径 9 + 语法边界 10 + 跨模块 5),`bytecode_parity` 全覆盖。
- **错误消息快照测试**(`tests/error_snapshots.rs`):4 个 uncaught-error fixture,跨机器路径规范化。
- **基准扩展**:`fib_rec` / `large_hash` / `class_instantiate` 三类微基准 + `bench/baselines/` 快照。
- **用户文档**:`docs/book/`(快速开始、数据类型、JS 迁移指南)+ `examples/scripts/`(4 个可运行示例)。

### Fixed

- **stdlib 函数值检测漏 `Object::Closure`**:bytecode VM 成为默认后,从脚本传入的函数是 `Closure`,但 events/retry/async/timers/test/sse/socket/ws/watch 等 10 处只检测 `Function|Builtin`,默认路径拒绝合法回调。全部补上 `Closure` 分支。
- **`@std/terminal` raw-mode 非 TTY 误报**:`capabilities()` 硬编码 `rawMode:true`、`terminal.start()` 在非 TTY 仍尝试 `enable_raw_mode`。改为 gate 在 `stdout.is_terminal()`(pipe/CI 报 false)。
- **`socket.acceptOne` handler 契约**:`listen(port, handler)` 传入的 handler 是 `Closure` 未被捕获,导致 `acceptOne()` 无 fallback handler(随上面 Closure 修复一并解决)。

### Changed

- `cargo fmt --all` 全量规范化(历史未格式化代码 + 本次改动)。
- `stdlib_tui::terminal_capabilities_report_raw_mode` 测试期望修正:`rawMode` 在非 TTY 下由 `true` → `false`(对齐修复后的真实能力)。

### Known gaps (登记到后续阶段)

- VM 尚不支持 `++`/`--`、解构赋值、`void`、`delete`、标签模板(见 `docs/test-failure-triage.md` §P0.3)→ Phase 2 B3.1。
- `--check-types` 类型检查器占位 → Phase 2。
- ES `import/export` re-export/`export *` 不完整 → Phase 2。
- Error 子类构造器 `super(msg)` 不绑 message → Phase 2。

## [0.1.0-dev] — 2026-06-23 之前

初始开发版本:字节码 VM 全量交付(阶段 0–11)、Tokio 异步 I/O、68 个 `@std/*` 模块、GTP 协议核心。详见 `docs/ARCHITECTURE.md`。
