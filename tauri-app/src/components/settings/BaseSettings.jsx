export function BaseSettings({
  apiBase,
  apiKey,
  theme,
  fontSize,
  onApiBaseChange,
  onApiKeyChange,
  onThemeChange,
  onFontSizeChange,
}) {
  return (
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
  );
}
