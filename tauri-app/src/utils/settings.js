export const DEFAULT_SERVER = 'http://localhost:17787';

export const STORAGE_KEYS = {
  apiBase: 'night24.apiBase',
  apiKey: 'night24.apiKey',
  provider: 'night24.provider',
  providerProfiles: 'night24.providerProfiles',
  providerProfileId: 'night24.providerProfileId',
  model: 'night24.model',
  baseUrl: 'night24.baseUrl',
  providerKey: 'night24.providerKey',
  contextThreshold: 'night24.contextThreshold',
  networkProxy: 'night24.networkProxy',
  accessMode: 'night24.accessMode',
  fullAccess: 'night24.fullAccess',
  theme: 'night24.theme',
  fontSize: 'night24.fontSize',
  currentWorkspacePath: 'night24.currentWorkspacePath',
  recentWorkspaces: 'night24.recentWorkspaces',
};

export const DEFAULT_CONTEXT_THRESHOLD = '24000';

export function readSetting(key, fallback = '') {
  try {
    return localStorage.getItem(key) || fallback;
  } catch {
    return fallback;
  }
}

export function writeSetting(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Ignore private-mode storage failures.
  }
}

export function readJsonSetting(key, fallback) {
  try {
    const raw = localStorage.getItem(key);
    return raw ? JSON.parse(raw) : fallback;
  } catch {
    return fallback;
  }
}

export function writeJsonSetting(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Ignore private-mode storage failures.
  }
}

export function readAccessMode() {
  const mode = readSetting(STORAGE_KEYS.accessMode);
  if (['strict', 'permissive', 'allow_all'].includes(mode)) return mode;
  return readSetting(STORAGE_KEYS.fullAccess) === 'true' ? 'allow_all' : 'strict';
}

export function createProviderProfile(overrides = {}) {
  return {
    id: overrides.id || `provider-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    name: overrides.name || '供应商',
    provider: overrides.provider || 'echo',
    model: overrides.model || 'echo-v1',
    baseUrl: overrides.baseUrl || '',
    apiKey: overrides.apiKey || '',
    contextThreshold: overrides.contextThreshold || DEFAULT_CONTEXT_THRESHOLD,
  };
}

export function normalizeProviderProfiles(value) {
  const source = Array.isArray(value) ? value : [];
  const profiles = source.map((item) => createProviderProfile({
    id: item?.id,
    name: item?.name,
    provider: item?.provider,
    model: item?.model,
    baseUrl: item?.baseUrl ?? item?.base_url,
    apiKey: item?.apiKey ?? item?.api_key,
    contextThreshold: String(item?.contextThreshold ?? item?.context_threshold_tokens ?? DEFAULT_CONTEXT_THRESHOLD),
  }));
  if (profiles.length) return profiles;
  return [createProviderProfile({
    id: 'default-provider',
    name: '默认供应商',
    provider: readSetting(STORAGE_KEYS.provider, 'echo'),
    model: readSetting(STORAGE_KEYS.model, 'echo-v1'),
    baseUrl: readSetting(STORAGE_KEYS.baseUrl),
    apiKey: readSetting(STORAGE_KEYS.providerKey),
    contextThreshold: readSetting(STORAGE_KEYS.contextThreshold, DEFAULT_CONTEXT_THRESHOLD),
  })];
}

export function readProviderProfiles() {
  return normalizeProviderProfiles(readJsonSetting(STORAGE_KEYS.providerProfiles, []));
}

export function parseOptionalPositiveInt(value) {
  const number = Number.parseInt(String(value || '').trim(), 10);
  return Number.isFinite(number) && number > 0 ? number : undefined;
}

export function apiUrl(base, path) {
  const normalizedBase = String(base || DEFAULT_SERVER).replace(/\/+$/, '');
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;
  return `${normalizedBase}${normalizedPath}`;
}

export function workspaceNameFromPath(path) {
  return String(path || '')
    .replace(/[\\/]+$/, '')
    .split(/[\\/]/)
    .filter(Boolean)
    .pop() || 'workspace';
}

function normalizeLocalPath(path) {
  return String(path || '')
    .replace(/\\/g, '/')
    .replace(/\/+$/, '')
    .toLowerCase();
}

export function sameWorkspacePath(left, right) {
  const a = normalizeLocalPath(left);
  const b = normalizeLocalPath(right);
  return Boolean(a && b && a === b);
}

export function compactWorkspaces(workspaces) {
  const seen = new Set();
  return (Array.isArray(workspaces) ? workspaces : [])
    .filter((item) => item?.root_path)
    .filter((item) => {
      const key = String(item.root_path).toLowerCase();
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .slice(0, 8);
}

export function rememberWorkspace(workspace) {
  if (!workspace?.root_path) return;
  writeSetting(STORAGE_KEYS.currentWorkspacePath, workspace.root_path);
  const stored = readJsonSetting(STORAGE_KEYS.recentWorkspaces, []);
  const next = compactWorkspaces([
    {
      id: workspace.id || `local-${workspace.root_path}`,
      name: workspace.name || workspaceNameFromPath(workspace.root_path),
      root_path: workspace.root_path,
      created_at: workspace.created_at || new Date().toISOString(),
      last_opened_at: workspace.last_opened_at || new Date().toISOString(),
    },
    ...stored,
  ]);
  writeJsonSetting(STORAGE_KEYS.recentWorkspaces, next);
}
