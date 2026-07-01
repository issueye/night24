export function SettingsStrip({
  open,
  apiBase,
  apiKey,
  provider,
  model,
  baseUrl,
  providerKey,
  onApiBaseChange,
  onApiKeyChange,
  onProviderChange,
  onModelChange,
  onBaseUrlChange,
  onProviderKeyChange,
}) {
  if (!open) return null;

  return (
    <section className="settings-strip">
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
        <input type="password" value={providerKey} onChange={(event) => onProviderKeyChange(event.target.value)} placeholder="not saved" />
      </label>
    </section>
  );
}
