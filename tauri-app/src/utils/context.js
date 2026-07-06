import { messageText } from './format.js';
import { parseOptionalPositiveInt } from './settings.js';

export function estimateContextUsage(messages, taskText, thresholdValue) {
  const threshold = parseOptionalPositiveInt(thresholdValue) || 0;
  const text = [
    ...(Array.isArray(messages) ? messages.map(messageText) : []),
    taskText || '',
  ].join('\n\n');
  const asciiChars = (text.match(/[\x00-\x7F]/g) || []).length;
  const nonAsciiChars = Math.max(0, text.length - asciiChars);
  const estimatedTokens = Math.ceil(asciiChars / 4 + nonAsciiChars * 0.8);
  const ratio = threshold > 0 ? Math.min(1, estimatedTokens / threshold) : 0;
  return {
    threshold,
    estimatedTokens,
    percent: threshold > 0 ? Math.round(ratio * 100) : 0,
    reached: threshold > 0 && estimatedTokens >= threshold,
    warning: threshold > 0 && estimatedTokens >= threshold * 0.7,
  };
}
