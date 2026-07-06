import { classNames } from '../../utils/format.js';

export function Toast({ children, className, detail, icon, loading = false, message, onClose, size = 'md', tone = 'neutral' }) {
  return (
    <div className={classNames('ui-toast', `ui-toast-${tone}`, `ui-toast-${size}`, loading && 'loading', className)} role="status">
      {icon && <span className="ui-toast-icon">{icon}</span>}
      <span className="ui-toast-body">
        <strong>{message || children}</strong>
        {detail && <small>{detail}</small>}
      </span>
      {onClose && (
        <button aria-label="关闭消息" className="ui-toast-close" onClick={onClose} type="button">
          ×
        </button>
      )}
    </div>
  );
}
