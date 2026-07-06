import { Ban, CheckCircle2, Circle, Clock3, Loader2, XCircle } from 'lucide-react';

const STATUS_META = {
  queued: { label: '排队中', tone: 'queued', icon: Clock3 },
  running: { label: '运行中', tone: 'running', icon: Loader2 },
  completed: { label: '已完成', tone: 'completed', icon: CheckCircle2 },
  failed: { label: '失败', tone: 'failed', icon: XCircle },
  cancelled: { label: '已取消', tone: 'cancelled', icon: Ban },
};

const LIVE_STATUSES = new Set(['queued', 'running']);

export function subAgentStatusMeta(status) {
  return STATUS_META[status] || { label: status || '未知', tone: 'unknown', icon: Circle };
}

export function isLiveSubAgentStatus(status) {
  return LIVE_STATUSES.has(status);
}

export function compactSubAgentText(value, max = 120) {
  const text = String(value || '').replace(/\s+/g, ' ').trim();
  if (text.length <= max) return text;
  return `${text.slice(0, max - 3)}...`;
}

export function deriveSubAgentStats(pool) {
  const agents = Array.isArray(pool?.subagents) ? pool.subagents : [];
  const count = (status) => agents.filter((agent) => agent?.status === status).length;
  return {
    total: Number.isFinite(pool?.total) ? pool.total : agents.length,
    queued: Number.isFinite(pool?.queued) ? pool.queued : count('queued'),
    running: Number.isFinite(pool?.running) ? pool.running : count('running'),
    completed: Number.isFinite(pool?.completed) ? pool.completed : count('completed'),
    failed: Number.isFinite(pool?.failed) ? pool.failed : count('failed'),
    cancelled: Number.isFinite(pool?.cancelled) ? pool.cancelled : count('cancelled'),
  };
}
