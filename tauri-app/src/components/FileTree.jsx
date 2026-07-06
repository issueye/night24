import { FileCode2 } from 'lucide-react';
import { LoadingBlock, Tree } from './ui/index.js';

export function FileTree({ loading, tree, selectedPath, onOpenFile }) {
  if (loading && !tree) {
    return <LoadingBlock label="目录加载中" size="sm" />;
  }
  return (
    <Tree
      empty="打开项目后显示文件"
      getIcon={(node) => (node.kind === 'directory' ? null : <FileCode2 size={14} />)}
      isSelected={(node) => selectedPath === node.path}
      node={tree}
      onSelect={onOpenFile}
    />
  );
}
