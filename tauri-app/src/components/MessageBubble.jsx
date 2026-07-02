import { messageText, messageToolBlocks, classNames } from '../utils/format.js';
import { MarkdownMessage } from './MarkdownMessage.jsx';
import { ToolCallBlock } from './ToolCallBlock.jsx';

export function MessageBubble({ message }) {
  const role = String(message.role || 'assistant').toLowerCase();
  const text = messageText(message);
  const toolBlocks = messageToolBlocks(message);
  if (!text.trim() && toolBlocks.length === 0) return null;
  return (
    <article className={classNames('message', role, message.tone)}>
      <div className="avatar">{role === 'user' ? 'U' : 'AI'}</div>
      <div className="message-body">
        <span>{role === 'user' ? 'You' : 'Assistant'}</span>
        {text.trim() && <MarkdownMessage text={text} />}
        {toolBlocks.length > 0 && (
          <div className="tool-call-list">
            {toolBlocks.map((block, index) => (
              <ToolCallBlock block={block} key={block.id || block.call_id || block.tool_call_id || index} />
            ))}
          </div>
        )}
      </div>
    </article>
  );
}
