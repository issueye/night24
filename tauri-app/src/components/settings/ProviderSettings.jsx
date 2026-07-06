import { Plus, Trash2 } from 'lucide-react';
import { classNames } from '../../utils/format.js';
import { activeProviderProfile } from '../../utils/settings.js';

export function ProviderSettings({
  providerProfiles,
  providerProfileId,
  provider,
  model,
  baseUrl,
  providerKey,
  contextThreshold,
  networkProxy,
  onProviderProfileChange,
  onProviderProfileCreate,
  onProviderProfileUpdate,
  onProviderProfileDelete,
  onProviderChange,
  onModelChange,
  onBaseUrlChange,
  onProviderKeyChange,
  onContextThresholdChange,
  onNetworkProxyChange,
}) {
  const activeProfile = activeProviderProfile(providerProfiles, providerProfileId);

  return (
    <div className="provider-manager">
      <aside className="provider-list" aria-label="供应商配置">
        <div className="provider-list-head">
          <strong>供应商配置</strong>
          <button className="icon-button compact" onClick={onProviderProfileCreate} title="新增供应商" type="button"><Plus size={14} /></button>
        </div>
        <div className="provider-list-scroll">
          {providerProfiles.map((item) => (
            <button
              className={classNames('provider-profile-row', item.id === providerProfileId && 'active')}
              key={item.id}
              onClick={() => onProviderProfileChange(item.id)}
              type="button"
            >
              <strong>{item.name || item.provider}</strong>
              <span>{item.provider} · {item.model || 'default'}</span>
            </button>
          ))}
        </div>
      </aside>

      <div className="settings-form provider-form">
        <label>
          <span>名称</span>
          <input
            value={activeProfile?.name || ''}
            onChange={(event) => activeProfile && onProviderProfileUpdate(activeProfile.id, { name: event.target.value })}
            placeholder="例如 OpenAI 工作模型"
          />
        </label>
        <label>
          <span>Provider</span>
          <select value={provider} onChange={(event) => onProviderChange(event.target.value)}>
            <option value="echo">echo</option>
            <option value="openai">openai</option>
            <option value="anthropic">anthropic</option>
            <option value="ollama">ollama</option>
            <option value="stepfun">stepfun</option>
          </select>
        </label>
        <label>
          <span>Model</span>
          <input value={model} onChange={(event) => onModelChange(event.target.value)} placeholder="echo-v1" />
        </label>
        <label>
          <span>Base URL</span>
          <input value={baseUrl} onChange={(event) => onBaseUrlChange(event.target.value)} placeholder="optional" />
        </label>
        <label>
          <span>Provider Key</span>
          <input type="password" value={providerKey} onChange={(event) => onProviderKeyChange(event.target.value)} placeholder="saved locally" />
        </label>
        <label>
          <span>摘要阈值 Token</span>
          <input
            inputMode="numeric"
            value={contextThreshold}
            onChange={(event) => onContextThresholdChange(event.target.value)}
            placeholder="24000"
          />
        </label>
        <label>
          <span>Network Proxy</span>
          <input value={networkProxy} onChange={(event) => onNetworkProxyChange(event.target.value)} placeholder="http://127.0.0.1:7890 or direct" />
        </label>
        <button
          className="danger-outline-button"
          disabled={providerProfiles.length <= 1 || !activeProfile}
          onClick={() => activeProfile && onProviderProfileDelete(activeProfile.id)}
          type="button"
        >
          <Trash2 size={14} />
          删除当前供应商
        </button>
      </div>
    </div>
  );
}
