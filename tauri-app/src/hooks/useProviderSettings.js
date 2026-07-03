import { useEffect, useMemo, useState } from 'react';
import {
  DEFAULT_CONTEXT_THRESHOLD,
  STORAGE_KEYS,
  createProviderProfile,
  readProviderProfiles,
  readSetting,
  writeJsonSetting,
  writeSetting,
} from '../utils/settings.js';

export function useProviderSettings() {
  const initialProviderProfiles = useMemo(readProviderProfiles, []);
  const initialProviderProfileId = useMemo(() => {
    const stored = readSetting(STORAGE_KEYS.providerProfileId, 'default-provider');
    return initialProviderProfiles.some((profile) => profile.id === stored)
      ? stored
      : initialProviderProfiles[0]?.id || '';
  }, [initialProviderProfiles]);
  const initialProviderProfile = useMemo(
    () => initialProviderProfiles.find((profile) => profile.id === initialProviderProfileId) || initialProviderProfiles[0],
    [initialProviderProfileId, initialProviderProfiles],
  );

  const [providerProfiles, setProviderProfiles] = useState(() => initialProviderProfiles);
  const [providerProfileId, setProviderProfileId] = useState(() => initialProviderProfileId);
  const [provider, setProvider] = useState(() => initialProviderProfile?.provider || 'echo');
  const [model, setModel] = useState(() => initialProviderProfile?.model || 'echo-v1');
  const [baseUrl, setBaseUrl] = useState(() => initialProviderProfile?.baseUrl || '');
  const [contextThreshold, setContextThreshold] = useState(() => initialProviderProfile?.contextThreshold || DEFAULT_CONTEXT_THRESHOLD);
  const [providerKey, setProviderKey] = useState(() => initialProviderProfile?.apiKey || '');

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
  }

  function selectProviderProfile(id) {
    const profile = providerProfiles.find((item) => item.id === id);
    if (!profile) return;
    setProviderProfileId(profile.id);
    setProvider(profile.provider || 'echo');
    setModel(profile.model || (profile.provider === 'echo' ? 'echo-v1' : ''));
    setBaseUrl(profile.baseUrl || '');
    setProviderKey(profile.apiKey || '');
    setContextThreshold(profile.contextThreshold || DEFAULT_CONTEXT_THRESHOLD);
  }

  function updateProviderProfile(id, patch) {
    setProviderProfiles((items) => items.map((item) => (item.id === id ? { ...item, ...patch } : item)));
  }

  function deleteProviderProfile(id) {
    if (providerProfiles.length <= 1) return;
    const next = providerProfiles.filter((item) => item.id !== id);
    setProviderProfiles(next);
    if (providerProfileId === id && next[0]) {
      setProviderProfileId(next[0].id);
      setProvider(next[0].provider || 'echo');
      setModel(next[0].model || (next[0].provider === 'echo' ? 'echo-v1' : ''));
      setBaseUrl(next[0].baseUrl || '');
      setProviderKey(next[0].apiKey || '');
      setContextThreshold(next[0].contextThreshold || DEFAULT_CONTEXT_THRESHOLD);
    }
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
