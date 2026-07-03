import { useCallback, useMemo } from 'react';
import { apiUrl } from '../utils/settings.js';

export function useApiClient(apiBase, apiKey) {
  const headers = useMemo(() => {
    const next = { 'Content-Type': 'application/json' };
    if (apiKey.trim()) {
      next.Authorization = `Bearer ${apiKey.trim()}`;
      next['X-API-Key'] = apiKey.trim();
    }
    return next;
  }, [apiKey]);

  const apiJson = useCallback(
    async (path, options = {}) => {
      const response = await fetch(apiUrl(apiBase, path), {
        ...options,
        headers: {
          ...(options.body ? { 'Content-Type': 'application/json' } : {}),
          ...(apiKey.trim() ? { Authorization: `Bearer ${apiKey.trim()}`, 'X-API-Key': apiKey.trim() } : {}),
          ...(options.headers || {}),
        },
      });
      const text = await response.text();
      if (!response.ok) {
        let detail = text;
        try {
          detail = JSON.parse(text).error || text;
        } catch {
          // Keep raw response text.
        }
        throw new Error(detail || `HTTP ${response.status}`);
      }
      if (!text) return null;
      try {
        return JSON.parse(text);
      } catch {
        return text;
      }
    },
    [apiBase, apiKey],
  );

  return { headers, apiJson };
}
