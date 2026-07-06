import { useCallback, useRef, useState } from 'react';

const DEFAULT_DURATION = 2800;

export function useToasts() {
  const [toasts, setToasts] = useState([]);
  const timersRef = useRef(new Map());

  const dismissToast = useCallback((id) => {
    window.clearTimeout(timersRef.current.get(id));
    timersRef.current.delete(id);
    setToasts((items) => items.filter((item) => item.id !== id));
  }, []);

  const notify = useCallback((messageOrOptions, options = {}) => {
    const config = typeof messageOrOptions === 'string'
      ? { ...options, message: messageOrOptions }
      : { ...messageOrOptions };
    const id = config.id || `toast-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const duration = config.loading ? 0 : Number(config.duration ?? DEFAULT_DURATION);
    const toast = {
      id,
      message: config.message || '',
      detail: config.detail || '',
      loading: Boolean(config.loading),
      tone: config.tone || 'neutral',
    };

    setToasts((items) => [toast, ...items.filter((item) => item.id !== id)].slice(0, 5));
    window.clearTimeout(timersRef.current.get(id));
    timersRef.current.delete(id);
    if (duration > 0) {
      const timer = window.setTimeout(() => dismissToast(id), duration);
      timersRef.current.set(id, timer);
    }
    return id;
  }, [dismissToast]);

  return {
    dismissToast,
    notify,
    toasts,
  };
}
