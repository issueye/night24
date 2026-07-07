import { useEffect, useMemo, useRef, useState } from 'react';
import {
  DEFAULT_CONTEXT_THRESHOLD,
  DEFAULT_MAX_TURNS,
  DEFAULT_REQUEST_RETRIES,
  DEFAULT_TOOL_TIMEOUT_SECONDS,
  DEFAULT_TOTAL_TIMEOUT_MINUTES,
  DEFAULT_TURN_TIMEOUT_SECONDS,
  STORAGE_KEYS,
  activeProviderProfile,
  createProviderProfile,
  providerProfileById,
  providerProfileFormState,
  PROVIDER_DEFAULT_MODELS,
  parseRequestRetries,
  parseMaxTurns,
  parseTimeoutMinutes,
  parseTimeoutSeconds,
  providerDisplayName,
  providerDefaultModel,
  readProviderProfiles,
  readSetting,
  validProviderProfileId,
  writeJsonSetting,
  writeSetting,
} from '../utils/settings.js';

function profileDraft(profile) {
  const form = providerProfileFormState(profile);
  return {
    name: profile?.name || '',
    provider: form.provider,
    model: form.model,
    baseUrl: form.baseUrl,
    apiKey: form.apiKey,
    contextThreshold: form.contextThreshold,
    requestRetries: form.requestRetries,
    maxTurns: form.maxTurns,
    turnTimeoutSeconds: form.turnTimeoutSeconds,
    toolTimeoutSeconds: form.toolTimeoutSeconds,
    totalTimeoutMinutes: form.totalTimeoutMinutes,
  };
}

function emptyProviderDraft() {
  const provider = 'openai-chat';
  return {
    name: '',
    provider,
    model: providerDefaultModel(provider),
    baseUrl: '',
    apiKey: '',
    contextThreshold: DEFAULT_CONTEXT_THRESHOLD,
    requestRetries: DEFAULT_REQUEST_RETRIES,
    maxTurns: DEFAULT_MAX_TURNS,
    turnTimeoutSeconds: DEFAULT_TURN_TIMEOUT_SECONDS,
    toolTimeoutSeconds: DEFAULT_TOOL_TIMEOUT_SECONDS,
    totalTimeoutMinutes: DEFAULT_TOTAL_TIMEOUT_MINUTES,
  };
}

function draftEqualsProfile(draft, profile) {
  if (!profile) return false;
  const current = profileDraft(profile);
  return [
    'name',
    'provider',
    'model',
    'baseUrl',
    'apiKey',
    'contextThreshold',
    'requestRetries',
    'maxTurns',
    'turnTimeoutSeconds',
    'toolTimeoutSeconds',
    'totalTimeoutMinutes',
  ]
    .every((key) => String(draft?.[key] || '') === String(current?.[key] || ''));
}

function profilePatchFromDraft(draft) {
  return {
    name: draft.name?.trim() || `${providerDisplayName(draft.provider)} · ${draft.model || 'default'}`,
    provider: draft.provider,
    model: draft.model,
    baseUrl: draft.baseUrl,
    apiKey: draft.apiKey,
    contextThreshold: draft.contextThreshold,
    requestRetries: String(parseRequestRetries(draft.requestRetries) ?? DEFAULT_REQUEST_RETRIES),
    maxTurns: String(parseMaxTurns(draft.maxTurns) ?? DEFAULT_MAX_TURNS),
    turnTimeoutSeconds: String(parseTimeoutSeconds(draft.turnTimeoutSeconds) ?? DEFAULT_TURN_TIMEOUT_SECONDS),
    toolTimeoutSeconds: String(parseTimeoutSeconds(draft.toolTimeoutSeconds) ?? DEFAULT_TOOL_TIMEOUT_SECONDS),
    totalTimeoutMinutes: String(parseTimeoutMinutes(draft.totalTimeoutMinutes) ?? DEFAULT_TOTAL_TIMEOUT_MINUTES),
  };
}

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

  const [providerProfiles, setProviderProfiles] = useState(() => initialProviderProfiles);
  const [providerProfileId, setProviderProfileId] = useState(() => initialProviderProfileId);
  const [draft, setDraft] = useState(() => profileDraft(initialProviderProfile));
  const [isCreatingProvider, setIsCreatingProvider] = useState(false);
  const previousDraftProviderRef = useRef(draft.provider);

  const activeProfile = activeProviderProfile(providerProfiles, providerProfileId);
  const savedForm = providerProfileFormState(activeProfile);
  const isDirty = isCreatingProvider || !draftEqualsProfile(draft, activeProfile);

  useEffect(() => {
    if (!providerProfiles.some((profile) => profile.id === providerProfileId)) {
      const next = providerProfiles[0] || null;
      setProviderProfileId(next?.id || '');
      setDraft(profileDraft(next));
      setIsCreatingProvider(false);
    }
  }, [providerProfileId, providerProfiles]);

  useEffect(() => {
    writeJsonSetting(STORAGE_KEYS.providerProfiles, providerProfiles);
  }, [providerProfiles]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.providerProfileId, providerProfileId);
  }, [providerProfileId]);

  useEffect(() => {
    writeSetting(STORAGE_KEYS.provider, savedForm.provider);
    writeSetting(STORAGE_KEYS.model, savedForm.model);
    writeSetting(STORAGE_KEYS.baseUrl, savedForm.baseUrl);
    writeSetting(STORAGE_KEYS.providerKey, savedForm.apiKey);
    writeSetting(STORAGE_KEYS.contextThreshold, savedForm.contextThreshold);
    writeSetting(STORAGE_KEYS.requestRetries, savedForm.requestRetries);
    writeSetting(STORAGE_KEYS.maxTurns, savedForm.maxTurns);
    writeSetting(STORAGE_KEYS.turnTimeoutSeconds, savedForm.turnTimeoutSeconds);
    writeSetting(STORAGE_KEYS.toolTimeoutSeconds, savedForm.toolTimeoutSeconds);
    writeSetting(STORAGE_KEYS.totalTimeoutMinutes, savedForm.totalTimeoutMinutes);
  }, [
    savedForm.apiKey,
    savedForm.baseUrl,
    savedForm.contextThreshold,
    savedForm.maxTurns,
    savedForm.model,
    savedForm.provider,
    savedForm.requestRetries,
    savedForm.toolTimeoutSeconds,
    savedForm.totalTimeoutMinutes,
    savedForm.turnTimeoutSeconds,
  ]);

  function patchDraft(patch) {
    setDraft((current) => ({ ...current, ...patch }));
  }

  function setProvider(value) {
    setDraft((current) => {
      const next = { ...current, provider: value };
      if (previousDraftProviderRef.current !== value) {
        const defaultModel = providerDefaultModel(value);
        const knownDefaultModels = Object.values(PROVIDER_DEFAULT_MODELS);
        if (defaultModel && (!current.model.trim() || knownDefaultModels.includes(current.model.trim()))) {
          next.model = defaultModel;
        }
        previousDraftProviderRef.current = value;
      }
      return next;
    });
  }

  function setModel(value) {
    patchDraft({ model: value });
  }

  function setBaseUrl(value) {
    patchDraft({ baseUrl: value });
  }

  function setProviderKey(value) {
    patchDraft({ apiKey: value });
  }

  function setContextThreshold(value) {
    patchDraft({ contextThreshold: value });
  }

  function setRequestRetries(value) {
    patchDraft({ requestRetries: value });
  }

  function setMaxTurns(value) {
    patchDraft({ maxTurns: value });
  }

  function setTurnTimeoutSeconds(value) {
    patchDraft({ turnTimeoutSeconds: value });
  }

  function setToolTimeoutSeconds(value) {
    patchDraft({ toolTimeoutSeconds: value });
  }

  function setTotalTimeoutMinutes(value) {
    patchDraft({ totalTimeoutMinutes: value });
  }

  function setProviderName(value) {
    patchDraft({ name: value });
  }

  function createProviderProfileFromCurrent() {
    const next = emptyProviderDraft();
    setIsCreatingProvider(true);
    setDraft(next);
    previousDraftProviderRef.current = next.provider;
    notify?.({ message: '正在新增供应商', detail: '填写后保存生效', tone: 'success', duration: 1800 });
  }

  function selectProviderProfile(id) {
    const profile = providerProfileById(providerProfiles, id);
    if (!profile) return;
    const next = profileDraft(profile);
    setProviderProfileId(profile.id);
    setDraft(next);
    setIsCreatingProvider(false);
    previousDraftProviderRef.current = next.provider;
    notify?.({ message: '已切换供应商配置', detail: profile.name || profile.provider, tone: 'success', duration: 1800 });
  }

  function saveProviderProfile() {
    const patch = profilePatchFromDraft(draft);
    if (isCreatingProvider) {
      const profile = createProviderProfile(patch);
      setProviderProfiles((items) => [...items, profile]);
      setProviderProfileId(profile.id);
      setDraft(profileDraft(profile));
      setIsCreatingProvider(false);
      notify?.({ message: '供应商配置已保存', detail: profile.name, tone: 'success' });
      return;
    }

    if (!activeProfile) return;
    setProviderProfiles((items) => items.map((item) => (
      item.id === activeProfile.id ? { ...item, ...patch } : item
    )));
    notify?.({ message: '供应商配置已保存', detail: patch.name, tone: 'success' });
  }

  function cancelProviderEdit() {
    const next = profileDraft(activeProfile);
    setDraft(next);
    setIsCreatingProvider(false);
    previousDraftProviderRef.current = next.provider;
    notify?.({ message: '已取消编辑', tone: 'success', duration: 1600 });
  }

  function updateProviderProfile(id, patch) {
    if (id === providerProfileId || isCreatingProvider) {
      patchDraft(patch);
      return;
    }
    setProviderProfiles((items) => items.map((item) => (item.id === id ? { ...item, ...patch } : item)));
  }

  function deleteProviderProfile(id) {
    if (providerProfiles.length <= 1 || isCreatingProvider) return;
    const deleted = providerProfileById(providerProfiles, id);
    const next = providerProfiles.filter((item) => item.id !== id);
    setProviderProfiles(next);
    if (providerProfileId === id && next[0]) {
      const formState = profileDraft(next[0]);
      setProviderProfileId(next[0].id);
      setDraft(formState);
      previousDraftProviderRef.current = formState.provider;
    }
    notify?.({ message: '供应商配置已删除', detail: deleted?.name || deleted?.provider || '', tone: 'success' });
  }

  return {
    providerProfiles,
    providerProfileId,
    provider: savedForm.provider,
    model: savedForm.model,
    baseUrl: savedForm.baseUrl,
    providerKey: savedForm.apiKey,
    contextThreshold: savedForm.contextThreshold,
    requestRetries: savedForm.requestRetries,
    maxTurns: savedForm.maxTurns,
    turnTimeoutSeconds: savedForm.turnTimeoutSeconds,
    toolTimeoutSeconds: savedForm.toolTimeoutSeconds,
    totalTimeoutMinutes: savedForm.totalTimeoutMinutes,
    providerDraft: draft,
    providerDraftDirty: isDirty,
    providerDraftCreating: isCreatingProvider,
    setProvider,
    setModel,
    setBaseUrl,
    setProviderKey,
    setContextThreshold,
    setRequestRetries,
    setMaxTurns,
    setTurnTimeoutSeconds,
    setToolTimeoutSeconds,
    setTotalTimeoutMinutes,
    setProviderName,
    createProviderProfileFromCurrent,
    selectProviderProfile,
    updateProviderProfile,
    deleteProviderProfile,
    saveProviderProfile,
    cancelProviderEdit,
  };
}
