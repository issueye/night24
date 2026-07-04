import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { TopBar } from './components/TopBar.jsx';
import { SettingsStrip } from './components/SettingsStrip.jsx';
import { Sidebar } from './components/Sidebar.jsx';
import { ChatPanel } from './components/ChatPanel.jsx';
import { ContextPanel } from './components/ContextPanel.jsx';
import { TimelinePanel } from './components/TimelinePanel.jsx';
import { useApiClient } from './hooks/useApiClient.js';
import { useProviderSettings } from './hooks/useProviderSettings.js';
import { useWorkspaceState } from './hooks/useWorkspaceState.js';
import { classNames, isVisibleChatMessage, messageText, messageToolBlocks, safeText } from './utils/format.js';
import { normalizeError, unwrapSsePayload } from './utils/events.js';
import { appendMessageDelta, withMessageText } from './utils/messages.js';
import {
  DEFAULT_SERVER,
  STORAGE_KEYS,
  apiUrl,
  parseOptionalPositiveInt,
  readAccessMode,
  readSetting,
  sameWorkspacePath,
  writeSetting,
} from './utils/settings.js';

export default function App() {
  const [apiBase, setApiBase] = useState(() => readSetting(STORAGE_KEYS.apiBase, DEFAULT_SERVER));
  const [apiKey, setApiKey] = useState(() => readSetting(STORAGE_KEYS.apiKey));
  const [accessMode, setAccessMode] = useState(readAccessMode);
  const [networkProxy, setNetworkProxy] = useState(() => readSetting(STORAGE_KEYS.networkProxy));
  const [theme, setTheme] = useState(() => readSetting(STORAGE_KEYS.theme, 'light'));
  const [fontSize, setFontSize] = useState(() => readSetting(STORAGE_KEYS.fontSize, 'normal'));
  const [settingsOpen, setSettingsOpen] = useState(false);
  const {
    providerProfiles,
    providerProfileId,
    provider,
    model,
    baseUrl,
    providerKey,
    contextThreshold,
    setProvider,
    setModel,
    setBaseUrl,
    setProviderKey,
    setContextThreshold,
    createProviderProfileFromCurrent,
    selectProviderProfile,
    updateProviderProfile,
    deleteProviderProfile,
  } = useProviderSettings();

  const [serverStatus, setServerStatus] = useState({ state: 'checking', detail: '正在连接 server' });
  const [sessions, setSessions] = useState([]);
  const [currentSessionId, setCurrentSessionId] = useState(null);
  const [messages, setMessages] = useState([]);
  const [taskText, setTaskText] = useState('');
  const [isRunning, setIsRunning] = useState(false);
  const [activeRun, setActiveRun] = useState(null);
  const [timeline, setTimeline] = useState([]);
  const [pendingPermissions, setPendingPermissions] = useState([]);
  const [eventsOpen, setEventsOpen] = useState(false);

  const abortRef = useRef(null);
  const runTerminalRef = useRef(null);
  const messageEndRef = useRef(null);
  const { headers, apiJson } = useApiClient(apiBase, apiKey);

  const addTimeline = useCallback((type, title, detail, tone = 'neutral') => {
    setTimeline((items) => [
      {
        id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        type,
        title,
        detail,
        tone,
        createdAt: new Date().toISOString(),
      },
      ...items,
    ].slice(0, 80));
  }, []);

  const showError = useCallback((message) => {
    const text = String(message || '发生未知错误');
    setMessages((items) => [
      ...items,
      {
        id: `error-${Date.now()}-${Math.random().toString(16).slice(2)}`,
        role: 'assistant',
        content: [{ type: 'text', text: `错误：${text}` }],
        tone: 'error',
        created_at: new Date().toISOString(),
      },
    ]);
  }, []);

  const addOrReplaceMessage = useCallback((message) => {
    if (!isVisibleChatMessage(message)) return;
    setMessages((items) => {
      const index = message.id ? items.findIndex((item) => item.id === message.id) : -1;
      if (index < 0) return [...items, message];
      return items.map((item, itemIndex) => (itemIndex === index ? message : item));
    });
  }, []);

  const addTypewriterMessage = useCallback((message) => {
    const fullText = messageText(message);
    if (!fullText.trim()) {
      addOrReplaceMessage(message);
      return;
    }

    const baseMessage = withMessageText(message, '');
    setMessages((items) => {
      const index = message.id ? items.findIndex((item) => item.id === message.id) : -1;
      if (index >= 0) return items.map((item, itemIndex) => (itemIndex === index ? message : item));
      return [...items, baseMessage];
    });

    let offset = 0;
    const step = () => {
      offset = Math.min(fullText.length, offset + Math.max(2, Math.ceil(fullText.length / 90)));
      const visibleMessage = withMessageText(message, fullText.slice(0, offset));
      setMessages((items) => items.map((item) => (item.id === message.id ? visibleMessage : item)));
      if (offset < fullText.length) {
        window.setTimeout(step, 16);
      }
    };
    window.setTimeout(step, 16);
  }, [addOrReplaceMessage]);

  const clearConversationView = useCallback(({ abortActive = false } = {}) => {
    if (abortActive) {
      abortRef.current?.abort();
      abortRef.current = null;
    }
    runTerminalRef.current = null;
    setMessages([]);
    setPendingPermissions([]);
    setTimeline([]);
    setActiveRun(null);
    setIsRunning(false);
  }, []);

  const {
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
  } = useWorkspaceState({
    apiJson,
    headers,
    addTimeline,
    showError,
    clearConversationView,
    onWorkspaceOpened: () => setCurrentSessionId(null),
  });

  const checkServer = useCallback(async () => {
    setServerStatus({ state: 'checking', detail: '正在连接 server' });
    try {
      await apiJson('/healthz');
      const ready = await apiJson('/readyz').catch(() => null);
      setServerStatus({
        state: 'connected',
        detail: ready?.ready ? 'server 与 core 已就绪' : ready?.core?.reason || 'server 已连接，core 尚未就绪',
        ready,
      });
      return true;
    } catch (error) {
      setServerStatus({ state: 'failed', detail: normalizeError(error) });
      return false;
    }
  }, [apiJson]);

  const loadSessions = useCallback(async () => {
    try {
      const data = await apiJson('/sessions');
      setSessions(Array.isArray(data) ? data : []);
    } catch (error) {
      showError(`加载会话失败：${normalizeError(error)}`);
    }
  }, [apiJson, showError]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.apiBase, apiBase);
  }, [apiBase]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.apiKey, apiKey);
  }, [apiKey]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.networkProxy, networkProxy);
  }, [networkProxy]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.accessMode, accessMode);
    writeSetting(STORAGE_KEYS.fullAccess, accessMode === 'allow_all' ? 'true' : 'false');
  }, [accessMode]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.theme, theme);
  }, [theme]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.fontSize, fontSize);
  }, [fontSize]);

  useEffect(() => {
    checkServer().then((ok) => {
      if (ok) {
        loadWorkspace();
        loadSessions();
      }
    });
  }, [checkServer, loadSessions, loadWorkspace]);

  useEffect(() => {
    messageEndRef.current?.scrollIntoView({ block: 'end' });
  }, [messages]);

  async function createSession() {
    clearConversationView({ abortActive: true });
    try {
      const session = await apiJson('/sessions', {
        method: 'POST',
        headers,
        body: JSON.stringify({
          name: 'session',
          session_type: 'user',
          working_dir: workspace?.root_path,
        }),
      });
      setSessions((items) => [session, ...items]);
      setCurrentSessionId(session.id);
    } catch (error) {
      showError(`新建会话失败：${normalizeError(error)}`);
    }
  }

  async function selectSession(id) {
    clearConversationView({ abortActive: true });
    try {
      const history = await apiJson(`/sessions/${id}/history`);
      setCurrentSessionId(id);
      setMessages(Array.isArray(history) ? history.filter(isVisibleChatMessage) : []);
    } catch (error) {
      showError(`加载会话失败：${normalizeError(error)}`);
    }
  }

  async function deleteSession(id, event) {
    event?.stopPropagation();
    if (!window.confirm('删除这个会话？')) return;
    try {
      await apiJson(`/sessions/${id}`, { method: 'DELETE' });
      setSessions((items) => items.filter((item) => item.id !== id));
      if (currentSessionId === id) {
        setCurrentSessionId(null);
        clearConversationView({ abortActive: true });
      }
    } catch (error) {
      showError(`删除会话失败：${normalizeError(error)}`);
    }
  }

  function handleAgentEvent(eventName, payload) {
    const envelope = unwrapSsePayload(eventName, payload);
    const eventType = envelope?.type || (eventName && eventName !== 'message' ? eventName : 'message');
    const eventPayload = envelope?.payload || envelope;
    const runId = envelope?.run_id || eventPayload?.run_id;
    if (runId) {
      setActiveRun((run) => ({
        ...(run || {}),
        run_id: runId,
        status: eventType === 'finish' ? 'finished' : eventType === 'error' ? 'error' : 'running',
      }));
    }

    if (!envelope?.type && envelope?.role) {
      addOrReplaceMessage(envelope);
      return;
    }

    if (eventType === 'message') {
      const message = eventPayload?.message || eventPayload;
      if (message?.role) {
        const existing = message.id && messages.some((item) => item.id === message.id);
        const canType =
          !existing &&
          String(message.role).toLowerCase() === 'assistant' &&
          messageText(message).length > 0 &&
          messageToolBlocks(message).length === 0;
        if (canType) {
          addTypewriterMessage(message);
        } else {
          addOrReplaceMessage(message);
        }
      } else {
        const text = eventPayload?.text || eventPayload?.content || safeText(eventPayload);
        if (String(text || '').trim()) {
          setMessages((items) => [
            ...items,
            {
              id: `${Date.now()}`,
              role: 'assistant',
              content: [{ type: 'text', text }],
              created_at: new Date().toISOString(),
            },
          ]);
        }
      }
      return;
    }

    if (eventType === 'message_delta') {
      const messageId = eventPayload?.message_id || eventPayload?.id || `${runId || 'run'}-delta`;
      const delta = eventPayload?.delta || eventPayload?.text || '';
      if (!delta) return;
      setMessages((items) => {
        const existingIndex = items.findIndex((item) => item.id === messageId);
        if (existingIndex < 0) {
          return [
            ...items,
            {
              id: messageId,
              role: 'assistant',
              content: [{ type: 'text', text: delta }],
              created_at: envelope?.created_at || new Date().toISOString(),
            },
          ];
        }
        return items.map((item, index) => (index === existingIndex ? appendMessageDelta(item, delta) : item));
      });
      return;
    }

    if (eventType === 'permission_required') {
      const permissionId =
        eventPayload?.permission_id ||
        envelope?.permission_id ||
        eventPayload?.tool_call_id ||
        `${runId || 'run'}-${Date.now()}`;
      const permission = {
        permission_id: permissionId,
        run_id: runId,
        tool_name: eventPayload?.tool_name || 'tool',
        risk: eventPayload?.risk || 'high',
        summary: eventPayload?.summary || '需要确认权限',
        arguments: eventPayload?.arguments || eventPayload?.params,
      };
      setPendingPermissions((items) => [permission, ...items.filter((item) => item.permission_id !== permission.permission_id)]);
      addTimeline(eventType, '等待权限确认', `${permission.tool_name} · ${permission.summary}`, 'warning');
      return;
    }

    if (eventType === 'tool_started') {
      addTimeline(eventType, eventPayload?.tool_name || '工具开始', eventPayload?.summary || safeText(eventPayload), 'neutral');
      return;
    }

    if (eventType === 'tool_finished') {
      addTimeline(
        eventType,
        eventPayload?.tool_name || '工具完成',
        eventPayload?.summary || eventPayload?.result_preview || safeText(eventPayload),
        eventPayload?.is_error ? 'error' : 'success',
      );
      return;
    }

    if (eventType === 'tool_failed') {
      const toolName = eventPayload?.tool_name || '工具';
      const detail = eventPayload?.error?.message || eventPayload?.error || safeText(eventPayload);
      addTimeline(eventType, `${toolName} 失败`, detail, 'error');
      setMessages((items) => [
        ...items,
        {
          id: `tool-error-${eventPayload?.tool_call_id || Date.now()}-${Math.random().toString(16).slice(2)}`,
          role: 'assistant',
          content: [{ type: 'text', text: `工具调用失败：${toolName}\n\n${detail}` }],
          tone: 'error',
          created_at: envelope?.created_at || new Date().toISOString(),
        },
      ]);
      return;
    }

    if (eventType === 'run_output') {
      addTimeline(eventType, eventPayload?.source || '运行输出', eventPayload?.text || safeText(eventPayload), eventPayload?.stream === 'stderr' ? 'warning' : 'neutral');
      return;
    }

    if (eventType === 'diff_ready') {
      openContextTab('diff');
      loadWorkspaceDiff();
      addTimeline(eventType, '变更已生成', eventPayload?.summary || safeText(eventPayload), 'success');
      return;
    }

    if (eventType === 'finish') {
      runTerminalRef.current = { type: 'finish', runId };
      const finishMessages = Array.isArray(eventPayload?.messages) ? eventPayload.messages : [];
      if (finishMessages.length) {
        setMessages((items) => {
          const seen = new Set(items.map((item) => item.id).filter(Boolean));
          const next = [...items];
          finishMessages.forEach((message) => {
            if (message?.role && isVisibleChatMessage(message) && (!message.id || !seen.has(message.id))) {
              next.push(message);
              if (message.id) seen.add(message.id);
            }
          });
          return next;
        });
      }
      setIsRunning(false);
      const finishStatus = eventPayload?.status || 'completed';
      setActiveRun((run) => (run ? { ...run, status: finishStatus } : null));
      if (runId) {
        setPendingPermissions((items) => items.filter((item) => item.run_id !== runId));
      }
      loadWorkspaceDiff();
      addTimeline(eventType, '任务结束', finishStatus, finishStatus === 'failed' ? 'error' : finishStatus === 'cancelled' ? 'warning' : 'success');
      loadSessions();
      return;
    }

    if (eventType === 'error') {
      runTerminalRef.current = { type: 'error', runId };
      const detail = eventPayload?.message || eventPayload?.error || safeText(eventPayload);
      addTimeline(eventType, '任务错误', detail, 'error');
      if (runId) {
        setPendingPermissions((items) => items.filter((item) => item.run_id !== runId));
      }
      setIsRunning(false);
      setActiveRun((run) => (run ? { ...run, status: 'error' } : { status: 'error' }));
      showError(detail);
      return;
    }

    addTimeline(eventType, eventType, safeText(eventPayload), 'neutral');
  }

  function parseSseBlock(block) {
    let eventName = 'message';
    const dataLines = [];
    block.split(/\r?\n/).forEach((line) => {
      if (line.startsWith('event:')) eventName = line.slice(6).trim() || 'message';
      if (line.startsWith('data:')) dataLines.push(line.slice(5).trimStart());
    });
    if (!dataLines.length) return;
    const raw = dataLines.join('\n');
    try {
      handleAgentEvent(eventName, JSON.parse(raw));
    } catch {
      handleAgentEvent(eventName, { type: 'message', payload: { text: raw } });
    }
  }

  async function ensureSession() {
    if (currentSessionId) return currentSessionId;
    const session = await apiJson('/sessions', {
      method: 'POST',
      headers,
      body: JSON.stringify({
        name: 'session',
        session_type: 'user',
        working_dir: workspace?.root_path,
      }),
    });
    setSessions((items) => [session, ...items]);
    setCurrentSessionId(session.id);
    return session.id;
  }

  async function sendTask() {
    const text = taskText.trim();
    if (!text || isRunning) return;
    if (!workspace) {
      showError('请先打开一个项目');
      return;
    }

    setTaskText('');
    setIsRunning(true);
    runTerminalRef.current = null;
    setActiveRun({ status: 'running', started_at: new Date().toISOString() });
    addTimeline('run', '任务已发送', text, 'neutral');

    const userMessage = {
      id: `${Date.now()}`,
      role: 'user',
      content: [{ type: 'text', text }],
      created_at: new Date().toISOString(),
    };
    setMessages((items) => [...items, userMessage]);

    try {
      const sessionId = await ensureSession();
      const controller = new AbortController();
      abortRef.current = controller;
      const response = await fetch(apiUrl(apiBase, '/reply'), {
        method: 'POST',
        headers,
        body: JSON.stringify({
          text,
          session_id: sessionId,
          provider,
          model: model.trim() || undefined,
          base_url: baseUrl.trim() || undefined,
          api_key: providerKey.trim() || undefined,
          permission_mode: accessMode,
          network_proxy: networkProxy.trim() || undefined,
          context_threshold_tokens: parseOptionalPositiveInt(contextThreshold),
        }),
        signal: controller.signal,
      });

      if (!response.ok) {
        const errorText = await response.text();
        throw new Error(errorText || `HTTP ${response.status}`);
      }

      const reader = response.body?.getReader();
      if (!reader) throw new Error('server 没有返回事件流');

      const decoder = new TextDecoder();
      let buffer = '';
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        let splitIndex = buffer.search(/\r?\n\r?\n/);
        while (splitIndex >= 0) {
          const block = buffer.slice(0, splitIndex);
          buffer = buffer.slice(buffer[splitIndex] === '\r' ? splitIndex + 4 : splitIndex + 2);
          parseSseBlock(block);
          splitIndex = buffer.search(/\r?\n\r?\n/);
        }
      }
      if (buffer.trim()) parseSseBlock(buffer);
      if (!runTerminalRef.current) {
        const detail = '事件流已断开，未收到任务结束信号';
        showError(detail);
        addTimeline('error', '连接中断', detail, 'error');
        setActiveRun((run) => (run ? { ...run, status: 'interrupted' } : { status: 'interrupted' }));
        setIsRunning(false);
      }
    } catch (error) {
      if (error.name === 'AbortError') {
        runTerminalRef.current = { type: 'cancelled' };
      } else {
        const detail = normalizeError(error);
        showError(`任务失败：${detail}`);
        addTimeline('error', '任务失败', detail, 'error');
        setActiveRun((run) => (run ? { ...run, status: 'error' } : { status: 'error' }));
        setIsRunning(false);
        runTerminalRef.current = { type: 'error' };
      }
    } finally {
      abortRef.current = null;
      loadSessions();
    }
  }

  async function cancelRun() {
    runTerminalRef.current = { type: 'cancelled', runId: activeRun?.run_id };
    abortRef.current?.abort();
    setActiveRun((run) => (run ? { ...run, status: 'cancelling' } : { status: 'cancelling' }));
    try {
      const result = await apiJson('/agent/cancel', {
        method: 'POST',
        headers,
        body: JSON.stringify({ run_id: activeRun?.run_id, reason: 'user_cancelled' }),
      });
      if (result?.accepted === false) {
        addTimeline('cancel', '本地已停止，server 未接管取消', result.reason || activeRun?.run_id || '当前任务', 'warning');
      } else {
        addTimeline('cancel', '已请求取消', result?.run_id || activeRun?.run_id || '当前任务', 'warning');
      }
      const cancelledRunId = result?.run_id || activeRun?.run_id;
      if (cancelledRunId) {
        setPendingPermissions((items) => items.filter((item) => item.run_id !== cancelledRunId));
      }
    } catch (error) {
      addTimeline('cancel', '本地已停止，取消接口不可用', normalizeError(error), 'warning');
    } finally {
      setIsRunning(false);
      setActiveRun((run) => (run ? { ...run, status: 'cancelled' } : { status: 'cancelled' }));
    }
  }

  async function resolvePermission(permission, decision) {
    if (!permission?.permission_id) return;
    try {
      const result = await apiJson(`/permissions/${encodeURIComponent(permission.permission_id)}/${decision}`, {
        method: 'POST',
        headers,
        body: JSON.stringify({ run_id: permission.run_id, reason: `user_${decision}` }),
      });
      setPendingPermissions((items) => items.filter((item) => item.permission_id !== permission.permission_id));
      if (result?.accepted === false) {
        addTimeline('permission', '权限接口暂不可用', result.reason || permission.summary, 'warning');
      } else {
        addTimeline('permission', decision === 'approve' ? '已批准权限' : '已拒绝权限', permission.summary, decision === 'approve' ? 'success' : 'warning');
      }
    } catch (error) {
      showError(`处理权限失败：${normalizeError(error)}`);
    }
  }

  const canSend = taskText.trim().length > 0 && !isRunning && Boolean(workspace);
  const projectSessions = useMemo(() => {
    if (!workspace?.root_path) return [];
    return sessions.filter((session) => sameWorkspacePath(session.working_dir, workspace.root_path));
  }, [sessions, workspace]);

  return (
    <div className={classNames('app-shell', `theme-${theme}`, `font-${fontSize}`)}>
      <TopBar
        serverStatus={serverStatus}
        settingsOpen={settingsOpen}
        onRetryServer={checkServer}
        onOpenWorkspace={() => openWorkspace()}
        onToggleSettings={() => setSettingsOpen((value) => !value)}
      />

      <SettingsStrip
        open={settingsOpen}
        apiBase={apiBase}
        apiKey={apiKey}
        providerProfiles={providerProfiles}
        providerProfileId={providerProfileId}
        provider={provider}
        model={model}
        baseUrl={baseUrl}
        providerKey={providerKey}
        contextThreshold={contextThreshold}
        networkProxy={networkProxy}
        theme={theme}
        fontSize={fontSize}
        workspace={workspace}
        apiJson={apiJson}
        onApiBaseChange={setApiBase}
        onApiKeyChange={setApiKey}
        onProviderProfileChange={selectProviderProfile}
        onProviderProfileCreate={createProviderProfileFromCurrent}
        onProviderProfileUpdate={updateProviderProfile}
        onProviderProfileDelete={deleteProviderProfile}
        onProviderChange={setProvider}
        onModelChange={setModel}
        onBaseUrlChange={setBaseUrl}
        onProviderKeyChange={setProviderKey}
        onContextThresholdChange={setContextThreshold}
        onNetworkProxyChange={setNetworkProxy}
        onThemeChange={setTheme}
        onFontSizeChange={setFontSize}
        onClose={() => setSettingsOpen(false)}
      />

      <main className="workspace-grid">
        <Sidebar
          workspace={workspace}
          recentWorkspaces={recentWorkspaces}
          sessions={projectSessions}
          currentSessionId={currentSessionId}
          onOpenWorkspace={openWorkspace}
          onCreateSession={createSession}
          onSelectSession={selectSession}
          onDeleteSession={deleteSession}
        />

        <ChatPanel
          title={sessions.find((item) => item.id === currentSessionId)?.name || 'New session'}
          serverDetail={serverStatus.detail}
          messages={messages}
          messageEndRef={messageEndRef}
          taskText={taskText}
          isRunning={isRunning}
          canSend={canSend}
          workspace={workspace}
          providerProfiles={providerProfiles}
          providerProfileId={providerProfileId}
          accessMode={accessMode}
          activeContext={contextOpen ? rightTab : null}
          pendingPermissions={pendingPermissions}
          onTaskTextChange={setTaskText}
          onProviderProfileChange={selectProviderProfile}
          onAccessModeChange={setAccessMode}
          onResolvePermission={resolvePermission}
          onSendTask={sendTask}
          onCancelRun={cancelRun}
          onOpenContext={(tab) => {
            openContextTab(tab);
          }}
        />

        <ContextPanel
          open={contextOpen}
          rightTab={rightTab}
          tree={tree}
          selectedPath={selectedFile?.path}
          selectedFile={selectedFile}
          diff={workspaceDiff}
          status={workspaceStatus}
          diffLoading={diffLoading}
          diffError={diffError}
          onTabChange={openContextTab}
          onClose={() => setContextOpen(false)}
          onOpenFile={openFile}
          onRefreshWorkspace={loadWorkspace}
          onRefreshDiff={loadWorkspaceDiff}
        />
      </main>

      <TimelinePanel
        timeline={timeline}
        activeRun={activeRun}
        open={eventsOpen}
        onToggle={() => setEventsOpen((value) => !value)}
        onClose={() => setEventsOpen(false)}
      />
    </div>
  );
}
