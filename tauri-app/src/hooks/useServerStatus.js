import { useCallback, useState } from 'react';
import { normalizeError } from '../utils/events.js';

export function useServerStatus({ apiJson, addTimeline, showError }) {
  const [serverStatus, setServerStatus] = useState({ state: 'checking', detail: '正在连接 server' });
  const [coreRestarting, setCoreRestarting] = useState(false);

  const checkServer = useCallback(async () => {
    setServerStatus({ state: 'checking', detail: '正在连接 server' });
    try {
      await apiJson('/healthz');
      const ready = await apiJson('/readyz').catch(() => null);
      setServerStatus({
        state: 'connected',
        detail: ready?.ready ? 'server 与 core 已就绪' : ready?.core?.reason || 'server 已连接，core 尚未就绪',
        ready,
      });
      return true;
    } catch (error) {
      setServerStatus({ state: 'failed', detail: normalizeError(error) });
      return false;
    }
  }, [apiJson]);

  const restartCore = useCallback(async () => {
    setCoreRestarting(true);
    setServerStatus({ state: 'checking', detail: '正在重启 core' });
    try {
      const result = await apiJson('/agent/core/restart', {
        method: 'POST',
      });
      if (result?.accepted === false) {
        throw new Error(result.reason || 'core 重启失败');
      }
      addTimeline('core', 'Core 已重启', result?.core?.reason || 'agent-core 已重新初始化', 'success');
      await checkServer();
    } catch (error) {
      const detail = normalizeError(error);
      setServerStatus({ state: 'failed', detail });
      addTimeline('core', 'Core 重启失败', detail, 'error');
      showError(`Core 重启失败：${detail}`);
    } finally {
      setCoreRestarting(false);
    }
  }, [addTimeline, apiJson, checkServer, showError]);

  return {
    serverStatus,
    coreRestarting,
    checkServer,
    restartCore,
  };
}
