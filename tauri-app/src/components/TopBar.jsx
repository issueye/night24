import { AlertTriangle, FolderOpen, Loader2, RotateCcw, Wifi, WifiOff } from 'lucide-react';
import night24Mark from '../assets/night24-mark.svg';
import { classNames } from '../utils/format.js';

export function TopBar({
  serverStatus,
  coreRestarting,
  onRetryServer,
  onRestartCore,
  onOpenWorkspace,
}) {
  return (
    <header className="topbar">
      <div className="brand">
        <div className="brand-mark"><img src={night24Mark} alt="" /></div>
        <div>
          <strong>Night24</strong>
          <span>本地 AI 编程助手</span>
        </div>
      </div>

      <button className={classNames('status-pill', serverStatus.state)} onClick={onRetryServer} type="button">
        {serverStatus.state === 'connected' ? <Wifi size={16} /> : serverStatus.state === 'checking' ? <Loader2 className="spin" size={16} /> : <WifiOff size={16} />}
        <span>{serverStatus.state === 'connected' ? '已连接' : serverStatus.state === 'checking' ? '连接中' : '未连接'}</span>
      </button>

      <div className="topbar-actions">
        <button className="icon-button" disabled={coreRestarting} onClick={onRestartCore} title="重启 Core" type="button">
          {coreRestarting ? <Loader2 className="spin" size={16} /> : <RotateCcw size={16} />}
        </button>
        <button className="toolbar-button" onClick={onOpenWorkspace} type="button">
          <FolderOpen size={16} />
          打开项目
        </button>
      </div>
    </header>
  );
}

export function ErrorBanner({ banner, onClose }) {
  if (!banner) return null;
  return (
    <div className={classNames('banner', banner.tone)}>
      <AlertTriangle size={16} />
      <span>{banner.message}</span>
      <button onClick={onClose} type="button">关闭</button>
    </div>
  );
}
