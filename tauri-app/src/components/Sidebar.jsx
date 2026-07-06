import { useEffect, useState } from 'react';
import { Bot, Clock3, MessageSquare, MessageSquarePlus, Settings2, Trash2 } from 'lucide-react';
import { classNames, formatTime } from '../utils/format.js';

export function Sidebar({
  workspace,
  recentWorkspaces,
  sessions,
  currentSessionId,
  settingsOpen,
  onOpenWorkspace,
  onCreateSession,
  onSelectSession,
  onDeleteSession,
  onToggleSettings,
}) {
  const [selectedRecentPath, setSelectedRecentPath] = useState('');

  useEffect(() => {
    setSelectedRecentPath('');
  }, [workspace?.root_path]);

  return (
    <aside className="left-panel menu-sidebar">
      <nav className="primary-nav" aria-label="主导航">
        <button className="nav-row active" type="button"><Bot size={15} />快速对话</button>
      </nav>

      <section className="menu-section project-block">
        <div className="menu-section-head">
          <span>项目</span>
          <button className="mini-button" onClick={() => onOpenWorkspace(selectedRecentPath || undefined)} type="button">打开</button>
        </div>
        {workspace ? (
          <div className="project-current">
            <strong title={workspace.name}>{workspace.name}</strong>
            <span title={workspace.root_path}>{workspace.root_path}</span>
          </div>
        ) : (
          <div className="empty-block">尚未打开项目</div>
        )}
        {recentWorkspaces.length > 0 && (
          <>
            <div className="menu-label"><Clock3 size={12} />最近</div>
            <div className="recent-list">
              {recentWorkspaces.slice(0, 4).map((item) => (
                <button
                  className={classNames(selectedRecentPath === item.root_path && 'selected')}
                  key={item.root_path}
                  onClick={() => setSelectedRecentPath(item.root_path)}
                  title={item.root_path}
                  type="button"
                >
                  {item.name}
                </button>
              ))}
            </div>
          </>
        )}
      </section>

      <section className="menu-section sessions-block">
        <div className="menu-section-head">
          <span><MessageSquare size={14} />对话</span>
          <button className="icon-button compact" onClick={onCreateSession} title="新建会话" type="button"><MessageSquarePlus size={14} /></button>
        </div>
        <div className="session-list">
          {sessions.length === 0 ? (
            <div className="empty-block">暂无会话</div>
          ) : sessions.map((session) => (
            <button
              className={classNames('session-row', session.id === currentSessionId && 'active')}
              key={session.id}
              onClick={() => onSelectSession(session.id)}
              type="button"
            >
              <span>{session.name || session.id}</span>
              <small>{formatTime(session.updated_at)}</small>
              <Trash2 size={13} onClick={(event) => onDeleteSession(session.id, event)} />
            </button>
          ))}
        </div>
      </section>

      <footer className="menu-footer">
        <button className={classNames('nav-row settings-row', settingsOpen && 'active')} onClick={onToggleSettings} type="button">
          <Settings2 size={15} />
          设置
        </button>
      </footer>
    </aside>
  );
}
