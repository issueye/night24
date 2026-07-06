import { classNames } from '../../utils/format.js';

export function Popover({ children, className, content, placement = 'top', size = 'md', triggerClassName }) {
  return (
    <span className={classNames('ui-popover', `ui-popover-${placement}`, `ui-popover-${size}`, className)}>
      <span className={classNames('ui-popover-trigger', triggerClassName)}>{children}</span>
      <span className="ui-popover-content" role="tooltip">
        {content}
      </span>
    </span>
  );
}
