import { CheckCircle2, ChevronDown, ClipboardList, Loader2, Wrench, XCircle } from 'lucide-react';
import { classNames, safeText } from '../utils/format.js';

function compactPreview(value) {
  const text = safeText(value).replace(/\s+/g, ' ').trim();
  if (!text) return '无内容';
  return text.length > 140 ? `${text.slice(0, 140)}...` : text;
}

function toolTitle(block) {
  if (block?.type === 'tool_response') return block.name || block.tool_name || '工具结果';
  return block?.name || block?.tool_name || '工具调用';
}

function toolRequestDetail(block) {
  if (block?.type === 'tool_activity') {
    return block.arguments ?? block.params ?? block.input ?? block.request ?? {};
  }
  if (block?.type === 'tool_response') return null;
  return block.arguments ?? block.params ?? block.input ?? block;
}

function toolResponseDetail(block) {
  if (block?.type === 'tool_activity') {
    return block.content ?? block.result ?? block.output ?? block.response ?? block.result_preview ?? null;
  }
  if (block?.type === 'tool_response') {
    return block.content ?? block.result ?? block.output ?? block;
  }
  return null;
}

function toolStatus(block) {
  if (block?.is_error || block?.status === 'failed' || block?.status === 'error') {
    return { label: '失败', tone: 'error', icon: XCircle };
  }
  if (block?.status === 'running' || block?.status === 'started' || block?.status === 'pending') {
    return { label: '运行中', tone: 'running', icon: Loader2 };
  }
  if (block?.type === 'tool_response' || block?.status === 'finished' || block?.status === 'completed' || block?.status === 'success') {
    return { label: '已完成', tone: 'success', icon: CheckCircle2 };
  }
  return { label: '待执行', tone: 'neutral', icon: Wrench };
}

export function ToolCallBlock({ block, className, size = 'md' }) {
  const requestDetail = toolRequestDetail(block);
  const responseDetail = toolResponseDetail(block);
  const previewDetail = responseDetail ?? requestDetail;
  const title = toolTitle(block);
  const status = toolStatus(block);
  const StatusIcon = status.icon;

  return (
    <details className={classNames('tool-call-block', `tool-call-block-${size}`, `tool-call-block-${status.tone}`, className)}>
      <summary>
        <span className="tool-call-icon">
          <StatusIcon size={14} />
        </span>
        <span className="tool-call-main">
          <strong>{title}</strong>
          <small>{status.label} · {compactPreview(previewDetail)}</small>
        </span>
        <span className={classNames('tool-call-status', status.tone)}>{status.label}</span>
        <ChevronDown className="tool-call-chevron" size={14} />
      </summary>
      <div className="tool-call-detail">
        {requestDetail != null && (
          <section>
            <div>
              <ClipboardList size={13} />
              <span>调用参数</span>
            </div>
            <pre>{safeText(requestDetail)}</pre>
          </section>
        )}
        {responseDetail != null && (
          <section>
            <div>
              <ClipboardList size={13} />
              <span>{status.tone === 'error' ? '错误结果' : '返回结果'}</span>
            </div>
            <pre>{safeText(responseDetail)}</pre>
          </section>
        )}
      </div>
    </details>
  );
}
