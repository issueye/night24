import { Select, TextField } from '../ui/index.js';

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
      <TextField label="Server" onChange={(event) => onApiBaseChange(event.target.value)} value={apiBase} />
      <TextField label="Server Key" onChange={(event) => onApiKeyChange(event.target.value)} type="password" value={apiKey} />
      <Select
        label="主题"
        onChange={onThemeChange}
        options={[
          { label: '明亮', value: 'light' },
          { label: '柔和', value: 'warm' },
          { label: '深色', value: 'dark' },
        ]}
        value={theme}
      />
      <Select
        label="字体大小"
        onChange={onFontSizeChange}
        options={[
          { label: '紧凑', value: 'compact' },
          { label: '标准', value: 'normal' },
          { label: '偏大', value: 'large' },
        ]}
        value={fontSize}
      />
    </div>
  );
}
