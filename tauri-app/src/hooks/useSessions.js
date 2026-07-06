import { useCallback, useState } from 'react';
import { normalizeError } from '../utils/events.js';
import { isVisibleChatMessage } from '../utils/format.js';

export function useSessions({
  apiJson,
  notify,
  workspace,
  showError,
  onBeforeSessionChange,
}) {
  const [sessions, setSessions] = useState([]);
  const [currentSessionId, setCurrentSessionId] = useState(null);
  const [messages, setMessages] = useState([]);
  const [sessionsLoading, setSessionsLoading] = useState(false);
  const [sessionActionId, setSessionActionId] = useState('');

  const clearConversationMessages = useCallback(() => {
    setMessages([]);
  }, []);

  const clearCurrentSession = useCallback(() => {
    setCurrentSessionId(null);
  }, []);

  const loadSessions = useCallback(async () => {
    setSessionsLoading(true);
    try {
      const data = await apiJson('/sessions');
      setSessions(Array.isArray(data) ? data : []);
    } catch (error) {
      notify?.({ message: '加载会话失败', detail: normalizeError(error), tone: 'danger' });
      showError(`加载会话失败：${normalizeError(error)}`, { toast: false });
    } finally {
      setSessionsLoading(false);
    }
  }, [apiJson, notify, showError]);

  const createSessionRecord = useCallback(async () => {
    const session = await apiJson('/sessions', {
      method: 'POST',
      body: JSON.stringify({
        name: 'session',
        session_type: 'user',
        working_dir: workspace?.root_path,
      }),
    });
    setSessions((items) => [session, ...items]);
    setCurrentSessionId(session.id);
    return session;
  }, [apiJson, workspace?.root_path]);

  const createSession = useCallback(async () => {
    onBeforeSessionChange?.({ abortActive: true, preserveRun: true });
    setSessionActionId('create');
    try {
      await createSessionRecord();
      notify?.({ message: '已新建会话', tone: 'success' });
    } catch (error) {
      notify?.({ message: '新建会话失败', detail: normalizeError(error), tone: 'danger' });
      showError(`新建会话失败：${normalizeError(error)}`, { toast: false });
    } finally {
      setSessionActionId('');
    }
  }, [createSessionRecord, notify, onBeforeSessionChange, showError]);

  const selectSession = useCallback(async (id) => {
    onBeforeSessionChange?.({ abortActive: true, preserveRun: true });
    setSessionActionId(id);
    try {
      const history = await apiJson(`/sessions/${id}/history`);
      const visibleMessages = Array.isArray(history) ? history.filter(isVisibleChatMessage) : [];
      setCurrentSessionId(id);
      setMessages(visibleMessages);
      return visibleMessages;
    } catch (error) {
      notify?.({ message: '加载会话失败', detail: normalizeError(error), tone: 'danger' });
      showError(`加载会话失败：${normalizeError(error)}`, { toast: false });
      return null;
    } finally {
      setSessionActionId('');
    }
  }, [apiJson, notify, onBeforeSessionChange, showError]);

  const deleteSession = useCallback(async (id, event) => {
    event?.stopPropagation();
    if (!window.confirm('删除这个会话？')) return;
    setSessionActionId(id);
    try {
      await apiJson(`/sessions/${id}`, { method: 'DELETE' });
      setSessions((items) => items.filter((item) => item.id !== id));
      if (currentSessionId === id) {
        setCurrentSessionId(null);
        onBeforeSessionChange?.({ abortActive: true, preserveRun: false });
      }
      notify?.({ message: '会话已删除', tone: 'success' });
    } catch (error) {
      notify?.({ message: '删除会话失败', detail: normalizeError(error), tone: 'danger' });
      showError(`删除会话失败：${normalizeError(error)}`, { toast: false });
    } finally {
      setSessionActionId('');
    }
  }, [apiJson, currentSessionId, notify, onBeforeSessionChange, showError]);

  const ensureSession = useCallback(async () => {
    if (currentSessionId) return currentSessionId;
    const session = await createSessionRecord();
    return session.id;
  }, [createSessionRecord, currentSessionId]);

  return {
    sessions,
    sessionsLoading,
    sessionActionId,
    currentSessionId,
    messages,
    setMessages,
    loadSessions,
    createSession,
    selectSession,
    deleteSession,
    ensureSession,
    clearConversationMessages,
    clearCurrentSession,
  };
}
