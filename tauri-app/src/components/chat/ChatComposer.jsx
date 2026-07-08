import { Play, Square } from 'lucide-react';
import { providerDisplayName } from '../../utils/settings.js';
import { classNames } from '../../utils/format.js';
import { Button, ChatInput, IconButton, Popover, ProgressRing, Select } from '../ui/index.js';

export function ChatComposer({
  className,
  taskText,
  isRunning,
  canSend,
  workspace,
  providerProfiles,
  providerProfileId,
  accessMode,
  contextUsage,
  onTaskTextChange,
  onProviderProfileChange,
  onAccessModeChange,
  onSendTask,
  onCancelRun,
}) {
  const thresholdTone = contextUsage?.reached ? 'danger' : contextUsage?.warning ? 'warning' : 'neutral';
  const thresholdTitle = contextUsage?.threshold
    ? `上下文估算 ${contextUsage.estimatedTokens} / ${contextUsage.threshold} tokens`
    : '未设置上下文摘要阈值';
  const usedText = formatTokenCount(contextUsage?.estimatedTokens || 0);
  const thresholdText = contextUsage?.threshold ? formatTokenCount(contextUsage.threshold) : '--';
  const contextPercent = contextUsage?.percent ?? 0;
  const providerOptions = providerProfiles.map((item) => ({
    label: `${item.name || providerDisplayName(item.provider)} · ${providerDisplayName(item.provider)} · ${item.model || 'default'}`,
    value: item.id,
  }));
  const accessOptions = [
    { label: '确认访问', value: 'strict' },
    { label: '宽松访问', value: 'permissive' },
    { label: '完全访问', value: 'allow_all' },
  ];

  return (
    <div className={classNames('composer', className)}>
      <ChatInput
        value={taskText}
        onChange={onTaskTextChange}
        onSubmit={() => {
          if (canSend) onSendTask();
        }}
        placeholder={isRunning ? '正在执行当前任务...' : workspace ? '给 red_panda 发消息...' : '请先打开项目'}
        disabled={isRunning}
      />
      <div className="composer-actions">
        <Select
          className="composer-model"
          disabled={isRunning}
          menuClassName="composer-select-menu"
          onChange={onProviderProfileChange}
          options={providerOptions}
          title="选择本次对话使用的供应商和模型"
          value={providerProfileId}
        />
        <Select
          className="composer-mode"
          disabled={isRunning}
          menuClassName="composer-select-menu"
          onChange={onAccessModeChange}
          options={accessOptions}
          title="选择本次任务的访问权限模式"
          value={accessMode}
        />
        <Popover
          className="composer-context"
          content={(
            <span className="composer-context-popover-body">
              <span>Context window:</span>
              <strong>{contextPercent}% full</strong>
              <small>{usedText} / {thresholdText} tokens used</small>
            </span>
          )}
        >
          <IconButton className={`composer-context-ring ${thresholdTone}`} label={thresholdTitle}>
            <ProgressRing percent={contextPercent} tone={thresholdTone} />
          </IconButton>
        </Popover>
        {isRunning ? (
          <Button className="danger-button" icon={<Square size={15} />} onClick={onCancelRun} tone="danger">
            取消
          </Button>
        ) : (
          <Button className="primary-button" disabled={!canSend} icon={<Play size={15} />} onClick={onSendTask} tone="primary">
            发送
          </Button>
        )}
      </div>
    </div>
  );
}

function formatTokenCount(value) {
  const tokens = Number(value || 0);
  if (tokens >= 1000) {
    return `${Math.round(tokens / 1000)}k`;
  }
  return String(tokens);
}
