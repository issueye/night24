import { useMemo } from 'react';
import { Bot, Circle, Code2, FileCode2, GitCompare } from 'lucide-react';
import { classNames } from '../utils/format.js';
import { deriveTaskProgress, isTaskProgressMessage } from '../utils/taskProgress.js';
import { ChatComposer } from './chat/ChatComposer.jsx';
import { ChatConversation } from './chat/ChatConversation.jsx';
import { buildConversationItems, RunStatusRow } from './ConversationActivity.jsx';
import { TaskProgressPanel } from './TaskProgressPanel.jsx';
import { IconButton } from './ui/index.js';

export function ChatPanel({
  title,
  serverDetail,
  messages,
  messageEndRef,
  taskText,
  isRunning,
  canSend,
  workspace,
  providerProfiles,
  providerProfileId,
  accessMode,
  contextUsage,
  activeContext,
  pendingPermissions,
  onTaskTextChange,
  onProviderProfileChange,
  onAccessModeChange,
  onResolvePermission,
  onSendTask,
  onCancelRun,
  onOpenContext,
}) {
  const taskProgress = useMemo(() => deriveTaskProgress(messages, isRunning), [messages, isRunning]);
  const chatMessages = useMemo(
    () => messages.filter((message) => !isTaskProgressMessage(message)),
    [messages],
  );
  const conversationItems = useMemo(() => buildConversationItems(chatMessages), [chatMessages]);

  return (
    <section className="center-panel">
      <div className="conversation-head">
        <div>
          <span>Chat</span>
          <strong>{title}</strong>
        </div>
        <div className="core-note" title={serverDetail}>
          <Circle size={8} fill="currentColor" />
          {serverDetail}
        </div>
        <div className="context-actions">
          <IconButton className={classNames('icon-button compact', activeContext === 'files' && 'active')} label="文件浮窗" onClick={() => onOpenContext('files')} size="sm"><FileCode2 size={14} /></IconButton>
          <IconButton className={classNames('icon-button compact', activeContext === 'diff' && 'active')} label="变更浮窗" onClick={() => onOpenContext('diff')} size="sm"><GitCompare size={14} /></IconButton>
          <IconButton className={classNames('icon-button compact', activeContext === 'preview' && 'active')} label="预览浮窗" onClick={() => onOpenContext('preview')} size="sm"><Code2 size={14} /></IconButton>
          <IconButton className={classNames('icon-button compact', activeContext === 'agents' && 'active')} label="子代理浮窗" onClick={() => onOpenContext('agents')} size="sm"><Bot size={14} /></IconButton>
        </div>
      </div>

      <ChatConversation
        chatMessages={chatMessages}
        conversationItems={conversationItems}
        messageEndRef={messageEndRef}
        pendingPermissions={pendingPermissions}
        showEmpty={conversationItems.length === 0 && !taskProgress.hasProgress}
        showTimeline
        onResolvePermission={onResolvePermission}
      />

      <div className="conversation-bottom-activity">
        <TaskProgressPanel progress={taskProgress} />
        <RunStatusRow isRunning={isRunning} />
      </div>
      <ChatComposer
        taskText={taskText}
        isRunning={isRunning}
        canSend={canSend}
        workspace={workspace}
        providerProfiles={providerProfiles}
        providerProfileId={providerProfileId}
        accessMode={accessMode}
        contextUsage={contextUsage}
        onTaskTextChange={onTaskTextChange}
        onProviderProfileChange={onProviderProfileChange}
        onAccessModeChange={onAccessModeChange}
        onSendTask={onSendTask}
        onCancelRun={onCancelRun}
      />
    </section>
  );
}
