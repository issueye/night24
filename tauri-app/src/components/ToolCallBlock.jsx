import { CheckCircle2, ChevronDown, ClipboardList, Wrench } from 'lucide-react';
import { safeText } from '../utils/format.js';

function compactPreview(value) {
  const text = safeText(value).replace(/\s+/g, ' ').trim();
  if (!text) return '无内容';
  return text.length > 140 ? `${text.slice(0, 140)}...` : text;
}

function toolTitle(block) {
  if (block?.type === 'tool_response') return block.name || block.tool_name || '工具结果';
  return block?.name || block?.tool_name || '工具调用';
}

function toolDetail(block) {
  if (block?.type === 'tool_response') {
    return block.content ?? block.result ?? block.output ?? block;
  }
  return block.arguments ?? block.params ?? block.input ?? block;
}

export function ToolCallBlock({ block }) {
  const isResponse = block?.type === 'tool_response';
  const detail = toolDetail(block);
  const title = toolTitle(block);

  return (
    <details className="tool-call-block">
      <summary>
        <span className="tool-call-icon">
          {isResponse ? <CheckCircle2 size={14} /> : <Wrench size={14} />}
        </span>
        <span className="tool-call-main">
          <strong>{title}</strong>
          <small>{isResponse ? '工具结果' : '工具参数'} · {compactPreview(detail)}</small>
        </span>
        <ChevronDown className="tool-call-chevron" size={14} />
      </summary>
      <div className="tool-call-detail">
        <div>
          <ClipboardList size={13} />
          <span>{isResponse ? '返回内容' : '调用参数'}</span>
        </div>
        <pre>{safeText(detail)}</pre>
      </div>
    </details>
  );
}
