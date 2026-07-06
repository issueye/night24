import { useEffect } from 'react';
import { STORAGE_KEYS, writeSetting } from '../utils/settings.js';

export function useAppSettingsPersistence({
  apiBase,
  apiKey,
  networkProxy,
  accessMode,
  theme,
  fontSize,
}) {
  useEffect(() => {
    writeSetting(STORAGE_KEYS.apiBase, apiBase);
  }, [apiBase]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.apiKey, apiKey);
  }, [apiKey]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.networkProxy, networkProxy);
  }, [networkProxy]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.accessMode, accessMode);
    writeSetting(STORAGE_KEYS.fullAccess, accessMode === 'allow_all' ? 'true' : 'false');
  }, [accessMode]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.theme, theme);
  }, [theme]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.fontSize, fontSize);
  }, [fontSize]);
}
