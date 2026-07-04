import { Plus, RefreshCw, Save, Trash2, Workflow } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { normalizeError } from '../../utils/events.js';
import { classNames } from '../../utils/format.js';

const EVENTS = [
  'run_started',
  'before_provider_request',
  'before_tool',
  'after_tool',
  'permission_required',
  'run_finished',
  'run_failed',
];

function createHook(overrides = {}) {
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
  };
}

function normalizeHook(item, index) {
  return createHook({
    id: item?.id || `hook-${index}-${Date.now()}`,
    event: EVENTS.includes(item?.event) ? item.event : undefined,
    name: item?.name,
    script: item?.script,
    inline_script: item?.inline_script,
    enabled: item?.enabled,
    timeout_ms: item?.timeout_ms,
    instruction_limit: item?.instruction_limit,
  });
}

function toConfig(hooks) {
  return {
    hooks: hooks.map(({ id, ...hook }) => ({
      ...hook,
      engine: 'gts',
      event: EVENTS.includes(hook.event) ? hook.event : 'before_tool',
      name: hook.name?.trim() || undefined,
      script: hook.script?.trim() || undefined,
      inline_script: hook.inline_script?.trim() || undefined,
      timeout_ms: Number.parseInt(String(hook.timeout_ms || ''), 10) || undefined,
      instruction_limit: Number.parseInt(String(hook.instruction_limit || ''), 10) || undefined,
    })),
  };
}

export function HookSettings({ apiJson, workspace }) {
  const [hooks, setHooks] = useState([]);
  const [activeId, setActiveId] = useState(null);
  const [configPath, setConfigPath] = useState('');
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [savedAt, setSavedAt] = useState('');

  const activeHook = useMemo(
    () => hooks.find((hook) => hook.id === activeId) || hooks[0] || null,
    [activeId, hooks],
  );

  async function loadHooks() {
    if (!workspace) {
      setHooks([]);
      setActiveId(null);
      setConfigPath('');
      setError('');
      return;
    }
    setLoading(true);
    setError('');
    setSavedAt('');
    try {
      const data = await apiJson('/workspace/hooks');
      const nextHooks = (data?.config?.hooks || []).map(normalizeHook);
      setHooks(nextHooks);
      setActiveId((current) => nextHooks.find((hook) => hook.id === current)?.id || nextHooks[0]?.id || null);
      setConfigPath(data?.path || '');
    } catch (err) {
      setError(normalizeError(err));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    loadHooks();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace?.root_path]);

  function updateActive(patch) {
    if (!activeHook) return;
    setSavedAt('');
    setHooks((items) => items.map((item) => (item.id === activeHook.id ? { ...item, ...patch } : item)));
  }

  function addHook() {
    const hook = createHook({ name: `hook-${hooks.length + 1}` });
    setSavedAt('');
    setHooks((items) => [...items, hook]);
    setActiveId(hook.id);
  }

  function deleteHook() {
    if (!activeHook) return;
    const next = hooks.filter((item) => item.id !== activeHook.id);
    setSavedAt('');
    setHooks(next);
    setActiveId(next[0]?.id || null);
  }

  async function saveHooks() {
    setSaving(true);
    setError('');
    try {
      const data = await apiJson('/workspace/hooks', {
        method: 'PUT',
        body: JSON.stringify(toConfig(hooks)),
      });
      const nextHooks = (data?.config?.hooks || []).map(normalizeHook);
      setHooks(nextHooks);
      setActiveId((current) => nextHooks.find((hook) => hook.id === current)?.id || nextHooks[0]?.id || null);
      setConfigPath(data?.path || '');
      setSavedAt(new Date().toLocaleTimeString());
    } catch (err) {
      setError(normalizeError(err));
    } finally {
      setSaving(false);
    }
  }

  if (!workspace) {
    return (
      <div className="hook-empty">
        <Workflow size={18} />
        <strong>先打开项目</strong>
        <span>钩子配置会保存到项目目录。</span>
      </div>
    );
  }

  return (
    <div className="hook-manager">
      <aside className="provider-list hook-list" aria-label="钩子列表">
        <div className="provider-list-head">
          <strong>钩子</strong>
          <div className="hook-list-actions">
            <button className="icon-button compact" disabled={loading} onClick={loadHooks} title="重新加载" type="button">
              <RefreshCw size={14} />
            </button>
            <button className="icon-button compact" onClick={addHook} title="新增钩子" type="button">
              <Plus size={14} />
            </button>
          </div>
        </div>
        <div className="provider-list-scroll">
          {hooks.length === 0 && <div className="hook-list-empty">暂无钩子</div>}
          {hooks.map((hook) => (
            <button
              className={classNames('provider-profile-row', hook.id === activeHook?.id && 'active', !hook.enabled && 'muted')}
              key={hook.id}
              onClick={() => setActiveId(hook.id)}
              type="button"
            >
              <strong>{hook.name || hook.event}</strong>
              <span>{hook.event} · {hook.enabled ? '启用' : '停用'}</span>
            </button>
          ))}
        </div>
      </aside>

      <div className="hook-editor">
        <div className="hook-toolbar">
          <span>{loading ? '加载中' : configPath || '.night24/hooks.json'}</span>
          <div>
            <button className="danger-outline-button compact-action" disabled={!activeHook} onClick={deleteHook} type="button">
              <Trash2 size={14} />
              删除
            </button>
            <button className="toolbar-button compact-action" disabled={saving || loading} onClick={saveHooks} type="button">
              <Save size={14} />
              {saving ? '保存中' : '保存'}
            </button>
          </div>
        </div>

        {error && <div className="hook-status error">{error}</div>}
        {savedAt && !error && <div className="hook-status success">已保存 {savedAt}</div>}

        {activeHook ? (
          <div className="settings-form hook-form">
            <label>
              <span>名称</span>
              <input value={activeHook.name} onChange={(event) => updateActive({ name: event.target.value })} />
            </label>
            <label>
              <span>事件</span>
              <select value={activeHook.event} onChange={(event) => updateActive({ event: event.target.value })}>
                {EVENTS.map((event) => (
                  <option key={event} value={event}>{event}</option>
                ))}
              </select>
            </label>
            <label>
              <span>脚本路径</span>
              <input
                value={activeHook.script}
                onChange={(event) => updateActive({ script: event.target.value })}
                placeholder="hooks/before_tool.gs"
              />
            </label>
            <label>
              <span>超时 ms</span>
              <input
                inputMode="numeric"
                value={activeHook.timeout_ms}
                onChange={(event) => updateActive({ timeout_ms: event.target.value })}
              />
            </label>
            <label>
              <span>指令限制</span>
              <input
                inputMode="numeric"
                value={activeHook.instruction_limit}
                onChange={(event) => updateActive({ instruction_limit: event.target.value })}
              />
            </label>
            <label className="hook-toggle">
              <span>启用</span>
              <input
                checked={Boolean(activeHook.enabled)}
                onChange={(event) => updateActive({ enabled: event.target.checked })}
                type="checkbox"
              />
            </label>
            <label className="hook-code-field">
              <span>内联脚本</span>
              <textarea
                spellCheck="false"
                value={activeHook.inline_script}
                onChange={(event) => updateActive({ inline_script: event.target.value })}
                placeholder={'function execute(args) {\n  return { outputs: [{ stream: "stdout", text: args.event }] };\n}'}
              />
            </label>
          </div>
        ) : (
          <div className="hook-empty inline">
            <Workflow size={18} />
            <strong>没有钩子</strong>
            <span>新增一个钩子后开始配置。</span>
          </div>
        )}
      </div>
    </div>
  );
}
