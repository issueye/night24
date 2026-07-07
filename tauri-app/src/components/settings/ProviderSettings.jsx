import { FlaskConical, Loader2, Plus, Save, Trash2, X } from 'lucide-react';
import { useState } from 'react';
import { normalizeError } from '../../utils/events.js';
import { classNames } from '../../utils/format.js';
import { activeProviderProfile, providerDisplayName } from '../../utils/settings.js';
import { Button, IconButton, Select, TextField } from '../ui/index.js';

export function ProviderSettings({
  providerProfiles,
  providerProfileId,
  provider,
  model,
  baseUrl,
  providerKey,
  contextThreshold,
  requestRetries,
  maxTurns,
  turnTimeoutSeconds,
  toolTimeoutSeconds,
  totalTimeoutMinutes,
  providerName,
  providerDraftDirty,
  providerDraftCreating,
  networkProxy,
  onProviderProfileChange,
  onProviderProfileCreate,
  onProviderProfileUpdate,
  onProviderProfileDelete,
  onProviderProfileSave,
  onProviderProfileCancel,
  onProviderProfileTest,
  onProviderNameChange,
  onProviderChange,
  onModelChange,
  onBaseUrlChange,
  onProviderKeyChange,
  onContextThresholdChange,
  onRequestRetriesChange,
  onMaxTurnsChange,
  onTurnTimeoutSecondsChange,
  onToolTimeoutSecondsChange,
  onTotalTimeoutMinutesChange,
  onNetworkProxyChange,
}) {
  const activeProfile = activeProviderProfile(providerProfiles, providerProfileId);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState(null);
  const responsesWithCompatibleEndpoint = provider === 'openai-responses'
    && baseUrl
    && !/api\.openai\.com/i.test(baseUrl);

  async function handleTest() {
    if (!onProviderProfileTest || testing) return;
    setTesting(true);
    setTestResult(null);
    try {
      const result = await onProviderProfileTest();
      setTestResult({
        tone: 'success',
        message: result?.message || '连接正常',
      });
    } catch (error) {
      setTestResult({
        tone: 'error',
        message: normalizeError(error),
      });
    } finally {
      setTesting(false);
    }
  }

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
              <span>{providerDisplayName(item.provider)} · {item.model || 'default'}</span>
            </Button>
          ))}
        </div>
      </aside>

      <div className="settings-form provider-form">
        <div className="provider-edit-head">
          <div>
            <strong>{providerDraftCreating ? '新增供应商' : (activeProfile?.name || '供应商配置')}</strong>
            <span>{providerDraftDirty ? '有未保存修改' : '已保存'}</span>
          </div>
        </div>
        <TextField
          label="名称"
          onChange={(event) => onProviderNameChange(event.target.value)}
          placeholder="例如 OpenAI 工作模型"
          value={providerName || ''}
        />
        <Select
          label="Provider"
          onChange={onProviderChange}
          options={[
            { label: 'echo', value: 'echo' },
            { label: 'openai-chat', value: 'openai-chat' },
            { label: 'openai-responses', value: 'openai-responses' },
            { label: 'anthropic', value: 'anthropic' },
            { label: 'ollama', value: 'ollama' },
            { label: 'stepfun', value: 'stepfun' },
          ]}
          value={provider}
        />
        <TextField label="Model" onChange={(event) => onModelChange(event.target.value)} placeholder="echo-v1" value={model} />
        <TextField label="Base URL" onChange={(event) => onBaseUrlChange(event.target.value)} placeholder="optional" value={baseUrl} />
        {responsesWithCompatibleEndpoint && (
          <p className="provider-warning">
            当前 Base URL 可能只兼容 Chat Completions。若请求失败，请将 Provider 切换为 openai-chat；openai-responses 需要服务端支持 /responses。
          </p>
        )}
        <TextField label="Provider Key" onChange={(event) => onProviderKeyChange(event.target.value)} placeholder="saved locally" type="password" value={providerKey} />
        <TextField
          inputMode="numeric"
          label="摘要阈值 Token"
          onChange={(event) => onContextThresholdChange(event.target.value)}
          placeholder="24000"
          value={contextThreshold}
        />
        <TextField
          inputMode="numeric"
          label="失败重试次数"
          max="5"
          min="0"
          onChange={(event) => onRequestRetriesChange(event.target.value)}
          placeholder="0-5"
          type="number"
          value={requestRetries}
        />
        <div className="provider-limit-grid">
          <TextField
            inputMode="numeric"
            label="最大轮次"
            min="1"
            onChange={(event) => onMaxTurnsChange(event.target.value)}
            placeholder="120"
            type="number"
            value={maxTurns}
          />
          <TextField
            inputMode="numeric"
            label="单轮超时（秒）"
            min="1"
            onChange={(event) => onTurnTimeoutSecondsChange(event.target.value)}
            placeholder="180"
            type="number"
            value={turnTimeoutSeconds}
          />
          <TextField
            inputMode="numeric"
            label="工具超时（秒）"
            min="1"
            onChange={(event) => onToolTimeoutSecondsChange(event.target.value)}
            placeholder="180"
            type="number"
            value={toolTimeoutSeconds}
          />
          <TextField
            inputMode="numeric"
            label="总超时（分钟）"
            min="1"
            onChange={(event) => onTotalTimeoutMinutesChange(event.target.value)}
            placeholder="30"
            type="number"
            value={totalTimeoutMinutes}
          />
        </div>
        <TextField label="Network Proxy" onChange={(event) => onNetworkProxyChange(event.target.value)} placeholder="http://127.0.0.1:7890 or direct" value={networkProxy} />
        <div className="provider-actions">
          <Button
            disabled={!providerDraftDirty}
            icon={<Save size={14} />}
            onClick={onProviderProfileSave}
            tone="primary"
            variant="solid"
          >
            保存
          </Button>
          <Button
            disabled={!providerDraftDirty}
            icon={<X size={14} />}
            onClick={onProviderProfileCancel}
            variant="soft"
          >
            取消
          </Button>
          <Button
            disabled={testing}
            icon={testing ? <Loader2 size={14} /> : <FlaskConical size={14} />}
            onClick={handleTest}
            variant="soft"
          >
            {testing ? '测试中' : '测试'}
          </Button>
          <Button
            className="danger-outline-button"
            disabled={providerProfiles.length <= 1 || !activeProfile || providerDraftCreating}
            icon={<Trash2 size={14} />}
            onClick={() => activeProfile && onProviderProfileDelete(activeProfile.id)}
            tone="danger"
            variant="soft"
          >
            删除
          </Button>
        </div>
        {testResult && (
          <p className={classNames('provider-test-result', testResult.tone)}>
            {testResult.tone === 'success' ? '测试成功：' : '测试失败：'}
            {testResult.message}
          </p>
        )}
      </div>
    </div>
  );
}
