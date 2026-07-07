import { useMemo } from 'react';
import { deriveTaskProgress, isTaskProgressMessage } from '../utils/taskProgress.js';
import { ChatComposer } from './chat/ChatComposer.jsx';
import { ChatConversation } from './chat/ChatConversation.jsx';
import { buildConversationItems, RunStatusRow } from './ConversationActivity.jsx';
import { TaskProgressPanel } from './TaskProgressPanel.jsx';
import { classNames } from '../utils/format.js';

export function ChatPanel({
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
  pendingPermissions,
  onTaskTextChange,
  onProviderProfileChange,
  onAccessModeChange,
  onResolvePermission,
  onSendTask,
  onCancelRun,
}) {
  const taskProgress = useMemo(() => deriveTaskProgress(messages, isRunning), [messages, isRunning]);
  const chatMessages = useMemo(
    () => messages.filter((message) => !isTaskProgressMessage(message)),
    [messages],
  );
  const conversationItems = useMemo(() => buildConversationItems(chatMessages), [chatMessages]);
  const hasConversationContent =
    conversationItems.length > 0 || taskProgress.hasProgress || pendingPermissions.length > 0;

  return (
    <section className={classNames('center-panel', hasConversationContent ? 'chat-active-state' : 'chat-initial-state')}>
      <ChatConversation
        chatMessages={chatMessages}
        conversationItems={conversationItems}
        messageEndRef={messageEndRef}
        pendingPermissions={pendingPermissions}
        showEmpty={!hasConversationContent}
        showTimeline={hasConversationContent}
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
