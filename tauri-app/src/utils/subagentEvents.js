export function subAgentRunParentId(runId) {
  const value = String(runId || '');
  const marker = ':subagent:';
  const index = value.indexOf(marker);
  return index > 0 ? value.slice(0, index) : '';
}

export function isSubAgentRunId(runId) {
  return Boolean(subAgentRunParentId(runId));
}

export function subAgentSessionInfo(payload = {}) {
  const eventPayload = payload?.payload && typeof payload.payload === 'object' ? payload.payload : payload;
  const subagentId = eventPayload?.subagent_id || eventPayload?.session_id || eventPayload?.id || '';
  const childRunId = eventPayload?.child_run_id || '';
  const parentRunId = eventPayload?.parent_run_id || subAgentRunParentId(childRunId) || '';
  return {
    subagentId,
    childRunId,
    parentRunId,
    parentSessionId: eventPayload?.parent_session_id || '',
    name: eventPayload?.name || subagentId || 'subagent',
    status: eventPayload?.status || '',
    task: eventPayload?.task || '',
    messages: Array.isArray(eventPayload?.messages) ? eventPayload.messages : [],
  };
}

export function isTerminalSubAgentStatus(status) {
  return ['completed', 'failed', 'cancelled'].includes(String(status || '').toLowerCase());
}

export function resolveEventSessionId({ eventRunId, fallbackSessionId, childRunSessionByRunId }) {
  if (!eventRunId) return fallbackSessionId || '';
  return childRunSessionByRunId?.get(eventRunId) || fallbackSessionId || '';
}
