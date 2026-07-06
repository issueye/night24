import { classNames, formatTime } from '../../utils/format.js';
import { Placeholder } from '../Placeholder.jsx';
import { subAgentStatusMeta } from './status.js';

function StatusPill({ status }) {
  const meta = subAgentStatusMeta(status);
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

export function SubAgentDetail({ selected }) {
  if (!selected) {
    return (
      <div className="subagent-detail">
        <Placeholder title="请选择子代理" detail="从左侧列表中选择一个子代理查看详情。" />
      </div>
    );
  }

  return (
    <div className="subagent-detail">
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
    </div>
  );
}
