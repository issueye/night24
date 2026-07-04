import { Palette, Server, Workflow, X } from 'lucide-react';
import { useState } from 'react';
import { classNames } from '../utils/format.js';
import { BaseSettings } from './settings/BaseSettings.jsx';
import { HookSettings } from './settings/HookSettings.jsx';
import { ProviderSettings } from './settings/ProviderSettings.jsx';

export function SettingsStrip({
  open,
  apiBase,
  apiKey,
  providerProfiles,
  providerProfileId,
  provider,
  model,
  baseUrl,
  providerKey,
  contextThreshold,
  networkProxy,
  theme,
  fontSize,
  workspace,
  apiJson,
  onApiBaseChange,
  onApiKeyChange,
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
  onThemeChange,
  onFontSizeChange,
  onClose,
}) {
  const [tab, setTab] = useState('provider');
  if (!open) return null;

  return (
    <div className="settings-modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="settings-modal" role="dialog" aria-modal="true" aria-label="设置" onMouseDown={(event) => event.stopPropagation()}>
        <header className="settings-modal-head">
          <div>
            <strong>设置</strong>
            <span>供应商与界面偏好</span>
          </div>
          <button className="icon-button compact" onClick={onClose} title="关闭设置" type="button"><X size={14} /></button>
        </header>

        <div className="settings-modal-body">
          <nav className="settings-nav" aria-label="设置分类">
            <button className={classNames(tab === 'provider' && 'active')} onClick={() => setTab('provider')} type="button">
              <Server size={15} />
              <span>供应商</span>
            </button>
            <button className={classNames(tab === 'base' && 'active')} onClick={() => setTab('base')} type="button">
              <Palette size={15} />
              <span>基本设置</span>
            </button>
            <button className={classNames(tab === 'hooks' && 'active')} onClick={() => setTab('hooks')} type="button">
              <Workflow size={15} />
              <span>钩子</span>
            </button>
          </nav>

          <div className="settings-content">
            {tab === 'provider' && (
              <ProviderSettings
                providerProfiles={providerProfiles}
                providerProfileId={providerProfileId}
                provider={provider}
                model={model}
                baseUrl={baseUrl}
                providerKey={providerKey}
                contextThreshold={contextThreshold}
                networkProxy={networkProxy}
                onProviderProfileChange={onProviderProfileChange}
                onProviderProfileCreate={onProviderProfileCreate}
                onProviderProfileUpdate={onProviderProfileUpdate}
                onProviderProfileDelete={onProviderProfileDelete}
                onProviderChange={onProviderChange}
                onModelChange={onModelChange}
                onBaseUrlChange={onBaseUrlChange}
                onProviderKeyChange={onProviderKeyChange}
                onContextThresholdChange={onContextThresholdChange}
                onNetworkProxyChange={onNetworkProxyChange}
              />
            )}

            {tab === 'base' && (
              <BaseSettings
                apiBase={apiBase}
                apiKey={apiKey}
                theme={theme}
                fontSize={fontSize}
                onApiBaseChange={onApiBaseChange}
                onApiKeyChange={onApiKeyChange}
                onThemeChange={onThemeChange}
                onFontSizeChange={onFontSizeChange}
              />
            )}

            {tab === 'hooks' && (
              <HookSettings
                apiJson={apiJson}
                workspace={workspace}
              />
            )}
          </div>
        </div>
      </section>
    </div>
  );
}
