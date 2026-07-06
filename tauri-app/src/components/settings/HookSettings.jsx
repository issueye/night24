import { Plus, RefreshCw, Save, Trash2, Workflow } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { normalizeError } from '../../utils/events.js';
import { classNames } from '../../utils/format.js';
import { HOOK_EVENTS, createHook, hooksToConfig, normalizeHook } from '../../utils/hooks.js';
import { Button, IconButton, Select, Switch, TextField } from '../ui/index.js';
import { SettingsListDetail } from './SettingsListDetail.jsx';

export function HookSettings({ apiJson, workspace }) {
  const [hooks, setHooks] = useState([]);
  const [activeId, setActiveId] = useState(null);
  const [configPath, setConfigPath] = useState('');
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [savedAt, setSavedAt] = useState('');
  const hookLoadRequestRef = useRef(0);
  const hookSaveRequestRef = useRef(0);

  const activeHook = useMemo(
    () => hooks.find((hook) => hook.id === activeId) || hooks[0] || null,
    [activeId, hooks],
  );

  async function loadHooks() {
    const requestId = hookLoadRequestRef.current + 1;
    hookLoadRequestRef.current = requestId;
    hookSaveRequestRef.current += 1;
    if (!workspace) {
      setHooks([]);
      setActiveId(null);
      setConfigPath('');
      setLoading(false);
      setSaving(false);
      setError('');
      return;
    }
    setLoading(true);
    setSaving(false);
    setError('');
    setSavedAt('');
    try {
      const data = await apiJson('/workspace/hooks');
      if (hookLoadRequestRef.current !== requestId) return;
      const nextHooks = (data?.config?.hooks || []).map(normalizeHook);
      setHooks(nextHooks);
      setActiveId((current) => nextHooks.find((hook) => hook.id === current)?.id || nextHooks[0]?.id || null);
      setConfigPath(data?.path || '');
    } catch (err) {
      if (hookLoadRequestRef.current !== requestId) return;
      setError(normalizeError(err));
    } finally {
      if (hookLoadRequestRef.current === requestId) setLoading(false);
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
    if (!workspace) return;
    const requestId = hookSaveRequestRef.current + 1;
    hookSaveRequestRef.current = requestId;
    hookLoadRequestRef.current += 1;
    setLoading(false);
    setSaving(true);
    setError('');
    try {
      const data = await apiJson('/workspace/hooks', {
        method: 'PUT',
        body: JSON.stringify(hooksToConfig(hooks)),
      });
      if (hookSaveRequestRef.current !== requestId) return;
      const nextHooks = (data?.config?.hooks || []).map(normalizeHook);
      setHooks(nextHooks);
      setActiveId((current) => nextHooks.find((hook) => hook.id === current)?.id || nextHooks[0]?.id || null);
      setConfigPath(data?.path || '');
      setSavedAt(new Date().toLocaleTimeString());
    } catch (err) {
      if (hookSaveRequestRef.current !== requestId) return;
      setError(normalizeError(err));
    } finally {
      if (hookSaveRequestRef.current === requestId) setSaving(false);
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
    <SettingsListDetail
      managerClassName="hook-manager"
      listClassName="hook-list"
      listLabel="钩子列表"
      listTitle="钩子"
      listActions={(
        <div className="hook-list-actions">
          <IconButton className="icon-button compact" disabled={loading} label="重新加载" onClick={loadHooks} size="sm">
            <RefreshCw size={14} />
          </IconButton>
          <IconButton className="icon-button compact" label="新增钩子" onClick={addHook} size="sm">
            <Plus size={14} />
          </IconButton>
        </div>
      )}
      listChildren={(
        <>
          {hooks.length === 0 && <div className="hook-list-empty">暂无钩子</div>}
          {hooks.map((hook) => (
            <Button
              className={classNames('provider-profile-row', hook.id === activeHook?.id && 'active', !hook.enabled && 'muted')}
              key={hook.id}
              onClick={() => setActiveId(hook.id)}
              variant="ghost"
            >
              <strong>{hook.name || hook.event}</strong>
              <span>{hook.event} · {hook.enabled ? '启用' : '停用'}</span>
            </Button>
          ))}
        </>
      )}
      detailClassName="hook-editor"
    >
        <div className="hook-toolbar">
          <span>{loading ? '加载中' : configPath || '.night24/hooks.json'}</span>
          <div>
            <Button className="danger-outline-button compact-action" disabled={!activeHook} icon={<Trash2 size={14} />} onClick={deleteHook} size="sm" tone="danger" variant="soft">
              删除
            </Button>
            <Button className="toolbar-button compact-action" disabled={saving || loading} icon={<Save size={14} />} onClick={saveHooks} size="sm">
              {saving ? '保存中' : '保存'}
            </Button>
          </div>
        </div>

        {error && <div className="hook-status error">{error}</div>}
        {savedAt && !error && <div className="hook-status success">已保存 {savedAt}</div>}

        {activeHook ? (
          <div className="settings-form hook-form">
            <TextField label="名称" onChange={(event) => updateActive({ name: event.target.value })} value={activeHook.name} />
            <Select
              label="事件"
              onChange={(value) => updateActive({ event: value })}
              options={HOOK_EVENTS.map((event) => ({ label: event, value: event }))}
              value={activeHook.event}
            />
            <TextField label="脚本路径" onChange={(event) => updateActive({ script: event.target.value })} placeholder="hooks/before_tool.gs" value={activeHook.script} />
            <TextField inputMode="numeric" label="超时 ms" onChange={(event) => updateActive({ timeout_ms: event.target.value })} value={activeHook.timeout_ms} />
            <TextField inputMode="numeric" label="指令限制" onChange={(event) => updateActive({ instruction_limit: event.target.value })} value={activeHook.instruction_limit} />
            <div className="hook-toggle">
              <span>启用</span>
              <Switch checked={Boolean(activeHook.enabled)} onChange={(checked) => updateActive({ enabled: checked })} />
            </div>
            <div className="hook-toggle">
              <span>模块白名单</span>
              <Switch checked={Boolean(activeHook.allowed_modules_enabled)} onChange={(checked) => updateActive({ allowed_modules_enabled: checked })} />
            </div>
            <TextField
              as="textarea"
              className="hook-code-field"
              disabled={!activeHook.allowed_modules_enabled}
              label="允许模块"
              onChange={(event) => updateActive({ allowed_modules_text: event.target.value })}
              placeholder={'fs\n@std/exec'}
              spellCheck="false"
              value={activeHook.allowed_modules_text}
            />
            <TextField
              as="textarea"
              className="hook-code-field"
              label="内联脚本"
              onChange={(event) => updateActive({ inline_script: event.target.value })}
              placeholder={'function execute(args) {\n  return { outputs: [{ stream: "stdout", text: args.event }] };\n}'}
              spellCheck="false"
              value={activeHook.inline_script}
            />
          </div>
        ) : (
          <div className="hook-empty inline">
            <Workflow size={18} />
            <strong>没有钩子</strong>
            <span>新增一个钩子后开始配置。</span>
          </div>
        )}
    </SettingsListDetail>
  );
}
