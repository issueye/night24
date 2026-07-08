import { ArrowDown, Bot } from 'lucide-react';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { classNames } from '../../utils/format.js';
import { buildConversationItems, ToolActivityRow } from '../ConversationActivity.jsx';
import { MessageBubble } from '../MessageBubble.jsx';
import { PermissionRequestCard } from '../PermissionRequestCard.jsx';
import { IconButton } from '../ui/index.js';
import { ConversationTimeline } from './ConversationTimeline.jsx';

export function ChatConversation({
  chatMessages,
  conversationItems = buildConversationItems(chatMessages || []),
  empty = null,
  messageEndRef,
  pendingPermissions = [],
  showTimeline = false,
  showEmpty = false,
  onResolvePermission,
}) {
  const scrollRef = useRef(null);
  const targetRefs = useRef(new Map());
  const highlightTimerRef = useRef(null);
  const scrollFrameRef = useRef(0);
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

  function scrollToBottomNow() {
    const node = scrollRef.current;
    if (!node) return;
    node.scrollTop = node.scrollHeight;
    updateScrollButton();
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

  useLayoutEffect(() => {
    window.cancelAnimationFrame(scrollFrameRef.current);
    scrollToBottomNow();
    scrollFrameRef.current = window.requestAnimationFrame(scrollToBottomNow);
    return () => window.cancelAnimationFrame(scrollFrameRef.current);
  }, [conversationItems, pendingPermissions.length, showEmpty]);

  useEffect(() => {
    updateScrollButton();
  }, [conversationItems.length, pendingPermissions.length, showEmpty]);

  useEffect(() => () => {
    window.clearTimeout(highlightTimerRef.current);
    window.cancelAnimationFrame(scrollFrameRef.current);
  }, []);

  return (
    <>
      <div
        className={classNames('conversation-area', !showTimeline && 'no-timeline')}
        onScroll={updateScrollButton}
        ref={scrollRef}
      >
        {showTimeline && <ConversationTimeline messages={chatMessages} onJump={scrollToTarget} />}

        <div className="messages">
          {showEmpty ? (
            empty || (
              <div className="welcome-panel">
                <Bot size={30} />
                <strong>开始一个编程任务</strong>
                <span>打开项目后，像聊天一样描述要修改、解释或检查的内容。</span>
              </div>
            )
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
    </>
  );
}
