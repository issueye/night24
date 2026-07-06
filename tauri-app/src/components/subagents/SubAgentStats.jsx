import { RefreshCw } from 'lucide-react';
import { classNames } from '../../utils/format.js';

function Stat({ label, value, tone }) {
  return (
    <div className={classNames('subagent-stat', tone)}>
      <span>{label}</span>
      <strong>{value || 0}</strong>
    </div>
  );
}

export function SubAgentStats({
  pool,
  loading,
  onRefresh,
}) {
  return (
    <div className="subagent-toolbar">
      <div className="subagent-stats">
        <Stat label="总数" value={pool?.total} />
        <Stat label="排队" value={pool?.queued} tone="queued" />
        <Stat label="运行中" value={pool?.running} tone="running" />
        <Stat label="完成" value={pool?.completed} tone="completed" />
        <Stat label="失败" value={pool?.failed} tone="failed" />
        <Stat label="取消" value={pool?.cancelled} tone="cancelled" />
      </div>
      <button className="icon-button compact" disabled={loading} onClick={onRefresh} title="刷新子代理" type="button">
        <RefreshCw className={loading ? 'spin' : ''} size={13} />
      </button>
    </div>
  );
}
