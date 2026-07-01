import { Bot, Circle, Code2, FileCode2, GitCompare, Play, ShieldAlert, Square } from 'lucide-react';
import { classNames } from '../utils/format.js';
import { MessageBubble } from './MessageBubble.jsx';

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
  activeContext,
  pendingPermissionCount,
  onTaskTextChange,
  onSendTask,
  onCancelRun,
  onOpenContext,
}) {
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
          <button className={classNames('icon-button compact permission-trigger', activeContext === 'permissions' && 'active')} onClick={() => onOpenContext('permissions')} title="权限浮窗" type="button">
            <ShieldAlert size={14} />
            {pendingPermissionCount > 0 && <span>{pendingPermissionCount}</span>}
          </button>
        </div>
      </div>

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
        <div ref={messageEndRef} />
      </div>

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
          placeholder={workspace ? '给 Night24 发消息...' : '请先打开项目'}
          disabled={isRunning}
        />
        <div className="composer-actions">
          <span>{provider} · {model || 'default'}</span>
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
