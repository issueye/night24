import { classNames } from '../../utils/format.js';

export function Avatar({ children, className, label, size = 'md', tone = 'assistant' }) {
  return <span className={classNames('ui-avatar', `ui-avatar-${tone}`, `ui-avatar-${size}`, className)}>{children || label}</span>;
}
