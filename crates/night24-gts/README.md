# GoScript (Rust) — `gts_r`

> GoScript 的 Rust 实现。GoScript 是一门语法风格参考 JavaScript 的**独立**动态脚本语言，文件后缀 `.gs`。本仓库是同目录下 Go 版本 `gts/` 的功能等价重写，目标是行为兼容、性能更优。

GoScript 解释器不是嵌入式脚本库，而是独立 CLI / REPL 产品。它通过 CLI 运行脚本，VM 内部用 `Promise` / `async/await` / `setTimeout` 做异步并发；跨实例共享状态走外部存储或 GTP 进程间协议。

- **执行后端**:字节码栈式 VM(默认,`Session::new()` 即 Bytecode),AST 树遍历作为 `--exec-mode=tree` legacy fallback
- **异步 I/O**:默认启用 `tokio` feature,HTTP/TCP/timer 走 Tokio,VM 线程 drain completion queue 回填 Promise
- **标准库**:68 个 `@std/*` 原生模块
- **解释器版本**:`VERSION = "0.1.0-dev"`

---

## 快速开始

```bash
# 构建 CLI(默认启用 tokio)
cargo build --release

# 运行脚本(默认字节码后端)
./target/release/gs examples/smoke.gs
# 或显式指定执行后端
./target/release/gs --exec-mode=tree main.gs

# 初始化一个新项目脚手架
./target/release/gs init hello-app

# 交互式 REPL
./target/release/gs

# 运行版本 / 运行模式
./target/release/gs -v     # bytecode + tokio-io
```

### 执行模式

| 模式 | 启用 | 适用 |
|------|------|------|
| `bytecode`(默认) | `Session::new()` | 生产路径,栈式 VM,全量 AST 覆盖 |
| `tree` | `--exec-mode=tree` | legacy fallback,仅用于极少数未下沉语法(如临时的 VM 覆盖缺口) |

VM 是否启用字节码由 `VirtualMachine::exec_mode: AtomicU8` 控制(`src/object/vm.rs`)。

---

## 项目结构

```
src/
├── lexer/         词法分析
├── parser/        语法分析 (AST)
├── ast/           AST 节点定义
├── evaluator/     树遍历求值器(legacy fallback + VM 桥接)
├── bytecode/      字节码 VM:opcode / chunk / compiler / interp / call / frame / closure / upvalue / class / resolve
├── object/        运行时对象系统、Environment、VM、Promise、awaitable、event_loop、timer_wheel、io_selector(多路复用)
├── async_runtime/ Native runtime / Tokio runtime / completion queue / awaitable bridge
├── runtime/       Session:VM + 模块缓存 + resolver 入口
├── module/        模块解析(require / import-export / 缓存 / 循环依赖)
├── stdlib/        68 个 @std/* 模块(modules/),helpers/,gtp/ 子树
├── gtp/           GTP 进程间协议(frame / codec / transport / plugin)
├── apidoc.rs      API 文档生成
├── bundler.rs     脚本打包
├── packagefile.rs .gspkg 打包格式
└── bin/gs.rs      CLI 入口

tests/             31 个集成测试套件 + tests/fixtures/parity/ 双跑 fixture
bench/             bench_client + bench_server 性能基准
```

完整设计、决策与模块对照见 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)。

---

## 特性状态

| 维度 | 状态 |
|------|------|
| 执行模型 | 字节码栈式 VM(默认)+ 树遍历 legacy fallback |
| 语言核心 | 词法/语法/AST、类与继承、闭包与 upvalue、模式匹配、try/catch、模板字符串、async/await 全覆盖 |
| 异步 I/O | Promise / async/await / timers;Tokio 默认驱动 HTTP/TCP/stream;单 worker 不阻塞 |
| 标准库 | 68 个 `@std/*` 模块(fs/path/os/process/exec/crypto/db/http/socket/ws/web/sse/mail/jwt/tui/...),详见 [parity-matrix.md](docs/parity-matrix.md) |
| 模块系统 | `require(path)`、ES `import/export`、模块缓存、循环依赖检测 |
| 打包 | `gs init` / `gs pack` / `gs dist` / `gs bundle`(部分) |
| GTP | frame / JSON Lines codec / stdio+TCP transport / `@std/gtp/client` |
| 资源保护 | `--timeout`(默认 10s,`0` 关闭) |
| 资源隔离 | `@std/web` 多 worker(prefork 共享 socket),每 worker 独立 VM |

---

## 构建

```bash
cargo build --release                 # 默认 tokio feature
cargo build                           # debug 构建
cargo build --no-default-features     # 关闭 tokio(仅 native I/O,单线程)

cargo test                            # 全量测试
cargo test --test bytecode_parity     # VM 双跑 parity
cargo test --release --test bytecode_perf -- --ignored   # 性能门(release)
```

默认 feature 配置(`Cargo.toml`):

```toml
[features]
default = ["tokio"]
tokio = ["dep:tokio", "dep:reqwest"]
```

---

## 文档

- [架构总览](docs/ARCHITECTURE.md) — 执行管线、对象系统、异步模型、并发模型、GTP
- [开发路线图](docs/development-roadmap.md) — 当前进度与剩余工作
- [Parity Matrix](docs/parity-matrix.md) — 与 Go 版逐项功能对齐状态
- [字节码 VM 开发计划](docs/bytecode-vm-development-plan.md) — 契约驱动的 VM 全量交付计划
- [@std/web 并发设计](docs/web-concurrency-design.md) — prefork 共享 socket 模型
- [@std/web 并发基准](docs/web-concurrency-benchmark.md) — 吞吐/延迟性能数据
- [Tokio 单 worker 并发方案](docs/tokio-single-worker-concurrency-plan.md) — 异步 I/O 演进方向

---

## 与 Go 版的关系

本仓库以 `../gts`(Go 实现)为**行为金标准**:用户可见行为、CLI、语言语义、模块系统、标准库、打包、GTP 协议保持一致。内部实现 Rust 化,采用字节码 VM + Tokio I/O 而非 Go 的 goroutine/每请求独立 VM 模型。两版的逐项对齐情况记录在 [parity-matrix.md](docs/parity-matrix.md),Go 版总览见 [`../gts/README.md`](../gts/README.md)。

---

## 许可证

MIT
