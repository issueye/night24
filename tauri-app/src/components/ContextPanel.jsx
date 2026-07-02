import { Code2, FileCode2, GitCompare, X } from 'lucide-react';
import { DiffPanel } from './DiffPanel.jsx';
import { FilePreview } from './FilePreview.jsx';
import { Placeholder } from './Placeholder.jsx';

export function ContextPanel({
  open,
  rightTab,
  selectedFile,
  diff,
  status,
  diffLoading,
  diffError,
  onTabChange,
  onClose,
  onRefreshDiff,
}) {
  if (!open) return null;
  const activeTab = ['files', 'diff', 'preview'].includes(rightTab) ? rightTab : 'files';

  return (
    <aside className="context-float">
      <div className="float-head">
        <strong>辅助面板</strong>
        <button className="icon-button compact" onClick={onClose} title="关闭浮窗" type="button"><X size={14} /></button>
      </div>

      <div className="tabs">
        <button className={activeTab === 'files' ? 'active' : ''} onClick={() => onTabChange('files')} type="button"><FileCode2 size={14} />Files</button>
        <button className={activeTab === 'diff' ? 'active' : ''} onClick={() => onTabChange('diff')} type="button"><GitCompare size={14} />Diff</button>
        <button className={activeTab === 'preview' ? 'active' : ''} onClick={() => onTabChange('preview')} type="button"><Code2 size={14} />Preview</button>
      </div>

      {activeTab === 'files' && <FilePreview file={selectedFile} />}
      {activeTab === 'diff' && <DiffPanel diff={diff} status={status} loading={diffLoading} error={diffError} onRefresh={onRefreshDiff} />}
      {activeTab === 'preview' && <Placeholder title="尚未启动预览" detail="后续接入进程管理后会在这里显示本地预览地址。" />}
    </aside>
  );
}
