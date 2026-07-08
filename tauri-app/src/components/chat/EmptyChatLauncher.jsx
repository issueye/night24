import { FolderOpen, Play } from 'lucide-react';
import { providerDisplayName } from '../../utils/settings.js';
import { Button, ChatInput, Select } from '../ui/index.js';

export function EmptyChatLauncher({
  accessMode,
  canSend,
  onAccessModeChange,
  onOpenWorkspace,
  onProviderProfileChange,
  onSendTask,
  onTaskTextChange,
  providerProfileId,
  providerProfiles,
  taskText,
  workspace,
  workspaceLoading,
}) {
  const providerOptions = providerProfiles.map((item) => ({
    label: `${item.name || providerDisplayName(item.provider)} · ${providerDisplayName(item.provider)} · ${item.model || 'default'}`,
    value: item.id,
  }));
  const accessOptions = [
    { label: '确认访问', value: 'strict' },
    { label: '宽松访问', value: 'permissive' },
    { label: '完全访问', value: 'allow_all' },
  ];
  const hasWorkspace = Boolean(workspace);

  function submit() {
    if (!hasWorkspace) {
      onOpenWorkspace?.();
      return;
    }
    if (canSend) {
      onSendTask?.();
    }
  }

  return (
    <div className="empty-chat-launcher">
      <h1>今天要完成什么？</h1>
      <div className="empty-chat-box">
        <ChatInput
          className="empty-chat-input"
          onChange={onTaskTextChange}
          onSubmit={submit}
          placeholder={hasWorkspace ? '给 red_panda 发消息...' : '先选择工作区目录...'}
          value={taskText}
        />
        <div className="empty-chat-actions">
          <Button
            className="empty-workspace-button"
            disabled={workspaceLoading}
            icon={<FolderOpen size={14} />}
            onClick={onOpenWorkspace}
            title={workspace?.root_path || '选择工作区目录'}
            variant="ghost"
          >
            {workspace ? workspace.name || workspace.root_path : '选择工作区'}
          </Button>
          <Select
            className="empty-chat-model"
            menuClassName="composer-select-menu"
            onChange={onProviderProfileChange}
            options={providerOptions}
            title="选择本次对话使用的供应商和模型"
            value={providerProfileId}
          />
          <Select
            className="empty-chat-mode"
            menuClassName="composer-select-menu"
            onChange={onAccessModeChange}
            options={accessOptions}
            title="选择本次任务的访问权限模式"
            value={accessMode}
          />
          <Button
            className="empty-send-button"
            disabled={hasWorkspace && !canSend}
            icon={hasWorkspace ? <Play size={15} /> : <FolderOpen size={15} />}
            onClick={submit}
            tone={hasWorkspace ? 'primary' : 'neutral'}
            title={hasWorkspace ? '发送' : '选择工作区目录'}
          >
            {hasWorkspace ? '发送' : '选择目录'}
          </Button>
        </div>
      </div>
    </div>
  );
}
