import assert from 'node:assert/strict';
import test from 'node:test';

import {
  normalizeAgentEvent,
  normalizeFinishEvent,
  normalizePermissionEvent,
} from './agentEvents.js';

test('normalizeAgentEvent keeps run ownership from the SSE envelope', () => {
  const event = normalizeAgentEvent('message_delta', {
    type: 'message_delta',
    run_id: 'run-session-a',
    seq: 7,
    payload: {
      message_id: 'assistant-1',
      delta: 'hello',
    },
  });

  assert.equal(event.eventType, 'message_delta');
  assert.equal(event.runId, 'run-session-a');
  assert.equal(event.runStatus, 'running');
  assert.equal(event.eventPayload.delta, 'hello');
});

test('normalizeAgentEvent falls back to payload run_id for legacy events', () => {
  const event = normalizeAgentEvent('permission_required', {
    type: 'permission_required',
    payload: {
      run_id: 'run-legacy',
      permission_id: 'permission-1',
    },
  });

  assert.equal(event.runId, 'run-legacy');
});

test('normalizePermissionEvent tags prompts with the owning run id', () => {
  const permission = normalizePermissionEvent(
    {
      permission_id: 'permission-1',
      tool_name: 'shell',
      summary: 'needs approval',
    },
    {},
    'run-session-b',
    'fallback-permission',
  );

  assert.deepEqual(permission, {
    permission_id: 'permission-1',
    run_id: 'run-session-b',
    tool_name: 'shell',
    risk: 'high',
    summary: 'needs approval',
    arguments: undefined,
  });
});

test('normalizeFinishEvent does not invent messages across runs', () => {
  const finish = normalizeFinishEvent({
    status: 'completed',
    messages: 'not an array',
  });

  assert.deepEqual(finish.messages, []);
  assert.equal(finish.status, 'completed');
  assert.equal(finish.tone, 'success');
});
