import { classNames } from '../../utils/format.js';
import { Button } from '../ui/index.js';

function formatTimelineTime(value) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function timelineLabel(message) {
  const role = String(message.role || 'assistant').toLowerCase();
  if (message.tone === 'error') return '错误';
  if (role === 'user') return '用户';
  return '回复';
}

export function ConversationTimeline({ messages, onJump }) {
  const timelineItems = messages.flatMap((message, index) => {
    const role = String(message.role || 'assistant').toLowerCase();
    if (role !== 'user') return [];
    return [{
      id: message.id || `message-${index}`,
      targetId: `message-${message.id || index}`,
      tone: message.tone,
      role,
      label: timelineLabel(message),
      time: formatTimelineTime(message.created_at || message.createdAt),
    }];
  });

  return (
    <aside className="conversation-timeline" aria-label="对话时间轴">
      <div className="timeline-rail" />
      {timelineItems.map((item, index) => (
        <Button
          className={classNames('timeline-point', item.role, item.tone, index === timelineItems.length - 1 && 'active')}
          key={item.id}
          onClick={() => onJump(item.targetId)}
          title={`${item.label}${item.time ? ` · ${item.time}` : ''}`}
          variant="ghost"
        >
          <span />
          <small>{item.time}</small>
        </Button>
      ))}
    </aside>
  );
}
