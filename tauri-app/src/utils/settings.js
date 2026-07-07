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
  requestRetries: 'night24.requestRetries',
  maxTurns: 'night24.maxTurns',
  turnTimeoutSeconds: 'night24.turnTimeoutSeconds',
  toolTimeoutSeconds: 'night24.toolTimeoutSeconds',
  totalTimeoutMinutes: 'night24.totalTimeoutMinutes',
  networkProxy: 'night24.networkProxy',
  accessMode: 'night24.accessMode',
  fullAccess: 'night24.fullAccess',
  theme: 'night24.theme',
  fontSize: 'night24.fontSize',
  currentWorkspacePath: 'night24.currentWorkspacePath',
  recentWorkspaces: 'night24.recentWorkspaces',
};

export const DEFAULT_CONTEXT_THRESHOLD = '24000';
export const DEFAULT_REQUEST_RETRIES = '2';
export const DEFAULT_MAX_TURNS = '120';
export const DEFAULT_TURN_TIMEOUT_SECONDS = '180';
export const DEFAULT_TOOL_TIMEOUT_SECONDS = '180';
export const DEFAULT_TOTAL_TIMEOUT_MINUTES = '30';

export function normalizeProviderValue(value) {
  const provider = String(value || '').trim().toLowerCase();
  if (provider === 'openai') return 'openai-chat';
  return provider || 'echo';
}

export function providerDisplayName(value) {
  switch (normalizeProviderValue(value)) {
    case 'openai-chat':
      return 'OpenAI Chat';
    case 'openai-responses':
      return 'OpenAI Responses';
    case 'stepfun':
      return 'StepFun';
    case 'anthropic':
      return 'Anthropic';
    case 'ollama':
      return 'Ollama';
    case 'echo':
      return 'Echo';
    default:
      return String(value || 'Provider');
  }
}

export const PROVIDER_DEFAULT_MODELS = {
  echo: 'echo-v1',
  'openai-chat': 'gpt-4o-mini',
  'openai-responses': 'gpt-4o',
  anthropic: 'claude-3-5-sonnet-latest',
  ollama: 'llama3.2',
  stepfun: 'step-3.7-flash',
};

export function providerDefaultModel(value) {
  return PROVIDER_DEFAULT_MODELS[normalizeProviderValue(value)] || '';
}

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
    provider: normalizeProviderValue(overrides.provider),
    model: overrides.model || 'echo-v1',
    baseUrl: overrides.baseUrl || '',
    apiKey: overrides.apiKey || '',
    contextThreshold: overrides.contextThreshold || DEFAULT_CONTEXT_THRESHOLD,
    requestRetries: String(parseRequestRetries(overrides.requestRetries ?? DEFAULT_REQUEST_RETRIES) ?? DEFAULT_REQUEST_RETRIES),
    maxTurns: String(parseMaxTurns(overrides.maxTurns ?? DEFAULT_MAX_TURNS) ?? DEFAULT_MAX_TURNS),
    turnTimeoutSeconds: String(parseTimeoutSeconds(overrides.turnTimeoutSeconds ?? DEFAULT_TURN_TIMEOUT_SECONDS) ?? DEFAULT_TURN_TIMEOUT_SECONDS),
    toolTimeoutSeconds: String(parseTimeoutSeconds(overrides.toolTimeoutSeconds ?? DEFAULT_TOOL_TIMEOUT_SECONDS) ?? DEFAULT_TOOL_TIMEOUT_SECONDS),
    totalTimeoutMinutes: String(parseTimeoutMinutes(overrides.totalTimeoutMinutes ?? DEFAULT_TOTAL_TIMEOUT_MINUTES) ?? DEFAULT_TOTAL_TIMEOUT_MINUTES),
  };
}

function secondsFromMs(value, fallback) {
  const number = Number.parseInt(String(value ?? '').trim(), 10);
  return Number.isFinite(number) && number > 0 ? String(Math.ceil(number / 1000)) : fallback;
}

function minutesFromMs(value, fallback) {
  const number = Number.parseInt(String(value ?? '').trim(), 10);
  return Number.isFinite(number) && number > 0 ? String(Math.ceil(number / 60000)) : fallback;
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
    requestRetries: String(item?.requestRetries ?? item?.request_retries ?? DEFAULT_REQUEST_RETRIES),
    maxTurns: String(item?.maxTurns ?? item?.max_turns ?? DEFAULT_MAX_TURNS),
    turnTimeoutSeconds: String(item?.turnTimeoutSeconds ?? secondsFromMs(item?.turn_timeout_ms, DEFAULT_TURN_TIMEOUT_SECONDS)),
    toolTimeoutSeconds: String(item?.toolTimeoutSeconds ?? secondsFromMs(item?.tool_timeout_ms, DEFAULT_TOOL_TIMEOUT_SECONDS)),
    totalTimeoutMinutes: String(item?.totalTimeoutMinutes ?? minutesFromMs(item?.total_timeout_ms, DEFAULT_TOTAL_TIMEOUT_MINUTES)),
  }));
  if (profiles.length) return profiles;
  return [createProviderProfile({
    id: 'default-provider',
    name: '默认供应商',
    provider: normalizeProviderValue(readSetting(STORAGE_KEYS.provider, 'echo')),
    model: readSetting(STORAGE_KEYS.model, 'echo-v1'),
    baseUrl: readSetting(STORAGE_KEYS.baseUrl),
    apiKey: readSetting(STORAGE_KEYS.providerKey),
    contextThreshold: readSetting(STORAGE_KEYS.contextThreshold, DEFAULT_CONTEXT_THRESHOLD),
    requestRetries: readSetting(STORAGE_KEYS.requestRetries, DEFAULT_REQUEST_RETRIES),
    maxTurns: readSetting(STORAGE_KEYS.maxTurns, DEFAULT_MAX_TURNS),
    turnTimeoutSeconds: readSetting(STORAGE_KEYS.turnTimeoutSeconds, DEFAULT_TURN_TIMEOUT_SECONDS),
    toolTimeoutSeconds: readSetting(STORAGE_KEYS.toolTimeoutSeconds, DEFAULT_TOOL_TIMEOUT_SECONDS),
    totalTimeoutMinutes: readSetting(STORAGE_KEYS.totalTimeoutMinutes, DEFAULT_TOTAL_TIMEOUT_MINUTES),
  })];
}

export function readProviderProfiles() {
  return normalizeProviderProfiles(readJsonSetting(STORAGE_KEYS.providerProfiles, []));
}

export function providerProfileById(profiles, id) {
  const source = Array.isArray(profiles) ? profiles : [];
  return source.find((profile) => profile?.id === id) || null;
}

export function activeProviderProfile(profiles, id) {
  const source = Array.isArray(profiles) ? profiles : [];
  return providerProfileById(source, id) || source[0] || null;
}

export function validProviderProfileId(profiles, id) {
  return activeProviderProfile(profiles, id)?.id || '';
}

export function providerProfileFormState(profile) {
  const provider = normalizeProviderValue(profile?.provider);
  return {
    provider,
    model: profile?.model || providerDefaultModel(provider),
    baseUrl: profile?.baseUrl || '',
    apiKey: profile?.apiKey || '',
    contextThreshold: profile?.contextThreshold || DEFAULT_CONTEXT_THRESHOLD,
    requestRetries: profile?.requestRetries || DEFAULT_REQUEST_RETRIES,
    maxTurns: profile?.maxTurns || DEFAULT_MAX_TURNS,
    turnTimeoutSeconds: profile?.turnTimeoutSeconds || DEFAULT_TURN_TIMEOUT_SECONDS,
    toolTimeoutSeconds: profile?.toolTimeoutSeconds || DEFAULT_TOOL_TIMEOUT_SECONDS,
    totalTimeoutMinutes: profile?.totalTimeoutMinutes || DEFAULT_TOTAL_TIMEOUT_MINUTES,
  };
}

export function parseOptionalPositiveInt(value) {
  const number = Number.parseInt(String(value || '').trim(), 10);
  return Number.isFinite(number) && number > 0 ? number : undefined;
}

export function parseRequestRetries(value) {
  const number = Number.parseInt(String(value ?? '').trim(), 10);
  if (!Number.isFinite(number) || number < 0) return undefined;
  return Math.min(number, 5);
}

export function parseMaxTurns(value) {
  const number = Number.parseInt(String(value ?? '').trim(), 10);
  if (!Number.isFinite(number) || number < 1) return undefined;
  return Math.min(number, 1000);
}

export function parseTimeoutSeconds(value) {
  const number = Number.parseInt(String(value ?? '').trim(), 10);
  if (!Number.isFinite(number) || number < 1) return undefined;
  return Math.min(number, 24 * 60 * 60);
}

export function parseTimeoutMinutes(value) {
  const number = Number.parseInt(String(value ?? '').trim(), 10);
  if (!Number.isFinite(number) || number < 1) return undefined;
  return Math.min(number, 24 * 60);
}

export function parseTimeoutSecondsMs(value) {
  const seconds = parseTimeoutSeconds(value);
  return seconds ? seconds * 1000 : undefined;
}

export function parseTimeoutMinutesMs(value) {
  const minutes = parseTimeoutMinutes(value);
  return minutes ? minutes * 60 * 1000 : undefined;
}

function normalizeApiBase(base) {
  const value = String(base || '').trim();
  return (value || DEFAULT_SERVER).replace(/\/+$/, '');
}

export function apiUrl(base, path) {
  const normalizedBase = normalizeApiBase(base);
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;
  return `${normalizedBase}${normalizedPath}`;
}

export function apiAuthHeaders(apiKey) {
  const key = String(apiKey || '').trim();
  return key ? { Authorization: `Bearer ${key}`, 'X-API-Key': key } : {};
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
    .trim()
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
      const key = normalizeLocalPath(item.root_path);
      if (!key) return false;
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
