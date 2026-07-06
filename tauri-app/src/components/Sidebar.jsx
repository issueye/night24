import { useEffect, useMemo, useState } from 'react';
import {
  CalendarClock,
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  Loader2,
  MessageSquare,
  MessageSquarePlus,
  Plug,
  Search,
  Settings2,
  Trash2,
} from 'lucide-react';
import { classNames, formatRelativeShort } from '../utils/format.js';
import { sameWorkspacePath } from '../utils/settings.js';

export function Sidebar({
  workspace,
  recentWorkspaces,
  sessions,
  runsById,
  activeRunBySession,
  currentSessionId,
  settingsOpen,
  onOpenWorkspace,
  onCreateSession,
  onSelectSession,
  onDeleteSession,
  onToggleSettings,
}) {
  const [projectOrder, setProjectOrder] = useState([]);
  const [expandedProjects, setExpandedProjects] = useState(() => new Set());

  const projectByPath = useMemo(() => {
    const byPath = new Map();
    for (const item of recentWorkspaces || []) {
      if (item?.root_path) {
        byPath.set(item.root_path, item);
      }
    }
    if (workspace?.root_path && !byPath.has(workspace.root_path)) {
      byPath.set(workspace.root_path, workspace);
    }
    return byPath;
  }, [recentWorkspaces, workspace]);

  useEffect(() => {
    const paths = Array.from(projectByPath.keys());
    setProjectOrder((items) => [
      ...items.filter((path) => projectByPath.has(path)),
      ...paths.filter((path) => !items.includes(path)),
    ]);
  }, [projectByPath]);

  useEffect(() => {
    if (!workspace?.root_path) return;
    setExpandedProjects((items) => {
      if (items.has(workspace.root_path)) return items;
      const next = new Set(items);
      next.add(workspace.root_path);
      return next;
    });
  }, [workspace?.root_path]);

  const projects = useMemo(
    () => projectOrder.map((path) => projectByPath.get(path)).filter(Boolean),
    [projectByPath, projectOrder],
  );

  const sessionsByProject = useMemo(() => {
    const grouped = new Map();
    for (const session of sessions || []) {
      for (const project of projects) {
        if (sameWorkspacePath(session.working_dir, project.root_path)) {
          const items = grouped.get(project.root_path) || [];
          items.push(session);
          grouped.set(project.root_path, items);
          break;
        }
      }
    }
    return grouped;
  }, [projects, sessions]);

  function toggleProject(path) {
    setExpandedProjects((items) => {
      const next = new Set(items);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  function openProject(item, isCurrentProject) {
    if (isCurrentProject) {
      toggleProject(item.root_path);
      return;
    }
    setExpandedProjects((items) => {
      const next = new Set(items);
      next.add(item.root_path);
      return next;
    });
    onOpenWorkspace(item.root_path);
  }

  async function selectProjectSession(item, isCurrentProject, sessionId) {
    if (!isCurrentProject) {
      await onOpenWorkspace(item.root_path);
    }
    onSelectSession(sessionId);
  }

  return (
    <aside className="left-panel menu-sidebar">
      <nav className="primary-nav" aria-label="主导航">
        <button className="nav-row" onClick={onCreateSession} type="button"><MessageSquarePlus size={15} />新对话</button>
        <button className="nav-row muted" type="button"><Search size={15} />搜索</button>
        <button className="nav-row muted" type="button"><CalendarClock size={15} />已安排</button>
        <button className="nav-row muted" type="button"><Plug size={15} />插件</button>
      </nav>

      <section className="menu-section project-block">
        <div className="menu-section-head">
          <span>项目</span>
          <button className="mini-button" onClick={() => onOpenWorkspace()} type="button">打开</button>
        </div>
        {projects.length === 0 ? (
          <div className="empty-block">尚未打开项目</div>
        ) : (
          <div className="project-session-tree">
            {projects.map((item) => {
              const isCurrentProject = workspace?.root_path === item.root_path;
              const isExpanded = expandedProjects.has(item.root_path);
              const projectSessions = sessionsByProject.get(item.root_path) || [];
              const ProjectIcon = isExpanded ? FolderOpen : Folder;
              const ExpandIcon = isExpanded ? ChevronDown : ChevronRight;
              return (
                <div className={classNames('project-tree-group', isExpanded && 'expanded')} key={item.root_path}>
                  <button
                    className={classNames('project-row', isCurrentProject && 'active')}
                    onClick={() => openProject(item, isCurrentProject)}
                    title={item.root_path}
                    type="button"
                  >
                    <ExpandIcon className="project-expand-icon" size={13} />
                    <ProjectIcon size={14} />
                    <span>{item.name || item.root_path}</span>
                  </button>
                  {isExpanded && (
                    <div className="project-session-list">
                      {projectSessions.length === 0 ? (
                        <div className="empty-block inline">暂无会话</div>
                      ) : projectSessions.map((session) => {
                        const runId = activeRunBySession?.[session.id];
                        const runState = runId ? runsById?.[runId] : null;
                        const isSessionRunning = Boolean(runState);
                        return (
                          <div
                            className={classNames(
                              'project-session-row',
                              session.id === currentSessionId && 'active',
                              isSessionRunning && 'running',
                            )}
                            key={session.id}
                          >
                            <button
                              className="project-session-main"
                              onClick={() => selectProjectSession(item, isCurrentProject, session.id)}
                              title={session.name || session.id}
                              type="button"
                            >
                              <span>{session.name || session.id}</span>
                              {isSessionRunning ? (
                                <Loader2 className="session-running-icon" size={13} />
                              ) : (
                                <small>{formatRelativeShort(session.updated_at)}</small>
                              )}
                            </button>
                            <button
                              className="session-delete"
                              onClick={(event) => onDeleteSession(session.id, event)}
                              title="删除会话"
                              type="button"
                            >
                              <Trash2 size={13} />
                            </button>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </section>

      <section className="menu-section sessions-block compact">
        <div className="sidebar-fold">
          <span><MessageSquare size={14} />对话</span>
          <ChevronRight size={14} />
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
