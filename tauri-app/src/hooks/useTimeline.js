import { useCallback, useState } from 'react';

const TIMELINE_LIMIT = 80;

export function useTimeline() {
  const [timeline, setTimeline] = useState([]);

  const addTimeline = useCallback((type, title, detail, tone = 'neutral') => {
    setTimeline((items) => [
      {
        id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        type,
        title,
        detail,
        tone,
        createdAt: new Date().toISOString(),
      },
      ...items,
    ].slice(0, TIMELINE_LIMIT));
  }, []);

  const clearTimeline = useCallback(() => {
    setTimeline([]);
  }, []);

  return { timeline, addTimeline, clearTimeline };
}
