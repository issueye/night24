import { useCallback, useEffect, useRef, useState } from 'react';
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
  addTimeline,
  notify,
  showError,
  clearConversationView,
  onWorkspaceOpened,
}) {
  const [workspace, setWorkspace] = useState(null);
  const [workspaceLoading, setWorkspaceLoading] = useState(false);
  const [treeLoading, setTreeLoading] = useState(false);
  const [fileLoading, setFileLoading] = useState(false);
  const [recentWorkspaces, setRecentWorkspaces] = useState(() => readJsonSetting(STORAGE_KEYS.recentWorkspaces, []));
  const [tree, setTree] = useState(null);
  const [selectedFile, setSelectedFile] = useState(null);
  const [rightTab, setRightTab] = useState('files');
  const [contextOpen, setContextOpen] = useState(false);
  const [workspaceStatus, setWorkspaceStatus] = useState(null);
  const [workspaceDiff, setWorkspaceDiff] = useState(null);
  const [diffLoading, setDiffLoading] = useState(false);
  const [diffError, setDiffError] = useState('');
  const diffRequestRef = useRef(null);
  const diffGenerationRef = useRef(0);
  const fileRequestRef = useRef(0);
  const loadWorkspaceRequestRef = useRef(0);
  const openWorkspaceRequestRef = useRef(0);
  const workspaceKeyRef = useRef('');

  useEffect(() => {
    workspaceKeyRef.current = workspace?.root_path || '';
  }, [workspace?.root_path]);

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
        setTreeLoading(true);
        const data = await apiJson('/workspace/tree');
        if (!isCurrentRequest()) return;
        setTree(data?.root || null);
      } else {
        setTree(null);
      }
      if (notifySuccess) {
        notify?.({ message: '项目数据已刷新', detail: current?.root_path || '', tone: 'success' });
      }
    } catch {
      if (!isCurrentRequest()) return;
      setWorkspace(null);
      setTree(null);
      setRecentWorkspaces(storedRecent);
      if (notifySuccess) {
        notify?.({ message: '刷新项目数据失败', tone: 'danger' });
      }
    } finally {
      if (isCurrentRequest()) {
        setWorkspaceLoading(false);
        setTreeLoading(false);
      }
    }
  }, [apiJson, notify]);

  const loadWorkspaceDiff = useCallback(async ({ notifySuccess = false } = {}) => {
    const workspaceKey = workspace?.root_path || '';
    if (!workspaceKey) return;
    if (diffRequestRef.current?.workspaceKey === workspaceKey) {
      return diffRequestRef.current.request;
    }

    const generation = diffGenerationRef.current;
    setDiffLoading(true);
    setDiffError('');
    const request = (async () => {
      try {
        const [status, diff] = await Promise.all([
          apiJson('/workspace/status'),
          apiJson('/workspace/diff'),
        ]);
        if (diffGenerationRef.current !== generation || diffRequestRef.current?.request !== request) return;
        setWorkspaceStatus(status);
        setWorkspaceDiff(diff);
        if (notifySuccess) {
          notify?.({ message: '变更数据已刷新', tone: 'success' });
        }
      } catch (error) {
        if (diffGenerationRef.current !== generation || diffRequestRef.current?.request !== request) return;
        setWorkspaceStatus(null);
        setWorkspaceDiff(null);
        setDiffError(normalizeError(error));
        if (notifySuccess) {
          notify?.({ message: '刷新变更失败', detail: normalizeError(error), tone: 'danger' });
        }
      } finally {
        if (diffGenerationRef.current === generation && diffRequestRef.current?.request === request) {
          setDiffLoading(false);
        }
      }
    })();
    diffRequestRef.current = { request, workspaceKey };
    request.finally(() => {
      if (diffRequestRef.current?.request === request) {
        diffRequestRef.current = null;
      }
    });
    return request;
  }, [apiJson, notify, workspace]);

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
      setTreeLoading(true);
      const opened = await apiJson('/workspaces/open', {
        method: 'POST',
        body: JSON.stringify({ path }),
      });
      if (!isCurrentOpenRequest()) return;
      loadWorkspaceRequestRef.current += 1;
      clearConversationView({ abortActive: true, preserveRun: true });
      onWorkspaceOpened?.();
      diffGenerationRef.current += 1;
      diffRequestRef.current = null;
      fileRequestRef.current += 1;
      setWorkspace(opened);
      rememberWorkspace(opened);
      setRightTab('files');
      setSelectedFile(null);
      setWorkspaceStatus(null);
      setWorkspaceDiff(null);
      setDiffLoading(false);
      setDiffError('');
      const data = await apiJson('/workspace/tree');
      if (!isCurrentOpenRequest()) return;
      setTree(data?.root || null);
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
        setTreeLoading(false);
      }
    }
  }

  async function openFile(node) {
    if (!node || node.kind !== 'file') return;
    const requestId = fileRequestRef.current + 1;
    const workspaceKey = workspaceKeyRef.current;
    fileRequestRef.current = requestId;
    try {
      setFileLoading(true);
      setRightTab('files');
      setContextOpen(true);
      const file = await apiJson(`/workspace/file?path=${encodeURIComponent(node.path)}`);
      if (fileRequestRef.current !== requestId || workspaceKeyRef.current !== workspaceKey) return;
      setSelectedFile(file);
    } catch (error) {
      if (fileRequestRef.current !== requestId || workspaceKeyRef.current !== workspaceKey) return;
      notify?.({ message: '读取文件失败', detail: normalizeError(error), tone: 'danger' });
      showError(`读取文件失败：${normalizeError(error)}`, { toast: false });
    } finally {
      if (fileRequestRef.current === requestId && workspaceKeyRef.current === workspaceKey) {
        setFileLoading(false);
      }
    }
  }

  function openContextTab(tab) {
    setRightTab(tab);
    setContextOpen(true);
    if (tab === 'diff') loadWorkspaceDiff();
  }

  return {
    workspace,
    workspaceLoading,
    recentWorkspaces,
    tree,
    treeLoading,
    selectedFile,
    fileLoading,
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
