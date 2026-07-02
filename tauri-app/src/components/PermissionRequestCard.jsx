import { ShieldAlert } from 'lucide-react';
import { safeText } from '../utils/format.js';

export function PermissionRequestCard({ permission, onResolve }) {
  return (
    <article className="message assistant permission-message">
      <div className="avatar"><ShieldAlert size={15} /></div>
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
            <button onClick={() => onResolve(permission, 'deny')} type="button">拒绝</button>
            <button onClick={() => onResolve(permission, 'approve')} type="button">批准</button>
          </div>
        </section>
      </div>
    </article>
  );
}
