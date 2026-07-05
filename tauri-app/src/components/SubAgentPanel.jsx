import { useEffect, useMemo, useState } from 'react';
import { AlertCircle, Ban, CheckCircle2, Circle, Clock3, Loader2, MessageSquareText, RefreshCw, XCircle } from 'lucide-react';
import { classNames, formatTime } from '../utils/format.js';
import { Placeholder } from './Placeholder.jsx';

const STATUS_META = {
  queued: { label: '排队中', tone: 'queued', icon: Clock3 },
  running: { label: '运行中', tone: 'running', icon: Loader2 },
  completed: { label: '已完成', tone: 'completed', icon: CheckCircle2 },
  failed: { label: '失败', tone: 'failed', icon: XCircle },
  cancelled: { label: '已取消', tone: 'cancelled', icon: Ban },
};

function statusMeta(status) {
  return STATUS_META[status] || { label: status || '未知', tone: 'unknown', icon: Circle };
}

function compactText(value, max = 120) {
  const text = String(value || '').replace(/\s+/g, ' ').trim();
  if (text.length <= max) return text;
  return `${text.slice(0, max - 3)}...`;
}

function Stat({ label, value, tone }) {
  return (
    <div className={classNames('subagent-stat', tone)}>
      <span>{label}</span>
      <strong>{value || 0}</strong>
    </div>
  );
}

function StatusPill({ status }) {
  const meta = statusMeta(status);
  const Icon = meta.icon;
  return (
    <span className={classNames('subagent-status', meta.tone)}>
      <Icon className={status === 'running' ? 'spin' : ''} size={13} />
      {meta.label}
    </span>
  );
}

function MessageRow({ message }) {
  const direction = message?.direction || 'parent_to_child';
  const label = direction === 'child_to_parent' ? '子代理' : '主代理';
  return (
    <div className={classNames('subagent-message', direction)}>
      <div>
        <strong>{label}</strong>
        <span>{formatTime(message?.created_at)}</span>
      </div>
      <p>{message?.text || '空消息'}</p>
    </div>
  );
}

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
      <div className="subagent-toolbar">
        <div className="subagent-stats">
          <Stat label="总数" value={pool?.total} />
          <Stat label="运行中" value={pool?.running} tone="running" />
          <Stat label="完成" value={pool?.completed} tone="completed" />
          <Stat label="失败" value={pool?.failed} tone="failed" />
        </div>
        <button className="icon-button compact" disabled={loading} onClick={onRefresh} title="刷新子代理" type="button">
          <RefreshCw className={loading ? 'spin' : ''} size={13} />
        </button>
      </div>

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
          <div className="subagent-list">
            {loading && agents.length === 0 ? (
              Array.from({ length: 4 }).map((_, index) => <div className="subagent-skeleton" key={index} />)
            ) : agents.map((agent) => {
              const meta = statusMeta(agent.status);
              return (
                <button
                  className={classNames('subagent-row', agent.id === selected?.id && 'active', meta.tone)}
                  key={agent.id}
                  onClick={() => setSelectedId(agent.id)}
                  type="button"
                >
                  <div>
                    <span className={classNames('subagent-dot', meta.tone)} />
                    <strong>{agent.name || 'subagent'}</strong>
                    <em>{agent.mode || 'async'}</em>
                  </div>
                  <p>{compactText(agent.task || agent.result_preview || agent.id)}</p>
                  <footer>
                    <span>{formatTime(agent.updated_at)}</span>
                    <span><MessageSquareText size={12} />{agent.message_count || 0}</span>
                  </footer>
                </button>
              );
            })}
          </div>

          <div className="subagent-detail">
            {selected ? (
              <>
                <div className="subagent-detail-head">
                  <div>
                    <strong>{selected.name || 'subagent'}</strong>
                    <span>{selected.id}</span>
                  </div>
                  <StatusPill status={selected.status} />
                </div>

                <div className="subagent-meta">
                  <label>
                    <span>模式</span>
                    <strong>{selected.mode || 'async'}</strong>
                  </label>
                  <label>
                    <span>更新时间</span>
                    <strong>{formatTime(selected.updated_at) || '-'}</strong>
                  </label>
                </div>

                <section className="subagent-section">
                  <div className="subagent-section-head">任务</div>
                  <p className="subagent-task">{selected.task || '无任务内容'}</p>
                </section>

                <section className="subagent-section subagent-messages">
                  <div className="subagent-section-head">通讯记录</div>
                  {Array.isArray(selected.messages) && selected.messages.length > 0 ? (
                    selected.messages.map((message, index) => <MessageRow key={`${message.created_at || index}-${index}`} message={message} />)
                  ) : (
                    <div className="subagent-empty-line">暂无主代理与子代理通讯记录</div>
                  )}
                </section>

                <section className="subagent-section">
                  <div className="subagent-section-head">{selected.status === 'failed' ? '错误' : '结果'}</div>
                  <pre className={classNames('subagent-result', selected.status === 'failed' && 'error')}>
                    {selected.status === 'failed'
                      ? selected.error || '子代理失败，但未返回错误详情'
                      : selected.result || selected.result_preview || '子代理尚未返回结果'}
                  </pre>
                </section>

                <details className="subagent-debug">
                  <summary>运行标识</summary>
                  <code>parent: {selected.parent_run_id || '-'}</code>
                  <code>child: {selected.child_run_id || '-'}</code>
                </details>
              </>
            ) : (
              <Placeholder title="请选择子代理" detail="从左侧列表中选择一个子代理查看详情。" />
            )}
          </div>
        </div>
      )}
    </section>
  );
}
