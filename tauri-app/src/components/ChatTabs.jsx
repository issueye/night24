import { Bot, Circle, Loader2, MessageSquarePlus, X } from 'lucide-react';
import { classNames } from '../utils/format.js';
import { IconButton } from './ui/index.js';

const MAX_TAB_LABEL = 24;

function compactLabel(text, max = MAX_TAB_LABEL) {
  const value = String(text || '').trim();
  if (value.length <= max) return value;
  return `${value.slice(0, max - 1)}…`;
}

export function ChatTabs({
  openSessions,
  activeSessionId,
  runningSessionIds,
  serverDetail,
  agentsActive,
  onSelectSession,
  onCloseSession,
  onNewSession,
  onToggleAgents,
}) {
  return (
    <div className="chat-tabs-bar" role="tablist">
      <div className="chat-tabs-list">
        {openSessions.length === 0 && (
          <div className="chat-tabs-empty">未打开会话</div>
        )}
        {openSessions.map((session) => {
          const isActive = session.id === activeSessionId;
          const isRunning = runningSessionIds.has(session.id);
          return (
            <div
              className={classNames('chat-tab', isActive && 'active', isRunning && 'running')}
              key={session.id}
              onClick={() => onSelectSession(session.id)}
              role="tab"
              aria-selected={isActive}
              title={session.name || session.id}
            >
              {isRunning ? (
                <Loader2 className="chat-tab-spinner" size={12} />
              ) : (
                <span className="chat-tab-dot" />
              )}
              <span className="chat-tab-label">{compactLabel(session.name || session.id)}</span>
              <button
                className="chat-tab-close"
                aria-label="关闭页签"
                onClick={(event) => {
                  event.stopPropagation();
                  onCloseSession(session.id);
                }}
                title="关闭页签"
                type="button"
              >
                <X size={12} />
              </button>
            </div>
          );
        })}
        <IconButton
          className="chat-tab-new"
          label="新建会话"
          onClick={onNewSession}
          size="sm"
        >
          <MessageSquarePlus size={14} />
        </IconButton>
      </div>
      <div className="chat-tabs-trailing">
        <div className="chat-tabs-server" title={serverDetail}>
          <Circle size={8} fill="currentColor" />
          <span>{serverDetail}</span>
        </div>
        <IconButton
          className={classNames('icon-button compact', agentsActive && 'active')}
          label="子代理浮窗"
          onClick={onToggleAgents}
          size="sm"
        >
          <Bot size={14} />
        </IconButton>
      </div>
    </div>
  );
}
