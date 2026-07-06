import { useCallback, useRef, useState } from 'react';

const LIVE_RUN_STATUSES = new Set(['pending', 'running', 'reconnecting', 'detached', 'cancelling']);

function createTemporaryRunId(sessionId) {
  return `pending-${sessionId || 'session'}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function isLiveRun(run) {
  return Boolean(run && LIVE_RUN_STATUSES.has(run.status));
}

export function useRunRegistry() {
  const [runRegistry, setRunRegistry] = useState({
    runsById: {},
    activeRunBySession: {},
  });
  const runRegistryRef = useRef(runRegistry);

  const updateRegistry = useCallback((updater) => {
    const next = updater(runRegistryRef.current);
    runRegistryRef.current = next;
    setRunRegistry(next);
  }, []);

  const startPendingSessionRun = useCallback((sessionId, metadata = {}) => {
    if (!sessionId) return null;
    const temporaryId = createTemporaryRunId(sessionId);
    const run = {
      runId: temporaryId,
      sessionId,
      workspacePath: metadata.workspacePath || '',
      status: metadata.status || 'pending',
      startedAt: metadata.startedAt || new Date().toISOString(),
      finishedAt: '',
      lastSeq: 0,
      controller: metadata.controller || null,
    };
    updateRegistry((items) => ({
      runsById: {
        ...items.runsById,
        [temporaryId]: run,
      },
      activeRunBySession: {
        ...items.activeRunBySession,
        [sessionId]: temporaryId,
      },
    }));
    return temporaryId;
  }, [updateRegistry]);

  const attachRunId = useCallback((sessionId, temporaryId, runId) => {
    if (!sessionId || !temporaryId || !runId || temporaryId === runId) return;
    updateRegistry((items) => {
      const current = items.runsById[temporaryId] || {
        runId: temporaryId,
        sessionId,
        status: 'running',
        startedAt: new Date().toISOString(),
        finishedAt: '',
        lastSeq: 0,
        controller: null,
      };
      const { [temporaryId]: _removed, ...remainingRuns } = items.runsById;
      return {
        runsById: {
          ...remainingRuns,
          [runId]: {
            ...current,
            runId,
            sessionId,
            status: current.status === 'pending' ? 'running' : current.status,
          },
        },
        activeRunBySession: {
          ...items.activeRunBySession,
          [sessionId]: runId,
        },
      };
    });
  }, [updateRegistry]);

  const markRunEvent = useCallback((runId, updates = {}) => {
    if (!runId) return;
    updateRegistry((items) => {
      const current = items.runsById[runId];
      if (!current) return items;
      const cleanUpdates = Object.fromEntries(
        Object.entries(updates).filter(([, value]) => value !== undefined),
      );
      return {
        ...items,
        runsById: {
          ...items.runsById,
          [runId]: {
            ...current,
            ...cleanUpdates,
          },
        },
      };
    });
  }, [updateRegistry]);

  const finishRun = useCallback((runId, status = 'finished') => {
    if (!runId) return;
    updateRegistry((items) => {
      const current = items.runsById[runId];
      if (!current) return items;
      const activeRunBySession = { ...items.activeRunBySession };
      if (activeRunBySession[current.sessionId] === runId) {
        delete activeRunBySession[current.sessionId];
      }
      return {
        runsById: {
          ...items.runsById,
          [runId]: {
            ...current,
            status,
            finishedAt: new Date().toISOString(),
          },
        },
        activeRunBySession,
      };
    });
  }, [updateRegistry]);

  const cancelRunState = useCallback((runId) => {
    finishRun(runId, 'cancelled');
  }, [finishRun]);

  const getSessionRun = useCallback((sessionId) => {
    if (!sessionId) return null;
    const registry = runRegistryRef.current;
    const runId = registry.activeRunBySession[sessionId];
    const run = runId ? registry.runsById[runId] : null;
    return isLiveRun(run) ? run : null;
  }, []);

  const getRunningSessions = useCallback(() => {
    const registry = runRegistryRef.current;
    return Object.entries(registry.activeRunBySession)
      .filter(([, runId]) => isLiveRun(registry.runsById[runId]))
      .map(([sessionId]) => sessionId);
  }, []);

  return {
    runRegistry,
    startPendingSessionRun,
    attachRunId,
    markRunEvent,
    finishRun,
    cancelRunState,
    getSessionRun,
    getRunningSessions,
  };
}
