# GTS Script Engine Hook Integration Plan

## 背景

Night24 Agent Core 的 hook 系统用于 run、provider、tool 和 permission 生命周期事件。当前方向已经收敛为 GTS-only：hook 只通过内置 GoScript 运行时执行，不再支持本地 command hook，也不通过外部 `gs` CLI 过渡。

已覆盖事件：

- `run_started`
- `before_provider_request`
- `before_tool`
- `after_tool`
- `permission_required`
- `run_finished`
- `run_failed`

配置入口仍是 `NIGHT24_HOOKS_FILE`。该环境变量只用于告诉 Agent Core 从哪里加载 hook 配置文件；hook 上下文不会通过环境变量注入给脚本。

## 当前决策

- 只支持 `engine: "gts"` 的 GoScript hook。
- `script` 和 `inline_script` 是有效执行入口；`command` hook 不再是受支持能力。
- GoScript hook 由 Night24 内置运行时执行，不依赖外部 `gs` 可执行文件。
- Hook 逻辑写在 `execute(args)` 函数中。Agent Core 每次匹配事件时调用一次 `execute(args)`。
- `args` 是结构化上下文对象，包含事件、run、provider、tool、参数、结果预览和状态字段。
- 不注入 `NIGHT24_*` hook 环境变量；脚本不要依赖环境变量获取 hook 上下文。
- `execute(args)` 可以返回结构化 `outputs`，并由 Agent Core 转成既有 `run_output` 事件。
- 当前采用单 VM worker 串行执行 hook。这样可以适配 GoScript VM 的 `Rc<RefCell<...>>` 非 `Send` 线程模型，并保持 hook 顺序稳定。
- 如需调用本地 CLI，应在 GoScript 内部使用标准模块或内置能力，例如 `@std/exec`、`@std/process`，而不是配置 command hook。

## 配置形态

```json
{
  "hooks": [
    {
      "event": "before_provider_request",
      "name": "provider-policy",
      "engine": "gts",
      "script": "hooks/provider_policy.gs",
      "timeout_ms": 5000,
      "enabled": true
    },
    {
      "event": "after_tool",
      "name": "tool-audit",
      "engine": "gts",
      "inline_script": "function execute(args) { return { outputs: [{ stream: \"stdout\", text: args.event }] }; }"
    }
  ]
}
```

Relative `NIGHT24_HOOKS_FILE` paths are resolved from the session working directory. Relative hook script paths are also resolved from the session working directory.

## Script API

Hook scripts expose `execute(args)`:

```javascript
function execute(args) {
  return {
    outputs: [
      {
        stream: "stdout",
        text: "provider=" + args.provider + " model=" + args.model
      }
    ]
  };
}
```

`args` shape:

```json
{
  "event": "before_provider_request",
  "run_id": "run-123",
  "working_dir": "E:/project",
  "provider": "openai",
  "model": "gpt-4o-mini",
  "message_count": 4,
  "tool_count": 18,
  "tool_call_id": null,
  "tool_name": null,
  "summary": null,
  "arguments": null,
  "result_preview": null,
  "error": null,
  "duration_ms": null,
  "finish_status": null
}
```

`outputs` is an array of output records:

```json
{
  "outputs": [
    { "stream": "stdout", "text": "policy ok" },
    { "stream": "stderr", "text": "audit warning" }
  ]
}
```

Only `stdout` and `stderr` streams are recognized. Unknown or malformed output records should be ignored or reported as non-fatal hook stderr, depending on implementation detail. Hook return values do not modify provider requests, tool arguments, permission decisions, or run status in the current phase.

VM-captured `println(...)`, `print(...)`, `console.log(...)`, and stderr output are also surfaced as hook output. Structured `outputs` is preferred for deterministic tests and for tools that need to distinguish stdout from stderr.

## Execution Model

`gts_r` is a Rust implementation of GoScript. It provides a bytecode VM, module system, and standard modules such as filesystem, process, exec, HTTP, Socket, DB, JSON, TOML, YAML, template, and diff.

Important constraint: `Session`, `Object`, and VM internals use `Rc<RefCell<...>>` and are not `Send`. Night24 therefore keeps GoScript execution isolated on a single worker and runs hook invocations serially. The worker owns the VM state and receives hook jobs in order.

This model favors correctness and deterministic ordering over parallel hook throughput. If hook performance becomes a bottleneck later, a worker pool can be considered, but each worker must own its own VM and no GoScript object may cross threads.

## Local CLI Calls

There is no command hook fallback. To call a local executable, write the call in GoScript using the runtime's standard modules or built-in capability:

```javascript
function execute(args) {
  let exec = require("@std/exec");
  let result = exec.run("git", ["status", "--short"], { cwd: args.working_dir });
  return {
    outputs: [
      { stream: "stdout", text: result.stdout }
    ]
  };
}
```

Exact module APIs should follow the bundled GoScript runtime documentation. The important boundary is that command execution is explicit script logic, not Agent Core hook configuration.

## Security Boundary

GoScript standard modules are powerful. With modules such as `@std/fs`, `@std/exec`, and `@std/process`, hook scripts can access local resources in ways similar to a trusted local automation script.

Current phase treats hook configuration as trusted local configuration. Module allowlists, sandbox policy, and policy hooks that can change model/tool behavior are separate follow-up work.

## Phases

### Phase 1: GTS-only Hook MVP

- Load hook config from `NIGHT24_HOOKS_FILE`.
- Accept only GTS script executors.
- Resolve relative script paths from the session working directory.
- Call `execute(args)` with structured hook context.
- Capture VM stdout/stderr.
- Convert structured `outputs` into `run_output`.
- Run hooks serially on a single VM worker.

Acceptance:

- Command-only hooks are ignored or rejected as unsupported.
- `before_provider_request` can read `args.provider` and `args.model`.
- `before_tool` can read `args.tool_name` and `args.arguments`.
- `execute(args)` return value can emit stdout and stderr outputs.
- No hook context is injected through environment variables.

### Phase 2: Output and Error Polish

- Normalize malformed `outputs` handling.
- Keep runtime errors non-fatal and visible on hook stderr.
- Preserve source labels such as `hook:before_tool:audit`.
- Document any size limits or truncation behavior.

### Phase 3: Optional Policy Hooks

Only after logging hooks are stable, design explicit policy hooks that may deny or alter provider/tool behavior. Do not silently make ordinary logging hooks mutate Agent Core behavior.

## Risks

### Threading

The VM is single-thread-oriented. Sharing one VM across async tasks or moving VM objects across threads is unsafe. The current single worker serial model avoids that class of bug.

### Permissions

GoScript scripts can call local modules. This is intentional for trusted local hooks, but it bypasses normal tool permission prompts because execution happens inside hook logic. Future sandboxing must be designed as a separate policy layer.

### Compatibility

Older command hook configs need migration to GTS. For command-like behavior, wrap the command in GoScript using standard modules.
