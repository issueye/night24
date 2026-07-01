import { Bot, Clock3, Files, MessageSquare, MessageSquarePlus, Plug, RefreshCw, Search, TimerReset, Trash2 } from 'lucide-react';
import { classNames, formatTime } from '../utils/format.js';
import { FileTree } from './FileTree.jsx';

export function Sidebar({
  workspace,
  recentWorkspaces,
  tree,
  selectedFile,
  sessions,
  currentSessionId,
  onOpenWorkspace,
  onRefreshWorkspace,
  onOpenFile,
  onCreateSession,
  onSelectSession,
  onDeleteSession,
}) {
  return (
    <aside className="left-panel menu-sidebar">
      <nav className="primary-nav" aria-label="主导航">
        <button className="nav-row active" type="button"><Bot size={15} />快速对话</button>
        <button className="nav-row" type="button"><Search size={15} />搜索</button>
        <button className="nav-row" type="button"><Plug size={15} />插件</button>
        <button className="nav-row" type="button"><TimerReset size={15} />自动化</button>
      </nav>

      <section className="menu-section project-block">
        <div className="menu-section-head">
          <span>项目</span>
          <button className="mini-button" onClick={() => onOpenWorkspace()} type="button">打开</button>
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
                <button key={item.root_path} onClick={() => onOpenWorkspace(item.root_path)} type="button">
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

      <section className="menu-section tree-block">
        <div className="menu-section-head">
          <span><Files size={14} />目录</span>
          <button className="icon-button compact" onClick={onRefreshWorkspace} title="刷新文件树" type="button"><RefreshCw size={14} /></button>
        </div>
        <div className="tree-scroll">
          <FileTree tree={tree} selectedPath={selectedFile?.path} onOpenFile={onOpenFile} />
        </div>
      </section>
    </aside>
  );
}
