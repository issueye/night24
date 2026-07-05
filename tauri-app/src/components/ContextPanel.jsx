import { useEffect, useState } from 'react';
import { Bot, Code2, FileCode2, GitCompare, Grip, RefreshCw, X } from 'lucide-react';
import { DiffPanel } from './DiffPanel.jsx';
import { FilePreview } from './FilePreview.jsx';
import { FileTree } from './FileTree.jsx';
import { Placeholder } from './Placeholder.jsx';
import { SubAgentPanel } from './SubAgentPanel.jsx';

function defaultPanelRect() {
  const width = Math.min(520, Math.max(320, window.innerWidth - 320));
  const height = Math.min(window.innerHeight - 124, 680);
  return {
    x: Math.max(260, window.innerWidth - width - 20),
    y: 68,
    width,
    height,
  };
}

function clampRect(rect) {
  const minWidth = 320;
  const minHeight = 300;
  const maxWidth = Math.max(minWidth, window.innerWidth - 36);
  const maxHeight = Math.max(minHeight, window.innerHeight - 72);
  const width = Math.min(Math.max(rect.width, minWidth), maxWidth);
  const height = Math.min(Math.max(rect.height, minHeight), maxHeight);
  return {
    width,
    height,
    x: Math.min(Math.max(rect.x, 12), window.innerWidth - width - 12),
    y: Math.min(Math.max(rect.y, 52), window.innerHeight - height - 20),
  };
}

export function ContextPanel({
  open,
  rightTab,
  tree,
  selectedPath,
  selectedFile,
  diff,
  status,
  diffLoading,
  diffError,
  subAgentPool,
  subAgentLoading,
  subAgentError,
  onTabChange,
  onClose,
  onOpenFile,
  onRefreshWorkspace,
  onRefreshDiff,
  onRefreshSubAgents,
}) {
  const [rect, setRect] = useState(defaultPanelRect);
  const [drag, setDrag] = useState(null);
  const activeTab = ['files', 'diff', 'preview', 'agents'].includes(rightTab) ? rightTab : 'files';

  useEffect(() => {
    if (!drag) return undefined;
    function handleMove(event) {
      const dx = event.clientX - drag.startX;
      const dy = event.clientY - drag.startY;
      if (drag.mode === 'move') {
        setRect((current) => clampRect({
          ...current,
          x: drag.startRect.x + dx,
          y: drag.startRect.y + dy,
        }));
      } else {
        setRect((current) => clampRect({
          ...current,
          width: drag.startRect.width + dx,
          height: drag.startRect.height + dy,
        }));
      }
    }
    function handleUp() {
      setDrag(null);
    }
    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
    return () => {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
    };
  }, [drag]);

  useEffect(() => {
    function handleResize() {
      setRect((current) => clampRect(current));
    }
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  if (!open) return null;

  function startDrag(event, mode) {
    if (mode === 'move' && event.target.closest('button')) return;
    event.preventDefault();
    setDrag({
      mode,
      startX: event.clientX,
      startY: event.clientY,
      startRect: rect,
    });
  }

  return (
    <aside
      className="context-float"
      style={{
        left: rect.x,
        top: rect.y,
        width: rect.width,
        height: rect.height,
      }}
    >
      <div className="float-head draggable" onPointerDown={(event) => startDrag(event, 'move')}>
        <span className="drag-indicator"><Grip size={13} /></span>
        <strong>辅助面板</strong>
        <button className="icon-button compact" onClick={onClose} title="关闭浮窗" type="button"><X size={14} /></button>
      </div>

      <div className="tabs">
        <button className={activeTab === 'files' ? 'active' : ''} onClick={() => onTabChange('files')} type="button"><FileCode2 size={14} />Files</button>
        <button className={activeTab === 'diff' ? 'active' : ''} onClick={() => onTabChange('diff')} type="button"><GitCompare size={14} />Diff</button>
        <button className={activeTab === 'preview' ? 'active' : ''} onClick={() => onTabChange('preview')} type="button"><Code2 size={14} />Preview</button>
        <button className={activeTab === 'agents' ? 'active' : ''} onClick={() => onTabChange('agents')} type="button"><Bot size={14} />Agents</button>
      </div>

      {activeTab === 'files' && (
        <section className="files-context">
          <div className="context-tree">
            <div className="context-section-head">
              <strong>项目目录</strong>
              <button className="icon-button compact" onClick={onRefreshWorkspace} title="刷新目录" type="button"><RefreshCw size={13} /></button>
            </div>
            <div className="tree-scroll">
              <FileTree tree={tree} selectedPath={selectedPath} onOpenFile={onOpenFile} />
            </div>
          </div>
          <FilePreview file={selectedFile} />
        </section>
      )}
      {activeTab === 'diff' && <DiffPanel diff={diff} status={status} loading={diffLoading} error={diffError} onRefresh={onRefreshDiff} />}
      {activeTab === 'preview' && <Placeholder title="尚未启动预览" detail="后续接入进程管理后会在这里显示本地预览地址。" />}
      {activeTab === 'agents' && (
        <SubAgentPanel
          pool={subAgentPool}
          loading={subAgentLoading}
          error={subAgentError}
          onRefresh={onRefreshSubAgents}
        />
      )}
      <button
        className="panel-resize-handle"
        onPointerDown={(event) => startDrag(event, 'resize')}
        title="拖动调整大小"
        type="button"
      />
    </aside>
  );
}
