import { classNames } from '../../utils/format.js';

function renderChildren(children) {
  if (children == null || children === false) return null;
  if (typeof children === 'string' || typeof children === 'number') {
    return <span>{children}</span>;
  }
  return children;
}

export function Button({
  as: Component = 'button',
  children,
  className,
  icon,
  size = 'md',
  tone = 'neutral',
  variant = 'solid',
  type = 'button',
  ...props
}) {
  return (
    <Component
      className={classNames('ui-button', `ui-button-${variant}`, `ui-button-${tone}`, `ui-button-${size}`, className)}
      type={Component === 'button' ? type : undefined}
      {...props}
    >
      {icon}
      {renderChildren(children)}
    </Component>
  );
}

export function IconButton({
  children,
  className,
  label,
  size = 'md',
  tone = 'neutral',
  variant = 'ghost',
  type = 'button',
  ...props
}) {
  return (
    <button
      aria-label={label}
      className={classNames('ui-icon-button', `ui-button-${variant}`, `ui-button-${tone}`, `ui-button-${size}`, className)}
      title={props.title || label}
      type={type}
      {...props}
    >
      {children}
    </button>
  );
}
