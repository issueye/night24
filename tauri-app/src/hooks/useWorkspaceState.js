import { useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
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
  headers,
  addTimeline,
  showError,
  clearConversationView,
  onWorkspaceOpened,
}) {
  const [workspace, setWorkspace] = useState(null);
  const [recentWorkspaces, setRecentWorkspaces] = useState(() => readJsonSetting(STORAGE_KEYS.recentWorkspaces, []));
  const [tree, setTree] = useState(null);
  const [selectedFile, setSelectedFile] = useState(null);
  const [rightTab, setRightTab] = useState('files');
  const [contextOpen, setContextOpen] = useState(false);
  const [workspaceStatus, setWorkspaceStatus] = useState(null);
  const [workspaceDiff, setWorkspaceDiff] = useState(null);
  const [diffLoading, setDiffLoading] = useState(false);
  const [diffError, setDiffError] = useState('');

  const loadWorkspace = useCallback(async () => {
    const storedRecent = readJsonSetting(STORAGE_KEYS.recentWorkspaces, []);
    try {
      let current = await apiJson('/workspaces/current');
      if (!current) {
        const savedPath = readSetting(STORAGE_KEYS.currentWorkspacePath);
        if (savedPath) {
          current = await apiJson('/workspaces/open', {
            method: 'POST',
            body: JSON.stringify({ path: savedPath }),
          }).catch(() => null);
        }
      }
      setWorkspace(current || null);
      const recent = await apiJson('/workspaces/recent').catch(() => ({ workspaces: [] }));
      const mergedRecent = compactWorkspaces([
        ...(current ? [current] : []),
        ...(Array.isArray(recent?.workspaces) ? recent.workspaces : []),
        ...storedRecent,
      ]);
      setRecentWorkspaces(mergedRecent);
      writeJsonSetting(STORAGE_KEYS.recentWorkspaces, mergedRecent);
      if (current) {
        rememberWorkspace(current);
        const data = await apiJson('/workspace/tree');
        setTree(data?.root || null);
      } else {
        setTree(null);
      }
    } catch {
      setWorkspace(null);
      setTree(null);
      setRecentWorkspaces(storedRecent);
    }
  }, [apiJson]);

  const loadWorkspaceDiff = useCallback(async () => {
    if (!workspace) return;
    setDiffLoading(true);
    setDiffError('');
    try {
      const [status, diff] = await Promise.all([
        apiJson('/workspace/status'),
        apiJson('/workspace/diff'),
      ]);
      setWorkspaceStatus(status);
      setWorkspaceDiff(diff);
    } catch (error) {
      setWorkspaceStatus(null);
      setWorkspaceDiff(null);
      setDiffError(normalizeError(error));
    } finally {
      setDiffLoading(false);
    }
  }, [apiJson, workspace]);

  async function openWorkspace(pathFromRecent) {
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
      const opened = await apiJson('/workspaces/open', {
        method: 'POST',
        headers,
        body: JSON.stringify({ path }),
      });
      clearConversationView({ abortActive: true });
      onWorkspaceOpened?.();
      setWorkspace(opened);
      rememberWorkspace(opened);
      setRightTab('files');
      setContextOpen(true);
      setSelectedFile(null);
      setWorkspaceStatus(null);
      setWorkspaceDiff(null);
      setDiffError('');
      const data = await apiJson('/workspace/tree');
      setTree(data?.root || null);
      await loadWorkspace();
      addTimeline('workspace', '已打开项目', opened?.root_path || path, 'success');
    } catch (error) {
      showError(`打开项目失败：${normalizeError(error)}`);
    }
  }

  async function openFile(node) {
    if (!node || node.kind !== 'file') return;
    try {
      setRightTab('files');
      setContextOpen(true);
      const file = await apiJson(`/workspace/file?path=${encodeURIComponent(node.path)}`);
      setSelectedFile(file);
    } catch (error) {
      showError(`读取文件失败：${normalizeError(error)}`);
    }
  }

  function openContextTab(tab) {
    setRightTab(tab);
    setContextOpen(true);
    if (tab === 'diff') loadWorkspaceDiff();
  }

  return {
    workspace,
    recentWorkspaces,
    tree,
    selectedFile,
    rightTab,
    contextOpen,
    workspaceStatus,
    workspaceDiff,
    diffLoading,
    diffError,
    setContextOpen,
    loadWorkspace,
    loadWorkspaceDiff,
    openWorkspace,
    openFile,
    openContextTab,
  };
}
