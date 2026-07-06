import { unwrapSsePayload } from './events.js';
import { safeText } from './format.js';

export function normalizeAgentEvent(eventName, payload) {
  const envelope = unwrapSsePayload(eventName, payload);
  const eventType = envelope?.type || (eventName && eventName !== 'message' ? eventName : 'message');
  const eventPayload = envelope?.payload || envelope;
  const runId = envelope?.run_id || eventPayload?.run_id;
  const runStatus = eventType === 'finish' ? 'finished' : eventType === 'error' ? 'error' : 'running';

  return {
    envelope,
    eventType,
    eventPayload,
    runId,
    runStatus,
    isBareMessage: !envelope?.type && envelope?.role,
  };
}

export function normalizeMessageText(eventPayload) {
  return eventPayload?.text || eventPayload?.content || safeText(eventPayload);
}

export function normalizePermissionEvent(eventPayload, envelope, runId, fallbackPermissionId) {
  const permissionId =
    eventPayload?.permission_id ||
    envelope?.permission_id ||
    eventPayload?.tool_call_id ||
    fallbackPermissionId;

  return {
    permission_id: permissionId,
    run_id: runId,
    tool_name: eventPayload?.tool_name || 'tool',
    risk: eventPayload?.risk || 'high',
    summary: eventPayload?.summary || '需要确认权限',
    arguments: eventPayload?.arguments || eventPayload?.params,
  };
}

export function normalizeToolStartedEvent(eventPayload) {
  return {
    title: eventPayload?.tool_name || '工具开始',
    detail: eventPayload?.summary || safeText(eventPayload),
    tone: 'neutral',
  };
}

export function normalizeToolFinishedEvent(eventPayload) {
  return {
    title: eventPayload?.tool_name || '工具完成',
    detail: eventPayload?.summary || eventPayload?.result_preview || safeText(eventPayload),
    tone: eventPayload?.is_error ? 'error' : 'success',
  };
}

export function normalizeToolFailedEvent(eventPayload) {
  const toolName = eventPayload?.tool_name || '工具';
  const detail = eventPayload?.error?.message || eventPayload?.error || safeText(eventPayload);
  return {
    toolName,
    detail,
    title: `${toolName} 失败`,
    tone: 'error',
    messageText: `工具调用失败：${toolName}\n\n${detail}`,
  };
}

export function normalizeRunOutputEvent(eventPayload) {
  return {
    title: eventPayload?.source || '运行输出',
    detail: eventPayload?.text || safeText(eventPayload),
    tone: eventPayload?.stream === 'stderr' ? 'warning' : 'neutral',
  };
}

export function normalizeDiffReadyEvent(eventPayload) {
  return {
    title: '变更已生成',
    detail: eventPayload?.summary || safeText(eventPayload),
    tone: 'success',
  };
}

export function normalizeFinishEvent(eventPayload) {
  const status = eventPayload?.status || 'completed';
  return {
    messages: Array.isArray(eventPayload?.messages) ? eventPayload.messages : [],
    status,
    tone: status === 'failed' ? 'error' : status === 'cancelled' ? 'warning' : 'success',
  };
}

export function normalizeErrorEvent(eventPayload) {
  return {
    detail: eventPayload?.message || eventPayload?.error || safeText(eventPayload),
  };
}

export function normalizeFallbackTimeline(eventPayload) {
  return safeText(eventPayload);
}
