import { safeText } from './format.js';

export function normalizeError(error) {
  if (!error) return '未知错误';
  if (typeof error === 'string') return error;
  if (error instanceof TypeError && /fetch/i.test(error.message || '')) {
    return '无法连接 server，请确认服务已启动或 Server 地址正确';
  }
  return error.message || safeText(error);
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
