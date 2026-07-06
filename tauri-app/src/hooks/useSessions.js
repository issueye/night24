import { useCallback, useState } from 'react';
import { normalizeError } from '../utils/events.js';
import { isVisibleChatMessage } from '../utils/format.js';

export function useSessions({
  apiJson,
  workspace,
  showError,
  onBeforeSessionChange,
}) {
  const [sessions, setSessions] = useState([]);
  const [currentSessionId, setCurrentSessionId] = useState(null);
  const [messages, setMessages] = useState([]);

  const clearConversationMessages = useCallback(() => {
    setMessages([]);
  }, []);

  const clearCurrentSession = useCallback(() => {
    setCurrentSessionId(null);
  }, []);

  const loadSessions = useCallback(async () => {
    try {
      const data = await apiJson('/sessions');
      setSessions(Array.isArray(data) ? data : []);
    } catch (error) {
      showError(`加载会话失败：${normalizeError(error)}`);
    }
  }, [apiJson, showError]);

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
    onBeforeSessionChange?.({ abortActive: true });
    try {
      await createSessionRecord();
    } catch (error) {
      showError(`新建会话失败：${normalizeError(error)}`);
    }
  }, [createSessionRecord, onBeforeSessionChange, showError]);

  const selectSession = useCallback(async (id) => {
    onBeforeSessionChange?.({ abortActive: true });
    try {
      const history = await apiJson(`/sessions/${id}/history`);
      setCurrentSessionId(id);
      setMessages(Array.isArray(history) ? history.filter(isVisibleChatMessage) : []);
    } catch (error) {
      showError(`加载会话失败：${normalizeError(error)}`);
    }
  }, [apiJson, onBeforeSessionChange, showError]);

  const deleteSession = useCallback(async (id, event) => {
    event?.stopPropagation();
    if (!window.confirm('删除这个会话？')) return;
    try {
      await apiJson(`/sessions/${id}`, { method: 'DELETE' });
      setSessions((items) => items.filter((item) => item.id !== id));
      if (currentSessionId === id) {
        setCurrentSessionId(null);
        onBeforeSessionChange?.({ abortActive: true });
      }
    } catch (error) {
      showError(`删除会话失败：${normalizeError(error)}`);
    }
  }, [apiJson, currentSessionId, onBeforeSessionChange, showError]);

  const ensureSession = useCallback(async () => {
    if (currentSessionId) return currentSessionId;
    const session = await createSessionRecord();
    return session.id;
  }, [createSessionRecord, currentSessionId]);

  return {
    sessions,
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
