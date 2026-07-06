import { Activity, Radio, TimerReset, X } from 'lucide-react';
import { classNames, formatTime } from '../utils/format.js';
import { Button, IconButton, Timeline } from './ui/index.js';

export function TimelinePanel({ timeline, activeRun, open, onToggle, onClose }) {
  const latest = timeline[0];
  const runStatus = activeRun?.status || 'idle';
  const runLabel =
    runStatus === 'running' ? '运行中' :
    runStatus === 'idle' ? '就绪' :
    runStatus === 'completed' ? '已完成' :
    runStatus === 'finished' ? '已完成' :
    runStatus === 'cancelled' ? '已取消' :
    runStatus === 'cancelling' ? '正在取消' :
    runStatus === 'interrupted' ? '已中断' :
    runStatus === 'error' || runStatus === 'failed' ? '出错' :
    runStatus;

  return (
    <>
      {open && (
        <aside className="events-float">
          <div className="float-head">
            <strong>事件</strong>
            <IconButton className="icon-button compact" label="关闭事件" onClick={onClose}><X size={14} /></IconButton>
          </div>
          <div className="event-list">
            <Timeline
              empty="暂无执行事件"
              items={timeline.map((item) => ({
                ...item,
                time: formatTime(item.createdAt),
              }))}
            />
          </div>
        </aside>
      )}

      <footer className="statusbar">
        <div className={classNames('statusbar-state', runStatus)}>
          <Radio size={13} />
          <span>{runLabel}</span>
        </div>

        <Button className={classNames('statusbar-event-button', open && 'active', latest?.tone)} icon={<Activity size={13} />} onClick={onToggle} variant="ghost">
          {latest ? latest.title : '无事件'}
        </Button>

        <div className="statusbar-meta">
          <span>{timeline.length} 条事件</span>
          <span><TimerReset size={12} />{latest ? formatTime(latest.createdAt) : '--:--'}</span>
        </div>
      </footer>
    </>
  );
}
