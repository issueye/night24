import { useEffect, useMemo, useState } from 'react';
import { AlertCircle, RefreshCw } from 'lucide-react';
import { Placeholder } from './Placeholder.jsx';
import { SubAgentDetail } from './subagents/SubAgentDetail.jsx';
import { compactSubAgentText, subAgentStatusMeta } from './subagents/status.js';
import { Button, IconButton, Tab, Tabs } from './ui/index.js';

export function SubAgentPanel({
  pool,
  sessions,
  currentSessionId,
  loading,
  error,
  spawning,
  onRefresh,
  onSelectSession,
}) {
  const agents = useMemo(() => {
    const poolItems = Array.isArray(pool?.subagents) ? pool.subagents : [];
    const byId = new Map(poolItems.map((item) => [item.id, item]));
    const sessionItems = Array.isArray(sessions) ? sessions : [];
    if (sessionItems.length > 0) {
      return sessionItems.map((session) => {
        const poolAgent = byId.get(session.id) || {};
        return {
          ...poolAgent,
          id: session.id,
          name: session.name || poolAgent.name || 'subagent',
          task: poolAgent.task || session.name || '',
          status: poolAgent.status || 'completed',
          updated_at: session.updated_at || poolAgent.updated_at,
          parent_session_id: session.parent_id,
          session,
          is_session_backed: true,
        };
      }).sort((a, b) => String(b.updated_at || '').localeCompare(String(a.updated_at || '')));
    }
    if (currentSessionId) {
      return [];
    }
    const items = poolItems;
    return [...items].sort((a, b) => String(b.updated_at || '').localeCompare(String(a.updated_at || '')));
  }, [currentSessionId, pool, sessions]);
  const [selectedId, setSelectedId] = useState('');

  useEffect(() => {
    if (!agents.length) {
      setSelectedId('');
      return;
    }
    if (!agents.some((item) => item.id === selectedId)) {
      setSelectedId(agents[0].id);
    }
  }, [agents, selectedId]);

  const selected = agents.find((item) => item.id === selectedId) || agents[0];

  return (
    <section className="subagent-panel">
      <div className="subagent-toolbar">
        <strong>子代理</strong>
        <IconButton className="icon-button compact" disabled={loading} label="刷新子代理" onClick={onRefresh} size="sm">
          <RefreshCw className={loading ? 'spin' : ''} size={13} />
        </IconButton>
      </div>

      {error && (
        <div className="subagent-error">
          <AlertCircle size={14} />
          <span>{error}</span>
        </div>
      )}

      {!loading && !spawning && agents.length === 0 ? (
        <Placeholder title="暂无子代理" detail="当前会话尚未创建子代理会话。" />
      ) : (
        <div className="subagent-layout">
          <Tabs ariaLabel="子代理页签" listClassName="subagent-tabs">
            {(loading || spawning) && agents.length === 0
              ? Array.from({ length: 3 }).map((_, index) => <div className="subagent-skeleton tab" key={index} />)
              : agents.map((agent, index) => {
                const meta = subAgentStatusMeta(agent.status);
                const label = agent.name || `Agent ${index + 1}`;
                return (
                  <Tab
                    active={selected?.id === agent.id}
                    key={agent.id || index}
                    onSelect={() => setSelectedId(agent.id)}
                    title={agent.task || label}
                  >
                    <span className={`subagent-dot ${meta.tone}`} />
                    <span>{compactSubAgentText(label, 22)}</span>
                    <em>{meta.label}</em>
                  </Tab>
                );
              })}
          </Tabs>
          {agents.length === 0 && spawning ? (
            <Placeholder title="正在创建子代理" detail="已检测到子代理调用，正在同步子代理会话。" />
          ) : (
            <SubAgentDetail selected={selected} onOpenSession={onSelectSession} />
          )}
        </div>
      )}
    </section>
  );
}
