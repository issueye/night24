import { MessageSquareText } from 'lucide-react';
import { classNames, formatTime } from '../../utils/format.js';
import { Button } from '../ui/index.js';
import { compactSubAgentText, subAgentStatusMeta } from './status.js';

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
        const meta = subAgentStatusMeta(agent.status);
        return (
          <Button
            className={classNames('subagent-row', agent.id === selectedId && 'active', meta.tone)}
            key={agent.id}
            onClick={() => onSelect(agent.id)}
            variant="ghost"
          >
            <div>
              <span className={classNames('subagent-dot', meta.tone)} />
              <strong>{agent.name || 'subagent'}</strong>
              <em>{agent.mode || 'async'}</em>
            </div>
            <p>{compactSubAgentText(agent.task || agent.result_preview || agent.id)}</p>
            <footer>
              <span>{formatTime(agent.updated_at)}</span>
              <span><MessageSquareText size={12} />{agent.message_count || 0}</span>
            </footer>
          </Button>
        );
      })}
    </div>
  );
}
