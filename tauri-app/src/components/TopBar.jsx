import { AlertTriangle, FolderOpen, Loader2, RotateCcw, Wifi, WifiOff } from 'lucide-react';
import night24Mark from '../assets/night24-mark.svg';
import { classNames } from '../utils/format.js';
import { Button, IconButton } from './ui/index.js';

export function TopBar({
  serverStatus,
  coreRestarting,
  workspaceLoading,
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

      <Button
        className={classNames('status-pill', serverStatus.state)}
        icon={serverStatus.state === 'connected' ? <Wifi size={16} /> : serverStatus.state === 'checking' ? <Loader2 className="spin" size={16} /> : <WifiOff size={16} />}
        onClick={onRetryServer}
        variant="ghost"
      >
        {serverStatus.state === 'connected' ? '已连接' : serverStatus.state === 'checking' ? '连接中' : '未连接'}
      </Button>

      <div className="topbar-actions">
        <IconButton className="icon-button" disabled={coreRestarting} label="重启 Core" onClick={onRestartCore}>
          {coreRestarting ? <Loader2 className="spin" size={16} /> : <RotateCcw size={16} />}
        </IconButton>
        <Button
          className="toolbar-button"
          disabled={workspaceLoading}
          icon={workspaceLoading ? <Loader2 className="spin" size={16} /> : <FolderOpen size={16} />}
          onClick={onOpenWorkspace}
        >
          {workspaceLoading ? '打开中' : '打开项目'}
        </Button>
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
      <Button onClick={onClose} size="sm" variant="soft">关闭</Button>
    </div>
  );
}
