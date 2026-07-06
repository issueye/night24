import { classNames } from '../../utils/format.js';

export function ProgressBar({ className, label, percent = 0, size = 'md', tone = 'neutral' }) {
  const value = Math.max(0, Math.min(100, Number(percent) || 0));
  return (
    <span
      aria-label={label || `Progress ${value}%`}
      aria-valuemax={100}
      aria-valuemin={0}
      aria-valuenow={value}
      className={classNames('ui-progress-bar', `ui-progress-bar-${tone}`, `ui-progress-bar-${size}`, className)}
      role="progressbar"
    >
      <span style={{ width: `${value}%` }} />
    </span>
  );
}
