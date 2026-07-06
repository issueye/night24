import { useCallback } from 'react';
import { isVisibleChatMessage, messageText, messageToolBlocks } from '../utils/format.js';
import { appendMessageDelta, mergeVisibleMessagesById } from '../utils/messages.js';
import {
  normalizeAgentEvent,
  normalizeDiffReadyEvent,
  normalizeErrorEvent,
  normalizeFallbackTimeline,
  normalizeFinishEvent,
  normalizeMessageText,
  normalizePermissionEvent,
  normalizeRunOutputEvent,
  normalizeToolFailedEvent,
  normalizeToolFinishedEvent,
  normalizeToolStartedEvent,
} from '../utils/agentEvents.js';

export function useAgentEvents({
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
}) {
  const handleAgentEvent = useCallback((eventName, payload) => {
    const { envelope, eventType, eventPayload, runId, runStatus, isBareMessage } = normalizeAgentEvent(eventName, payload);
    if (runId) {
      setActiveRun((run) => ({
        ...(run || {}),
        run_id: runId,
        status: runStatus,
      }));
    }

    if (isBareMessage) {
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
        const text = normalizeMessageText(eventPayload);
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
      const permission = normalizePermissionEvent(eventPayload, envelope, runId, `${runId || 'run'}-${Date.now()}`);
      setPendingPermissions((items) => [permission, ...items.filter((item) => item.permission_id !== permission.permission_id)]);
      addTimeline(eventType, '等待权限确认', `${permission.tool_name} · ${permission.summary}`, 'warning');
      return;
    }

    if (eventType === 'tool_started') {
      const timeline = normalizeToolStartedEvent(eventPayload);
      addTimeline(eventType, timeline.title, timeline.detail, timeline.tone);
      return;
    }

    if (eventType === 'tool_finished') {
      const timeline = normalizeToolFinishedEvent(eventPayload);
      addTimeline(eventType, timeline.title, timeline.detail, timeline.tone);
      return;
    }

    if (eventType === 'tool_failed') {
      const tool = normalizeToolFailedEvent(eventPayload);
      addTimeline(eventType, tool.title, tool.detail, tool.tone);
      setMessages((items) => [
        ...items,
        {
          id: `tool-error-${eventPayload?.tool_call_id || Date.now()}-${Math.random().toString(16).slice(2)}`,
          role: 'assistant',
          content: [{ type: 'text', text: tool.messageText }],
          tone: 'error',
          created_at: envelope?.created_at || new Date().toISOString(),
        },
      ]);
      return;
    }

    if (eventType === 'run_output') {
      const timeline = normalizeRunOutputEvent(eventPayload);
      addTimeline(eventType, timeline.title, timeline.detail, timeline.tone);
      return;
    }

    if (eventType === 'diff_ready') {
      const timeline = normalizeDiffReadyEvent(eventPayload);
      openContextTab('diff');
      addTimeline(eventType, timeline.title, timeline.detail, timeline.tone);
      return;
    }

    if (eventType === 'finish') {
      runTerminalRef.current = { type: 'finish', runId };
      const finish = normalizeFinishEvent(eventPayload);
      const finishMessages = finish.messages;
      if (finishMessages.length) {
        setMessages((items) => mergeVisibleMessagesById(items, finishMessages, isVisibleChatMessage));
      }
      setIsRunning(false);
      const finishStatus = finish.status;
      setActiveRun((run) => (run ? { ...run, status: finishStatus } : null));
      if (runId) {
        setPendingPermissions((items) => items.filter((item) => item.run_id !== runId));
      }
      loadWorkspaceDiff();
      addTimeline(eventType, '任务结束', finishStatus, finish.tone);
      return;
    }

    if (eventType === 'error') {
      runTerminalRef.current = { type: 'error', runId };
      const { detail } = normalizeErrorEvent(eventPayload);
      addTimeline(eventType, '任务错误', detail, 'error');
      if (runId) {
        setPendingPermissions((items) => items.filter((item) => item.run_id !== runId));
      }
      setIsRunning(false);
      setActiveRun((run) => (run ? { ...run, status: 'error' } : { status: 'error' }));
      showError(detail);
      return;
    }

    addTimeline(eventType, eventType, normalizeFallbackTimeline(eventPayload), 'neutral');
  }, [
    addOrReplaceMessage,
    addTimeline,
    addTypewriterMessage,
    loadWorkspaceDiff,
    messages,
    openContextTab,
    runTerminalRef,
    setActiveRun,
    setIsRunning,
    setMessages,
    setPendingPermissions,
    showError,
  ]);

  return { handleAgentEvent };
}
