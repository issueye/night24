export function classNames(...items) {
  return items.filter(Boolean).join(' ');
}

export function safeText(value) {
  if (value == null) return '';
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

export function messageText(message) {
  if (!message) return '';
  if (typeof message.content === 'string') return message.content;
  if (Array.isArray(message.content)) {
    return message.content
      .map((block) => {
        if (block?.type === 'text') return block.text || '';
        if (block?.type === 'thinking') return block.text || '';
        if (block?.type === 'tool_request' || block?.type === 'tool_response') return '';
        return safeText(block);
      })
      .filter(Boolean)
      .join('\n\n');
  }
  return safeText(message.content);
}

export function messageToolBlocks(message) {
  if (!message || !Array.isArray(message.content)) return [];
  return message.content.filter((block) => block?.type === 'tool_request' || block?.type === 'tool_response');
}

export function isVisibleChatMessage(message) {
  return messageText(message).trim().length > 0 || messageToolBlocks(message).length > 0;
}

export function formatTime(value) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

export function formatBytes(size) {
  if (!Number.isFinite(size)) return '';
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}
