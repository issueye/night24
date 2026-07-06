import { classNames } from '../../utils/format.js';

export function ChatInput({
  className,
  disabled = false,
  onChange,
  onSubmit,
  placeholder,
  size = 'md',
  value,
  ...props
}) {
  return (
    <textarea
      className={classNames('ui-chat-input', `ui-chat-input-${size}`, className)}
      disabled={disabled}
      onChange={(event) => onChange?.(event.target.value)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && !event.shiftKey) {
          event.preventDefault();
          onSubmit?.();
        }
      }}
      placeholder={placeholder}
      value={value}
      {...props}
    />
  );
}
