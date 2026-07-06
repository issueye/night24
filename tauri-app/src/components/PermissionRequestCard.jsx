import { ShieldAlert } from 'lucide-react';
import { safeText } from '../utils/format.js';
import { Avatar, Button } from './ui/index.js';

export function PermissionRequestCard({ permission, onResolve }) {
  return (
    <article className="message assistant permission-message">
      <Avatar tone="warning"><ShieldAlert size={15} /></Avatar>
      <div className="message-body">
        <span>Permission</span>
        <section className="permission-chat-card">
          <div className="permission-chat-head">
            <strong>{permission.tool_name}</strong>
            <small>{permission.risk || 'high'}</small>
          </div>
          <p>{permission.summary}</p>
          <pre>{safeText(permission.arguments)}</pre>
          <div className="permission-chat-actions">
            <Button onClick={() => onResolve(permission, 'deny')} variant="soft">拒绝</Button>
            <Button onClick={() => onResolve(permission, 'approve')} tone="primary">批准</Button>
          </div>
        </section>
      </div>
    </article>
  );
}
