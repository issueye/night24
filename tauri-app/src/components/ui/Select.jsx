import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, ChevronDown } from 'lucide-react';
import { classNames } from '../../utils/format.js';

export function Select({
  className,
  disabled = false,
  label,
  menuClassName,
  onChange,
  options = [],
  placeholder = 'Select',
  size = 'md',
  title,
  value,
}) {
  const [open, setOpen] = useState(false);
  const [menuStyle, setMenuStyle] = useState(null);
  const rootRef = useRef(null);
  const menuRef = useRef(null);
  const triggerRef = useRef(null);
  const selected = options.find((item) => item.value === value);
  const optionLayoutKey = options.map((item) => `${item.value}:${item.label}`).join('\n');

  useLayoutEffect(() => {
    if (!open) return undefined;

    function updateMenuPosition() {
      const trigger = triggerRef.current;
      if (!trigger) return;

      const rect = trigger.getBoundingClientRect();
      const viewportHeight = window.innerHeight || document.documentElement.clientHeight;
      const viewportWidth = window.innerWidth || document.documentElement.clientWidth;
      const margin = 8;
      const gap = 6;
      const maxMenuWidth = Math.max(rect.width, Math.min(420, viewportWidth - margin * 2));
      const measuredMenuWidth = estimateMenuWidth(trigger, options);
      const width = Math.min(maxMenuWidth, Math.max(rect.width, measuredMenuWidth));
      const contentHeight = Math.min(260, Math.max(42, options.length * 30 + 10));
      const spaceBelow = viewportHeight - rect.bottom - margin;
      const spaceAbove = rect.top - margin;
      const placeAbove = spaceBelow < contentHeight + gap && spaceAbove > spaceBelow;
      const availableHeight = Math.max(42, placeAbove ? spaceAbove - gap : spaceBelow - gap);
      const menuHeight = Math.min(contentHeight, availableHeight);
      const top = placeAbove
        ? Math.max(margin, rect.top - menuHeight - gap)
        : Math.min(viewportHeight - menuHeight - margin, rect.bottom + gap);
      const preferredLeft = Math.max(margin, rect.left);
      const left = Math.min(preferredLeft, Math.max(margin, viewportWidth - width - margin));

      setMenuStyle({
        left: `${left}px`,
        top: `${Math.max(8, top)}px`,
        width: `${width}px`,
        maxHeight: `${menuHeight}px`,
      });
    }

    updateMenuPosition();
    window.addEventListener('resize', updateMenuPosition);
    window.addEventListener('scroll', updateMenuPosition, true);
    return () => {
      window.removeEventListener('resize', updateMenuPosition);
      window.removeEventListener('scroll', updateMenuPosition, true);
    };
  }, [open, optionLayoutKey]);

  useEffect(() => {
    if (!open) return undefined;
    function handlePointerDown(event) {
      if (rootRef.current?.contains(event.target) || menuRef.current?.contains(event.target)) return;
      setOpen(false);
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
        aria-label={title || label || placeholder}
        className="ui-select-trigger"
        disabled={disabled}
        onClick={() => setOpen((value) => !value)}
        ref={triggerRef}
        title={title}
        type="button"
      >
        <span>{selected?.label || placeholder}</span>
        <ChevronDown size={13} />
      </button>
      {open && createPortal(
        <span className={classNames('ui-select-menu', menuClassName)} ref={menuRef} role="listbox" style={menuStyle || undefined}>
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
        </span>,
        document.body,
      )}
    </span>
  );
}

function estimateMenuWidth(trigger, options) {
  const computed = window.getComputedStyle(trigger);
  const font = `${computed.fontStyle} ${computed.fontVariant} ${computed.fontWeight} ${computed.fontSize} ${computed.fontFamily}`;
  const longestLabelWidth = options.reduce((width, item) => {
    const label = String(item.label || '');
    return Math.max(width, measureTextWidth(label, font));
  }, 0);

  return Math.ceil(longestLabelWidth + 54);
}

function measureTextWidth(text, font) {
  if (!measureTextWidth.canvas) {
    measureTextWidth.canvas = document.createElement('canvas');
  }
  const context = measureTextWidth.canvas.getContext('2d');
  if (!context) return text.length * 8;
  context.font = font;
  return context.measureText(text).width;
}
