import { messageText, messageToolBlocks, classNames } from '../utils/format.js';
import { AiToolCall, Avatar, Markdown } from './ui/index.js';

export function MessageBubble({ message }) {
  const role = String(message.role || 'assistant').toLowerCase();
  const text = messageText(message);
  const toolBlocks = messageToolBlocks(message);
  if (!text.trim() && toolBlocks.length === 0) return null;
  return (
    <article className={classNames('message', role, message.tone)}>
      <Avatar label={role === 'user' ? 'U' : 'AI'} tone={role === 'user' ? 'user' : 'assistant'} />
      <div className="message-body">
        <span>{role === 'user' ? 'You' : 'Assistant'}</span>
        {text.trim() && <Markdown text={text} />}
        {toolBlocks.length > 0 && (
          <div className="tool-call-list">
            {toolBlocks.map((block, index) => (
              <AiToolCall block={block} key={block.id || block.call_id || block.tool_call_id || index} />
            ))}
          </div>
        )}
      </div>
    </article>
  );
}
