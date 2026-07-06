import { useEffect, useMemo, useState } from 'react';
import {
  STORAGE_KEYS,
  activeProviderProfile,
  createProviderProfile,
  providerProfileById,
  providerProfileFormState,
  readProviderProfiles,
  readSetting,
  validProviderProfileId,
  writeJsonSetting,
  writeSetting,
} from '../utils/settings.js';

export function useProviderSettings({ notify } = {}) {
  const initialProviderProfiles = useMemo(readProviderProfiles, []);
  const initialProviderProfileId = useMemo(() => {
    const stored = readSetting(STORAGE_KEYS.providerProfileId, 'default-provider');
    return validProviderProfileId(initialProviderProfiles, stored);
  }, [initialProviderProfiles]);
  const initialProviderProfile = useMemo(
    () => activeProviderProfile(initialProviderProfiles, initialProviderProfileId),
    [initialProviderProfileId, initialProviderProfiles],
  );
  const initialFormState = useMemo(
    () => providerProfileFormState(initialProviderProfile),
    [initialProviderProfile],
  );

  const [providerProfiles, setProviderProfiles] = useState(() => initialProviderProfiles);
  const [providerProfileId, setProviderProfileId] = useState(() => initialProviderProfileId);
  const [provider, setProvider] = useState(() => initialFormState.provider);
  const [model, setModel] = useState(() => initialFormState.model);
  const [baseUrl, setBaseUrl] = useState(() => initialFormState.baseUrl);
  const [contextThreshold, setContextThreshold] = useState(() => initialFormState.contextThreshold);
  const [providerKey, setProviderKey] = useState(() => initialFormState.apiKey);

  useEffect(() => {
    if (!providerProfiles.some((profile) => profile.id === providerProfileId)) {
      selectProviderProfile(providerProfiles[0]?.id || '');
    }
  }, [providerProfileId, providerProfiles]);

  useEffect(() => {
    writeJsonSetting(STORAGE_KEYS.providerProfiles, providerProfiles);
  }, [providerProfiles]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.providerProfileId, providerProfileId);
  }, [providerProfileId]);

  useEffect(() => {
    if (!providerProfileId) return;
    setProviderProfiles((items) => items.map((item) => (
      item.id === providerProfileId
        ? { ...item, provider, model, baseUrl, apiKey: providerKey, contextThreshold }
        : item
    )));
  }, [providerProfileId, provider, model, baseUrl, providerKey, contextThreshold]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.provider, provider);
    if (provider === 'echo' && !model.trim()) {
      setModel('echo-v1');
    }
  }, [provider, model]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.model, model);
  }, [model]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.baseUrl, baseUrl);
  }, [baseUrl]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.contextThreshold, contextThreshold);
  }, [contextThreshold]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.providerKey, providerKey);
  }, [providerKey]);

  function createProviderProfileFromCurrent() {
    const profile = createProviderProfile({
      name: `${provider || 'provider'} · ${model || 'default'}`,
      provider,
      model,
      baseUrl,
      apiKey: providerKey,
      contextThreshold,
    });
    setProviderProfiles((items) => [...items, profile]);
    setProviderProfileId(profile.id);
    notify?.({ message: '供应商配置已新增', detail: profile.name, tone: 'success' });
  }

  function selectProviderProfile(id) {
    const profile = providerProfileById(providerProfiles, id);
    if (!profile) return;
    const formState = providerProfileFormState(profile);
    setProviderProfileId(profile.id);
    setProvider(formState.provider);
    setModel(formState.model);
    setBaseUrl(formState.baseUrl);
    setProviderKey(formState.apiKey);
    setContextThreshold(formState.contextThreshold);
    notify?.({ message: '已切换供应商配置', detail: profile.name || profile.provider, tone: 'success', duration: 1800 });
  }

  function updateProviderProfile(id, patch) {
    setProviderProfiles((items) => items.map((item) => (item.id === id ? { ...item, ...patch } : item)));
  }

  function deleteProviderProfile(id) {
    if (providerProfiles.length <= 1) return;
    const deleted = providerProfileById(providerProfiles, id);
    const next = providerProfiles.filter((item) => item.id !== id);
    setProviderProfiles(next);
    if (providerProfileId === id && next[0]) {
      const formState = providerProfileFormState(next[0]);
      setProviderProfileId(next[0].id);
      setProvider(formState.provider);
      setModel(formState.model);
      setBaseUrl(formState.baseUrl);
      setProviderKey(formState.apiKey);
      setContextThreshold(formState.contextThreshold);
    }
    notify?.({ message: '供应商配置已删除', detail: deleted?.name || deleted?.provider || '', tone: 'success' });
  }

  return {
    providerProfiles,
    providerProfileId,
    provider,
    model,
    baseUrl,
    providerKey,
    contextThreshold,
    setProvider,
    setModel,
    setBaseUrl,
    setProviderKey,
    setContextThreshold,
    createProviderProfileFromCurrent,
    selectProviderProfile,
    updateProviderProfile,
    deleteProviderProfile,
  };
}
