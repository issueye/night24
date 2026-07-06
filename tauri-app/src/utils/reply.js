import { parseOptionalPositiveInt } from './settings.js';

export function buildReplyRequestBody({
  text,
  sessionId,
  provider,
  model,
  baseUrl,
  providerKey,
  accessMode,
  networkProxy,
  contextThreshold,
}) {
  return {
    text,
    session_id: sessionId,
    provider,
    model: model.trim() || undefined,
    base_url: baseUrl.trim() || undefined,
    api_key: providerKey.trim() || undefined,
    permission_mode: accessMode,
    network_proxy: networkProxy.trim() || undefined,
    context_threshold_tokens: parseOptionalPositiveInt(contextThreshold),
  };
}
