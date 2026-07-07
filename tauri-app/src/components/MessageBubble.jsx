import { useMemo, useState } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { messageText, messageToolBlocks, classNames } from '../utils/format.js';
import { AiToolCall, Avatar, IconButton, Markdown } from './ui/index.js';

function compactMessagePreview(text, toolBlocks) {
  const normalized = String(text || '').replace(/\s+/g, ' ').trim();
  if (normalized) {
    return normalized.length > 72 ? `${normalized.slice(0, 72)}...` : normalized;
  }
  if (toolBlocks.length > 0) {
    return `包含 ${toolBlocks.length} 条工具调用`;
  }
  return '空消息';
}

export function MessageBubble({ message }) {
  const role = String(message.role || 'assistant').toLowerCase();
  const text = messageText(message);
  const toolBlocks = messageToolBlocks(message);
  const canCollapse = role !== 'user';
  const [collapsed, setCollapsed] = useState(false);
  const preview = useMemo(() => compactMessagePreview(text, toolBlocks), [text, toolBlocks]);

  if (!text.trim() && toolBlocks.length === 0) return null;
  return (
    <article className={classNames('message', role, message.tone, collapsed && 'collapsed')}>
      <Avatar label={role === 'user' ? 'U' : 'AI'} tone={role === 'user' ? 'user' : 'assistant'} />
      <div className="message-body">
        <div className="message-head">
          <span>{role === 'user' ? 'You' : 'Assistant'}</span>
          {canCollapse && (
            <IconButton
              className="message-collapse-button"
              label={collapsed ? '展开消息' : '收起消息'}
              onClick={() => setCollapsed((value) => !value)}
              size="sm"
              variant="ghost"
            >
              {collapsed ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
            </IconButton>
          )}
        </div>
        {collapsed ? (
          <button className="message-preview" onClick={() => setCollapsed(false)} type="button">
            {preview}
          </button>
        ) : (
          <>
            {text.trim() && <Markdown text={text} />}
            {toolBlocks.length > 0 && (
              <div className="tool-call-list">
                {toolBlocks.map((block, index) => (
                  <AiToolCall block={block} key={block.id || block.call_id || block.tool_call_id || index} />
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </article>
  );
}
