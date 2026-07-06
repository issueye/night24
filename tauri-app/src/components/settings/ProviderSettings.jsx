import { Plus, Trash2 } from 'lucide-react';
import { classNames } from '../../utils/format.js';
import { activeProviderProfile } from '../../utils/settings.js';
import { Button, IconButton, Select, TextField } from '../ui/index.js';

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
          <IconButton className="icon-button compact" label="新增供应商" onClick={onProviderProfileCreate} size="sm"><Plus size={14} /></IconButton>
        </div>
        <div className="provider-list-scroll">
          {providerProfiles.map((item) => (
            <Button
              className={classNames('provider-profile-row', item.id === providerProfileId && 'active')}
              key={item.id}
              onClick={() => onProviderProfileChange(item.id)}
              variant="ghost"
            >
              <strong>{item.name || item.provider}</strong>
              <span>{item.provider} · {item.model || 'default'}</span>
            </Button>
          ))}
        </div>
      </aside>

      <div className="settings-form provider-form">
        <TextField
          label="名称"
          onChange={(event) => activeProfile && onProviderProfileUpdate(activeProfile.id, { name: event.target.value })}
          placeholder="例如 OpenAI 工作模型"
          value={activeProfile?.name || ''}
        />
        <Select
          label="Provider"
          onChange={onProviderChange}
          options={[
            { label: 'echo', value: 'echo' },
            { label: 'openai', value: 'openai' },
            { label: 'anthropic', value: 'anthropic' },
            { label: 'ollama', value: 'ollama' },
            { label: 'stepfun', value: 'stepfun' },
          ]}
          value={provider}
        />
        <TextField label="Model" onChange={(event) => onModelChange(event.target.value)} placeholder="echo-v1" value={model} />
        <TextField label="Base URL" onChange={(event) => onBaseUrlChange(event.target.value)} placeholder="optional" value={baseUrl} />
        <TextField label="Provider Key" onChange={(event) => onProviderKeyChange(event.target.value)} placeholder="saved locally" type="password" value={providerKey} />
        <TextField
          inputMode="numeric"
          label="摘要阈值 Token"
          onChange={(event) => onContextThresholdChange(event.target.value)}
          placeholder="24000"
          value={contextThreshold}
        />
        <TextField label="Network Proxy" onChange={(event) => onNetworkProxyChange(event.target.value)} placeholder="http://127.0.0.1:7890 or direct" value={networkProxy} />
        <Button
          className="danger-outline-button"
          disabled={providerProfiles.length <= 1 || !activeProfile}
          icon={<Trash2 size={14} />}
          onClick={() => activeProfile && onProviderProfileDelete(activeProfile.id)}
          tone="danger"
          variant="soft"
        >
          删除当前供应商
        </Button>
      </div>
    </div>
  );
}
