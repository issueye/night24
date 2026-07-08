# Subagent Realtime Channels Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Stream each subagent's agent-core events to the server in real time, using the child run id as an isolated event channel.

**Architecture:** Keep the existing `AgentEvent.run_id` as the channel key. Agent-core will give subagent runs the same outbound event sender as the parent while preserving each child's distinct `run_id`, and the server will persist/publish each incoming event under its own `run_id` without letting child terminal events stop the parent run.

**Tech Stack:** Rust, Tokio channels, JSON-RPC notifications, JSONL run event store, axum SSE.

---

## Current Findings

- `AgentEvent` already includes `run_id`; the child run id format is `parent:subagent:<uuid>`, so no new protocol field is required for channel isolation.
- `crates/night24-agent-core/src/lib.rs` creates a child `RunContext` with `output: None` and `collected: Some(...)`. This prevents child events from reaching the server until the child finishes.
- `crates/night24-server/src/core_client.rs` routes `agent.event` notifications by `run_id`, but it only registers an event sender for the parent run. Child run events would currently have no receiver if emitted live.
- `crates/night24-server/src/reply.rs` currently drops events whose `run_id` differs from the parent run id, and persists all events under the parent run id.
- `RunEventStore` already isolates events by run id and supports `/runs/{run_id}/events`, which is the correct server-side channel abstraction.

## Design

- Agent-core:
  - Give subagent `RunContext` the parent's `output` sender so child events are emitted immediately.
  - Keep `collected` enabled for subagents so existing result extraction and final subagent session summary continue to work.
  - Emit a `sub_agent_session` event when the child is created, before the worker starts, so UI/server can open the child channel early.
  - Continue emitting completion/failure `sub_agent_session` updates on the parent run.

- Server core client:
  - Allow a parent reply subscription to receive all `agent.event` notifications emitted by that agent-core process, including child run ids.
  - Track `sub_agent_session` parent/child run relationships so async child events can continue to route after the parent run has emitted its terminal event.
  - Remove parent senders only after the parent is terminal and all known child run channels are terminal.
  - For per-run processes, keep forwarding events until the parent run has reached terminal state and all known child runs have finished, not when any child emits `finish`.

- Server reply pump:
  - Persist/publish each event to `RunEventStore` using `event.run_id`, not always the parent run id.
  - Only mutate the parent session conversation for parent run events.
  - Only add diff events on parent terminal events; keep the pump alive after parent terminal while known child runs are still active.
  - Forward child events through the active parent SSE too, so the current request stream can update UI immediately; replay is still available per child run id.
  - Persist `sub_agent_session` events even if they are parent metadata while child content lives in the child run channel.
  - Persist child message/message_delta/finish events into the child session incrementally so switching sessions while a child is running can show current content.

## Task 1: Agent-core emits subagent events live

**Files:**
- Modify: `crates/night24-agent-core/src/lib.rs`
- Test: `crates/night24-agent-core/src/tests.rs` or inline tests in `lib.rs`

**Steps:**
1. Add/adjust a failing test showing sync subagent child events appear in the parent output stream before final completion and use the child run id.
2. Change `spawn_subagent` child `RunContext` so `output` is `context.output.clone()` while preserving `collected`.
3. Run targeted agent-core tests.

## Task 2: Server core client forwards process-wide events to the active reply

**Files:**
- Modify: `crates/night24-server/src/core_client.rs`
- Modify: `crates/night24-server/src/agent_runner.rs`

**Steps:**
1. Add route state in `AgentCoreClient` for parent senders plus child-run-to-parent mappings.
2. Route child `run_id` events to the parent stream when a direct child sender does not exist.
3. Delay parent sender cleanup until the parent terminal event and all known child terminal events have been observed.
4. Change per-run forwarding to stop only when the parent terminal event has been observed and no known child runs remain active.
5. Run targeted server tests for event routing.

## Task 3: Server persists and broadcasts events by event run id

**Files:**
- Modify: `crates/night24-server/src/reply.rs`
- Test: `crates/night24-server/src/reply.rs`

**Steps:**
1. Replace the "drop different run id" behavior with a dispatch model that marks whether the event belongs to the parent run.
2. Persist each dispatch event under `event.run_id`.
3. Publish child events through the current parent SSE without mutating the parent session.
4. Add tests proving child finish events are not dropped, are not parent-terminal, and do not update parent conversation.
5. Add/adjust tests proving parent terminal behavior still appends diff events and finalizes the parent.

## Task 4: Subagent session persistence remains compatible

**Files:**
- Modify: `crates/night24-server/src/reply.rs`

**Steps:**
1. Keep `sub_agent_session` events tied to parent run metadata.
2. Ensure saved subagent sessions are created on the initial `running` event with task as the first user message.
3. Ensure final `sub_agent_session` updates can replace the session conversation with the completed child messages.

## Task 5: Verification

**Commands:**
- `cargo test -p night24-agent-core`
- `cargo test -p night24-server`
- `cargo build -p night24-server -p night24-agent-core`

**Completion Criteria:**
- Child events are emitted live from agent-core while the subagent is running.
- Server does not drop child run events.
- `/runs/{child_run_id}/events` can replay child events independently.
- Parent run SSE can still carry child events during the active request.
- Child terminal events do not terminate the parent run pump.
- Parent session history contains only parent conversation messages; child session history is stored separately.
