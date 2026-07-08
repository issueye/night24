import { ChevronDown, TerminalSquare } from 'lucide-react';
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

  const role = String(message?.role || '').toLowerCase();
  const text = messageText(message).trim();
  if (role === 'tool' && text) {
    const toolName = message.name || message.tool_name || '工具结果';
    return [{
      type: 'tool_response',
      id: message.tool_call_id || message.call_id || message.id,
      call_id: message.call_id,
      tool_call_id: message.tool_call_id,
      name: toolName,
      tool_name: toolName,
      content: text,
      is_error: Boolean(message.is_error || message.tone === 'error'),
    }];
  }

  if (!isLegacyToolErrorMessage(message)) return [];
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

function toolBlockKey(block, fallback) {
  return block?.tool_call_id || block?.call_id || block?.id || `${block?.tool_name || block?.name || 'tool'}-${fallback}`;
}

export function mergeToolBlocks(blocks) {
  const entries = [];
  const byKey = new Map();

  blocks.forEach((block, index) => {
    if (block?.type === 'tool_activity') {
      entries.push(block);
      return;
    }

    const key = toolBlockKey(block, index);
    let activity = byKey.get(key);
    if (!activity) {
      activity = {
        type: 'tool_activity',
        id: key,
        tool_call_id: key,
        name: block?.name || block?.tool_name || '工具',
        tool_name: block?.tool_name || block?.name || '工具',
        status: 'pending',
      };
      byKey.set(key, activity);
      entries.push(activity);
    }

    if (block?.type === 'tool_response') {
      activity.status = block.is_error ? 'failed' : 'completed';
      activity.content = block.content ?? block.result ?? block.output ?? block;
      activity.is_error = Boolean(block.is_error);
    } else {
      activity.status = activity.status === 'pending' ? 'running' : activity.status;
      activity.arguments = block.arguments ?? block.params ?? block.input ?? block;
    }
  });

  return entries;
}

export function isToolOnlyMessage(message) {
  const blocks = toolBlocksFromActivityMessage(message);
  const text = messageText(message).trim();
  const isStructuredTool = blocks.length > 0 && !text;
  const isToolRoleMessage = blocks.length > 0 && String(message?.role || '').toLowerCase() === 'tool';
  return (isStructuredTool || isToolRoleMessage || isLegacyToolErrorMessage(message)) &&
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
  const blocks = mergeToolBlocks(messages.flatMap((message) => toolBlocksFromActivityMessage(message)));
  if (blocks.length === 0) return null;
  const runningCount = blocks.filter((block) => block?.status === 'running' || block?.status === 'started').length;
  const failedCount = blocks.filter((block) => block?.is_error || block?.status === 'failed' || block?.status === 'error').length;
  const label = runningCount > 0
    ? `正在运行 ${runningCount} / ${blocks.length} 条工具调用`
    : failedCount > 0
      ? `已运行 ${blocks.length} 条工具调用（${failedCount} 条失败）`
      : `已运行 ${blocks.length} 条工具调用`;

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
      <RunningPanda className="run-status-panda" label="正在思考" showLabel={false} />
      <span>正在思考</span>
    </div>
  );
}
