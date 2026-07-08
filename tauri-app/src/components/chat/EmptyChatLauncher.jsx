import { ChatComposer } from './ChatComposer.jsx';

export function EmptyChatLauncher({
  accessMode,
  canSend,
  contextUsage,
  onAccessModeChange,
  onProviderProfileChange,
  onSendTask,
  onTaskTextChange,
  providerProfileId,
  providerProfiles,
  taskText,
  workspace,
}) {
  return (
    <div className="empty-chat-launcher">
      <h1>今天要完成什么？</h1>
      <ChatComposer
        accessMode={accessMode}
        canSend={canSend}
        className="empty-chat-composer"
        contextUsage={contextUsage}
        isRunning={false}
        onAccessModeChange={onAccessModeChange}
        onCancelRun={() => {}}
        onProviderProfileChange={onProviderProfileChange}
        onSendTask={onSendTask}
        onTaskTextChange={onTaskTextChange}
        providerProfileId={providerProfileId}
        providerProfiles={providerProfiles}
        taskText={taskText}
        workspace={workspace}
      />
    </div>
  );
}
