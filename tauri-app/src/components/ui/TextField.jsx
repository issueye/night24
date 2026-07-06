import { classNames } from '../../utils/format.js';

export function TextField({ as = 'input', className, icon, label, size = 'md', ...props }) {
  const Component = as;
  return (
    <label className={classNames('ui-field', `ui-field-${size}`, className)}>
      {label && <span>{label}</span>}
      {icon ? (
        <span className="ui-field-affix">
          {icon}
          <Component className="ui-field-control" {...props} />
        </span>
      ) : (
        <Component className="ui-field-control" {...props} />
      )}
    </label>
  );
}
