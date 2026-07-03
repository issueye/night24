import { useEffect, useRef, useState } from 'react';
import { ArrowDown, Bot, Circle, Code2, FileCode2, GitCompare } from 'lucide-react';
import { classNames } from '../utils/format.js';
import { ChatComposer } from './chat/ChatComposer.jsx';
import { ConversationTimeline } from './chat/ConversationTimeline.jsx';
import { MessageBubble } from './MessageBubble.jsx';
import { PermissionRequestCard } from './PermissionRequestCard.jsx';

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
  }, [messages.length, pendingPermissions.length]);

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
          <button className={classNames('icon-button compact', activeContext === 'files' && 'active')} onClick={() => onOpenContext('files')} title="文件浮窗" type="button"><FileCode2 size={14} /></button>
          <button className={classNames('icon-button compact', activeContext === 'diff' && 'active')} onClick={() => onOpenContext('diff')} title="变更浮窗" type="button"><GitCompare size={14} /></button>
          <button className={classNames('icon-button compact', activeContext === 'preview' && 'active')} onClick={() => onOpenContext('preview')} title="预览浮窗" type="button"><Code2 size={14} /></button>
        </div>
      </div>

      <div className="conversation-area" onScroll={updateScrollButton} ref={scrollRef}>
        <ConversationTimeline messages={messages} onJump={scrollToTarget} />

        <div className="messages">
          {messages.length === 0 ? (
            <div className="welcome-panel">
              <Bot size={30} />
              <strong>开始一个编程任务</strong>
              <span>打开项目后，像聊天一样描述要修改、解释或检查的内容。</span>
            </div>
          ) : messages.map((message, index) => (
            <div
              className={classNames('message-anchor', highlightedTarget === `message-${message.id || index}` && 'highlighted')}
              key={message.id || index}
              ref={setTargetRef(`message-${message.id || index}`)}
            >
              <MessageBubble message={message} />
            </div>
          ))}
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
        <button className="scroll-bottom-button" onClick={scrollToBottom} type="button" title="回到底部">
          <ArrowDown size={16} />
        </button>
      )}

      <ChatComposer
        taskText={taskText}
        isRunning={isRunning}
        canSend={canSend}
        workspace={workspace}
        providerProfiles={providerProfiles}
        providerProfileId={providerProfileId}
        accessMode={accessMode}
        onTaskTextChange={onTaskTextChange}
        onProviderProfileChange={onProviderProfileChange}
        onAccessModeChange={onAccessModeChange}
        onSendTask={onSendTask}
        onCancelRun={onCancelRun}
      />
    </section>
  );
}
