export const HOOK_EVENTS = [
  'run_started',
  'before_provider_request',
  'before_tool',
  'after_tool',
  'permission_required',
  'run_finished',
  'run_failed',
];

export function modulesToText(modules) {
  return Array.isArray(modules) ? modules.join('\n') : '';
}

export function textToModules(text) {
  return String(text || '')
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

export function createHook(overrides = {}) {
  const hasAllowedModules = Array.isArray(overrides.allowed_modules);
  return {
    id: overrides.id || `hook-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    event: overrides.event || 'before_tool',
    name: overrides.name || 'hook',
    engine: 'gts',
    script: overrides.script || '',
    inline_script: overrides.inline_script || '',
    enabled: overrides.enabled ?? true,
    timeout_ms: overrides.timeout_ms ?? 5000,
    instruction_limit: overrides.instruction_limit ?? 1000000,
    allowed_modules_enabled: overrides.allowed_modules_enabled ?? hasAllowedModules,
    allowed_modules_text: overrides.allowed_modules_text ?? modulesToText(overrides.allowed_modules),
  };
}

export function normalizeHook(item, index) {
  return createHook({
    id: item?.id || `hook-${index}-${Date.now()}`,
    event: HOOK_EVENTS.includes(item?.event) ? item.event : undefined,
    name: item?.name,
    script: item?.script,
    inline_script: item?.inline_script,
    enabled: item?.enabled,
    timeout_ms: item?.timeout_ms,
    instruction_limit: item?.instruction_limit,
    allowed_modules: item?.allowed_modules,
  });
}

export function hooksToConfig(hooks) {
  return {
    hooks: hooks.map(({ id, ...hook }) => ({
      ...hook,
      engine: 'gts',
      event: HOOK_EVENTS.includes(hook.event) ? hook.event : 'before_tool',
      name: hook.name?.trim() || undefined,
      script: hook.script?.trim() || undefined,
      inline_script: hook.inline_script?.trim() || undefined,
      timeout_ms: Number.parseInt(String(hook.timeout_ms || ''), 10) || undefined,
      instruction_limit: Number.parseInt(String(hook.instruction_limit || ''), 10) || undefined,
      allowed_modules: hook.allowed_modules_enabled ? textToModules(hook.allowed_modules_text) : undefined,
    })),
  };
}
