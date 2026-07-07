import assert from 'node:assert/strict';
import test from 'node:test';

import { normalizeError } from './events.js';

test('normalizeError extracts gateway json error messages', () => {
  const detail = normalizeError(JSON.stringify({
    error: {
      message: 'upstream returned status 503: 无可用账号，请稍后重试',
      type: 'invalid_request_error',
    },
  }));

  assert.equal(detail, 'upstream returned status 503: 无可用账号，请稍后重试');
});

test('normalizeError keeps provider status prefix around embedded gateway json', () => {
  const detail = normalizeError(
    'OpenAI Responses API error 502 after 2 attempts: {"error":{"message":"upstream returned status 502","type":"invalid_request_error"}} Hint: switch provider',
  );

  assert.equal(
    detail,
    'OpenAI Responses API error 502 after 2 attempts：upstream returned status 502：Hint: switch provider',
  );
});
