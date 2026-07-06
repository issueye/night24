import { CheckCircle2, Circle, AlertTriangle } from 'lucide-react';
import { classNames } from '../../utils/format.js';

export function Timeline({ empty = '暂无记录', items = [], size = 'md' }) {
  if (!items.length) return <div className="empty-block">{empty}</div>;

  return (
    <div className={classNames('ui-timeline', `ui-timeline-${size}`)}>
      {items.map((item, index) => (
        <div className={classNames('ui-timeline-item', item.tone)} key={item.id || index}>
          <span className="ui-timeline-dot">
            {item.tone === 'success' ? (
              <CheckCircle2 size={14} />
            ) : item.tone === 'warning' || item.tone === 'error' ? (
              <AlertTriangle size={14} />
            ) : (
              <Circle size={10} />
            )}
          </span>
          <span className="ui-timeline-body">
            <strong>{item.title}</strong>
            {item.detail && <small>{item.detail}</small>}
          </span>
          {item.time && <time>{item.time}</time>}
        </div>
      ))}
    </div>
  );
}
