import { Palette, Server, X } from 'lucide-react';
import { useState } from 'react';
import { classNames } from '../utils/format.js';

export function SettingsStrip({
  open,
  apiBase,
  apiKey,
  provider,
  model,
  baseUrl,
  providerKey,
  theme,
  fontSize,
  onApiBaseChange,
  onApiKeyChange,
  onProviderChange,
  onModelChange,
  onBaseUrlChange,
  onProviderKeyChange,
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
          </nav>

          <div className="settings-content">
            {tab === 'provider' && (
              <div className="settings-form">
                <label>
                  <span>Server</span>
                  <input value={apiBase} onChange={(event) => onApiBaseChange(event.target.value)} />
                </label>
                <label>
                  <span>Server Key</span>
                  <input type="password" value={apiKey} onChange={(event) => onApiKeyChange(event.target.value)} />
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
              </div>
            )}

            {tab === 'base' && (
              <div className="settings-form">
                <label>
                  <span>主题</span>
                  <select value={theme} onChange={(event) => onThemeChange(event.target.value)}>
                    <option value="light">明亮</option>
                    <option value="warm">柔和</option>
                    <option value="dark">深色</option>
                  </select>
                </label>
                <label>
                  <span>字体大小</span>
                  <select value={fontSize} onChange={(event) => onFontSizeChange(event.target.value)}>
                    <option value="compact">紧凑</option>
                    <option value="normal">标准</option>
                    <option value="large">偏大</option>
                  </select>
                </label>
              </div>
            )}
          </div>
        </div>
      </section>
    </div>
  );
}
