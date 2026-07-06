import { classNames } from '../../utils/format.js';

export function ProgressRing({ className, label, percent = 0, size = 'md', tone = 'neutral' }) {
  const value = Math.max(0, Math.min(100, Number(percent) || 0));
  return (
    <svg
      aria-label={label || `Progress ${value}%`}
      className={classNames('ui-progress-ring', `ui-progress-ring-${tone}`, `ui-progress-ring-${size}`, className)}
      role="img"
      style={{ '--ring-offset': 100 - value }}
      viewBox="0 0 36 36"
    >
      <circle className="ui-progress-ring-track" cx="18" cy="18" r="14" />
      <circle
        className="ui-progress-ring-value"
        cx="18"
        cy="18"
        pathLength="100"
        r="14"
        strokeDasharray="100"
        strokeDashoffset="var(--ring-offset)"
      />
    </svg>
  );
}
