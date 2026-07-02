import { useEffect, useRef, useState } from 'react';
import { ArrowDown, Bot, Circle, Code2, FileCode2, GitCompare, Play, Square } from 'lucide-react';
import { classNames } from '../utils/format.js';
import { MessageBubble } from './MessageBubble.jsx';
import { PermissionRequestCard } from './PermissionRequestCard.jsx';

const ACCESS_LABELS = {
  strict: '确认访问',
  permissive: '宽松访问',
  allow_all: '完全访问',
};

function formatTimelineTime(value) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function timelineLabel(message) {
  const role = String(message.role || 'assistant').toLowerCase();
  if (message.tone === 'error') return '错误';
  if (role === 'user') return '用户';
  return '回复';
}

export function ChatPanel({
  title,
  serverDetail,
  messages,
  messageEndRef,
  taskText,
  isRunning,
  canSend,
  workspace,
  provider,
  model,
  accessMode,
  activeContext,
  pendingPermissions,
  onTaskTextChange,
  onAccessModeChange,
  onResolvePermission,
  onSendTask,
  onCancelRun,
  onOpenContext,
}) {
  const scrollRef = useRef(null);
  const [showScrollBottom, setShowScrollBottom] = useState(false);

  function updateScrollButton() {
    const node = scrollRef.current;
    if (!node) return;
    const distance = node.scrollHeight - node.scrollTop - node.clientHeight;
    setShowScrollBottom(distance > 180);
  }

  function scrollToBottom() {
    messageEndRef.current?.scrollIntoView({ block: 'end', behavior: 'smooth' });
  }

  useEffect(() => {
    updateScrollButton();
  }, [messages.length, pendingPermissions.length]);

  const timelineItems = [
    ...messages.map((message, index) => ({
      id: message.id || `message-${index}`,
      tone: message.tone,
      role: String(message.role || 'assistant').toLowerCase(),
      label: timelineLabel(message),
      time: formatTimelineTime(message.created_at || message.createdAt),
    })),
    ...pendingPermissions.map((permission) => ({
      id: permission.permission_id,
      tone: 'permission',
      role: 'permission',
      label: '权限',
      time: '',
    })),
  ];

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
        <aside className="conversation-timeline" aria-label="对话时间轴">
          <div className="timeline-rail" />
          {timelineItems.map((item, index) => (
            <div
              className={classNames('timeline-point', item.role, item.tone, index === timelineItems.length - 1 && 'active')}
              key={item.id}
              title={`${item.label}${item.time ? ` · ${item.time}` : ''}`}
            >
              <span />
              <small>{item.time}</small>
            </div>
          ))}
        </aside>

        <div className="messages">
          {messages.length === 0 ? (
            <div className="welcome-panel">
              <Bot size={30} />
              <strong>开始一个编程任务</strong>
              <span>打开项目后，像聊天一样描述要修改、解释或检查的内容。</span>
            </div>
          ) : messages.map((message, index) => (
            <MessageBubble key={message.id || index} message={message} />
          ))}
          {pendingPermissions.map((permission) => (
            <PermissionRequestCard
              key={permission.permission_id}
              permission={permission}
              onResolve={onResolvePermission}
            />
          ))}
          <div ref={messageEndRef} />
        </div>
      </div>
      {showScrollBottom && (
        <button className="scroll-bottom-button" onClick={scrollToBottom} type="button" title="回到底部">
          <ArrowDown size={16} />
        </button>
      )}

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
          <span>{provider} · {model || 'default'}</span>
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
    </section>
  );
}
