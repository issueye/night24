import { useEffect, useRef, useState } from 'react';
import { Check, ChevronDown } from 'lucide-react';
import { classNames } from '../../utils/format.js';

export function Select({
  className,
  disabled = false,
  label,
  onChange,
  options = [],
  placeholder = 'Select',
  size = 'md',
  value,
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef(null);
  const selected = options.find((item) => item.value === value);

  useEffect(() => {
    if (!open) return undefined;
    function handlePointerDown(event) {
      if (!rootRef.current?.contains(event.target)) setOpen(false);
    }
    function handleKeyDown(event) {
      if (event.key === 'Escape') setOpen(false);
    }
    window.addEventListener('pointerdown', handlePointerDown);
    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('pointerdown', handlePointerDown);
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [open]);

  function choose(nextValue) {
    onChange?.(nextValue);
    setOpen(false);
  }

  return (
    <span className={classNames('ui-select', `ui-select-${size}`, open && 'open', disabled && 'disabled', className)} ref={rootRef}>
      {label && <span className="ui-select-label">{label}</span>}
      <button
        aria-expanded={open}
        className="ui-select-trigger"
        disabled={disabled}
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <span>{selected?.label || placeholder}</span>
        <ChevronDown size={13} />
      </button>
      {open && (
        <span className="ui-select-menu" role="listbox">
          {options.map((item) => (
            <button
              aria-selected={item.value === value}
              className={classNames('ui-select-option', item.value === value && 'selected')}
              disabled={item.disabled}
              key={item.value}
              onClick={() => choose(item.value)}
              role="option"
              type="button"
            >
              <span>{item.label}</span>
              {item.value === value && <Check size={13} />}
            </button>
          ))}
        </span>
      )}
    </span>
  );
}
