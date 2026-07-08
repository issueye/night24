import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { isLiveSubAgentStatus } from '../components/subagents/status.js';
import { normalizeError } from '../utils/events.js';

export function useSubAgents({ apiJson, notify, running, parentSessionId }) {
  const [pool, setPool] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const requestRef = useRef({ id: 0, request: null });
  const lastParentSessionIdRef = useRef(parentSessionId);

  const loadSubAgents = useCallback(async ({ notifySuccess = false, silent = false } = {}) => {
    if (silent && requestRef.current.request) {
      return requestRef.current.request;
    }

    const requestId = requestRef.current.id + 1;
    requestRef.current.id = requestId;
    if (!silent) {
      setLoading(true);
    }
    setError('');

    const request = (async () => {
      try {
        const endpoint = parentSessionId
          ? `/agent/subagents?include_messages=true&include_result=true&parent_session_id=${encodeURIComponent(parentSessionId)}`
          : '/agent/subagents?include_messages=true&include_result=true';
        const data = await apiJson(endpoint);
        if (requestRef.current.id === requestId) {
          setPool(data || null);
          if (notifySuccess) {
            notify?.({ message: '子代理数据已刷新', tone: 'success' });
          }
        }
      } catch (loadError) {
        if (requestRef.current.id === requestId) {
          setError(normalizeError(loadError));
          if (notifySuccess) {
            notify?.({ message: '刷新子代理失败', detail: normalizeError(loadError), tone: 'danger' });
          }
        }
      } finally {
        if (requestRef.current.request === request) {
          requestRef.current.request = null;
        }
        if (!silent && requestRef.current.id === requestId) {
          setLoading(false);
        }
      }
    })();
    requestRef.current.request = request;
    return request;
  }, [apiJson, notify, parentSessionId]);

  useEffect(() => {
    return () => {
      requestRef.current.id += 1;
      requestRef.current.request = null;
    };
  }, []);

  // 切换会话时丢弃旧池数据并重新拉取，避免短暂展示其他会话的子代理
  useEffect(() => {
    if (lastParentSessionIdRef.current === parentSessionId) return;
    lastParentSessionIdRef.current = parentSessionId;
    requestRef.current.id += 1;
    requestRef.current.request = null;
    setPool(null);
    if (running) {
      loadSubAgents();
    }
  }, [loadSubAgents, parentSessionId, running]);

  const hasLiveSubAgents = useMemo(() => {
    const agents = Array.isArray(pool?.subagents) ? pool.subagents : [];
    return agents.some((agent) => isLiveSubAgentStatus(agent.status));
  }, [pool]);

  useEffect(() => {
    if (running) {
      loadSubAgents({ silent: true });
    }
  }, [loadSubAgents, running]);

  useEffect(() => {
    if (!running && !hasLiveSubAgents) return undefined;
    const timer = window.setInterval(() => {
      loadSubAgents({ silent: true });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [hasLiveSubAgents, loadSubAgents, running]);

  return {
    subAgentPool: pool,
    subAgentLoading: loading,
    subAgentError: error,
    loadSubAgents,
  };
}
