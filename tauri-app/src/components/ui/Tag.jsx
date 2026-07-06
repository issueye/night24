import { classNames } from '../../utils/format.js';

export function Tag({ children, className, icon, size = 'md', tone = 'neutral' }) {
  return (
    <span className={classNames('ui-tag', `ui-tag-${tone}`, `ui-tag-${size}`, className)}>
      {icon}
      <span>{children}</span>
    </span>
  );
}
