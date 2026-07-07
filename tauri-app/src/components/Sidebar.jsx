import { useEffect, useMemo, useState } from 'react';
import {
  Bot,
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  Loader2,
  MessageSquarePlus,
  Settings2,
  Trash2,
} from 'lucide-react';
import { classNames, formatRelativeShort } from '../utils/format.js';
import { sameWorkspacePath } from '../utils/settings.js';
import { Button, IconButton } from './ui/index.js';

function isSubAgentSession(session) {
  const type = String(session?.session_type || '').toLowerCase().replace(/[_\s-]/g, '');
  return type === 'subagent';
}

export function Sidebar({
  workspace,
  recentWorkspaces,
  sessions,
  sessionsLoading,
  sessionActionId,
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
      if (isSubAgentSession(session)) continue;
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

  const subAgentSessionsByParent = useMemo(() => {
    const grouped = new Map();
    for (const session of sessions || []) {
      if (!isSubAgentSession(session) || !session.parent_id) continue;
      const items = grouped.get(session.parent_id) || [];
      items.push(session);
      grouped.set(session.parent_id, items);
    }
    for (const items of grouped.values()) {
      items.sort((a, b) => String(b.updated_at || '').localeCompare(String(a.updated_at || '')));
    }
    return grouped;
  }, [sessions]);

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
        <Button className="nav-row" icon={<MessageSquarePlus size={15} />} onClick={onCreateSession} variant="ghost">新对话</Button>
      </nav>

      <section className="menu-section project-block">
        <div className="menu-section-head">
          <span>项目</span>
          <Button className="mini-button" onClick={() => onOpenWorkspace()} size="sm" variant="soft">打开</Button>
        </div>
        {sessionsLoading && <div className="empty-block inline">会话加载中...</div>}
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
                  <Button
                    className={classNames('project-row', isCurrentProject && 'active')}
                    onClick={() => openProject(item, isCurrentProject)}
                    title={item.root_path}
                    variant="ghost"
                  >
                    <ExpandIcon className="project-expand-icon" size={13} />
                    <ProjectIcon size={14} />
                    <span>{item.name || item.root_path}</span>
                    {projectSessions.length > 0 && <small className="project-session-count">{projectSessions.length}</small>}
                  </Button>
                  {isExpanded && (
                    <div className="project-session-list">
                      {projectSessions.length === 0 ? (
                        <div className="empty-block inline">暂无会话</div>
                      ) : projectSessions.map((session) => {
                        const runId = activeRunBySession?.[session.id];
                        const runState = runId ? runsById?.[runId] : null;
                        const isSessionRunning = Boolean(runState);
                        const childSessions = subAgentSessionsByParent.get(session.id) || [];
                        return (
                          <div className="project-session-branch" key={session.id}>
                            <div
                              className={classNames(
                                'project-session-row',
                                session.id === currentSessionId && 'active',
                                isSessionRunning && 'running',
                              )}
                            >
                              <Button
                                disabled={sessionActionId === session.id}
                                className="project-session-main"
                                onClick={() => selectProjectSession(item, isCurrentProject, session.id)}
                                title={session.name || session.id}
                                variant="ghost"
                              >
                                <span className="session-title">{session.name || session.id}</span>
                                {isSessionRunning ? (
                                  <Loader2 className="session-running-icon" size={13} />
                                ) : (
                                  <small>{formatRelativeShort(session.updated_at)}</small>
                                )}
                              </Button>
                              <IconButton
                                className="session-delete"
                                disabled={sessionActionId === session.id}
                                onClick={(event) => onDeleteSession(session.id, event)}
                                label="删除会话"
                                size="sm"
                              >
                                <Trash2 size={13} />
                              </IconButton>
                            </div>
                            {childSessions.length > 0 && (
                              <div className="subagent-session-list">
                                {childSessions.map((child) => {
                                  const childRunId = activeRunBySession?.[child.id];
                                  const childRunState = childRunId ? runsById?.[childRunId] : null;
                                  const isChildRunning = Boolean(childRunState);
                                  return (
                                    <div
                                      className={classNames(
                                        'project-session-row',
                                        'subagent-session-row',
                                        child.id === currentSessionId && 'active',
                                        isChildRunning && 'running',
                                      )}
                                      key={child.id}
                                    >
                                      <Button
                                        disabled={sessionActionId === child.id}
                                        className="project-session-main"
                                        onClick={() => selectProjectSession(item, isCurrentProject, child.id)}
                                        title={child.name || child.id}
                                        variant="ghost"
                                      >
                                        <Bot size={13} />
                                        <span className="session-title">{child.name || child.id}</span>
                                        {isChildRunning ? (
                                          <Loader2 className="session-running-icon" size={13} />
                                        ) : (
                                          <small>{formatRelativeShort(child.updated_at)}</small>
                                        )}
                                      </Button>
                                      <IconButton
                                        className="session-delete"
                                        disabled={sessionActionId === child.id}
                                        onClick={(event) => onDeleteSession(child.id, event)}
                                        label="删除子代理会话"
                                        size="sm"
                                      >
                                        <Trash2 size={13} />
                                      </IconButton>
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
                </div>
              );
            })}
          </div>
        )}
      </section>

      <footer className="menu-footer">
        <Button className={classNames('nav-row settings-row', settingsOpen && 'active')} icon={<Settings2 size={15} />} onClick={onToggleSettings} variant="ghost">
          设置
        </Button>
      </footer>
    </aside>
  );
}
