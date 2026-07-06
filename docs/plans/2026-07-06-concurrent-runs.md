# Concurrent Runs Implementation Plan

**Goal:** Allow multiple tasks to run at the same time without frontend state pollution, while preparing the server to isolate runs through pluggable agent runner backends.

**Architecture:** The first delivery supports concurrent runs across different sessions and keeps one active run per session to avoid session-history overwrite races. The frontend owns one context object per session. The server introduces a runner abstraction and run manager so the current single agent-core transport can be kept as a compatibility runner while per-run or pooled agent-core processes are added behind the same API.

**Tech Stack:** React 18, Vite, Tauri 1, Rust, Axum, Tokio, JSON-RPC over stdio, SSE.

---

## Requirements

- Multiple sessions can run tasks concurrently.
- One session has one active foreground run in phase 1.
- Switching sessions must not abort unrelated running tasks.
- Each session owns its own messages, draft text, timeline, pending permissions, run checkpoint, and visible active run.
- The sidebar continues to show running state per session.
- Server code has a clear runner boundary for `single_core`, `per_run_process`, and later `process_pool`.
- Existing `/reply`, `/runs/{run_id}/events`, `/agent/cancel`, and permission endpoints remain compatible.
- Builds must pass: `npm run build`, `cargo build --release -p night24-server -p night24-agent-core`, and `cargo build --release` under `tauri-app`.

## Non-Goals For Phase 1

- Same-session multi-run concurrency.
- Full process-pool scheduler implementation with warm workers.
- Session-store append-only migration.
- UI for comparing several active runs inside the same conversation.

## Current Constraints

- `tauri-app/src/App.jsx` uses global `isRunning`, `activeRun`, `abortRef`, `streamCheckpointRef`, `pendingPermissions`, and `messages`.
- `crates/night24-server/src/reply.rs` loads a session snapshot and saves the whole session after a run finishes.
- `crates/night24-server/src/core_client.rs` already routes agent events by `run_id`, which makes it a good candidate for `single_core` runner compatibility.
- `RunEventStore` persists and replays events per `run_id`; keep this as the authoritative event replay mechanism.

---

### Task 1: Frontend Session Context Registry

**Files:**
- Create: `tauri-app/src/hooks/useSessionContexts.js`
- Modify: `tauri-app/src/App.jsx`
- Modify: `tauri-app/src/hooks/useSessions.js`

**Step 1: Create the context shape**

Create a hook that stores:

```js
{
  [sessionId]: {
    messages: [],
    draftText: '',
    timeline: [],
    pendingPermissions: [],
    activeRunId: '',
    runCheckpoints: {
      [runId]: { runId, lastSeq: 0, status: 'running' }
    }
  }
}
```

Expose:

- `getSessionContext(sessionId)`
- `patchSessionContext(sessionId, patchOrUpdater)`
- `setSessionMessages(sessionId, messagesOrUpdater)`
- `setSessionDraft(sessionId, text)`
- `addSessionTimeline(sessionId, item)`
- `setSessionPermissions(sessionId, updater)`
- `setSessionRunCheckpoint(sessionId, runId, checkpoint)`
- `clearSessionRun(sessionId, runId)`

**Step 2: Move current session view state**

In `App.jsx`, compute `currentContext` from `currentSessionId`. Replace direct global `messages`, `taskText`, `pendingPermissions`, and timeline reads in `ChatPanel` with `currentContext` values.

**Step 3: Keep session history loading scoped**

Update `selectSession` so it returns history and `App.jsx` writes that history into the selected session context, instead of overwriting one global `messages` array.

**Step 4: Verify**

Run:

```powershell
npm run build
```

Expected: Vite build succeeds.

---

### Task 2: Frontend Run Registry And Concurrent Streams

**Files:**
- Create: `tauri-app/src/hooks/useRunRegistry.js`
- Modify: `tauri-app/src/App.jsx`
- Modify: `tauri-app/src/hooks/useAgentEvents.js`
- Modify: `tauri-app/src/hooks/useRunControls.js`
- Modify: `tauri-app/src/components/Sidebar.jsx`
- Modify: `tauri-app/src/components/chat/ChatComposer.jsx`

**Step 1: Create run registry**

Store:

```js
{
  runsById: {
    [runId]: {
      runId,
      sessionId,
      workspacePath,
      status,
      startedAt,
      finishedAt,
      lastSeq,
      controller
    }
  },
  activeRunBySession: {
    [sessionId]: runId
  }
}
```

Expose:

- `startPendingSessionRun(sessionId, metadata)`
- `attachRunId(sessionId, temporaryId, runId)`
- `markRunEvent(runId, updates)`
- `finishRun(runId, status)`
- `cancelRunState(runId)`
- `getSessionRun(sessionId)`
- `getRunningSessions()`

**Step 2: Allow concurrent sends across sessions**

Change `sendTask()` so it blocks only when `getSessionRun(currentSessionId)` is live. Other sessions running must not disable current composer.

**Step 3: Stream readers are per run**

Each `/reply` call gets its own `AbortController`. Event callbacks must call:

```js
handleAgentEvent(event.eventName, event.payload, { sessionId, runId })
```

Do not use a global `activeRunSessionIdRef` for event ownership.

**Step 4: Scope event mutations**

Update `useAgentEvents` to accept `sessionId/runId` and update only the matching session context. If the event belongs to a background session, update checkpoint and permissions but do not append to the visible session unless that session is currently selected.

**Step 5: Scope cancel and permissions**

`cancelRun` must accept `runId` and `sessionId`. Permission resolution must keep filtering by `permission.run_id`.

**Step 6: Sidebar state**

`Sidebar` should consume session run status from `activeRunBySession`, not a loose boolean map.

**Step 7: Verify**

Run:

```powershell
npm run build
```

Expected: Vite build succeeds.

---

### Task 3: Server Runner Boundary

**Files:**
- Create: `crates/night24-server/src/agent_runner.rs`
- Modify: `crates/night24-server/src/main.rs`
- Modify: `crates/night24-server/src/state.rs`
- Modify: `crates/night24-server/src/reply.rs`
- Modify: `crates/night24-server/src/core_client.rs`

**Step 1: Define runner types**

Create:

```rust
pub(crate) enum RunnerMode {
    SingleCore,
    PerRunProcess,
    ProcessPool,
}

pub(crate) struct RunStart {
    pub(crate) accepted: ReplyAccepted,
    pub(crate) events: tokio::sync::mpsc::Receiver<serde_json::Value>,
}

#[async_trait::async_trait]
pub(crate) trait AgentRunner: Send + Sync {
    async fn start_reply(&self, params: ReplyParams) -> anyhow::Result<RunStart>;
    async fn cancel(&self, run_id: String, reason: Option<String>) -> anyhow::Result<serde_json::Value>;
    async fn resolve_permission(
        &self,
        run_id: String,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> anyhow::Result<serde_json::Value>;
}
```

**Step 2: Wrap current `AgentCoreClient`**

Implement `SingleCoreRunner` around the current shared `AgentCoreClient`. This preserves behavior while moving `reply.rs`, cancel, and permission endpoints off direct `core_client` usage.

**Step 3: Add runner selection**

Read `NIGHT24_AGENT_RUNNER`:

- missing or `single_core` => `SingleCoreRunner`
- `per_run_process` => initially return a clear unsupported error or guarded implementation from Task 4
- `process_pool` => initially return a clear unsupported error

**Step 4: Verify**

Run:

```powershell
cargo build --release -p night24-server -p night24-agent-core
```

Expected: Rust build succeeds.

---

### Task 4: Per-Run Agent Process Prototype

**Files:**
- Modify: `crates/night24-server/src/agent_runner.rs`
- Modify: `crates/night24-server/src/core_client.rs`

**Step 1: Make agent-core client spawnable as owned instance**

Refactor `AgentCoreClient::spawn()` so it can be used both as the existing shared client and as a per-run client.

**Step 2: Implement `PerRunProcessRunner`**

For each `start_reply`:

- spawn a fresh `AgentCoreClient`
- call `reply(params)`
- keep the client alive until terminal event
- on terminal event, remove the run from the runner map and allow process cleanup

**Step 3: Track run to process**

Maintain:

```rust
HashMap<String, Arc<AgentCoreClient>>
```

`cancel` and `resolve_permission` route to the specific client by `run_id`.

**Step 4: Add guardrails**

Add env var `NIGHT24_AGENT_MAX_PROCESSES`, default `4`. If exceeded, return an error event or HTTP error explaining that the run limit is reached.

**Step 5: Verify**

Run:

```powershell
cargo test -p night24-server agent_runner -- --nocapture
cargo build --release -p night24-server -p night24-agent-core
```

Expected: tests and build pass.

---

### Task 5: Tests And Integration Verification

**Files:**
- Add tests near changed frontend utilities where practical.
- Add Rust tests in `crates/night24-server/src/agent_runner.rs` or existing module tests.

**Step 1: Frontend smoke checks**

Add unit-style pure function tests if helpers are extracted. Minimum build gate:

```powershell
npm run build
```

**Step 2: Server runner tests**

Test:

- `RunnerMode` parses env strings.
- `SingleCoreRunner` delegates cancel and permission resolution.
- per-run max process guard rejects excess runs.

**Step 3: Full build**

Run:

```powershell
cargo build --release -p night24-server -p night24-agent-core
npm run build
cargo build --release
```

Expected: all pass.

---

## Parallel Worker Assignment

- Worker A: Task 1 and Task 2 frontend context/run registry.
- Worker B: Task 3 server runner boundary.
- Worker C: Task 4 per-run process prototype and guardrails.
- Worker D: Task 5 tests, review, and integration checks.

Worker B should land before Worker C merges deeply, but C can prototype against the planned `AgentRunner` interface in parallel.

## Risks And Mitigations

- **Risk:** same-session concurrent runs corrupt history.  
  **Mitigation:** phase 1 blocks one active run per session.

- **Risk:** per-run processes consume too many resources.  
  **Mitigation:** `NIGHT24_AGENT_MAX_PROCESSES`, default 4.

- **Risk:** frontend background events append to wrong session.  
  **Mitigation:** every event handler receives explicit `{ sessionId, runId }`.

- **Risk:** existing restart/status UI assumes one core.  
  **Mitigation:** keep `single_core` mode default and preserve current status behavior; expose richer runner status later.

## Completion Criteria

- Different sessions can run tasks simultaneously from the desktop app.
- Current session composer is disabled only for that session's active run.
- Running indicators remain scoped per session in the tree sidebar.
- Cancelling a run cancels only that run.
- Permission prompts resolve against the owning run.
- Server has `AgentRunner` abstraction and supports current single-core behavior through it.
- Per-run process runner is available behind env flag or has a tested guarded prototype.
- All build commands pass.

## Worker D Verification Checklist

- Frontend session isolation:
  - Start a run in session A, switch to session B, confirm B can send a task while A remains running.
  - Confirm A's streamed messages, timeline entries, pending permissions, and checkpoint do not append to B's visible conversation.
  - Switch back to A while it is running and confirm event replay resumes from the last checkpoint without duplicate messages.
  - Cancel A and confirm B's run, permissions, and composer state are unchanged.
- Server runner isolation:
  - `NIGHT24_AGENT_RUNNER` missing or `single_core` keeps current behavior.
  - `per_run_process` respects `NIGHT24_AGENT_MAX_PROCESSES` and returns a clear limit error when exceeded.
  - `/agent/cancel` and permission resolution route by `run_id`, not by the currently selected frontend session.
  - `/runs/{run_id}/events?after_seq=N` returns only that run's events and stops after terminal events.
- Regression gates:
  - `node --test tauri-app/src/utils/*.test.mjs`
  - `cargo test -p night24-server run_events -- --nocapture`
  - `cargo test -p night24-server agent_runner -- --nocapture` once `agent_runner.rs` lands.
  - `npm run build`
  - `cargo build --release -p night24-server -p night24-agent-core`
  - `cargo build --release` under `tauri-app`
