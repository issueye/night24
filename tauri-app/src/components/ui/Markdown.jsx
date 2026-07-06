import { MarkdownMessage } from '../MarkdownMessage.jsx';

export function Markdown({ className, size = 'md', text }) {
  return <MarkdownMessage className={className} size={size} text={text} />;
}
