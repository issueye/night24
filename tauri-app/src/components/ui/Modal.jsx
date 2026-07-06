import { X } from 'lucide-react';
import { classNames } from '../../utils/format.js';
import { IconButton } from './Button.jsx';

export function Modal({
  ariaLabel,
  bodyClassName,
  children,
  className,
  headClassName,
  onBackdropMouseDown,
  onClose,
  open,
  size = 'md',
  subtitle,
  title,
}) {
  if (!open) return null;
  return (
    <div className="ui-modal-backdrop" onMouseDown={onBackdropMouseDown} role="presentation">
      <section
        aria-label={ariaLabel || title}
        aria-modal="true"
        className={classNames('ui-modal', `ui-modal-${size}`, className)}
        onMouseDown={(event) => event.stopPropagation()}
        role="dialog"
      >
        <header className={classNames('ui-modal-head', headClassName)}>
          <div>
            <strong>{title}</strong>
            {subtitle && <span>{subtitle}</span>}
          </div>
          {onClose && (
            <IconButton label="关闭" onClick={onClose} size="sm">
              <X size={14} />
            </IconButton>
          )}
        </header>
        <div className={classNames('ui-modal-body', bodyClassName)}>{children}</div>
      </section>
    </div>
  );
}
