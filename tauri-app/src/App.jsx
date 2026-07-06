import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { TopBar } from './components/TopBar.jsx';
import { SettingsStrip } from './components/SettingsStrip.jsx';
import { Sidebar } from './components/Sidebar.jsx';
import { ChatPanel } from './components/ChatPanel.jsx';
import { ContextPanel } from './components/ContextPanel.jsx';
import { TimelinePanel } from './components/TimelinePanel.jsx';
import { useApiClient } from './hooks/useApiClient.js';
import { useProviderSettings } from './hooks/useProviderSettings.js';
import { useRunControls } from './hooks/useRunControls.js';
import { useSessions } from './hooks/useSessions.js';
import { useMessageActions } from './hooks/useMessageActions.js';
import { useAgentEvents } from './hooks/useAgentEvents.js';
import { useAppSettingsPersistence } from './hooks/useAppSettingsPersistence.js';
import { useServerStatus } from './hooks/useServerStatus.js';
import { useSubAgents } from './hooks/useSubAgents.js';
import { useTimeline } from './hooks/useTimeline.js';
import { useWorkspaceState } from './hooks/useWorkspaceState.js';
import { classNames, isVisibleChatMessage } from './utils/format.js';
import { estimateContextUsage } from './utils/context.js';
import { normalizeError } from './utils/events.js';
import { buildReplyRequestBody } from './utils/reply.js';
import { readSseStream } from './utils/sse.js';
import {
  DEFAULT_SERVER,
  STORAGE_KEYS,
  apiUrl,
  readAccessMode,
  readSetting,
  sameWorkspacePath,
} from './utils/settings.js';

const STREAM_RECOVERY_ATTEMPTS = 40;
const STREAM_RECOVERY_DELAY_MS = 1500;

function delay(ms) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

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

  const [taskText, setTaskText] = useState('');
  const [isRunning, setIsRunning] = useState(false);
  const [activeRun, setActiveRun] = useState(null);
  const [pendingPermissions, setPendingPermissions] = useState([]);
  const [eventsOpen, setEventsOpen] = useState(false);

  const abortRef = useRef(null);
  const runTerminalRef = useRef(null);
  const streamCheckpointRef = useRef({ runId: null, lastSeq: 0 });
  const runCheckpointBySessionRef = useRef(new Map());
  const activeRunSessionIdRef = useRef(null);
  const messageEndRef = useRef(null);
  const messageSetterRef = useRef(null);
  const clearCurrentSessionRef = useRef(null);
  const { headers, apiJson } = useApiClient(apiBase, apiKey);
  const { timeline, addTimeline, clearTimeline } = useTimeline();

  const showError = useCallback((message) => {
    const text = String(message || '发生未知错误');
    messageSetterRef.current?.((items) => [
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

  const clearConversationView = useCallback(({ abortActive = false, preserveRun = false } = {}) => {
    if (abortActive) {
      if (preserveRun) {
        runTerminalRef.current = { type: 'detached', runId: streamCheckpointRef.current.runId };
      }
      abortRef.current?.abort();
      abortRef.current = null;
    }
    if (!preserveRun) {
      runTerminalRef.current = null;
      streamCheckpointRef.current = { runId: null, lastSeq: 0 };
      activeRunSessionIdRef.current = null;
    }
    messageSetterRef.current?.([]);
    setPendingPermissions([]);
    clearTimeline();
    setActiveRun(null);
    setIsRunning(false);
  }, [clearTimeline]);

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
    addTimeline,
    showError,
    clearConversationView,
    onWorkspaceOpened: () => clearCurrentSessionRef.current?.(),
  });

  const {
    sessions,
    currentSessionId,
    messages,
    setMessages,
    loadSessions,
    createSession,
    selectSession,
    deleteSession,
    ensureSession,
    clearCurrentSession,
  } = useSessions({
    apiJson,
    workspace,
    showError,
    onBeforeSessionChange: clearConversationView,
  });

  messageSetterRef.current = setMessages;
  clearCurrentSessionRef.current = clearCurrentSession;

  const { addOrReplaceMessage, addTypewriterMessage } = useMessageActions(setMessages);

  const { serverStatus, coreRestarting, checkServer, restartCore } = useServerStatus({
    apiJson,
    addTimeline,
    showError,
  });

  useAppSettingsPersistence({ apiBase, apiKey, networkProxy, accessMode, theme, fontSize });

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

  const { handleAgentEvent } = useAgentEvents({
    messages,
    setMessages,
    addTimeline,
    openContextTab,
    loadWorkspaceDiff,
    setIsRunning,
    setActiveRun,
    setPendingPermissions,
    showError,
    addOrReplaceMessage,
    addTypewriterMessage,
    runTerminalRef,
  });

  async function refreshSessionHistory(sessionId) {
    const history = await apiJson(`/sessions/${sessionId}/history`);
    const visibleMessages = Array.isArray(history) ? history.filter(isVisibleChatMessage) : [];
    setMessages(visibleMessages);
    return visibleMessages;
  }

  function rememberStreamEvent(event, sessionId = activeRunSessionIdRef.current || currentSessionId) {
    const payload = event?.payload;
    const runId = typeof payload?.run_id === 'string' ? payload.run_id : null;
    const seq = Number.isFinite(payload?.seq) ? payload.seq : null;
    const checkpoint = {
      runId: runId || streamCheckpointRef.current.runId,
      lastSeq: seq == null ? streamCheckpointRef.current.lastSeq : Math.max(streamCheckpointRef.current.lastSeq, seq),
    };
    streamCheckpointRef.current = checkpoint;
    if (sessionId && checkpoint.runId) {
      if (['finish', 'error'].includes(event?.eventName) || ['finish', 'error'].includes(payload?.type)) {
        runCheckpointBySessionRef.current.delete(sessionId);
      } else {
        runCheckpointBySessionRef.current.set(sessionId, checkpoint);
      }
    }
  }

  async function replayRunEvents(runId, afterSeq, sessionId = activeRunSessionIdRef.current || currentSessionId, signal) {
    const response = await fetch(apiUrl(apiBase, `/runs/${encodeURIComponent(runId)}/events?after_seq=${afterSeq}`), {
      method: 'GET',
      headers,
      signal,
    });
    if (!response.ok) {
      const errorText = await response.text();
      throw new Error(errorText || `HTTP ${response.status}`);
    }
    await readSseStream(response.body, (event) => {
      rememberStreamEvent(event, sessionId);
      handleAgentEvent(event.eventName, event.payload);
    });
  }

  async function recoverDisconnectedStream(sessionId, baselineVisibleCount, detail) {
    const checkpoint = streamCheckpointRef.current;
    if (checkpoint.runId) {
      addTimeline('stream_recovering', '连接恢复中', `${detail} · ${checkpoint.runId}`, 'warning');
      for (let attempt = 0; attempt < STREAM_RECOVERY_ATTEMPTS; attempt += 1) {
        if (attempt > 0) {
          await delay(STREAM_RECOVERY_DELAY_MS);
        }

        try {
          await replayRunEvents(checkpoint.runId, streamCheckpointRef.current.lastSeq, sessionId);
          if (runTerminalRef.current) {
            return true;
          }
        } catch (error) {
          addTimeline('stream_recovering', '重连重试', normalizeError(error), 'warning');
        }
      }
    }

    addTimeline('stream_recovering', '连接恢复中', `${detail} · 回退到会话历史同步`, 'warning');

    for (let attempt = 0; attempt < STREAM_RECOVERY_ATTEMPTS; attempt += 1) {
      if (attempt > 0) {
        await delay(STREAM_RECOVERY_DELAY_MS);
      }

      try {
        const visibleMessages = await refreshSessionHistory(sessionId);
        if (visibleMessages.length >= baselineVisibleCount + 2) {
          runTerminalRef.current = { type: 'recovered' };
          setIsRunning(false);
          setActiveRun((run) => (run ? { ...run, status: 'synced' } : { status: 'synced' }));
          addTimeline('stream_recovered', '会话已同步', '已从会话历史补齐后台结果', 'success');
          loadWorkspaceDiff();
          return true;
        }
      } catch (error) {
        addTimeline('stream_recovering', '同步重试', normalizeError(error), 'warning');
      }
    }

    const timeoutDetail = '事件流已断开，后台任务可能仍在运行；稍后重新打开当前会话会同步结果';
    setIsRunning(false);
    setActiveRun((run) => (run ? { ...run, status: 'detached' } : { status: 'detached' }));
    addTimeline('stream_detached', '连接已分离', timeoutDetail, 'warning');
    showError(timeoutDetail);
    return false;
  }

  async function handleSelectSession(id) {
    const visibleMessages = await selectSession(id);
    if (!visibleMessages) return;

    const checkpoint = runCheckpointBySessionRef.current.get(id);
    if (!checkpoint?.runId) {
      return;
    }

    const controller = new AbortController();
    abortRef.current = controller;
    activeRunSessionIdRef.current = id;
    streamCheckpointRef.current = checkpoint;
    runTerminalRef.current = null;
    setIsRunning(true);
    setActiveRun({ run_id: checkpoint.runId, status: 'reconnecting' });
    addTimeline('stream_recovering', '继续接收会话事件', checkpoint.runId, 'warning');

    try {
      await replayRunEvents(checkpoint.runId, checkpoint.lastSeq, id, controller.signal);
      if (!runTerminalRef.current) {
        await refreshSessionHistory(id);
        setIsRunning(false);
        setActiveRun((run) => (run ? { ...run, status: 'synced' } : { status: 'synced' }));
      }
    } catch (error) {
      if (error.name !== 'AbortError') {
        setIsRunning(false);
        setActiveRun((run) => (run ? { ...run, status: 'detached' } : { status: 'detached' }));
        addTimeline('stream_detached', '会话事件暂未接上', normalizeError(error), 'warning');
      }
    } finally {
      if (abortRef.current === controller) {
        abortRef.current = null;
      }
      if (activeRunSessionIdRef.current === id) {
        activeRunSessionIdRef.current = null;
      }
      loadSessions();
    }
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
    streamCheckpointRef.current = { runId: null, lastSeq: 0 };
    setActiveRun({ status: 'running', started_at: new Date().toISOString() });
    addTimeline('run', '任务已发送', text, 'neutral');

    const userMessage = {
      id: `${Date.now()}`,
      role: 'user',
      content: [{ type: 'text', text }],
      created_at: new Date().toISOString(),
    };
    const baselineVisibleCount = messages.length;
    setMessages((items) => [...items, userMessage]);

    let streamWasOpen = false;
    let activeSessionId = null;
    try {
      const sessionId = await ensureSession();
      activeSessionId = sessionId;
      activeRunSessionIdRef.current = sessionId;
      const controller = new AbortController();
      abortRef.current = controller;
      const response = await fetch(apiUrl(apiBase, '/reply'), {
        method: 'POST',
        headers,
        body: JSON.stringify(buildReplyRequestBody({
          text,
          sessionId,
          provider,
          model,
          baseUrl,
          providerKey,
          accessMode,
          networkProxy,
          contextThreshold,
        })),
        signal: controller.signal,
      });

      if (!response.ok) {
        const errorText = await response.text();
        throw new Error(errorText || `HTTP ${response.status}`);
      }

      streamWasOpen = true;
      await readSseStream(response.body, (event) => {
        rememberStreamEvent(event, sessionId);
        handleAgentEvent(event.eventName, event.payload);
      });
      if (!runTerminalRef.current) {
        const detail = '事件流已断开，未收到任务结束信号';
        await recoverDisconnectedStream(sessionId, baselineVisibleCount, detail);
      }
    } catch (error) {
      if (error.name === 'AbortError') {
        if (runTerminalRef.current?.type !== 'detached') {
          runTerminalRef.current = { type: 'cancelled' };
        }
      } else if (streamWasOpen && !runTerminalRef.current) {
        await recoverDisconnectedStream(
          activeSessionId,
          baselineVisibleCount,
          `事件流读取失败：${normalizeError(error)}`,
        );
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
      if (activeRunSessionIdRef.current === activeSessionId) {
        activeRunSessionIdRef.current = null;
      }
      loadSessions();
    }
  }

  const canSend = taskText.trim().length > 0 && !isRunning && Boolean(workspace);
  const contextUsage = useMemo(
    () => estimateContextUsage(messages, taskText, contextThreshold),
    [contextThreshold, messages, taskText],
  );
  const {
    cancelRun,
    compactContext,
    contextCompacting,
    resolvePermission,
  } = useRunControls({
    abortRef,
    activeRun,
    apiJson,
    addTimeline,
    contextUsage,
    currentSessionId,
    isRunning,
    loadSessions,
    runTerminalRef,
    setActiveRun,
    setIsRunning,
    setMessages,
    setPendingPermissions,
    showError,
  });
  const projectSessions = useMemo(() => {
    if (!workspace?.root_path) return [];
    return sessions.filter((session) => sameWorkspacePath(session.working_dir, workspace.root_path));
  }, [sessions, workspace]);
  const {
    subAgentPool,
    subAgentLoading,
    subAgentError,
    loadSubAgents,
  } = useSubAgents({
    apiJson,
    active: contextOpen && rightTab === 'agents',
    running: isRunning,
  });

  return (
    <div className={classNames('app-shell', `theme-${theme}`, `font-${fontSize}`)}>
      <TopBar
        serverStatus={serverStatus}
        coreRestarting={coreRestarting}
        onRetryServer={checkServer}
        onRestartCore={restartCore}
        onOpenWorkspace={() => openWorkspace()}
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
          settingsOpen={settingsOpen}
          onOpenWorkspace={openWorkspace}
          onCreateSession={createSession}
          onSelectSession={handleSelectSession}
          onDeleteSession={deleteSession}
          onToggleSettings={() => setSettingsOpen((value) => !value)}
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
          contextUsage={contextUsage}
          contextCompacting={contextCompacting}
          canCompactContext={Boolean(currentSessionId && messages.length > 1 && !isRunning)}
          activeContext={contextOpen ? rightTab : null}
          pendingPermissions={pendingPermissions}
          onTaskTextChange={setTaskText}
          onProviderProfileChange={selectProviderProfile}
          onAccessModeChange={setAccessMode}
          onCompactContext={compactContext}
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
          subAgentPool={subAgentPool}
          subAgentLoading={subAgentLoading}
          subAgentError={subAgentError}
          onTabChange={openContextTab}
          onClose={() => setContextOpen(false)}
          onOpenFile={openFile}
          onRefreshWorkspace={loadWorkspace}
          onRefreshDiff={loadWorkspaceDiff}
          onRefreshSubAgents={loadSubAgents}
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
