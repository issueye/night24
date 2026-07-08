import { Circle, Loader2, MessageSquarePlus, X } from 'lucide-react';
import { classNames } from '../utils/format.js';
import { IconButton, Tab, Tabs } from './ui/index.js';

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
  onSelectSession,
  onCloseSession,
  onNewSession,
}) {
  return (
    <Tabs
      ariaLabel="会话页签"
      className="chat-tabs-bar"
      listClassName="chat-tabs-list"
      empty={openSessions.length === 0 ? <div className="chat-tabs-empty">未打开会话</div> : null}
      trailing={(
        <div className="chat-tabs-trailing">
          <div className="chat-tabs-server" title={serverDetail}>
            <Circle size={8} fill="currentColor" />
            <span>{serverDetail}</span>
          </div>
        </div>
      )}
    >
        {openSessions.map((session) => {
          const isActive = session.id === activeSessionId;
          const isRunning = runningSessionIds.has(session.id);
          return (
            <div
              className={classNames('chat-tab-shell', isActive && 'active', isRunning && 'running')}
              key={session.id}
              title={session.name || session.id}
            >
              <Tab
                active={isActive}
                className="chat-tab"
                onSelect={() => onSelectSession(session.id)}
                title={session.name || session.id}
              >
                {isRunning ? (
                  <Loader2 className="chat-tab-spinner" size={12} />
                ) : (
                  <span className="chat-tab-dot" />
                )}
                <span className="chat-tab-label">{compactLabel(session.name || session.id)}</span>
              </Tab>
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
    </Tabs>
  );
}
