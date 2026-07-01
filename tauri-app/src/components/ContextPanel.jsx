import { Code2, FileCode2, GitCompare, ShieldAlert, X } from 'lucide-react';
import { safeText } from '../utils/format.js';
import { FilePreview } from './FilePreview.jsx';
import { Placeholder } from './Placeholder.jsx';

export function ContextPanel({
  open,
  rightTab,
  selectedFile,
  pendingPermissions,
  onTabChange,
  onClose,
  onResolvePermission,
}) {
  if (!open) return null;

  return (
    <aside className="context-float">
      <div className="float-head">
        <strong>辅助面板</strong>
        <button className="icon-button compact" onClick={onClose} title="关闭浮窗" type="button"><X size={14} /></button>
      </div>

      <div className="tabs">
        <button className={rightTab === 'files' ? 'active' : ''} onClick={() => onTabChange('files')} type="button"><FileCode2 size={14} />Files</button>
        <button className={rightTab === 'diff' ? 'active' : ''} onClick={() => onTabChange('diff')} type="button"><GitCompare size={14} />Diff</button>
        <button className={rightTab === 'preview' ? 'active' : ''} onClick={() => onTabChange('preview')} type="button"><Code2 size={14} />Preview</button>
        <button className={rightTab === 'permissions' ? 'active' : ''} onClick={() => onTabChange('permissions')} type="button"><ShieldAlert size={14} />Permissions</button>
      </div>

      {rightTab === 'files' && <FilePreview file={selectedFile} />}
      {rightTab === 'diff' && <Placeholder title="当前任务尚未产生可审阅变更" detail="后续接入 workspace diff API 后会在这里显示修改。" />}
      {rightTab === 'preview' && <Placeholder title="尚未启动预览" detail="后续接入进程管理后会在这里显示本地预览地址。" />}
      {rightTab === 'permissions' && <section className="permissions">
        <div className="section-title">
          <span>Permissions</span>
          <ShieldAlert size={14} />
        </div>
        {pendingPermissions.length === 0 ? (
          <div className="empty-block">没有待确认权限</div>
        ) : pendingPermissions.map((permission) => (
          <div className="permission-card" key={permission.permission_id}>
            <strong>{permission.tool_name}</strong>
            <span>{permission.summary}</span>
            <pre>{safeText(permission.arguments)}</pre>
            <div>
              <button onClick={() => onResolvePermission(permission, 'deny')} type="button">拒绝</button>
              <button onClick={() => onResolvePermission(permission, 'approve')} type="button">批准</button>
            </div>
          </div>
        ))}
      </section>}
    </aside>
  );
}
