import { CheckCircle2, ChevronDown, TerminalSquare } from 'lucide-react';
import { messageText, messageToolBlocks } from '../utils/format.js';
import { RunningPanda } from './RunningPanda.jsx';
import { AiToolCall } from './ui/index.js';

function isLegacyToolErrorMessage(message) {
  const text = messageText(message).trim();
  return String(message?.id || '').startsWith('tool-error-') ||
    text.startsWith('工具调用失败：') ||
    text.startsWith('工具调用失败:');
}

function toolBlocksFromActivityMessage(message) {
  const blocks = messageToolBlocks(message);
  if (blocks.length > 0) return blocks;

  if (!isLegacyToolErrorMessage(message)) return [];
  const text = messageText(message).trim();
  const firstLine = text.split(/\r?\n/).find(Boolean) || '工具调用失败';
  const toolName = firstLine.replace(/^工具调用失败[:：]\s*/, '').trim() || '工具';
  return [{
    type: 'tool_response',
    id: message.id,
    name: toolName,
    tool_name: toolName,
    content: text,
    is_error: true,
  }];
}

export function isToolOnlyMessage(message) {
  const blocks = toolBlocksFromActivityMessage(message);
  const text = messageText(message).trim();
  const isStructuredTool = blocks.length > 0 && !text;
  return (isStructuredTool || isLegacyToolErrorMessage(message)) &&
    String(message?.role || '').toLowerCase() !== 'user';
}

export function buildConversationItems(messages) {
  const items = [];
  let toolGroup = [];

  function flushToolGroup() {
    if (toolGroup.length > 0) {
      items.push({
        type: 'tool_group',
        id: `tools-${toolGroup.map((message) => message.id).filter(Boolean).join('-') || items.length}`,
        messages: toolGroup,
      });
      toolGroup = [];
    }
  }

  messages.forEach((message) => {
    if (isToolOnlyMessage(message)) {
      toolGroup.push(message);
      return;
    }
    flushToolGroup();
    items.push({
      type: 'message',
      id: message.id || `message-${items.length}`,
      message,
    });
  });
  flushToolGroup();

  return items;
}

export function ToolActivityRow({ messages }) {
  const blocks = messages.flatMap((message) => toolBlocksFromActivityMessage(message));
  if (blocks.length === 0) return null;
  const hasError = blocks.some((block) => block?.is_error);
  const label = hasError ? `已运行 ${blocks.length} 条工具调用（含失败）` : `已运行 ${blocks.length} 条工具调用`;

  return (
    <details className="conversation-activity-row tool-activity-row">
      <summary>
        <TerminalSquare size={14} />
        <span>{label}</span>
        <ChevronDown className="activity-chevron" size={14} />
      </summary>
      <div className="activity-detail tool-call-list">
        {blocks.map((block, index) => (
          <AiToolCall block={block} key={block.id || block.call_id || block.tool_call_id || index} />
        ))}
      </div>
    </details>
  );
}

export function RunStatusRow({ isRunning }) {
  if (!isRunning) return null;

  return (
    <div className="conversation-activity-row run-status-row">
      <CheckCircle2 size={14} />
      <RunningPanda className="run-status-panda" label="正在思考" showLabel={false} />
      <span>正在思考</span>
    </div>
  );
}
