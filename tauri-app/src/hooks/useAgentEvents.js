import { useCallback } from 'react';
import { isVisibleChatMessage, messageText, messageToolBlocks } from '../utils/format.js';
import { appendMessageDelta, mergeVisibleMessagesById, withMessageText } from '../utils/messages.js';
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

const findMessageIndex = (items, message) => (message.id ? items.findIndex((item) => item.id === message.id) : -1);

export function useAgentEvents({
  getSessionContext,
  setSessionMessages,
  addSessionTimeline,
  setSessionPermissions,
  setSessionRunCheckpoint,
  clearSessionRun,
  currentSessionIdRef,
  markRunEvent,
  finishRun,
  onSubAgentTool,
  onSubAgentSession,
  showError,
  markRunTerminal,
}) {
  const addOrReplaceSessionMessage = useCallback((sessionId, message) => {
    if (!sessionId || !isVisibleChatMessage(message)) return;
    setSessionMessages(sessionId, (items) => {
      const index = findMessageIndex(items, message);
      if (index < 0) return [...items, message];
      return items.map((item, itemIndex) => (itemIndex === index ? message : item));
    });
  }, [setSessionMessages]);

  const addTypewriterSessionMessage = useCallback((sessionId, message) => {
    if (!sessionId || !message?.id) {
      addOrReplaceSessionMessage(sessionId, message);
      return;
    }

    const fullText = messageText(message);
    if (!fullText.trim()) {
      addOrReplaceSessionMessage(sessionId, message);
      return;
    }

    const baseMessage = withMessageText(message, '');
    setSessionMessages(sessionId, (items) => {
      const index = findMessageIndex(items, message);
      if (index >= 0) return items.map((item, itemIndex) => (itemIndex === index ? message : item));
      return [...items, baseMessage];
    });

    let offset = 0;
    const step = () => {
      offset = Math.min(fullText.length, offset + Math.max(2, Math.ceil(fullText.length / 90)));
      const visibleMessage = withMessageText(message, fullText.slice(0, offset));
      setSessionMessages(sessionId, (items) => items.map((item) => (item.id === message.id ? visibleMessage : item)));
      if (offset < fullText.length) {
        window.setTimeout(step, 16);
      }
    };
    window.setTimeout(step, 16);
  }, [addOrReplaceSessionMessage, setSessionMessages]);

  const upsertToolActivityMessage = useCallback((sessionId, eventPayload, update) => {
    if (!sessionId) return;
    const toolCallId =
      eventPayload?.tool_call_id ||
      eventPayload?.call_id ||
      eventPayload?.id ||
      `${eventPayload?.tool_name || 'tool'}-${Date.now()}`;
    const toolName = eventPayload?.tool_name || eventPayload?.name || '工具';
    const messageId = `tool-activity-${toolCallId}`;

    setSessionMessages(sessionId, (items) => {
      let found = false;
      const next = items.map((item) => {
        if (item.id !== messageId) return item;
        found = true;
        const content = Array.isArray(item.content) ? item.content : [];
        const existing = content.find((block) => block?.type === 'tool_activity') || {};
        const activity = {
          ...existing,
          type: 'tool_activity',
          id: toolCallId,
          tool_call_id: toolCallId,
          name: existing.name || toolName,
          tool_name: existing.tool_name || toolName,
          ...update(existing),
        };
        return {
          ...item,
          content: [activity],
          tone: activity.is_error ? 'error' : item.tone,
          updated_at: new Date().toISOString(),
        };
      });

      if (found) return next;
      const activity = {
        type: 'tool_activity',
        id: toolCallId,
        tool_call_id: toolCallId,
        name: toolName,
        tool_name: toolName,
        ...update({}),
      };
      return [
        ...next,
        {
          id: messageId,
          role: 'tool',
          content: [activity],
          tone: activity.is_error ? 'error' : undefined,
          created_at: new Date().toISOString(),
        },
      ];
    });
  }, [setSessionMessages]);

  const handleAgentEvent = useCallback((eventName, payload, eventContext = {}) => {
    const {
      envelope,
      eventType,
      eventPayload,
      runId: normalizedRunId,
      runStatus,
      isBareMessage,
    } = normalizeAgentEvent(eventName, payload);
    const sessionId = eventContext.sessionId;
    const runId = eventContext.runId || normalizedRunId;
    if (!sessionId) return;

    const isCurrentSession = currentSessionIdRef.current === sessionId;
    if (runId) {
      markRunEvent(runId, { status: runStatus, runId });
      setSessionRunCheckpoint(sessionId, runId, { status: runStatus });
    }

    if (isBareMessage) {
      addOrReplaceSessionMessage(sessionId, envelope);
      return;
    }

    if (eventType === 'message') {
      const message = eventPayload?.message || eventPayload;
      if (message?.role) {
        const existing = message.id && getSessionContext(sessionId).messages.some((item) => item.id === message.id);
        const canType =
          !existing &&
          String(message.role).toLowerCase() === 'assistant' &&
          messageText(message).length > 0 &&
          messageToolBlocks(message).length === 0;
        if (canType && isCurrentSession) {
          addTypewriterSessionMessage(sessionId, message);
        } else {
          addOrReplaceSessionMessage(sessionId, message);
        }
      } else {
        const text = normalizeMessageText(eventPayload);
        if (String(text || '').trim()) {
          setSessionMessages(sessionId, (items) => [
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
      setSessionMessages(sessionId, (items) => {
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
      const permission = {
        ...normalizePermissionEvent(eventPayload, envelope, runId, `${runId || 'run'}-${Date.now()}`),
        session_id: sessionId,
      };
      setSessionPermissions(sessionId, (items) => [
        permission,
        ...items.filter((item) => item.permission_id !== permission.permission_id),
      ]);
      addSessionTimeline(sessionId, eventType, '等待权限确认', `${permission.tool_name} · ${permission.summary}`, 'warning');
      return;
    }

    if (eventType === 'tool_started') {
      const timeline = normalizeToolStartedEvent(eventPayload);
      addSessionTimeline(sessionId, eventType, timeline.title, timeline.detail, timeline.tone);
      upsertToolActivityMessage(sessionId, eventPayload, () => ({
        status: 'running',
        arguments: eventPayload?.arguments ?? eventPayload?.params ?? eventPayload?.input ?? {},
        summary: eventPayload?.summary,
      }));
      if (eventPayload?.tool_name === 'developer__subagent_spawn') {
        onSubAgentTool?.({ phase: 'started', payload: eventPayload, runId, sessionId });
      }
      return;
    }

    if (eventType === 'tool_finished') {
      const timeline = normalizeToolFinishedEvent(eventPayload);
      addSessionTimeline(sessionId, eventType, timeline.title, timeline.detail, timeline.tone);
      upsertToolActivityMessage(sessionId, eventPayload, (existing) => ({
        status: eventPayload?.is_error ? 'failed' : 'completed',
        arguments: existing.arguments ?? eventPayload?.arguments ?? eventPayload?.params ?? eventPayload?.input ?? {},
        content: eventPayload?.content ?? eventPayload?.result ?? eventPayload?.output ?? eventPayload?.result_preview ?? timeline.detail,
        is_error: Boolean(eventPayload?.is_error),
        summary: eventPayload?.summary,
      }));
      if (eventPayload?.tool_name === 'developer__subagent_spawn') {
        onSubAgentTool?.({ phase: 'finished', payload: eventPayload, runId, sessionId });
      }
      return;
    }

    if (eventType === 'tool_failed') {
      const tool = normalizeToolFailedEvent(eventPayload);
      addSessionTimeline(sessionId, eventType, tool.title, tool.detail, tool.tone);
      upsertToolActivityMessage(sessionId, eventPayload, (existing) => ({
        status: 'failed',
        arguments: existing.arguments ?? eventPayload?.arguments ?? eventPayload?.params ?? eventPayload?.input ?? {},
        content: tool.detail,
        is_error: true,
      }));
      return;
    }

    if (eventType === 'run_output') {
      const timeline = normalizeRunOutputEvent(eventPayload);
      addSessionTimeline(sessionId, eventType, timeline.title, timeline.detail, timeline.tone);
      return;
    }

    if (eventType === 'diff_ready') {
      const timeline = normalizeDiffReadyEvent(eventPayload);
      addSessionTimeline(sessionId, eventType, timeline.title, timeline.detail, timeline.tone);
      return;
    }

    if (eventType === 'sub_agent_session') {
      onSubAgentSession?.({ payload: eventPayload, runId, sessionId });
      addSessionTimeline(
        sessionId,
        eventType,
        '子代理会话已更新',
        eventPayload?.name || eventPayload?.subagent_id || 'subagent',
        'neutral',
      );
      return;
    }

    if (eventType === 'finish') {
      markRunTerminal(sessionId, runId, 'finish');
      const finish = normalizeFinishEvent(eventPayload);
      const finishMessages = finish.messages;
      if (finishMessages.length) {
        setSessionMessages(sessionId, (items) => mergeVisibleMessagesById(
          items,
          finishMessages,
          isVisibleChatMessage,
          { pruneSyntheticToolActivity: true },
        ));
      }
      const finishStatus = finish.status;
      if (runId) {
        setSessionPermissions(sessionId, (items) => items.filter((item) => item.run_id !== runId));
        finishRun(runId, finishStatus);
        clearSessionRun(sessionId, runId);
      }
      addSessionTimeline(sessionId, eventType, '任务结束', finishStatus, finish.tone);
      return;
    }

    if (eventType === 'error') {
      markRunTerminal(sessionId, runId, 'error');
      const { detail } = normalizeErrorEvent(eventPayload);
      addSessionTimeline(sessionId, eventType, '任务错误', detail, 'error');
      if (runId) {
        setSessionPermissions(sessionId, (items) => items.filter((item) => item.run_id !== runId));
        finishRun(runId, 'error');
        clearSessionRun(sessionId, runId);
      }
      showError(detail, { sessionId });
      return;
    }

    addSessionTimeline(sessionId, eventType, eventType, normalizeFallbackTimeline(eventPayload), 'neutral');
  }, [
    addOrReplaceSessionMessage,
    addSessionTimeline,
    addTypewriterSessionMessage,
    clearSessionRun,
    currentSessionIdRef,
    finishRun,
    getSessionContext,
    markRunEvent,
    markRunTerminal,
    onSubAgentSession,
    onSubAgentTool,
    setSessionMessages,
    setSessionPermissions,
    setSessionRunCheckpoint,
    showError,
    upsertToolActivityMessage,
  ]);

  return { handleAgentEvent };
}
