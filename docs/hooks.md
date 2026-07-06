# Night24 Hooks

Night24 Agent Core supports native GoScript hooks for run and tool lifecycle events. Hooks are disabled by default and load from the workspace `.night24/hooks.json` file, or from `NIGHT24_HOOKS_FILE` when that override is set.

## Events

- `run_started`
- `before_provider_request`
- `before_tool`
- `after_tool`
- `permission_required`
- `run_finished`
- `run_failed`

## Config

```json
{
  "hooks": [
    {
      "event": "before_provider_request",
      "name": "provider-audit",
      "engine": "gts",
      "script": "hooks/provider_audit.gs",
      "timeout_ms": 5000,
      "enabled": true
    },
    {
      "event": "after_tool",
      "name": "gts-audit",
      "engine": "gts",
      "script": "hooks/after_tool.gs",
      "timeout_ms": 5000,
      "enabled": true
    }
  ]
}
```

Relative `NIGHT24_HOOKS_FILE` paths are resolved from the session working directory. Relative hook script paths are also resolved from the session working directory.

`gts` hooks run with the built-in GoScript engine copied into the Night24 workspace. They do not require an external `gs` executable. Relative script paths are resolved from the session working directory.

Night24 only supports GoScript hooks. Local command hooks are not supported. If a hook needs to call a local CLI, do it inside GoScript using the standard modules or built-in capabilities provided by the GoScript runtime.

```javascript
let exec = require("@std/exec");

function execute(args) {
  let output = exec.output("cmd", ["/C", "echo hello"]);
  return {
    outputs: [{ stream: "stdout", text: output }]
  };
}
```

Hook scripts expose an `execute(args)` function. Night24 calls that function once per matching event and passes the hook context as the `args` object. Hook context is not injected through environment variables, and scripts should not depend on `NIGHT24_*` hook environment variables.

GoScript hooks currently run with the engine's standard module access enabled. Security boundaries and module allowlists are intentionally deferred until after the native script engine integration is settled.

Hook output is emitted as existing `run_output` events with a source like `hook:before_tool:audit`. Hook failures are non-fatal and are reported on stderr output.

## Script API

```javascript
function execute(args) {
  return {
    outputs: [
      {
        stream: "stdout",
        text: "hook " + args.event + " for " + args.run_id
      }
    ]
  };
}
```

`args` contains:

- `event`
- `run_id`
- `working_dir`
- `provider`
- `model`
- `message_count`
- `tool_count`
- `tool_call_id`
- `tool_name`
- `summary`
- `arguments`
- `result_preview`
- `error`
- `duration_ms`
- `finish_status`

Run lifecycle hooks include the selected provider, model, request message count, and available tool count. Tool and permission hooks also include tool-specific fields such as `tool_call_id`, `tool_name`, `summary`, and `arguments`.

The `execute(args)` return value may include `outputs`, an array of `{ "stream": "stdout" | "stderr", "text": "..." }` objects. `println(...)`, `console.log(...)`, and stderr output from the VM are also collected as hook output.

## Execution Model

Hook execution currently uses a single GoScript VM worker and runs hooks serially. This keeps the non-`Send` GoScript VM isolated from the async Agent Core while preserving deterministic hook ordering.
