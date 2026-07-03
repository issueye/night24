import { messageText } from './format.js';

export function appendMessageDelta(message, delta) {
  const content = Array.isArray(message.content) ? message.content : [{ type: 'text', text: messageText(message) }];
  let appended = false;
  const nextContent = content.map((block) => {
    if (!appended && block?.type === 'text') {
      appended = true;
      return { ...block, text: `${block.text || ''}${delta}` };
    }
    return block;
  });
  if (!appended) {
    nextContent.push({ type: 'text', text: delta });
  }
  return { ...message, content: nextContent };
}

export function withMessageText(message, text) {
  const content = Array.isArray(message.content) ? message.content : [];
  let replaced = false;
  const nextContent = content.map((block) => {
    if (!replaced && block?.type === 'text') {
      replaced = true;
      return { ...block, text };
    }
    return block;
  });
  if (!replaced) {
    nextContent.unshift({ type: 'text', text });
  }
  return { ...message, content: nextContent };
}
