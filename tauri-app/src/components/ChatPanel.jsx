import { useEffect, useMemo, useRef, useState } from 'react';
import { ArrowDown, Bot, Circle, Code2, FileCode2, GitCompare } from 'lucide-react';
import { classNames } from '../utils/format.js';
import { deriveTaskProgress, isTaskProgressMessage } from '../utils/taskProgress.js';
import { ChatComposer } from './chat/ChatComposer.jsx';
import { ConversationTimeline } from './chat/ConversationTimeline.jsx';
import { buildConversationItems, RunStatusRow, ToolActivityRow } from './ConversationActivity.jsx';
import { MessageBubble } from './MessageBubble.jsx';
import { PermissionRequestCard } from './PermissionRequestCard.jsx';
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
  const scrollRef = useRef(null);
  const targetRefs = useRef(new Map());
  const highlightTimerRef = useRef(null);
  const [showScrollBottom, setShowScrollBottom] = useState(false);
  const [highlightedTarget, setHighlightedTarget] = useState('');
  const taskProgress = useMemo(() => deriveTaskProgress(messages, isRunning), [messages, isRunning]);
  const chatMessages = useMemo(
    () => messages.filter((message) => !isTaskProgressMessage(message)),
    [messages],
  );
  const conversationItems = useMemo(() => buildConversationItems(chatMessages), [chatMessages]);

  function updateScrollButton() {
    const node = scrollRef.current;
    if (!node) return;
    const distance = node.scrollHeight - node.scrollTop - node.clientHeight;
    setShowScrollBottom(distance > 180);
  }

  function scrollToBottom() {
    messageEndRef.current?.scrollIntoView({ block: 'end', behavior: 'smooth' });
  }

  function setTargetRef(id) {
    return (node) => {
      if (!id) return;
      if (node) targetRefs.current.set(id, node);
      else targetRefs.current.delete(id);
    };
  }

  function scrollToTarget(id) {
    const node = targetRefs.current.get(id);
    if (!node) return;
    node.scrollIntoView({ block: 'center', behavior: 'smooth' });
    setHighlightedTarget(id);
    window.clearTimeout(highlightTimerRef.current);
    highlightTimerRef.current = window.setTimeout(() => {
      setHighlightedTarget('');
    }, 1400);
  }

  useEffect(() => {
    updateScrollButton();
  }, [conversationItems.length, pendingPermissions.length, taskProgress.hasProgress, taskProgress.updatedAt]);

  useEffect(() => () => window.clearTimeout(highlightTimerRef.current), []);

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

      <div className="conversation-area" onScroll={updateScrollButton} ref={scrollRef}>
        <ConversationTimeline messages={chatMessages} onJump={scrollToTarget} />

        <div className="messages">
          {conversationItems.length === 0 && !taskProgress.hasProgress ? (
            <div className="welcome-panel">
              <Bot size={30} />
              <strong>开始一个编程任务</strong>
              <span>打开项目后，像聊天一样描述要修改、解释或检查的内容。</span>
            </div>
          ) : conversationItems.map((item, index) => {
            if (item.type === 'tool_group') {
              return <ToolActivityRow key={item.id || `tools-${index}`} messages={item.messages} />;
            }

            const message = item.message;
            return (
              <div
                className={classNames('message-anchor', highlightedTarget === `message-${message.id || index}` && 'highlighted')}
                key={message.id || index}
                ref={setTargetRef(`message-${message.id || index}`)}
              >
                <MessageBubble message={message} />
              </div>
            );
          })}
          {pendingPermissions.map((permission) => (
            <div
              className={classNames('message-anchor', highlightedTarget === `permission-${permission.permission_id}` && 'highlighted')}
              key={permission.permission_id}
              ref={setTargetRef(`permission-${permission.permission_id}`)}
            >
              <PermissionRequestCard
                permission={permission}
                onResolve={onResolvePermission}
              />
            </div>
          ))}
          <div ref={messageEndRef} />
        </div>
      </div>
      {showScrollBottom && (
        <IconButton className="scroll-bottom-button" label="回到底部" onClick={scrollToBottom}>
          <ArrowDown size={16} />
        </IconButton>
      )}

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
