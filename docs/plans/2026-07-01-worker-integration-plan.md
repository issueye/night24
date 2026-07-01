# Worker Integration Plan

> Date: 2026-07-01  
> Scope: integration coordination only. This file is the handoff checklist for parallel workers moving Night24 toward "basically usable".  
> Sources: `docs/desktop-current-scope.md`, `docs/server-definition.md`, `docs/protocol-server-agent-core-json-rpc.md`, `docs/plan-visual-vibe-coding.md`, and `docs/plans/2026-07-01-parallel-dispatch.md`.

## 1. Integration Goal

Reach a minimal desktop loop:

1. Start `night24-server`.
2. Open the Tauri desktop page.
3. Open a local project/workspace.
4. Create or select a session bound to that workspace.
5. Send one task/message.
6. See assistant messages and Agent timeline events.
7. Browse the workspace file tree and open a text file.
8. Cancel and permission placeholder flows do not crash the UI or server, even if full Core-side permission pausing is not complete.

This is not the full visual vibe coding MVP. Diff, preview, process logs, Git UI, Monaco editing, patch review, and real permission-gated tool execution can stay as placeholders if their API/UI surfaces are stable.

## Current Repository Status

- The desktop UI has switched to Vite + React under `tauri-app/index.html`, `tauri-app/src/**`, `tauri-app/package.json`, and `tauri-app/vite.config.js`.
- Tauri is expected to load the React dev server during development and the Vite build output for packaged runs.
- The server has the basic desktop route surface, including health/readiness, workspace, tools, cancel, and permission routes.
- The server now has a minimal Core bridge: it spawns/locates `night24-agent-core`, calls `core.initialize`, forwards `/tools` to `agent.tools`, converts `/reply` to `agent.reply`, and relays `agent.event` notifications as SSE.
- Current smoke evidence shows `/readyz.ready=true`, `/tools.source=night24-agent-core`, `/reply` emits `message` and `finish`, and session history persists the user/assistant messages.
- Remaining Core bridge work is hardening rather than first usability: restart/backoff, concurrent run routing under load, real permission pause/resume, and provider support beyond the current echo skeleton.

## 2. Parallel Lines

### Line A: protocol / agent-core

Purpose: create the process boundary and typed event contract that server and UI can converge on.

Owner: protocol / agent-core worker.

File scope:

- `crates/night24-protocol/**`
- `crates/night24-agent-core/**`
- root `Cargo.toml` only to add new workspace members or shared dependencies
- tests under the two new crates

Do not modify:

- `crates/night24-server/**`
- `tauri-app/**`

Required deliverables:

- `night24-protocol` crate compiles as part of the workspace.
- `night24-agent-core` binary compiles as part of the workspace.
- Newline-delimited JSON-RPC over stdio.
- `core.initialize`, `core.ping`, `core.shutdown`.
- `agent.tools`.
- `agent.reply` minimum behavior:
  - validates initialized state,
  - returns accepted with `run_id`,
  - emits at least one `agent.event` `message`,
  - emits terminal `finish` or `error`.
- `agent.cancel` request exists and returns stable accepted/not-found/cancelled semantics.
- stdout is protocol-only JSON lines; logs go to stderr.

Minimum tests:

- JSON-RPC request/response serialization.
- Unknown method returns JSON-RPC method-not-found error.
- Business method before `core.initialize` returns `CoreNotInitialized`.
- `agent.reply` emits ordered events ending in `finish` or `error`.

Integration contract:

- Event payloads should preserve the documented envelope:

```json
{
  "run_id": "run-1",
  "seq": 1,
  "type": "message",
  "created_at": "2026-07-01T10:00:00Z",
  "payload": {}
}
```

- First pass may keep `payload` as `serde_json::Value`; do not block integration on a perfect Rust enum.
- Permission events can be emitted as placeholders, but `permission.resolve` must not be required for the basic echo/message flow.

### Line B: server API / bridge

Purpose: keep the desktop API stable while introducing workspace APIs and a bridge to Core.

Owner: server API / bridge worker.

File scope:

- `crates/night24-server/**`
- root `Cargo.toml` only if needed to depend on Line A crates after they land

Do not modify:

- `crates/night24-protocol/**` except through agreed protocol changes
- `crates/night24-agent-core/**`
- `tauri-app/**`

Required deliverables:

- Health/readiness:
  - `GET /healthz`
  - `GET /readyz`
- Session basics:
  - `GET /sessions`
  - `POST /sessions`
  - `GET /sessions/{id}/history`
  - `DELETE /sessions/{id}`
  - `PUT /sessions/{id}/name` if already supported or low-risk
- Workspace basics:
  - `POST /workspaces/open`
  - `GET /workspaces/current`
  - `GET /workspaces/recent`
  - `GET /workspace/tree?path=`
  - `GET /workspace/file?path=`
- Agent/task APIs:
  - `POST /reply` returning SSE
  - `POST /agent/cancel`
  - `GET /tools`
- Permission APIs:
  - `POST /permissions/{id}/approve`
  - `POST /permissions/{id}/deny`

Bridge deliverables:

- Server can start or locate `night24-agent-core` when available.
- Server calls `core.initialize`.
- `/readyz` distinguishes server alive from Core ready.
- `/tools` calls Core when Line A is available, or returns a stable placeholder/error response when Core is unavailable.
- `/reply` converts desktop request to `agent.reply` and forwards `agent.event` notifications as SSE.
- If Core crashes or is unavailable, `/reply` sends a structured `error` event instead of hanging.

Workspace safety requirements:

- Resolve paths under the current workspace root.
- Reject `..` escape and symlink/canonical escape.
- Skip at least `.git`, `target`, `node_modules`, `.venv`, `venv`.
- Return a clear binary/too-large response for files that should not be previewed.

Compatibility requirement:

- SSE should support the new `AgentEvent` envelope. If old UI code still expects message-only chunks during integration, server may temporarily emit compatible data, but every run must end with `finish` or `error`.

### Line C: Tauri UI

Purpose: make the desktop shell usable against the server API and robust against incomplete backend pieces.

Owner: Tauri UI worker.

File scope:

- `tauri-app/index.html`
- `tauri-app/src/**`
- `tauri-app/package.json`
- `tauri-app/package-lock.json`
- `tauri-app/vite.config.js`
- `tauri-app/src-tauri/src/main.rs` only for desktop bridge commands such as directory picking or HTTP proxy helpers
- `tauri-app/src-tauri/Cargo.toml` only if needed by Tauri bridge changes

Do not modify:

- `crates/**`
- protocol or server docs unless agreed

Required deliverables:

- Server connection state:
  - connecting,
  - connected,
  - failed with retry,
  - visible server URL, default `http://localhost:17787`.
- Open project:
  - directory picker via Tauri if available,
  - calls `POST /workspaces/open`,
  - shows current project name/path,
  - handles no workspace state.
- File browsing:
  - loads `GET /workspace/tree`,
  - opens text files via `GET /workspace/file`,
  - shows empty, binary, too-large, and error states.
- Sessions:
  - list, create, select, delete with confirmation,
  - load history,
  - session title uses `name`.
- Task input:
  - provider/model controls or settings placeholders,
  - Enter sends and Shift+Enter inserts newline,
  - sending disabled during active run,
  - cancel button calls `POST /agent/cancel`.
- Event rendering:
  - `message` goes to chat/messages,
  - `tool_started`, `tool_finished`, `tool_failed`, `permission_required`, `finish`, `error` go to timeline/status,
  - unknown event types do not crash the page.
- Permission placeholder:
  - shows approve/deny UI for `permission_required`,
  - calls approve/deny endpoints,
  - handles 404/unimplemented/expired responses as visible nonfatal timeline entries.

UI acceptance:

- Missing server shows a clear failed state rather than a blank page.
- Missing API returns a visible empty/error state, not a JavaScript crash.
- React/Vite entry loads through Tauri in dev and packaged modes.
- No dependency on removed standalone `chat-ui`.

### Line D: verification

Purpose: validate integration readiness, identify gaps, and stop regressions before declaring "basically usable".

Owner: integration / verification worker.

File scope:

- `docs/plans/2026-07-01-worker-integration-plan.md`
- small related docs fixes only when there is an obvious contradiction

Do not modify:

- `crates/**`
- `tauri-app/**`

Required deliverables:

- This integration plan.
- Merge order and dependency gates.
- Risk register.
- Final command checklist.
- Manual desktop verification script.
- Short gap report after worker branches land, if a follow-up pass is requested.

## 3. Dependencies and Merge Order

### Preferred merge order

1. Merge Line A protocol / agent-core.
2. Merge Line B server API / bridge.
3. Merge Line C Tauri UI.
4. Run Line D verification checklist and patch only the owning line that failed.

Reasoning:

- Line B can bridge cleanly only after Line A defines crate names, binary name, and protocol types.
- Line C should target the final server route names and event envelope rather than guessing.
- Verification should run after all three lines land, but this plan should be present before they merge.

### Allowed fallback order

If Line A is delayed:

1. Merge Line B with a feature-compatible placeholder bridge:
   - `/tools` returns stable placeholder data or a recoverable error.
   - `/reply` can use the existing server Agent path temporarily.
   - SSE still emits `message` and terminal `finish`/`error` envelopes.
2. Merge Line C against Line B placeholders.
3. Merge Line A.
4. Replace the placeholder bridge with Core-backed calls.

If Line B is delayed:

1. Merge Line A.
2. Merge Line C only if it handles 404/offline states and does not assume workspace APIs exist.
3. Merge Line B and rerun full manual flow.

### Merge gates

Line A gate:

- `cargo test --workspace` reaches Line A crates.
- Manual Core stdio initialize works.
- No non-JSON stdout from Core.

Line B gate:

- `cargo test --workspace` passes or failures are documented as unrelated.
- `GET /healthz` returns success.
- `GET /readyz` returns structured ready/unready status.
- Workspace file APIs cannot read outside the opened root.
- `/reply` never hangs indefinitely when Core is absent.

Line C gate:

- Tauri page opens.
- Server offline state is visible.
- No uncaught UI crash during open project, session load, send, cancel, permission placeholder, file open.

Final gate:

- Minimal acceptance in section 4 passes end to end.

## 4. "Basically Usable" Minimum Acceptance

All items below are P0 for the integration milestone.

Server:

- Server process starts on the expected bind address.
- `/healthz` confirms the server process is alive.
- `/readyz` reports Core ready when available and a clear unready state when not.
- API key mode, if enabled, rejects protected routes without a key and allows them with the configured key.

Tauri:

- Desktop page opens without depending on standalone `chat-ui`.
- Connection status recovers from server-offline to connected after retry.
- Settings or visible controls show current server URL, provider, and model fields/placeholders.

Workspace:

- User can choose/open a local project.
- Current workspace name/path is visible.
- Recent workspace endpoint either returns data or a stable empty list.
- File tree loads.
- A text file opens in the viewer.
- Binary/too-large file state does not crash.

Sessions:

- User can create a session.
- New session uses the current workspace path by default.
- User can select an existing session and load history.
- User can delete a session after confirmation.

Task/message:

- User can send one message/task.
- User message appears immediately.
- Assistant message or structured error appears from SSE.
- `finish` or `error` ends the active run.
- Input is disabled while run is active and re-enabled afterward.

Timeline/events:

- Timeline renders at least `message`, `finish`, and `error`.
- Timeline tolerates `tool_started`, `tool_finished`, `tool_failed`, and unknown event types.
- Event ordering uses `seq` when provided.

Cancel and permission:

- Cancel button calls `POST /agent/cancel` and produces a visible nonfatal status.
- Receiving `permission_required` shows a permission card.
- Approve/deny calls do not crash when the server returns success, expired, not found, or not implemented.
- Permission timeout/expired state is visible if surfaced by server.

## 5. Conflict Risks and Resolution Strategy

| Risk | Likely conflict | Resolution |
|---|---|---|
| Root `Cargo.toml` edited by multiple workers | Line A adds crates while Line B adds dependencies | Merge Line A workspace member changes first. Line B should depend on crate names after they exist. Keep root edits minimal and sorted if possible. |
| Event shape drift | Protocol defines `AgentEvent`, server emits legacy `Message`, UI expects another shape | Treat `run_id/seq/type/created_at/payload` as the canonical envelope. UI may support legacy chunks temporarily, but server must emit terminal `finish/error`. |
| Permission scope disagreement | Protocol says permission can be later, desktop scope says permission confirmation is required | For "basically usable", implement API/UI placeholders and non-crashing approve/deny. Full Core pause/resume is not required unless Line A/B can finish it cleanly. |
| Cancel semantics disagreement | Core may return accepted while run still emits final events | UI should mark cancelling and wait for terminal `finish/error` when possible. Server should make duplicate cancel idempotent or return a clear not-found/already-finished response. |
| Workspace path security | UI sends absolute paths, server expects relative paths, Windows path separators differ | Server is the authority. UI can display absolute root but tree/file requests should use relative paths where possible. Server must canonicalize and reject escape. |
| Server bridge blocks UI | `/reply` waits for Core final answer before opening SSE | `/reply` must create SSE stream early, call Core, then forward events. If Core is unavailable, emit structured `error` and close. |
| Core stdout pollution | tracing/debug logs break JSON-RPC parsing | Line A must route logs to stderr. Line B should treat non-JSON stdout as protocol violation and mark Core unhealthy. |
| API key handling | Tauri requests fail when `NIGHT24_API_KEY` is set | UI settings must support a key header. Server should leave `/healthz` and `/readyz` unauthenticated per docs. |
| Session field mismatch | Old UI uses `title`, server uses `name` | Use `name` as canonical. UI can display fallback values but sends `name` on create/rename. |
| React/Tauri dev setup | Validators run only Cargo commands and miss the Vite React surface | Verification should use the `tauri-app` npm scripts plus Tauri smoke checks so both the React app and desktop shell are covered. |
| Parallel edits in `tauri-app/src-tauri/Cargo.toml` | UI worker and existing changes touch desktop dependencies | Do not rewrite the file wholesale. Merge only the exact dependency or permission entries needed for directory picking/proxy helpers. |
| `chat-ui` removal | Existing `chat-ui/index.html` deletion conflicts with workers referencing it | Treat `tauri-app` as the only desktop entry. Any worker adding new code under `chat-ui` should be rejected for this milestone. |

## 6. Final Verification Commands

Run from repo root unless noted.

Baseline inspection:

```powershell
git status --short
rg -n "chat-ui|Chat UI" .
rg -n "old static HTML|retired static HTML|legacy web entry" docs tauri-app
rg -n "TODO|panic!|unwrap\\(|expect\\(" crates/night24-server
```

After Line A lands, also inspect the new Core/protocol crates:

```powershell
rg -n "TODO|panic!|unwrap\\(|expect\\(" crates/night24-agent-core crates/night24-protocol
```

Rust build and tests:

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo build --workspace
```

Core stdio smoke test, after Line A lands:

```powershell
'{"jsonrpc":"2.0","id":"rpc-1","method":"core.initialize","params":{"protocol_version":"2026-07-01","client":{"name":"manual","version":"0"},"capabilities":[]}}' | cargo run --bin night24-agent-core -- --stdio
```

Expected:

- stdout contains one valid JSON-RPC response.
- stderr may contain logs.
- stdout contains no plain text logs.

Server smoke test:

```powershell
cargo run --bin night24-server
```

In another terminal:

```powershell
Invoke-RestMethod http://localhost:17787/healthz
Invoke-RestMethod http://localhost:17787/readyz
Invoke-RestMethod http://localhost:17787/sessions
Invoke-RestMethod http://localhost:17787/workspaces/current
Invoke-RestMethod http://localhost:17787/tools
```

Current-state expectation before the Core bridge is complete:

- `/healthz` should succeed.
- `/readyz` may return `ready: false` or a Core-unavailable reason.
- `/tools` may return a stable fallback list or structured unavailable response.
- `/reply` must emit either renderable events or a structured error and must not hang.

Final integration expectation after the Core bridge lands:

- `/readyz` reports Core ready.
- `/tools` is backed by `agent.tools`.
- `/reply` calls `agent.reply` and forwards `agent.event` SSE envelopes through to the desktop UI.

Workspace open smoke test:

```powershell
$body = @{ path = "E:\code\issueye\ai_agent\night24" } | ConvertTo-Json
Invoke-RestMethod -Method Post -Uri http://localhost:17787/workspaces/open -ContentType "application/json" -Body $body
Invoke-RestMethod "http://localhost:17787/workspace/tree?path="
Invoke-RestMethod "http://localhost:17787/workspace/file?path=Cargo.toml"
```

Path escape negative test:

```powershell
Invoke-WebRequest "http://localhost:17787/workspace/file?path=..\..\Windows\win.ini"
```

Expected: non-2xx rejection or structured error, never file contents.

Tauri build/smoke:

```powershell
Set-Location tauri-app
npm install
npm run build
npm run dev
```

In another terminal, while `npm run dev` is serving React:

```powershell
Set-Location tauri-app
cargo build --manifest-path src-tauri/Cargo.toml
cargo run --manifest-path src-tauri/Cargo.toml
```

If the Tauri CLI is available, also run the project-specific Tauri dev command agreed by the UI worker. Do not validate against any retired static HTML entry.

## 7. Manual Verification Steps

1. Start `night24-server`.
2. Open the Tauri app.
3. Confirm the top/status area shows connected and the server URL.
4. Stop the server, click retry, confirm failed state is visible; start server again and retry to recover.
5. Open project `E:\code\issueye\ai_agent\night24`.
6. Confirm project name/path are visible.
7. Expand the file tree.
8. Open `Cargo.toml`; confirm text is readable.
9. Try opening a path or file that should be rejected or unavailable; confirm the UI shows an error state without crashing.
10. Create a new session.
11. Confirm the session is selected and associated with the current project.
12. Send a small task such as `hello`.
13. Confirm the user message appears immediately.
14. Confirm SSE returns either an assistant `message` and `finish`, or a structured recoverable `error`.
15. Confirm the timeline shows the run events separately from the chat message area.
16. While a run is active, click cancel; confirm input eventually recovers.
17. Trigger or simulate `permission_required` if available; approve and deny it once each.
18. Confirm approve/deny success, not found, expired, or unimplemented responses are visible and nonfatal.
19. Delete the test session after confirmation.
20. Refresh/reopen the app and confirm it can reload current/recent workspace and sessions without a blank screen.

## 8. Known Gaps Allowed at This Milestone

Allowed, if clearly surfaced as placeholders:

- Real diff API and diff viewer.
- Preview/dev-server process API.
- Full Core permission pause/resume.
- Monaco editor or file editing.
- Git status/commit UI.
- Multiple concurrent Core processes.
- Full provider key persistence.

Not allowed:

- Blank desktop page when server is offline.
- `/reply` stream that never terminates on success/error.
- Workspace file API reading outside the opened root.
- High-risk permission event being silently ignored by the UI.
- UI JavaScript crash on unknown event type.
- Core writing logs to stdout.
- Reintroducing standalone `chat-ui` as the main entry.

## 9. Final Report Template

Use this after the three implementation lines merge:

```markdown
## Integration Result

- Commit/range verified:
- Server started:
- Tauri opened:
- Project opened:
- Session flow:
- Message/SSE flow:
- Timeline events:
- File browsing:
- Cancel:
- Permission placeholder:

## Blockers

- P0:
- P1:

## Follow-up Owners

- protocol / agent-core:
- server API / bridge:
- Tauri UI:
- verification:
```
