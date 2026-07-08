import assert from 'node:assert/strict';
import test from 'node:test';

import {
  isSubAgentRunId,
  isTerminalSubAgentStatus,
  resolveEventSessionId,
  subAgentRunParentId,
  subAgentSessionInfo,
} from './subagentEvents.js';

test('subAgentRunParentId extracts parent run channel', () => {
  assert.equal(subAgentRunParentId('run-parent:subagent:child'), 'run-parent');
  assert.equal(subAgentRunParentId('run-parent'), '');
  assert.equal(isSubAgentRunId('run-parent:subagent:child'), true);
  assert.equal(isSubAgentRunId('run-parent'), false);
});

test('subAgentSessionInfo normalizes backend sub_agent_session payload', () => {
  const info = subAgentSessionInfo({
    subagent_id: 'subagent-1',
    child_run_id: 'run-parent:subagent:child',
    parent_session_id: 'session-parent',
    status: 'running',
    messages: [{ id: 'task', role: 'user' }],
  });

  assert.equal(info.subagentId, 'subagent-1');
  assert.equal(info.childRunId, 'run-parent:subagent:child');
  assert.equal(info.parentRunId, 'run-parent');
  assert.equal(info.parentSessionId, 'session-parent');
  assert.equal(info.messages.length, 1);
});

test('resolveEventSessionId prefers mapped child session', () => {
  const mapping = new Map([['run-parent:subagent:child', 'subagent-1']]);

  assert.equal(resolveEventSessionId({
    eventRunId: 'run-parent:subagent:child',
    fallbackSessionId: 'session-parent',
    childRunSessionByRunId: mapping,
  }), 'subagent-1');
  assert.equal(resolveEventSessionId({
    eventRunId: 'run-parent',
    fallbackSessionId: 'session-parent',
    childRunSessionByRunId: mapping,
  }), 'session-parent');
});

test('isTerminalSubAgentStatus recognizes final statuses', () => {
  assert.equal(isTerminalSubAgentStatus('completed'), true);
  assert.equal(isTerminalSubAgentStatus('failed'), true);
  assert.equal(isTerminalSubAgentStatus('cancelled'), true);
  assert.equal(isTerminalSubAgentStatus('running'), false);
});
