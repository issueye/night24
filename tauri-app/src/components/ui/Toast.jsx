import { classNames } from '../../utils/format.js';

export function Toast({ children, className, size = 'md', tone = 'neutral' }) {
  return <div className={classNames('ui-toast', `ui-toast-${tone}`, `ui-toast-${size}`, className)}>{children}</div>;
}
