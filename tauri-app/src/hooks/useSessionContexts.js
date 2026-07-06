import { useCallback, useRef, useState } from 'react';

const SESSION_TIMELINE_LIMIT = 80;

function createSessionContext() {
  return {
    messages: [],
    draftText: '',
    timeline: [],
    pendingPermissions: [],
    activeRunId: '',
    runCheckpoints: {},
  };
}

function resolveUpdater(current, patchOrUpdater) {
  if (typeof patchOrUpdater === 'function') {
    return patchOrUpdater(current);
  }
  return patchOrUpdater;
}

export function useSessionContexts() {
  const [sessionContexts, setSessionContexts] = useState({});
  const sessionContextsRef = useRef(sessionContexts);

  const updateContexts = useCallback((updater) => {
    const next = updater(sessionContextsRef.current);
    sessionContextsRef.current = next;
    setSessionContexts(next);
  }, []);

  const getSessionContext = useCallback((sessionId) => {
    if (!sessionId) return createSessionContext();
    return sessionContextsRef.current[sessionId] || createSessionContext();
  }, []);

  const patchSessionContext = useCallback((sessionId, patchOrUpdater) => {
    if (!sessionId) return;
    updateContexts((items) => {
      const current = items[sessionId] || createSessionContext();
      const patch = resolveUpdater(current, patchOrUpdater) || {};
      return {
        ...items,
        [sessionId]: {
          ...current,
          ...patch,
        },
      };
    });
  }, [updateContexts]);

  const setSessionMessages = useCallback((sessionId, messagesOrUpdater) => {
    patchSessionContext(sessionId, (current) => ({
      messages: resolveUpdater(current.messages, messagesOrUpdater) || [],
    }));
  }, [patchSessionContext]);

  const setSessionDraft = useCallback((sessionId, text) => {
    patchSessionContext(sessionId, { draftText: text });
  }, [patchSessionContext]);

  const addSessionTimeline = useCallback((sessionId, type, title, detail, tone = 'neutral') => {
    patchSessionContext(sessionId, (current) => ({
      timeline: [
        {
          id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
          type,
          title,
          detail,
          tone,
          createdAt: new Date().toISOString(),
        },
        ...current.timeline,
      ].slice(0, SESSION_TIMELINE_LIMIT),
    }));
  }, [patchSessionContext]);

  const setSessionPermissions = useCallback((sessionId, updater) => {
    patchSessionContext(sessionId, (current) => ({
      pendingPermissions: resolveUpdater(current.pendingPermissions, updater) || [],
    }));
  }, [patchSessionContext]);

  const setSessionRunCheckpoint = useCallback((sessionId, runId, checkpoint) => {
    if (!runId) return;
    const cleanCheckpoint = Object.fromEntries(
      Object.entries(checkpoint || {}).filter(([, value]) => value !== undefined),
    );
    patchSessionContext(sessionId, (current) => ({
      activeRunId: cleanCheckpoint.status === 'running' || cleanCheckpoint.status === 'reconnecting'
        ? runId
        : current.activeRunId,
      runCheckpoints: {
        ...current.runCheckpoints,
        [runId]: {
          runId,
          lastSeq: 0,
          status: 'running',
          ...(current.runCheckpoints[runId] || {}),
          ...cleanCheckpoint,
        },
      },
    }));
  }, [patchSessionContext]);

  const clearSessionRun = useCallback((sessionId, runId) => {
    if (!sessionId) return;
    patchSessionContext(sessionId, (current) => {
      if (!runId) {
        return {
          activeRunId: '',
          runCheckpoints: {},
          pendingPermissions: [],
        };
      }
      const nextCheckpoints = { ...current.runCheckpoints };
      delete nextCheckpoints[runId];
      return {
        activeRunId: current.activeRunId === runId ? '' : current.activeRunId,
        runCheckpoints: nextCheckpoints,
        pendingPermissions: current.pendingPermissions.filter((item) => item.run_id !== runId),
      };
    });
  }, [patchSessionContext]);

  return {
    sessionContexts,
    getSessionContext,
    patchSessionContext,
    setSessionMessages,
    setSessionDraft,
    addSessionTimeline,
    setSessionPermissions,
    setSessionRunCheckpoint,
    clearSessionRun,
  };
}
