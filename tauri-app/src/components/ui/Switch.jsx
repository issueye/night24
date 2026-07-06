import { classNames } from '../../utils/format.js';

export function Switch({ checked, className, disabled, label, onChange, size = 'md' }) {
  return (
    <label className={classNames('ui-switch', `ui-switch-${size}`, checked && 'checked', disabled && 'disabled', className)}>
      <input
        checked={Boolean(checked)}
        disabled={disabled}
        onChange={(event) => onChange?.(event.target.checked)}
        type="checkbox"
      />
      <span className="ui-switch-track"><span /></span>
      {label && <span className="ui-switch-label">{label}</span>}
    </label>
  );
}
