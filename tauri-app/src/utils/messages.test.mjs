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
