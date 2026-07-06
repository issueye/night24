import { Ban, CheckCircle2, Circle, Clock3, Loader2, MessageSquareText, XCircle } from 'lucide-react';
import { classNames, formatTime } from '../../utils/format.js';

const STATUS_META = {
  queued: { tone: 'queued', icon: Clock3 },
  running: { tone: 'running', icon: Loader2 },
  completed: { tone: 'completed', icon: CheckCircle2 },
  failed: { tone: 'failed', icon: XCircle },
  cancelled: { tone: 'cancelled', icon: Ban },
};

function statusMeta(status) {
  return STATUS_META[status] || { tone: 'unknown', icon: Circle };
}

function compactText(value, max = 120) {
  const text = String(value || '').replace(/\s+/g, ' ').trim();
  if (text.length <= max) return text;
  return `${text.slice(0, max - 3)}...`;
}

export function SubAgentList({
  agents,
  loading,
  selectedId,
  onSelect,
}) {
  return (
    <div className="subagent-list">
      {loading && agents.length === 0 ? (
        Array.from({ length: 4 }).map((_, index) => <div className="subagent-skeleton" key={index} />)
      ) : agents.map((agent) => {
        const meta = statusMeta(agent.status);
        return (
          <button
            className={classNames('subagent-row', agent.id === selectedId && 'active', meta.tone)}
            key={agent.id}
            onClick={() => onSelect(agent.id)}
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
  );
}
