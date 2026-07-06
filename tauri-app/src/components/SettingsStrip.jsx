import { Palette, Server, Sparkles, Workflow } from 'lucide-react';
import { useState } from 'react';
import { classNames } from '../utils/format.js';
import { BaseSettings } from './settings/BaseSettings.jsx';
import { HookSettings } from './settings/HookSettings.jsx';
import { ProviderSettings } from './settings/ProviderSettings.jsx';
import { SkillSettings } from './settings/SkillSettings.jsx';
import { Button, Modal } from './ui/index.js';

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
  notify,
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
    <Modal
      ariaLabel="设置"
      bodyClassName="settings-modal-body"
      className="settings-modal"
      headClassName="settings-modal-head"
      onBackdropMouseDown={onClose}
      onClose={onClose}
      open={open}
      subtitle="供应商与界面偏好"
      title="设置"
    >
          <nav className="settings-nav" aria-label="设置分类">
            <Button className={classNames(tab === 'provider' && 'active')} icon={<Server size={15} />} onClick={() => setTab('provider')} variant="ghost">供应商</Button>
            <Button className={classNames(tab === 'base' && 'active')} icon={<Palette size={15} />} onClick={() => setTab('base')} variant="ghost">基本设置</Button>
            <Button className={classNames(tab === 'hooks' && 'active')} icon={<Workflow size={15} />} onClick={() => setTab('hooks')} variant="ghost">钩子</Button>
            <Button className={classNames(tab === 'skills' && 'active')} icon={<Sparkles size={15} />} onClick={() => setTab('skills')} variant="ghost">技能</Button>
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
                notify={notify}
                workspace={workspace}
              />
            )}

            {tab === 'skills' && (
              <SkillSettings
                apiJson={apiJson}
                notify={notify}
                workspace={workspace}
              />
            )}
          </div>
    </Modal>
  );
}
