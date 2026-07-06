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
import { classNames } from './utils/format.js';
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

  const clearConversationView = useCallback(({ abortActive = false } = {}) => {
    if (abortActive) {
      abortRef.current?.abort();
      abortRef.current = null;
    }
    runTerminalRef.current = null;
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

      await readSseStream(response.body, (event) => handleAgentEvent(event.eventName, event.payload));
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
          onSelectSession={selectSession}
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
