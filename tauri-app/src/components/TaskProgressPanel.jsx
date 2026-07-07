import { CheckCircle2, ChevronDown, Circle, ClipboardList, FileCheck2, Loader2 } from 'lucide-react';
import { classNames } from '../utils/format.js';
import { Markdown, ProgressBar, Tag } from './ui/index.js';

function progressPercent(progress) {
  if (!progress.total) return 0;
  return Math.round((progress.completed / progress.total) * 100);
}

function progressTone(progress) {
  if (progress.isComplete || progress.report) return 'success';
  if (progress.isRunning) return 'warning';
  return 'neutral';
}

export function TaskProgressPanel({ progress }) {
  if (!progress?.hasProgress) return null;

  const percent = progressPercent(progress);
  const tone = progressTone(progress);
  const statusText = progress.report
    ? '已生成报告'
    : progress.isRunning
      ? '执行中'
      : '待继续';

  return (
    <details className="conversation-activity-row task-progress-panel" open={progress.isRunning && !progress.report}>
      <summary className="task-progress-head">
        <div>
          <ClipboardList size={16} />
          <strong>任务列表</strong>
          {progress.total > 0 && <span>{progress.completed}/{progress.total}</span>}
        </div>
        <div className="task-progress-actions">
          <Tag tone={tone} size="sm" icon={progress.isRunning && !progress.report ? <Loader2 size={13} /> : <FileCheck2 size={13} />}>
            {statusText}
          </Tag>
          <ChevronDown className="activity-chevron" size={14} />
        </div>
      </summary>

      <div className="activity-detail task-progress-detail">
        {progress.total > 0 && (
          <ProgressBar
            label={`任务进度 ${percent}%`}
            percent={percent}
            size="sm"
            tone={tone === 'warning' ? 'warning' : 'neutral'}
          />
        )}

        {progress.tasks.length > 0 && (
          <div className="task-progress-scroll">
            <ol className="task-progress-list">
              {progress.tasks.map((task, index) => (
                <li className={classNames(task.completed && 'completed')} key={task.id || `${index}-${task.title}`}>
                  <span className="task-progress-icon" aria-hidden="true">
                    {task.completed ? <CheckCircle2 size={15} /> : <Circle size={15} />}
                  </span>
                  <span>{task.title}</span>
                </li>
              ))}
            </ol>
          </div>
        )}

        {progress.report && (
          <div className="task-progress-report">
            <div>
              <FileCheck2 size={15} />
              <strong>完成报告</strong>
            </div>
            <Markdown text={progress.report} size="sm" />
          </div>
        )}
      </div>
    </details>
  );
}
