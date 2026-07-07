import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { isLiveSubAgentStatus } from '../components/subagents/status.js';
import { normalizeError } from '../utils/events.js';

export function useSubAgents({ apiJson, active, notify, running }) {
  const [pool, setPool] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const requestRef = useRef({ id: 0, request: null });

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
        const data = await apiJson('/agent/subagents?include_messages=true&include_result=true');
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
  }, [apiJson, notify]);

  useEffect(() => {
    return () => {
      requestRef.current.id += 1;
      requestRef.current.request = null;
    };
  }, []);

  const hasLiveSubAgents = useMemo(() => {
    const agents = Array.isArray(pool?.subagents) ? pool.subagents : [];
    return agents.some((agent) => isLiveSubAgentStatus(agent.status));
  }, [pool]);

  useEffect(() => {
    if (active) {
      loadSubAgents();
    } else if (running) {
      loadSubAgents({ silent: true });
    }
  }, [active, loadSubAgents, running]);

  useEffect(() => {
    if (!active && !running) return undefined;
    if (!running && !hasLiveSubAgents) return undefined;
    const timer = window.setInterval(() => {
      loadSubAgents({ silent: true });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [active, hasLiveSubAgents, loadSubAgents, running]);

  return {
    subAgentPool: pool,
    subAgentLoading: loading,
    subAgentError: error,
    loadSubAgents,
  };
}
