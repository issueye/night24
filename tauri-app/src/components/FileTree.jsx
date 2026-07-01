import { useState } from 'react';
import { ChevronDown, ChevronRight, FileCode2 } from 'lucide-react';
import { classNames } from '../utils/format.js';

export function FileTree({ tree, selectedPath, onOpenFile }) {
  if (!tree) {
    return <div className="empty-block">打开项目后显示文件</div>;
  }

  return <TreeNode node={tree} selectedPath={selectedPath} onOpenFile={onOpenFile} depth={0} />;
}

function TreeNode({ node, onOpenFile, selectedPath, depth }) {
  const [open, setOpen] = useState(depth < 2);
  const isDir = node.kind === 'directory';

  return (
    <div className="tree-node">
      <button
        className={classNames('tree-row', selectedPath === node.path && 'selected')}
        style={{ paddingLeft: 8 + depth * 14 }}
        onClick={() => {
          if (isDir) setOpen((value) => !value);
          else onOpenFile(node);
        }}
        type="button"
      >
        {isDir ? (open ? <ChevronDown size={14} /> : <ChevronRight size={14} />) : <FileCode2 size={14} />}
        <span title={node.path}>{node.name || node.path}</span>
      </button>
      {isDir && open && Array.isArray(node.children) && node.children.map((child) => (
        <TreeNode
          key={child.path}
          node={child}
          onOpenFile={onOpenFile}
          selectedPath={selectedPath}
          depth={depth + 1}
        />
      ))}
    </div>
  );
}
