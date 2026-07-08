import { messageText, messageToolBlocks } from './format.js';

function updateFirstTextBlock(content, updateText) {
  let updated = false;
  const nextContent = content.map((block) => {
    if (!updated && block?.type === 'text') {
      updated = true;
      return { ...block, text: updateText(block.text || '') };
    }
    return block;
  });
  return { nextContent, updated };
}

export function appendMessageDelta(message, delta) {
  const content = Array.isArray(message.content) ? message.content : [{ type: 'text', text: messageText(message) }];
  const { nextContent, updated } = updateFirstTextBlock(content, (text) => `${text}${delta}`);
  if (!updated) {
    nextContent.push({ type: 'text', text: delta });
  }
  return { ...message, content: nextContent };
}

export function withMessageText(message, text) {
  const content = Array.isArray(message.content) ? message.content : [];
  const { nextContent, updated } = updateFirstTextBlock(content, () => text);
  if (!updated) {
    nextContent.unshift({ type: 'text', text });
  }
  return { ...message, content: nextContent };
}

function isSyntheticToolActivityMessage(message) {
  if (String(message?.id || '').startsWith('tool-activity-')) return true;
  return messageToolBlocks(message).some((block) => block?.type === 'tool_activity');
}

function hasCanonicalToolBlocks(messages) {
  return messages.some((message) => (
    messageToolBlocks(message).some((block) => block?.type === 'tool_request' || block?.type === 'tool_response')
  ));
}

export function mergeVisibleMessagesById(items, incomingMessages, isVisibleMessage, options = {}) {
  const incomingVisible = incomingMessages.filter((message) => message?.role && isVisibleMessage(message));
  const shouldPruneSyntheticTools = options.pruneSyntheticToolActivity && hasCanonicalToolBlocks(incomingVisible);
  const next = shouldPruneSyntheticTools
    ? items.filter((message) => !isSyntheticToolActivityMessage(message))
    : [...items];
  const indexById = new Map();
  next.forEach((item, index) => {
    if (item?.id && !indexById.has(item.id)) {
      indexById.set(item.id, index);
    }
  });

  incomingVisible.forEach((message) => {
    if (message.id && indexById.has(message.id)) {
      next[indexById.get(message.id)] = message;
      return;
    }

    next.push(message);
    if (message.id) {
      indexById.set(message.id, next.length - 1);
    }
  });

  return next;
}
