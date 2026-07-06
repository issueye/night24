import { useCallback, useState } from 'react';
import { normalizeError } from '../utils/events.js';
import { isVisibleChatMessage } from '../utils/format.js';

const filterByKey = (items, key, value) => items.filter((item) => item?.[key] !== value);

export function useRunControls({
  abortRef,
  activeRun,
  apiJson,
  addTimeline,
  contextUsage,
  currentSessionId,
  isRunning,
  loadSessions,
  runTerminalRef,
  setActiveRun,
  setIsRunning,
  setMessages,
  setPendingPermissions,
  showError,
}) {
  const [contextCompacting, setContextCompacting] = useState(false);

  const cancelRun = useCallback(async () => {
    const activeRunId = activeRun?.run_id;
    runTerminalRef.current = { type: 'cancelled', runId: activeRunId };
    abortRef.current?.abort();
    setActiveRun((run) => (run ? { ...run, status: 'cancelling' } : { status: 'cancelling' }));
    try {
      const result = await apiJson('/agent/cancel', {
        method: 'POST',
        body: JSON.stringify({ run_id: activeRunId, reason: 'user_cancelled' }),
      });
      if (result?.accepted === false) {
        addTimeline('cancel', '本地已停止，server 未接管取消', result.reason || activeRunId || '当前任务', 'warning');
      } else {
        addTimeline('cancel', '已请求取消', result?.run_id || activeRunId || '当前任务', 'warning');
      }
      const cancelledRunId = result?.run_id || activeRunId;
      if (cancelledRunId) {
        setPendingPermissions((items) => filterByKey(items, 'run_id', cancelledRunId));
      }
    } catch (error) {
      addTimeline('cancel', '本地已停止，取消接口不可用', normalizeError(error), 'warning');
      if (activeRunId) {
        setPendingPermissions((items) => filterByKey(items, 'run_id', activeRunId));
      }
    } finally {
      setIsRunning(false);
      setActiveRun((run) => (run ? { ...run, status: 'cancelled' } : { status: 'cancelled' }));
    }
  }, [
    abortRef,
    activeRun,
    addTimeline,
    apiJson,
    runTerminalRef,
    setActiveRun,
    setIsRunning,
    setPendingPermissions,
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
      setMessages(nextMessages);
      loadSessions();
      if (data?.compacted) {
        addTimeline(
          'context',
          '上下文已压缩',
          `移除 ${data.removed} 条，当前 ${data.current} 条，估算 ${data.token_estimate} tokens`,
          'success',
        );
      } else {
        addTimeline('context', '无需压缩', '当前会话上下文还不足以压缩', 'neutral');
      }
    } catch (error) {
      const detail = normalizeError(error);
      addTimeline('context', '压缩失败', detail, 'error');
      showError(`压缩摘要失败：${detail}`);
    } finally {
      setContextCompacting(false);
    }
  }, [
    addTimeline,
    apiJson,
    contextCompacting,
    contextUsage,
    currentSessionId,
    isRunning,
    loadSessions,
    setMessages,
    showError,
  ]);

  const resolvePermission = useCallback(async (permission, decision) => {
    if (!permission?.permission_id) return;
    try {
      const result = await apiJson(`/permissions/${encodeURIComponent(permission.permission_id)}/${decision}`, {
        method: 'POST',
        body: JSON.stringify({ run_id: permission.run_id, reason: `user_${decision}` }),
      });
      setPendingPermissions((items) => filterByKey(items, 'permission_id', permission.permission_id));
      if (result?.accepted === false) {
        addTimeline('permission', '权限接口暂不可用', result.reason || permission.summary, 'warning');
      } else {
        addTimeline(
          'permission',
          decision === 'approve' ? '已批准权限' : '已拒绝权限',
          permission.summary,
          decision === 'approve' ? 'success' : 'warning',
        );
      }
    } catch (error) {
      showError(`处理权限失败：${normalizeError(error)}`);
    }
  }, [addTimeline, apiJson, setPendingPermissions, showError]);

  return {
    cancelRun,
    compactContext,
    contextCompacting,
    resolvePermission,
  };
}
