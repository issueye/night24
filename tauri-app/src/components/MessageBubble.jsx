import { messageText, classNames } from '../utils/format.js';
import { MarkdownMessage } from './MarkdownMessage.jsx';

export function MessageBubble({ message }) {
  const role = String(message.role || 'assistant').toLowerCase();
  return (
    <article className={classNames('message', role, message.tone)}>
      <div className="avatar">{role === 'user' ? 'U' : 'AI'}</div>
      <div className="message-body">
        <span>{role === 'user' ? 'You' : 'Assistant'}</span>
        <MarkdownMessage text={messageText(message)} />
      </div>
    </article>
  );
}
