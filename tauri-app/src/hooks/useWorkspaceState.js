import { useCallback, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { normalizeError } from '../utils/events.js';
import {
  STORAGE_KEYS,
  compactWorkspaces,
  readJsonSetting,
  readSetting,
  rememberWorkspace,
  writeJsonSetting,
} from '../utils/settings.js';

export function useWorkspaceState({
  apiJson,
  addTimeline,
  notify,
  showError,
  clearConversationView,
  onWorkspaceOpened,
}) {
  const [workspace, setWorkspace] = useState(null);
  const [workspaceLoading, setWorkspaceLoading] = useState(false);
  const [recentWorkspaces, setRecentWorkspaces] = useState(() => readJsonSetting(STORAGE_KEYS.recentWorkspaces, []));
  const [contextOpen, setContextOpen] = useState(false);
  const loadWorkspaceRequestRef = useRef(0);
  const openWorkspaceRequestRef = useRef(0);

  const loadWorkspace = useCallback(async ({ notifySuccess = false } = {}) => {
    const requestId = loadWorkspaceRequestRef.current + 1;
    loadWorkspaceRequestRef.current = requestId;
    const isCurrentRequest = () => loadWorkspaceRequestRef.current === requestId;
    const storedRecent = readJsonSetting(STORAGE_KEYS.recentWorkspaces, []);
    setWorkspaceLoading(true);
    try {
      let current = await apiJson('/workspaces/current');
      if (!isCurrentRequest()) return;
      if (!current) {
        const savedPath = readSetting(STORAGE_KEYS.currentWorkspacePath);
        if (savedPath) {
          current = await apiJson('/workspaces/open', {
            method: 'POST',
            body: JSON.stringify({ path: savedPath }),
          }).catch(() => null);
          if (!isCurrentRequest()) return;
        }
      }
      setWorkspace(current || null);
      const recent = await apiJson('/workspaces/recent').catch(() => ({ workspaces: [] }));
      if (!isCurrentRequest()) return;
      const mergedRecent = compactWorkspaces([
        ...(current ? [current] : []),
        ...(Array.isArray(recent?.workspaces) ? recent.workspaces : []),
        ...storedRecent,
      ]);
      setRecentWorkspaces(mergedRecent);
      writeJsonSetting(STORAGE_KEYS.recentWorkspaces, mergedRecent);
      if (current) {
        rememberWorkspace(current);
      }
      if (notifySuccess) {
        notify?.({ message: '项目数据已刷新', detail: current?.root_path || '', tone: 'success' });
      }
    } catch {
      if (!isCurrentRequest()) return;
      setWorkspace(null);
      setRecentWorkspaces(storedRecent);
      if (notifySuccess) {
        notify?.({ message: '刷新项目数据失败', tone: 'danger' });
      }
    } finally {
      if (isCurrentRequest()) {
        setWorkspaceLoading(false);
      }
    }
  }, [apiJson, notify]);

  async function openWorkspace(pathFromRecent) {
    let requestId = null;
    const isCurrentOpenRequest = () => requestId === null || openWorkspaceRequestRef.current === requestId;
    try {
      let path = pathFromRecent;
      if (!path) {
        try {
          path = await invoke('select_directory');
        } catch {
          path = window.prompt('输入项目目录路径');
        }
      }
      if (!path) return;
      requestId = openWorkspaceRequestRef.current + 1;
      openWorkspaceRequestRef.current = requestId;
      setWorkspaceLoading(true);
      const opened = await apiJson('/workspaces/open', {
        method: 'POST',
        body: JSON.stringify({ path }),
      });
      if (!isCurrentOpenRequest()) return;
      loadWorkspaceRequestRef.current += 1;
      clearConversationView({ abortActive: true, preserveRun: true });
      onWorkspaceOpened?.();
      setWorkspace(opened);
      rememberWorkspace(opened);
      await loadWorkspace();
      if (!isCurrentOpenRequest()) return;
      addTimeline('workspace', '已打开项目', opened?.root_path || path, 'success');
      notify?.({ message: '项目已打开', detail: opened?.root_path || path, tone: 'success' });
    } catch (error) {
      if (!isCurrentOpenRequest()) return;
      notify?.({ message: '打开项目失败', detail: normalizeError(error), tone: 'danger' });
      showError(`打开项目失败：${normalizeError(error)}`, { toast: false });
    } finally {
      if (isCurrentOpenRequest()) {
        setWorkspaceLoading(false);
      }
    }
  }

  function openContextTab() {
    setContextOpen(true);
  }

  return {
    workspace,
    workspaceLoading,
    recentWorkspaces,
    contextOpen,
    setContextOpen,
    loadWorkspace,
    openWorkspace,
    openContextTab,
  };
}
