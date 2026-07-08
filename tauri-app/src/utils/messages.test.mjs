import assert from 'node:assert/strict';
import test from 'node:test';

import { mergeVisibleMessagesById } from './messages.js';

const isVisibleMessage = (message) => Boolean(message?.role);

test('mergeVisibleMessagesById preserves local streaming messages missing from history', () => {
  const localMessages = [
    { id: 'user-1', role: 'user', content: [{ type: 'text', text: 'start' }] },
    { id: 'assistant-stream', role: 'assistant', content: [{ type: 'text', text: 'partial answer' }] },
  ];
  const historyMessages = [
    { id: 'user-1', role: 'user', content: [{ type: 'text', text: 'start' }] },
  ];

  const merged = mergeVisibleMessagesById(localMessages, historyMessages, isVisibleMessage);

  assert.equal(merged.length, 2);
  assert.equal(merged[1].id, 'assistant-stream');
  assert.equal(merged[1].content[0].text, 'partial answer');
});

test('mergeVisibleMessagesById updates messages that are confirmed by history', () => {
  const localMessages = [
    { id: 'assistant-1', role: 'assistant', content: [{ type: 'text', text: 'partial' }] },
  ];
  const historyMessages = [
    { id: 'assistant-1', role: 'assistant', content: [{ type: 'text', text: 'complete' }] },
  ];

  const merged = mergeVisibleMessagesById(localMessages, historyMessages, isVisibleMessage);

  assert.equal(merged.length, 1);
  assert.equal(merged[0].content[0].text, 'complete');
});

test('mergeVisibleMessagesById prunes synthetic tool activity when canonical history arrives', () => {
  const localMessages = [
    {
      id: 'tool-activity-call-1',
      role: 'tool',
      content: [{
        type: 'tool_activity',
        id: 'call-1',
        tool_call_id: 'call-1',
        tool_name: 'developer_shell',
        status: 'running',
      }],
    },
    { id: 'assistant-stream', role: 'assistant', content: [{ type: 'text', text: 'partial answer' }] },
  ];
  const historyMessages = [
    {
      id: 'tool-request-call-1',
      role: 'assistant',
      content: [{
        type: 'tool_request',
        id: 'call-1',
        tool_call_id: 'call-1',
        tool_name: 'developer_shell',
        arguments: { command: 'go test ./...' },
      }],
    },
    {
      id: 'tool-response-call-1',
      role: 'tool',
      content: [{
        type: 'tool_response',
        id: 'call-1',
        tool_call_id: 'call-1',
        tool_name: 'developer_shell',
        content: 'ok',
      }],
    },
  ];

  const merged = mergeVisibleMessagesById(localMessages, historyMessages, isVisibleMessage, {
    pruneSyntheticToolActivity: true,
  });

  assert.equal(merged.some((message) => message.id === 'tool-activity-call-1'), false);
  assert.equal(merged.some((message) => message.id === 'assistant-stream'), true);
  assert.equal(merged.some((message) => message.id === 'tool-request-call-1'), true);
  assert.equal(merged.some((message) => message.id === 'tool-response-call-1'), true);
});
