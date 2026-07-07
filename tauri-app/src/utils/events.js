import { safeText } from './format.js';

export function normalizeError(error) {
  if (!error) return '未知错误';
  if (typeof error === 'string') return normalizeErrorText(error);
  if (error instanceof TypeError && /fetch/i.test(error.message || '')) {
    return '无法连接 server，请确认服务已启动或 Server 地址正确';
  }
  return normalizeErrorText(error.message || safeText(error));
}

export function normalizeErrorText(value) {
  const text = String(value || '').trim();
  if (!text) return '未知错误';

  const parsed = parseEmbeddedJson(text);
  if (!parsed) return text;

  const detail = parsedJsonErrorMessage(parsed.value);
  if (!detail) return text;

  const prefix = text.slice(0, parsed.start).trim().replace(/[:：]\s*$/, '');
  const suffix = text.slice(parsed.end).trim();
  const hint = suffix && !suffix.startsWith('{') ? suffix : '';
  return [prefix, detail, hint].filter(Boolean).join('：');
}

function parseEmbeddedJson(text) {
  const direct = tryParseJson(text);
  if (direct) {
    return { value: direct, start: 0, end: text.length };
  }

  const start = text.indexOf('{');
  const end = text.lastIndexOf('}');
  if (start < 0 || end <= start) return null;

  const value = tryParseJson(text.slice(start, end + 1));
  return value ? { value, start, end: end + 1 } : null;
}

function tryParseJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function parsedJsonErrorMessage(value) {
  if (typeof value?.error === 'string') return value.error;
  if (typeof value?.error?.message === 'string') return value.error.message;
  if (typeof value?.message === 'string') return value.message;
  return '';
}

export function unwrapSsePayload(eventName, payload) {
  if (payload?.method === 'agent.event' && payload.params) {
    return payload.params;
  }
  if (eventName === 'agent.event' && payload?.params) {
    return payload.params;
  }
  return payload;
}
