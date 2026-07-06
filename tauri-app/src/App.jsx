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
import { useAgentEvents } from './hooks/useAgentEvents.js';
import { useAppSettingsPersistence } from './hooks/useAppSettingsPersistence.js';
import { useServerStatus } from './hooks/useServerStatus.js';
import { useSubAgents } from './hooks/useSubAgents.js';
import { useWorkspaceState } from './hooks/useWorkspaceState.js';
import { useSessionContexts } from './hooks/useSessionContexts.js';
import { useRunRegistry } from './hooks/useRunRegistry.js';
import { useToasts } from './hooks/useToasts.js';
import { ToastViewport } from './components/ui/index.js';
import { classNames, isVisibleChatMessage } from './utils/format.js';
import { estimateContextUsage } from './utils/context.js';
import { normalizeError } from './utils/events.js';
import { mergeVisibleMessagesById } from './utils/messages.js';
import { buildReplyRequestBody } from './utils/reply.js';
import { readSseStream } from './utils/sse.js';
import {
  DEFAULT_SERVER,
  STORAGE_KEYS,
  apiUrl,
  readAccessMode,
  readSetting,
} from './utils/settings.js';

const NEW_SESSION_CONTEXT_ID = '__new_session__';
const STREAM_RECOVERY_ATTEMPTS = 40;
const STREAM_RECOVERY_DELAY_MS = 1500;
const TERMINAL_RUN_EVENTS = new Set(['finish', 'error']);
const LIVE_CHECKPOINT_STATUSES = new Set(['running', 'reconnecting', 'detached', 'cancelling']);

function delay(ms) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function isPendingRunId(runId) {
  return typeof runId === 'string' && runId.startsWith('pending-');
}

function isLiveCheckpoint(checkpoint) {
  return Boolean(checkpoint?.runId && LIVE_CHECKPOINT_STATUSES.has(checkpoint.status || 'running'));
}

export default function App() {
  const { dismissToast, notify, toasts } = useToasts();
  const [apiBase, setApiBase] = useState(() => readSetting(STORAGE_KEYS.apiBase, DEFAULT_SERVER));
  const [apiKey, setApiKey] = useState(() => readSetting(STORAGE_KEYS.apiKey));
  const [accessMode, setAccessMode] = useState(readAccessMode);
  const [networkProxy, setNetworkProxy] = useState(() => readSetting(STORAGE_KEYS.networkProxy));
  const [theme, setTheme] = useState(() => readSetting(STORAGE_KEYS.theme, 'light'));
  const [fontSize, setFontSize] = useState(() => readSetting(STORAGE_KEYS.fontSize, 'normal'));
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [eventsOpen, setEventsOpen] = useState(false);
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
  } = useProviderSettings({ notify });

  const currentSessionIdRef = useRef(null);
  const messageEndRef = useRef(null);
  const clearCurrentSessionRef = useRef(null);
  const runTerminalsRef = useRef(new Map());
  const runEventListenersRef = useRef(new Map());
  const { headers, apiJson } = useApiClient(apiBase, apiKey);

  const {
    sessionContexts,
    getSessionContext,
    patchSessionContext,
    setSessionMessages,
    setSessionDraft,
    addSessionTimeline,
    setSessionPermissions,
    setSessionRunCheckpoint,
    clearSessionRun: clearSessionRunContext,
  } = useSessionContexts();

  const {
    runRegistry,
    startPendingSessionRun,
    attachRunId,
    markRunEvent,
    finishRun,
    cancelRunState,
    getSessionRun,
  } = useRunRegistry();

  const markRunTerminal = useCallback((sessionId, runId, type) => {
    if (!runId) return;
    runTerminalsRef.current.set(runId, { sessionId, runId, type });
  }, []);

  const showError = useCallback((message, options = {}) => {
    const sessionId = options.sessionId || currentSessionIdRef.current || NEW_SESSION_CONTEXT_ID;
    const text = String(message || '发生未知错误');
    setSessionMessages(sessionId, (items) => [
      ...items,
      {
        id: `error-${Date.now()}-${Math.random().toString(16).slice(2)}`,
        role: 'assistant',
        content: [{ type: 'text', text: `错误：${text}` }],
        tone: 'error',
        created_at: new Date().toISOString(),
      },
    ]);
    if (options.toast !== false) {
      notify({ message: text, tone: 'danger' });
    }
  }, [notify, setSessionMessages]);

  const addCurrentTimeline = useCallback((type, title, detail, tone = 'neutral') => {
    addSessionTimeline(currentSessionIdRef.current || NEW_SESSION_CONTEXT_ID, type, title, detail, tone);
  }, [addSessionTimeline]);

  const clearConversationView = useCallback(({ preserveRun = false } = {}) => {
    const sessionId = currentSessionIdRef.current || NEW_SESSION_CONTEXT_ID;
    patchSessionContext(sessionId, {
      messages: [],
      draftText: '',
      timeline: [],
      pendingPermissions: preserveRun ? getSessionContext(sessionId).pendingPermissions : [],
    });
    if (!preserveRun) {
      clearSessionRunContext(sessionId);
    }
  }, [clearSessionRunContext, getSessionContext, patchSessionContext]);

  const {
    workspace,
    recentWorkspaces,
    tree,
    treeLoading,
    selectedFile,
    fileLoading,
    rightTab,
    contextOpen,
    workspaceStatus,
    workspaceDiff,
    workspaceLoading,
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
    addTimeline: addCurrentTimeline,
    notify,
    showError,
    clearConversationView,
    onWorkspaceOpened: () => clearCurrentSessionRef.current?.(),
  });

  const {
    sessions,
    sessionsLoading,
    sessionActionId,
    currentSessionId,
    loadSessions,
    createSession,
    selectSession,
    deleteSession,
    ensureSession,
    clearCurrentSession,
  } = useSessions({
    apiJson,
    notify,
    workspace,
    showError,
    onBeforeSessionChange: () => {},
  });

  clearCurrentSessionRef.current = clearCurrentSession;
  currentSessionIdRef.current = currentSessionId;

  const currentContextId = currentSessionId || NEW_SESSION_CONTEXT_ID;
  const currentContext = sessionContexts[currentContextId] || getSessionContext(currentContextId);
  const currentSessionRun = currentSessionId ? getSessionRun(currentSessionId) : null;
  const visibleSessionRunning = Boolean(currentSessionRun);
  const visibleActiveRun = currentSessionRun ? {
    run_id: currentSessionRun.runId,
    status: currentSessionRun.status || 'running',
  } : null;

  const { serverStatus, coreRestarting, checkServer, restartCore } = useServerStatus({
    apiJson,
    addTimeline: addCurrentTimeline,
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
  }, [currentContext.messages]);

  const { handleAgentEvent } = useAgentEvents({
    getSessionContext,
    setSessionMessages,
    addSessionTimeline,
    setSessionPermissions,
    setSessionRunCheckpoint,
    clearSessionRun: clearSessionRunContext,
    currentSessionIdRef,
    markRunEvent,
    finishRun,
    openContextTab,
    loadWorkspaceDiff,
    showError,
    markRunTerminal,
  });

  function mergeSessionHistory(sessionId, visibleMessages, { replace = false } = {}) {
    if (replace) {
      setSessionMessages(sessionId, visibleMessages);
      return;
    }
    setSessionMessages(sessionId, (items) => mergeVisibleMessagesById(
      items,
      visibleMessages,
      isVisibleChatMessage,
    ));
  }

  async function refreshSessionHistory(sessionId, options = {}) {
    const history = await apiJson(`/sessions/${sessionId}/history`);
    const visibleMessages = Array.isArray(history) ? history.filter(isVisibleChatMessage) : [];
    mergeSessionHistory(sessionId, visibleMessages, options);
    return visibleMessages;
  }

  function rememberStreamEvent(event, sessionId, fallbackRunId) {
    const payload = event?.payload;
    const eventRunId = typeof payload?.run_id === 'string' ? payload.run_id : null;
    const runId = eventRunId || fallbackRunId;
    if (!sessionId || !runId) return runId;

    const seq = Number.isFinite(payload?.seq) ? payload.seq : null;
    const eventType = payload?.type || event?.eventName;
    const currentCheckpoint = getSessionContext(sessionId).runCheckpoints[runId] || {};
    const lastSeq = seq == null ? (currentCheckpoint.lastSeq || 0) : Math.max(currentCheckpoint.lastSeq || 0, seq);
    const status = TERMINAL_RUN_EVENTS.has(eventType) ? eventType : 'running';

    markRunEvent(runId, { lastSeq, status });
    setSessionRunCheckpoint(sessionId, runId, { runId, lastSeq, status });
    return runId;
  }

  async function replayRunEvents(runId, afterSeq, sessionId, signal) {
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
      const eventRunId = rememberStreamEvent(event, sessionId, runId);
      handleAgentEvent(event.eventName, event.payload, { sessionId, runId: eventRunId });
    });
  }

  function listenerKey(sessionId, runId) {
    return `${sessionId || ''}:${runId || ''}`;
  }

  function ensureRunEventListener(sessionId, runId, options = {}) {
    if (!sessionId || !runId || isPendingRunId(runId)) return null;
    const key = listenerKey(sessionId, runId);
    if (runEventListenersRef.current.has(key)) {
      return runEventListenersRef.current.get(key);
    }

    let run = getSessionRun(sessionId);
    if (!run || run.runId !== runId) {
      const temporaryId = startPendingSessionRun(sessionId, {
        controller: null,
        status: options.status || 'reconnecting',
        workspacePath: workspace?.root_path,
      });
      attachRunId(sessionId, temporaryId, runId);
      run = { runId, sessionId };
    }

    const checkpoint = getSessionContext(sessionId).runCheckpoints[runId] || { runId, lastSeq: 0 };
    const controller = new AbortController();
    const listener = { sessionId, runId, controller };
    runEventListenersRef.current.set(key, listener);

    markRunEvent(runId, {
      controller,
      lastSeq: checkpoint.lastSeq || 0,
      status: options.status || 'reconnecting',
    });
    setSessionRunCheckpoint(sessionId, runId, {
      runId,
      lastSeq: checkpoint.lastSeq || 0,
      status: options.status || 'reconnecting',
    });
    if (options.timeline !== false) {
      addSessionTimeline(
        sessionId,
        'stream_recovering',
        options.title || '继续接收会话事件',
        options.detail || runId,
        'warning',
      );
    }

    (async () => {
      try {
        await replayRunEvents(runId, checkpoint.lastSeq || 0, sessionId, controller.signal);
        if (!runTerminalsRef.current.get(runId)) {
          markRunEvent(runId, { status: 'detached' });
          setSessionRunCheckpoint(sessionId, runId, {
            runId,
            lastSeq: getSessionContext(sessionId).runCheckpoints[runId]?.lastSeq || checkpoint.lastSeq || 0,
            status: 'detached',
          });
          addSessionTimeline(sessionId, 'stream_detached', '会话事件暂未接上', runId, 'warning');
        }
      } catch (error) {
        if (error.name !== 'AbortError') {
          markRunEvent(runId, { status: 'detached' });
          setSessionRunCheckpoint(sessionId, runId, {
            runId,
            lastSeq: getSessionContext(sessionId).runCheckpoints[runId]?.lastSeq || checkpoint.lastSeq || 0,
            status: 'detached',
          });
          addSessionTimeline(sessionId, 'stream_detached', '会话事件暂未接上', normalizeError(error), 'warning');
        }
      } finally {
        if (runEventListenersRef.current.get(key) === listener) {
          runEventListenersRef.current.delete(key);
        }
        loadSessions();
      }
    })();

    return listener;
  }

  async function recoverDisconnectedStream(sessionId, runId, baselineVisibleCount, detail, signal) {
    const checkpoint = getSessionContext(sessionId).runCheckpoints[runId] || { runId, lastSeq: 0 };
    if (checkpoint.runId) {
      addSessionTimeline(sessionId, 'stream_recovering', '连接恢复中', `${detail} · ${checkpoint.runId}`, 'warning');
      for (let attempt = 0; attempt < STREAM_RECOVERY_ATTEMPTS; attempt += 1) {
        if (attempt > 0) {
          await delay(STREAM_RECOVERY_DELAY_MS);
        }

        try {
          const latest = getSessionContext(sessionId).runCheckpoints[checkpoint.runId] || checkpoint;
          await replayRunEvents(checkpoint.runId, latest.lastSeq || 0, sessionId, signal);
          if (runTerminalsRef.current.get(checkpoint.runId)) {
            return true;
          }
        } catch (error) {
          addSessionTimeline(sessionId, 'stream_recovering', '重连重试', normalizeError(error), 'warning');
        }
      }
    }

    addSessionTimeline(sessionId, 'stream_recovering', '连接恢复中', `${detail} · 回退到会话历史同步`, 'warning');

    for (let attempt = 0; attempt < STREAM_RECOVERY_ATTEMPTS; attempt += 1) {
      if (attempt > 0) {
        await delay(STREAM_RECOVERY_DELAY_MS);
      }

      try {
        const visibleMessages = await refreshSessionHistory(sessionId);
        if (visibleMessages.length >= baselineVisibleCount + 2) {
          markRunTerminal(sessionId, runId, 'recovered');
          finishRun(runId, 'synced');
          clearSessionRunContext(sessionId, runId);
          addSessionTimeline(sessionId, 'stream_recovered', '会话已同步', '已从会话历史补齐后台结果', 'success');
          if (currentSessionIdRef.current === sessionId) {
            loadWorkspaceDiff();
          }
          return true;
        }
      } catch (error) {
        addSessionTimeline(sessionId, 'stream_recovering', '同步重试', normalizeError(error), 'warning');
      }
    }

    const timeoutDetail = '事件流已断开，后台任务可能仍在运行；稍后重新打开当前会话会同步结果';
    markRunEvent(runId, { status: 'detached' });
    setSessionRunCheckpoint(sessionId, runId, { runId, lastSeq: checkpoint.lastSeq || 0, status: 'detached' });
    addSessionTimeline(sessionId, 'stream_detached', '连接已分离', timeoutDetail, 'warning');
    showError(timeoutDetail, { sessionId });
    return false;
  }

  async function handleSelectSession(id) {
    const visibleMessages = await selectSession(id);
    if (!visibleMessages) return;
    mergeSessionHistory(id, visibleMessages);

    const activeRun = getSessionRun(id);
    if (activeRun?.runId && !isPendingRunId(activeRun.runId)) {
      if (
        activeRun.status === 'detached' ||
        activeRun.status === 'reconnecting' ||
        !activeRun.controller
      ) {
        ensureRunEventListener(id, activeRun.runId, {
          status: 'reconnecting',
          detail: activeRun.runId,
        });
      }
      return;
    }

    const liveCheckpoint = Object.values(getSessionContext(id).runCheckpoints).find(isLiveCheckpoint);
    if (!liveCheckpoint?.runId || isPendingRunId(liveCheckpoint.runId)) return;

    ensureRunEventListener(id, liveCheckpoint.runId, {
      status: 'reconnecting',
      detail: liveCheckpoint.runId,
    });
  }

  async function sendTask() {
    const text = currentContext.draftText.trim();
    if (!text || (currentSessionId && getSessionRun(currentSessionId))) return;
    if (!workspace) {
      showError('请先打开一个项目');
      return;
    }

    const draftContextId = currentContextId;
    setSessionDraft(draftContextId, '');

    let sessionId = currentSessionId;
    let runId = null;
    let streamWasOpen = false;
    let baselineVisibleCount = 0;
    const controller = new AbortController();

    try {
      sessionId = await ensureSession();
      if (!sessionId) return;
      if (getSessionRun(sessionId)) return;

      if (draftContextId !== sessionId) {
        setSessionDraft(sessionId, '');
      }

      runId = startPendingSessionRun(sessionId, {
        controller,
        status: 'running',
        workspacePath: workspace.root_path,
      });
      setSessionRunCheckpoint(sessionId, runId, { runId, lastSeq: 0, status: 'running' });
      runTerminalsRef.current.delete(runId);
      addSessionTimeline(sessionId, 'run', '任务已发送', text, 'neutral');

      const userMessage = {
        id: `${Date.now()}`,
        role: 'user',
        content: [{ type: 'text', text }],
        created_at: new Date().toISOString(),
      };
      baselineVisibleCount = getSessionContext(sessionId).messages.length;
      setSessionMessages(sessionId, (items) => [...items, userMessage]);

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
      let attachedRunId = runId;
      await readSseStream(response.body, (event) => {
        const eventRunId = rememberStreamEvent(event, sessionId, attachedRunId);
        if (eventRunId && eventRunId !== attachedRunId) {
          if (isPendingRunId(attachedRunId)) {
            attachRunId(sessionId, attachedRunId, eventRunId);
          }
          attachedRunId = eventRunId;
          runId = eventRunId;
        }
        handleAgentEvent(event.eventName, event.payload, { sessionId, runId: eventRunId });
      });
      if (!runTerminalsRef.current.get(runId)) {
        const detail = '事件流已断开，未收到任务结束信号';
        await recoverDisconnectedStream(sessionId, runId, baselineVisibleCount, detail, controller.signal);
      }
    } catch (error) {
      if (error.name === 'AbortError') {
        markRunTerminal(sessionId, runId, 'cancelled');
      } else if (streamWasOpen && sessionId && runId && !runTerminalsRef.current.get(runId)) {
        await recoverDisconnectedStream(
          sessionId,
          runId,
          baselineVisibleCount,
          `事件流读取失败：${normalizeError(error)}`,
          controller.signal,
        );
      } else {
        const detail = normalizeError(error);
        if (sessionId && runId) {
          finishRun(runId, 'error');
          clearSessionRunContext(sessionId, runId);
          addSessionTimeline(sessionId, 'error', '任务失败', detail, 'error');
        }
        showError(`任务失败：${detail}`, { sessionId });
        markRunTerminal(sessionId, runId, 'error');
      }
    } finally {
      loadSessions();
    }
  }

  const canSend = currentContext.draftText.trim().length > 0 && !visibleSessionRunning && Boolean(workspace);
  const contextUsage = useMemo(
    () => estimateContextUsage(currentContext.messages, currentContext.draftText, contextThreshold),
    [contextThreshold, currentContext.draftText, currentContext.messages],
  );
  const {
    cancelRun,
    resolvePermission,
  } = useRunControls({
    apiJson,
    addSessionTimeline,
    contextUsage,
    currentSessionId,
    getSessionRun,
    cancelRunState,
    loadSessions,
    markRunTerminal,
    setSessionMessages,
    setSessionPermissions,
    clearSessionRun: clearSessionRunContext,
    isRunning: visibleSessionRunning,
    showError,
  });
  const {
    subAgentPool,
    subAgentLoading,
    subAgentError,
    loadSubAgents,
  } = useSubAgents({
    apiJson,
    active: contextOpen && rightTab === 'agents',
    notify,
    running: visibleSessionRunning,
  });

  return (
    <div className={classNames('app-shell', `theme-${theme}`, `font-${fontSize}`)}>
      <TopBar
        serverStatus={serverStatus}
        coreRestarting={coreRestarting}
        workspaceLoading={workspaceLoading}
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
        notify={notify}
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
          sessions={sessions}
          sessionsLoading={sessionsLoading}
          sessionActionId={sessionActionId}
          runsById={runRegistry.runsById}
          activeRunBySession={runRegistry.activeRunBySession}
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
          messages={currentContext.messages}
          messageEndRef={messageEndRef}
          taskText={currentContext.draftText}
          isRunning={visibleSessionRunning}
          canSend={canSend}
          workspace={workspace}
          providerProfiles={providerProfiles}
          providerProfileId={providerProfileId}
          accessMode={accessMode}
          contextUsage={contextUsage}
          activeContext={contextOpen ? rightTab : null}
          pendingPermissions={currentContext.pendingPermissions}
          onTaskTextChange={(value) => setSessionDraft(currentContextId, value)}
          onProviderProfileChange={selectProviderProfile}
          onAccessModeChange={setAccessMode}
          onResolvePermission={resolvePermission}
          onSendTask={sendTask}
          onCancelRun={() => cancelRun({ sessionId: currentSessionId, runId: currentSessionRun?.runId })}
          onOpenContext={(tab) => {
            openContextTab(tab);
          }}
        />

        <ContextPanel
          open={contextOpen}
          rightTab={rightTab}
          tree={tree}
          treeLoading={treeLoading}
          selectedPath={selectedFile?.path}
          selectedFile={selectedFile}
          fileLoading={fileLoading}
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
          onRefreshSubAgents={() => loadSubAgents({ notifySuccess: true })}
        />
      </main>

      <TimelinePanel
        timeline={currentContext.timeline}
        activeRun={visibleActiveRun}
        open={eventsOpen}
        onToggle={() => setEventsOpen((value) => !value)}
        onClose={() => setEventsOpen(false)}
      />
      <ToastViewport onDismiss={dismissToast} toasts={toasts} />
    </div>
  );
}
