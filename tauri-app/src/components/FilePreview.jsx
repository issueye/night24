import { formatBytes } from '../utils/format.js';
import { Placeholder } from './Placeholder.jsx';

export function FilePreview({ file }) {
  if (!file) {
    return <Placeholder title="选择一个文件查看内容" detail="项目目录支持文本文件预览。" />;
  }

  return (
    <section className="file-preview">
      <div className="file-head">
        <strong title={file.path}>{file.name || file.path}</strong>
        <span>{formatBytes(file.size)}</span>
      </div>
      {file.error ? (
        <div className="empty-block">{file.error}</div>
      ) : (
        <pre>{file.content || ''}</pre>
      )}
    </section>
  );
}
