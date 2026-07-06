import { useCallback, useState } from 'react';
import { normalizeError } from '../utils/events.js';
import { isVisibleChatMessage } from '../utils/format.js';

const filterByKey = (items, key, value) => items.filter((item) => item?.[key] !== value);

export function useRunControls({
  apiJson,
  addSessionTimeline,
  contextUsage,
  currentSessionId,
  getSessionRun,
  cancelRunState,
  loadSessions,
  markRunTerminal,
  setSessionMessages,
  setSessionPermissions,
  clearSessionRun,
  isRunning,
  showError,
}) {
  const [contextCompacting, setContextCompacting] = useState(false);

  const cancelRun = useCallback(async ({ sessionId = currentSessionId, runId } = {}) => {
    const run = getSessionRun(sessionId);
    const activeRunId = runId || run?.runId;
    if (!sessionId || !activeRunId) return;

    markRunTerminal(sessionId, activeRunId, 'cancelled');
    if (!runId || run?.runId === activeRunId) {
      run?.controller?.abort();
    }
    try {
      const result = await apiJson('/agent/cancel', {
        method: 'POST',
        body: JSON.stringify({ run_id: activeRunId, reason: 'user_cancelled' }),
      });
      if (result?.accepted === false) {
        addSessionTimeline(sessionId, 'cancel', '本地已停止，server 未接管取消', result.reason || activeRunId, 'warning');
      } else {
        addSessionTimeline(sessionId, 'cancel', '已请求取消', result?.run_id || activeRunId, 'warning');
      }
      const cancelledRunId = result?.run_id || activeRunId;
      setSessionPermissions(sessionId, (items) => filterByKey(items, 'run_id', cancelledRunId));
    } catch (error) {
      addSessionTimeline(sessionId, 'cancel', '本地已停止，取消接口不可用', normalizeError(error), 'warning');
      setSessionPermissions(sessionId, (items) => filterByKey(items, 'run_id', activeRunId));
    } finally {
      cancelRunState(activeRunId);
      clearSessionRun(sessionId, activeRunId);
      loadSessions();
    }
  }, [
    addSessionTimeline,
    apiJson,
    cancelRunState,
    clearSessionRun,
    currentSessionId,
    getSessionRun,
    loadSessions,
    markRunTerminal,
    setSessionPermissions,
  ]);

  const compactContext = useCallback(async () => {
    if (!currentSessionId || contextCompacting || isRunning) return;
    setContextCompacting(true);
    try {
      const data = await apiJson(`/sessions/${encodeURIComponent(currentSessionId)}/compact`, {
        method: 'POST',
        body: JSON.stringify({
          threshold_tokens: contextUsage?.threshold || undefined,
          force: true,
        }),
      });
      const nextMessages = Array.isArray(data?.conversation)
        ? data.conversation.filter(isVisibleChatMessage)
        : [];
      setSessionMessages(currentSessionId, nextMessages);
      loadSessions();
      if (data?.compacted) {
        addSessionTimeline(
          currentSessionId,
          'context',
          '上下文已压缩',
          `移除 ${data.removed} 条，当前 ${data.current} 条，估算 ${data.token_estimate} tokens`,
          'success',
        );
      } else {
        addSessionTimeline(currentSessionId, 'context', '无需压缩', '当前会话上下文还不足以压缩', 'neutral');
      }
    } catch (error) {
      const detail = normalizeError(error);
      addSessionTimeline(currentSessionId, 'context', '压缩失败', detail, 'error');
      showError(`压缩摘要失败：${detail}`, { sessionId: currentSessionId });
    } finally {
      setContextCompacting(false);
    }
  }, [
    addSessionTimeline,
    apiJson,
    contextCompacting,
    contextUsage,
    currentSessionId,
    isRunning,
    loadSessions,
    setSessionMessages,
    showError,
  ]);

  const resolvePermission = useCallback(async (permission, decision) => {
    if (!permission?.permission_id) return;
    const sessionId = permission.session_id || currentSessionId;
    try {
      const result = await apiJson(`/permissions/${encodeURIComponent(permission.permission_id)}/${decision}`, {
        method: 'POST',
        body: JSON.stringify({ run_id: permission.run_id, reason: `user_${decision}` }),
      });
      setSessionPermissions(sessionId, (items) => filterByKey(items, 'permission_id', permission.permission_id));
      if (result?.accepted === false) {
        addSessionTimeline(sessionId, 'permission', '权限接口暂不可用', result.reason || permission.summary, 'warning');
      } else {
        addSessionTimeline(
          sessionId,
          'permission',
          decision === 'approve' ? '已批准权限' : '已拒绝权限',
          permission.summary,
          decision === 'approve' ? 'success' : 'warning',
        );
      }
    } catch (error) {
      showError(`处理权限失败：${normalizeError(error)}`, { sessionId });
    }
  }, [addSessionTimeline, apiJson, currentSessionId, setSessionPermissions, showError]);

  return {
    cancelRun,
    compactContext,
    contextCompacting,
    resolvePermission,
  };
}
