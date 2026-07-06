export function parseSseBlock(block) {
  let eventName = 'message';
  const dataLines = [];
  block.split(/\r?\n/).forEach((line) => {
    if (line.startsWith('event:')) eventName = line.slice(6).trim() || 'message';
    if (line.startsWith('data:')) dataLines.push(line.slice(5).trimStart());
  });
  if (!dataLines.length) return null;

  const raw = dataLines.join('\n');
  try {
    return { eventName, payload: JSON.parse(raw) };
  } catch {
    return { eventName, payload: { type: 'message', payload: { text: raw } } };
  }
}

function takeSseBlock(buffer) {
  const match = /\r?\n\r?\n/.exec(buffer);
  if (!match) return null;
  return {
    block: buffer.slice(0, match.index),
    rest: buffer.slice(match.index + match[0].length),
  };
}

export async function readSseStream(body, onEvent) {
  const reader = body?.getReader();
  if (!reader) throw new Error('server 没有返回事件流');

  const decoder = new TextDecoder();
  let buffer = '';
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let nextBlock = takeSseBlock(buffer);
    while (nextBlock) {
      buffer = nextBlock.rest;
      const block = nextBlock.block;
      const event = parseSseBlock(block);
      if (event) onEvent(event);
      nextBlock = takeSseBlock(buffer);
    }
  }
  buffer += decoder.decode();

  if (buffer.trim()) {
    const event = parseSseBlock(buffer);
    if (event) onEvent(event);
  }
}
