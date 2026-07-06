import { useCallback, useMemo } from 'react';
import { apiAuthHeaders, apiUrl } from '../utils/settings.js';

function normalizeRequestHeaders(headers) {
  if (!headers) return {};
  if (typeof Headers !== 'undefined' && headers instanceof Headers) {
    return Object.fromEntries(headers.entries());
  }
  if (Array.isArray(headers)) {
    return Object.fromEntries(headers);
  }
  return { ...headers };
}

function apiRequestHeaders(apiKey, { json = false, headers = null } = {}) {
  return {
    ...(json ? { 'Content-Type': 'application/json' } : {}),
    ...apiAuthHeaders(apiKey),
    ...normalizeRequestHeaders(headers),
  };
}

export function useApiClient(apiBase, apiKey) {
  const headers = useMemo(() => {
    return apiRequestHeaders(apiKey, { json: true });
  }, [apiKey]);

  const apiJson = useCallback(
    async (path, options = {}) => {
      const response = await fetch(apiUrl(apiBase, path), {
        ...options,
        headers: apiRequestHeaders(apiKey, {
          json: Boolean(options.body),
          headers: options.headers,
        }),
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
