import { ToolCallBlock } from '../ToolCallBlock.jsx';

export function AiToolCall({ block, className, size = 'md' }) {
  return <ToolCallBlock block={block} className={className} size={size} />;
}
