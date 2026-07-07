import {
  parseMaxTurns,
  parseOptionalPositiveInt,
  parseRequestRetries,
  parseTimeoutMinutesMs,
  parseTimeoutSecondsMs,
} from './settings.js';

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
  requestRetries,
  maxTurns,
  turnTimeoutSeconds,
  toolTimeoutSeconds,
  totalTimeoutMinutes,
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
    request_retries: parseRequestRetries(requestRetries),
    max_turns: parseMaxTurns(maxTurns),
    turn_timeout_ms: parseTimeoutSecondsMs(turnTimeoutSeconds),
    tool_timeout_ms: parseTimeoutSecondsMs(toolTimeoutSeconds),
    total_timeout_ms: parseTimeoutMinutesMs(totalTimeoutMinutes),
  };
}
