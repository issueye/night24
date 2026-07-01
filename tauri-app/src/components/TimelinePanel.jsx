import { Activity, AlertTriangle, CheckCircle2, Circle, Radio, TimerReset, X } from 'lucide-react';
import { classNames, formatTime } from '../utils/format.js';

export function TimelinePanel({ timeline, activeRun, open, onToggle, onClose }) {
  const latest = timeline[0];
  const runStatus = activeRun?.status || 'idle';

  return (
    <>
      {open && (
        <aside className="events-float">
          <div className="float-head">
            <strong>事件</strong>
            <button className="icon-button compact" onClick={onClose} title="关闭事件" type="button"><X size={14} /></button>
          </div>
          <div className="event-list">
            {timeline.length === 0 ? (
              <div className="empty-block">暂无执行事件</div>
            ) : timeline.map((item) => (
              <div className={classNames('event-row', item.tone)} key={item.id}>
                {item.tone === 'success' ? <CheckCircle2 size={14} /> : item.tone === 'warning' || item.tone === 'error' ? <AlertTriangle size={14} /> : <Circle size={10} />}
                <div>
                  <strong>{item.title}</strong>
                  <span>{item.detail}</span>
                </div>
                <time>{formatTime(item.createdAt)}</time>
              </div>
            ))}
          </div>
        </aside>
      )}

      <footer className="statusbar">
        <div className={classNames('statusbar-state', runStatus)}>
          <Radio size={13} />
          <span>{runStatus === 'running' ? '运行中' : runStatus === 'idle' ? '就绪' : runStatus}</span>
        </div>

        <button className={classNames('statusbar-event-button', open && 'active', latest?.tone)} onClick={onToggle} type="button">
          <Activity size={13} />
          <span>{latest ? latest.title : '无事件'}</span>
        </button>

        <div className="statusbar-meta">
          <span>{timeline.length} 条事件</span>
          <span><TimerReset size={12} />{latest ? formatTime(latest.createdAt) : '--:--'}</span>
        </div>
      </footer>
    </>
  );
}
