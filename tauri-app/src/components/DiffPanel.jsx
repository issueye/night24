import { FileDiff, GitBranch, Loader2, RefreshCw } from 'lucide-react';
import { classNames } from '../utils/format.js';
import { Placeholder } from './Placeholder.jsx';
import { IconButton } from './ui/index.js';

export function DiffPanel({ diff, status, loading, error, onRefresh }) {
  const files = Array.isArray(status?.files) ? status.files : [];
  const diffText = typeof diff?.diff === 'string' ? diff.diff : '';
  const hasChanges = Boolean(status?.has_changes || diff?.has_changes || files.length || diffText.trim());

  return (
    <section className="diff-panel">
      <div className="diff-toolbar">
        <div>
          <span>Workspace Diff</span>
          <strong>
            <GitBranch size={13} />
            {status?.branch || 'no branch'}
          </strong>
        </div>
        <IconButton className="icon-button compact" disabled={loading} label="刷新变更" onClick={onRefresh} size="sm">
          {loading ? <Loader2 className="spin" size={14} /> : <RefreshCw size={14} />}
        </IconButton>
      </div>

      {error ? <div className="diff-error">{error}</div> : null}

      {!loading && !error && !hasChanges ? (
        <Placeholder title="当前没有可审阅变更" detail="Agent 产生文件修改后会在这里显示 diff。" />
      ) : (
        <>
          <div className="diff-summary">
            <span><FileDiff size={13} />{files.length} 个文件</span>
            <span>{diff?.staged ? 'staged' : 'working tree'}</span>
          </div>

          {files.length > 0 && (
            <div className="changed-files">
              {files.map((file) => (
                <div className="changed-file" key={file.path}>
                  <span title={file.path}>{file.path}</span>
                  <small>{file.index_status || '-'} / {file.worktree_status || '-'}</small>
                </div>
              ))}
            </div>
          )}

          <pre className="diff-code">
            {loading && !diffText ? '正在加载 diff...' : renderDiffLines(diffText)}
          </pre>
        </>
      )}
    </section>
  );
}

function renderDiffLines(diffText) {
  if (!diffText.trim()) return '暂无 diff 内容';
  return diffText.split('\n').map((line, index) => (
    <span className={classNames('diff-line', diffLineTone(line))} key={`${index}-${line}`}>
      {line || ' '}
      {'\n'}
    </span>
  ));
}

function diffLineTone(line) {
  if (line.startsWith('+++') || line.startsWith('---')) return 'meta';
  if (line.startsWith('@@')) return 'hunk';
  if (line.startsWith('+')) return 'add';
  if (line.startsWith('-')) return 'remove';
  if (line.startsWith('diff --git') || line.startsWith('index ')) return 'meta';
  return '';
}
