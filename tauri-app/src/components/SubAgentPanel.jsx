import { useEffect, useMemo, useState } from 'react';
import { AlertCircle } from 'lucide-react';
import { Placeholder } from './Placeholder.jsx';
import { SubAgentDetail } from './subagents/SubAgentDetail.jsx';
import { SubAgentList } from './subagents/SubAgentList.jsx';
import { SubAgentStats } from './subagents/SubAgentStats.jsx';

export function SubAgentPanel({
  pool,
  loading,
  error,
  onRefresh,
}) {
  const agents = useMemo(() => {
    const items = Array.isArray(pool?.subagents) ? pool.subagents : [];
    return [...items].sort((a, b) => String(b.updated_at || '').localeCompare(String(a.updated_at || '')));
  }, [pool]);
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
      <SubAgentStats loading={loading} onRefresh={onRefresh} pool={pool} />

      {error && (
        <div className="subagent-error">
          <AlertCircle size={14} />
          <span>{error}</span>
        </div>
      )}

      {!loading && agents.length === 0 ? (
        <Placeholder title="暂无子代理" detail="当前任务尚未创建子代理，代理池为空。" />
      ) : (
        <div className="subagent-layout">
          <SubAgentList agents={agents} loading={loading} onSelect={setSelectedId} selectedId={selected?.id} />
          <SubAgentDetail selected={selected} />
        </div>
      )}
    </section>
  );
}
