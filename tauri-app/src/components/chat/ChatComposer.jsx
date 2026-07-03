import { Play, Square } from 'lucide-react';

const ACCESS_LABELS = {
  strict: '确认访问',
  permissive: '宽松访问',
  allow_all: '完全访问',
};

export function ChatComposer({
  taskText,
  isRunning,
  canSend,
  workspace,
  providerProfiles,
  providerProfileId,
  accessMode,
  onTaskTextChange,
  onProviderProfileChange,
  onAccessModeChange,
  onSendTask,
  onCancelRun,
}) {
  return (
    <div className="composer">
      <textarea
        value={taskText}
        onChange={(event) => onTaskTextChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' && !event.shiftKey) {
            event.preventDefault();
            if (canSend) onSendTask();
          }
        }}
        placeholder={isRunning ? '正在执行当前任务...' : workspace ? '给 Night24 发消息...' : '请先打开项目'}
        disabled={isRunning}
      />
      <div className="composer-actions">
        <label className="composer-model" title="切换本次及后续任务使用的模型">
          <span>模型</span>
          <select
            value={providerProfileId}
            onChange={(event) => onProviderProfileChange(event.target.value)}
            disabled={isRunning}
          >
            {providerProfiles.map((item) => (
              <option key={item.id} value={item.id}>
                {(item.name || item.provider)} · {item.model || 'default'}
              </option>
            ))}
          </select>
        </label>
        <label className="composer-mode" title="选择本次及后续任务的工具访问模式">
          <select
            value={accessMode}
            onChange={(event) => onAccessModeChange(event.target.value)}
            disabled={isRunning}
          >
            <option value="strict">确认访问</option>
            <option value="permissive">宽松访问</option>
            <option value="allow_all">完全访问</option>
          </select>
          <small>{ACCESS_LABELS[accessMode] || '确认访问'}</small>
        </label>
        {isRunning ? (
          <button className="danger-button" onClick={onCancelRun} type="button"><Square size={15} />取消</button>
        ) : (
          <button className="primary-button" disabled={!canSend} onClick={onSendTask} type="button"><Play size={15} />发送</button>
        )}
      </div>
    </div>
  );
}
