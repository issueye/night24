import { messageText } from './format.js';

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
