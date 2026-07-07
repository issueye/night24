import { classNames } from '../../utils/format.js';

export function Tabs({
  ariaLabel,
  children,
  className,
  empty = null,
  listClassName,
  trailing = null,
}) {
  function handleKeyDown(event) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    const tabs = [...event.currentTarget.querySelectorAll('[role="tab"]:not(:disabled)')];
    if (tabs.length === 0) return;
    const currentIndex = Math.max(0, tabs.indexOf(document.activeElement));
    let nextIndex = currentIndex;
    if (event.key === 'ArrowLeft') nextIndex = (currentIndex - 1 + tabs.length) % tabs.length;
    if (event.key === 'ArrowRight') nextIndex = (currentIndex + 1) % tabs.length;
    if (event.key === 'Home') nextIndex = 0;
    if (event.key === 'End') nextIndex = tabs.length - 1;
    event.preventDefault();
    tabs[nextIndex].focus();
    tabs[nextIndex].click();
  }

  return (
    <div className={classNames('ui-tabs', className)}>
      <div
        className={classNames('ui-tabs-list', listClassName)}
        role="tablist"
        aria-label={ariaLabel}
        onKeyDown={handleKeyDown}
      >
        {empty}
        {children}
      </div>
      {trailing}
    </div>
  );
}

export function Tab({
  active = false,
  children,
  className,
  id,
  onSelect,
  title,
  ...props
}) {
  return (
    <button
      className={classNames('ui-tab', active && 'active', className)}
      id={id}
      onClick={onSelect}
      role="tab"
      aria-selected={active}
      tabIndex={active ? 0 : -1}
      title={title}
      type="button"
      {...props}
    >
      {children}
    </button>
  );
}
